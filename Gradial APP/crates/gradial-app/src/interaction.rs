use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, SendTimeoutError, Sender, TrySendError, bounded};
use gradial_core::{
    CancelReason, GestureConfig, InputEvent, InputEventKind, InteractionController,
    InteractionEffect, InteractionOutcome, InteractionState, InvocationSnapshot,
};
use rtrb::{Consumer, RingBuffer};
use uuid::Uuid;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use crate::{
    diagnostics::TimingMarkers,
    input::{HookThread, HookThreadMetrics, InputConfiguration},
    platform,
};

const INPUT_QUEUE_CAPACITY: usize = 4_096;
const OVERLAY_COMMAND_CAPACITY: usize = 64;
const HOOK_UNRESPONSIVE_AFTER_MS: i64 = 750;
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(1);
const LIFECYCLE_COMMAND_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Debug, Default)]
struct EventCounters {
    activation_down: u64,
    pointer_moved: u64,
    raw_pointer_delta: u64,
    activation_up: u64,
    cancel: u64,
}

#[derive(Debug)]
struct EffectDispatcher<'a> {
    command_sender: &'a Sender<OverlayCommand>,
    egui_context: &'a eframe::egui::Context,
    timing_markers: &'a TimingMarkers,
    overlay_wake_handle: platform::OverlayWakeHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OverlayCommand {
    Open { snapshot: InvocationSnapshot },
    SelectionChanged { invocation_id: Uuid, selected_slice: Option<usize> },
    Close { invocation_id: Uuid },
}

#[derive(Debug)]
pub(crate) struct InteractionRuntime {
    hook_thread: HookThread,
    shutdown: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
    commands: Receiver<OverlayCommand>,
}

impl InteractionRuntime {
    pub(crate) fn start(
        egui_context: eframe::egui::Context,
        input_configuration: InputConfiguration,
        timing_markers: Arc<TimingMarkers>,
        overlay_wake_handle: platform::OverlayWakeHandle,
    ) -> Result<Self> {
        let (producer, consumer) = RingBuffer::new(INPUT_QUEUE_CAPACITY);
        let hook_thread = HookThread::spawn(producer, input_configuration)?;
        let hook_metrics = hook_thread.metrics.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let (command_sender, commands) = bounded(OVERLAY_COMMAND_CAPACITY);

        let join_handle = thread::Builder::new()
            .name("gradial-interaction".to_owned())
            .spawn(move || {
                run_interaction_thread(
                    consumer,
                    &command_sender,
                    &egui_context,
                    &hook_metrics,
                    &thread_shutdown,
                    &timing_markers,
                    overlay_wake_handle,
                );
            })
            .context("could not spawn the interaction thread")?;

        Ok(Self { hook_thread, shutdown, join_handle: Some(join_handle), commands })
    }

    pub(crate) fn try_command(&self) -> Option<OverlayCommand> {
        self.commands.try_recv().ok()
    }
}

impl Drop for InteractionRuntime {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.hook_thread.stop();

        if let Some(join_handle) = self.join_handle.take()
            && join_handle.join().is_err()
        {
            log::error!(
                target: "interaction",
                "component=interaction event_code=interaction_thread_panicked \
                 result_code=failure config_revision=0"
            );
        }

        let frequency = query_qpc_frequency();
        let maximum_qpc = self.hook_thread.metrics.maximum_callback_qpc.load(Ordering::Acquire);
        let maximum_microseconds =
            if frequency > 0 { maximum_qpc.saturating_mul(1_000_000) / frequency } else { 0 };
        #[cfg(debug_assertions)]
        log::info!(
            target: "input",
            "component=input event_code=hook_debug_summary result_code=ok config_revision=0 \
             mouse_callback_count={} last_mouse_message={} last_mouse_data={}",
            self.hook_thread.metrics.debug.mouse_callback_count.load(Ordering::Acquire),
            self.hook_thread.metrics.debug.last_mouse_message.load(Ordering::Acquire),
            self.hook_thread.metrics.debug.last_mouse_data.load(Ordering::Acquire),
        );
        log::info!(
            target: "input",
            "component=input event_code=hook_thread_stopped result_code=ok config_revision=0 \
             maximum_callback_us={maximum_microseconds}"
        );
    }
}

fn run_interaction_thread(
    mut consumer: Consumer<InputEvent>,
    command_sender: &Sender<OverlayCommand>,
    egui_context: &eframe::egui::Context,
    metrics: &HookThreadMetrics,
    shutdown: &Arc<AtomicBool>,
    timing_markers: &TimingMarkers,
    overlay_wake_handle: platform::OverlayWakeHandle,
) {
    let mut controller = InteractionController::new(GestureConfig::default());
    let mut active_snapshot = None;
    let mut counters = EventCounters::default();
    let dispatcher =
        EffectDispatcher { command_sender, egui_context, timing_markers, overlay_wake_handle };
    let qpc_frequency = query_qpc_frequency();
    let hook_timeout_qpc = qpc_frequency.saturating_mul(HOOK_UNRESPONSIVE_AFTER_MS) / 1_000;

    while !shutdown.load(Ordering::Acquire) {
        if metrics.queue_overflow.swap(false, Ordering::AcqRel) {
            if let Some(effect) = controller.fault(CancelReason::InputQueueOverflow) {
                dispatch_effect(effect, query_qpc(), active_snapshot, &dispatcher);
                active_snapshot = None;
            }
            log::error!(
                target: "input",
                "component=input event_code=input_queue_overflow result_code=failure \
                 config_revision=0 capacity={INPUT_QUEUE_CAPACITY}"
            );
        }

        let now = query_qpc();
        let heartbeat = metrics.heartbeat_qpc.load(Ordering::Acquire);
        if is_active(controller.state())
            && qpc_frequency > 0
            && now.saturating_sub(heartbeat) > hook_timeout_qpc
        {
            if let Some(effect) = controller.fault(CancelReason::HookUnresponsive) {
                dispatch_effect(effect, now, active_snapshot, &dispatcher);
                active_snapshot = None;
            }
            log::error!(
                target: "input",
                "component=input event_code=hook_thread_unresponsive result_code=failure \
                 config_revision=0 timeout_ms={HOOK_UNRESPONSIVE_AFTER_MS}"
            );
        }

        let mut handled_event = false;
        while let Ok(event) = consumer.pop() {
            handled_event = true;
            handle_input_event(
                event,
                &mut controller,
                &mut active_snapshot,
                &mut counters,
                &dispatcher,
            );
        }

        if !handled_event {
            thread::sleep(IDLE_POLL_INTERVAL);
        }
    }

    if let Some(effect) = controller.fault(CancelReason::Shutdown) {
        dispatch_effect(effect, query_qpc(), active_snapshot, &dispatcher);
    }
    log::info!(
        target: "interaction",
        "component=interaction event_code=interaction_thread_stopped result_code=ok \
         config_revision=0 activation_down_events={} pointer_moved_events={} \
         activation_up_events={} raw_pointer_delta_events={} cancel_events={}",
        counters.activation_down,
        counters.pointer_moved,
        counters.activation_up,
        counters.raw_pointer_delta,
        counters.cancel
    );
}

fn handle_input_event(
    event: InputEvent,
    controller: &mut InteractionController,
    active_snapshot: &mut Option<InvocationSnapshot>,
    counters: &mut EventCounters,
    dispatcher: &EffectDispatcher<'_>,
) {
    let source_qpc = event.qpc_timestamp;
    let effect = match event.kind {
        InputEventKind::ActivationDown(geometry) => {
            counters.activation_down = counters.activation_down.saturating_add(1);
            let snapshot = capture_snapshot(event, geometry);
            *active_snapshot = Some(snapshot);
            dispatcher.timing_markers.emit(
                "activation_dequeued",
                Some(snapshot.invocation_id),
                source_qpc,
                &format!("geometry={}", geometry.behavior.as_str()),
            );
            controller.activation_down(snapshot)
        }
        InputEventKind::PointerMoved => {
            counters.pointer_moved = counters.pointer_moved.saturating_add(1);
            controller.pointer_moved(event.point)
        }
        InputEventKind::RawPointerDelta { dx, dy } => {
            counters.raw_pointer_delta = counters.raw_pointer_delta.saturating_add(1);
            controller.raw_pointer_delta(dx, dy)
        }
        InputEventKind::ActivationUp => {
            counters.activation_up = counters.activation_up.saturating_add(1);
            dispatcher.timing_markers.emit(
                "release_dequeued",
                active_snapshot.map(|snapshot| snapshot.invocation_id),
                source_qpc,
                "",
            );
            controller.activation_up()
        }
        InputEventKind::Cancel(reason) => {
            counters.cancel = counters.cancel.saturating_add(1);
            controller.cancel(reason)
        }
    };

    if let Some(effect) = effect {
        dispatch_effect(effect, source_qpc, *active_snapshot, dispatcher);
    }
    if matches!(controller.state(), InteractionState::Idle | InteractionState::Faulted) {
        *active_snapshot = None;
    }
}

fn capture_snapshot(
    event: InputEvent,
    geometry: gradial_core::InvocationGeometry,
) -> InvocationSnapshot {
    let foreground = platform::foreground_window_identity();
    InvocationSnapshot {
        invocation_id: Uuid::new_v4(),
        qpc_timestamp: event.qpc_timestamp,
        foreground_window: foreground.window,
        foreground_pid: foreground.process_id,
        cursor_physical_px: event.point,
        monitor_dpi: geometry.monitor_dpi,
        geometry,
    }
}

fn dispatch_effect(
    effect: InteractionEffect,
    source_qpc: i64,
    active_snapshot: Option<InvocationSnapshot>,
    dispatcher: &EffectDispatcher<'_>,
) {
    let command = match effect {
        InteractionEffect::Open { invocation_id, .. } => {
            let Some(snapshot) =
                active_snapshot.filter(|snapshot| snapshot.invocation_id == invocation_id)
            else {
                log::error!(
                    target: "interaction",
                    "component=interaction event_code=invocation_snapshot_missing \
                     result_code=failure config_revision=0 invocation_id={invocation_id}"
                );
                return;
            };
            log::info!(
                target: "interaction",
                "component=interaction event_code=invocation_started result_code=ok \
                 config_revision=0 invocation_id={invocation_id} geometry={}",
                snapshot.geometry.behavior.as_str()
            );
            dispatcher.timing_markers.emit(
                "open_requested",
                Some(invocation_id),
                source_qpc,
                &format!("geometry={}", snapshot.geometry.behavior.as_str()),
            );
            if let Err(error) = platform::request_overlay_wake(
                dispatcher.overlay_wake_handle,
                snapshot.geometry.overlay_origin_physical_px,
            ) {
                log::error!(
                    target: "renderer",
                    "component=renderer event_code=overlay_wake_request_failed \
                     result_code=failure config_revision=0 invocation_id={invocation_id} \
                     error={error}"
                );
            } else {
                dispatcher.timing_markers.emit(
                    "overlay_wake_requested",
                    Some(invocation_id),
                    source_qpc,
                    "",
                );
            }
            OverlayCommand::Open { snapshot }
        }
        InteractionEffect::SelectionChanged { invocation_id, selected_slice } => {
            log::debug!(
                target: "interaction",
                "component=interaction event_code=selection_changed result_code=ok \
                 config_revision=0 invocation_id={invocation_id} \
                 selected_slice={selected_slice:?}"
            );
            dispatcher.timing_markers.emit(
                "highlight_requested",
                Some(invocation_id),
                source_qpc,
                &format!("selected_slice={selected_slice:?}"),
            );
            OverlayCommand::SelectionChanged { invocation_id, selected_slice }
        }
        InteractionEffect::Close { invocation_id, outcome } => {
            match outcome {
                InteractionOutcome::Selected { slice_index } => {
                    log::info!(
                        target: "interaction",
                        "component=interaction event_code=slice_selected result_code=ok \
                         config_revision=0 invocation_id={invocation_id} \
                         slice_index={slice_index} action_executed=false"
                    );
                }
                InteractionOutcome::Cancelled { reason } => {
                    log::info!(
                        target: "interaction",
                        "component=interaction event_code=invocation_cancelled result_code=ok \
                         config_revision=0 invocation_id={invocation_id} reason={reason:?}"
                    );
                }
            }
            dispatcher.timing_markers.emit("close_requested", Some(invocation_id), source_qpc, "");
            OverlayCommand::Close { invocation_id }
        }
    };

    let lifecycle = matches!(command, OverlayCommand::Open { .. } | OverlayCommand::Close { .. });
    let enqueued =
        send_overlay_command(command, dispatcher.command_sender, dispatcher.egui_context);
    if lifecycle
        && !enqueued
        && let Err(error) = platform::request_overlay_hide(dispatcher.overlay_wake_handle)
    {
        log::error!(
            target: "renderer",
            "component=renderer event_code=overlay_fail_safe_hide_failed \
             result_code=failure config_revision=0 error={error}"
        );
    }
}

fn send_overlay_command(
    command: OverlayCommand,
    command_sender: &Sender<OverlayCommand>,
    egui_context: &eframe::egui::Context,
) -> bool {
    match command_sender.try_send(command) {
        Ok(()) => {
            egui_context.request_repaint();
            true
        }
        Err(TrySendError::Full(
            command @ (OverlayCommand::Open { .. } | OverlayCommand::Close { .. }),
        )) => {
            // A selection repaint may be dropped under pressure, but the
            // overlay's visible lifetime must remain balanced. Wake the UI and
            // give it one bounded opportunity to drain without ever blocking
            // the low-level hook callback.
            egui_context.request_repaint();
            match command_sender.send_timeout(command, LIFECYCLE_COMMAND_TIMEOUT) {
                Ok(()) => {
                    egui_context.request_repaint();
                    true
                }
                Err(SendTimeoutError::Timeout(_)) => {
                    log::error!(
                        target: "interaction",
                        "component=interaction event_code=overlay_lifecycle_command_timeout \
                         result_code=failure config_revision=0 timeout_ms=50"
                    );
                    false
                }
                Err(SendTimeoutError::Disconnected(_)) => {
                    log::warn!(
                        target: "interaction",
                        "component=interaction event_code=overlay_command_queue_closed \
                         result_code=failure config_revision=0"
                    );
                    false
                }
            }
        }
        Err(TrySendError::Full(_)) => {
            log::error!(
                target: "interaction",
                "component=interaction event_code=overlay_command_queue_full result_code=failure \
                 config_revision=0 capacity={OVERLAY_COMMAND_CAPACITY}"
            );
            false
        }
        Err(TrySendError::Disconnected(_)) => {
            log::warn!(
                target: "interaction",
                "component=interaction event_code=overlay_command_queue_closed result_code=failure \
                 config_revision=0"
            );
            false
        }
    }
}

const fn is_active(state: InteractionState) -> bool {
    matches!(
        state,
        InteractionState::Opening
            | InteractionState::Tracking
            | InteractionState::Cancelling
            | InteractionState::Closing
    )
}

fn query_qpc() -> i64 {
    let mut timestamp = 0_i64;
    // SAFETY: the output pointer refers to a live local i64.
    if unsafe { QueryPerformanceCounter(&raw mut timestamp) }.is_ok() { timestamp } else { 0 }
}

fn query_qpc_frequency() -> i64 {
    let mut frequency = 0_i64;
    // SAFETY: the output pointer refers to a live local i64.
    if unsafe { QueryPerformanceFrequency(&raw mut frequency) }.is_ok() { frequency } else { 0 }
}
