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
    InteractionEffect, InteractionOutcome, InteractionState, InvocationSnapshot, ScreenPoint,
};
use rtrb::{Consumer, RingBuffer};
use uuid::Uuid;
use windows::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint},
    System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    },
};

use crate::input::{HookThread, HookThreadMetrics};

const INPUT_QUEUE_CAPACITY: usize = 4_096;
const OVERLAY_COMMAND_CAPACITY: usize = 64;
const HOOK_UNRESPONSIVE_AFTER_MS: i64 = 750;
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(1);
const LIFECYCLE_COMMAND_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OverlayCommand {
    Open { origin: ScreenPoint },
    SelectionChanged { selected_slice: Option<usize> },
    Close,
}

#[derive(Debug)]
pub(crate) struct InteractionRuntime {
    hook_thread: HookThread,
    shutdown: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
    commands: Receiver<OverlayCommand>,
}

impl InteractionRuntime {
    pub(crate) fn start(egui_context: eframe::egui::Context) -> Result<Self> {
        let (producer, consumer) = RingBuffer::new(INPUT_QUEUE_CAPACITY);
        let hook_thread = HookThread::spawn(producer)?;
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
) {
    let mut controller = InteractionController::new(GestureConfig::default());
    let mut activation_down_events = 0_u64;
    let mut pointer_moved_events = 0_u64;
    let mut activation_up_events = 0_u64;
    let mut cancel_events = 0_u64;
    let qpc_frequency = query_qpc_frequency();
    let hook_timeout_qpc = qpc_frequency.saturating_mul(HOOK_UNRESPONSIVE_AFTER_MS) / 1_000;

    while !shutdown.load(Ordering::Acquire) {
        if metrics.queue_overflow.swap(false, Ordering::AcqRel) {
            if let Some(effect) = controller.fault(CancelReason::InputQueueOverflow) {
                dispatch_effect(effect, command_sender, egui_context);
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
                dispatch_effect(effect, command_sender, egui_context);
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
            let effect = match event.kind {
                InputEventKind::ActivationDown => {
                    activation_down_events = activation_down_events.saturating_add(1);
                    controller.activation_down(capture_snapshot(event))
                }
                InputEventKind::PointerMoved => {
                    pointer_moved_events = pointer_moved_events.saturating_add(1);
                    controller.pointer_moved(event.point)
                }
                InputEventKind::ActivationUp => {
                    activation_up_events = activation_up_events.saturating_add(1);
                    controller.activation_up()
                }
                InputEventKind::Cancel(reason) => {
                    cancel_events = cancel_events.saturating_add(1);
                    controller.cancel(reason)
                }
            };

            if let Some(effect) = effect {
                dispatch_effect(effect, command_sender, egui_context);
            }
        }

        if !handled_event {
            thread::sleep(IDLE_POLL_INTERVAL);
        }
    }

    if let Some(effect) = controller.fault(CancelReason::Shutdown) {
        dispatch_effect(effect, command_sender, egui_context);
    }
    log::info!(
        target: "interaction",
        "component=interaction event_code=interaction_thread_stopped result_code=ok \
         config_revision=0 activation_down_events={activation_down_events} \
         pointer_moved_events={pointer_moved_events} activation_up_events={activation_up_events} \
         cancel_events={cancel_events}"
    );
}

fn capture_snapshot(event: InputEvent) -> InvocationSnapshot {
    let foreground_window = unsafe { GetForegroundWindow() };
    let mut foreground_pid = 0_u32;
    if !foreground_window.is_invalid() {
        // SAFETY: the optional output pointer refers to a live local u32.
        unsafe {
            GetWindowThreadProcessId(foreground_window, Some(&raw mut foreground_pid));
        }
    }

    let monitor_dpi = monitor_dpi(event.point);
    InvocationSnapshot {
        invocation_id: Uuid::new_v4(),
        qpc_timestamp: event.qpc_timestamp,
        foreground_window: foreground_window.0.addr(),
        foreground_pid,
        cursor_physical_px: event.point,
        monitor_dpi,
    }
}

fn monitor_dpi(point: ScreenPoint) -> u32 {
    let monitor =
        unsafe { MonitorFromPoint(POINT { x: point.x, y: point.y }, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return 96;
    }

    let mut dpi_x = 96_u32;
    let mut dpi_y = 96_u32;
    // SAFETY: `monitor` is resolved from the supplied virtual-screen point,
    // and both output pointers refer to live local values.
    if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y) }
        .is_err()
    {
        96
    } else {
        dpi_x
    }
}

fn dispatch_effect(
    effect: InteractionEffect,
    command_sender: &Sender<OverlayCommand>,
    egui_context: &eframe::egui::Context,
) {
    let command = match effect {
        InteractionEffect::Open { invocation_id, origin } => {
            log::info!(
                target: "interaction",
                "component=interaction event_code=invocation_started result_code=ok \
                 config_revision=0 invocation_id={invocation_id}"
            );
            OverlayCommand::Open { origin }
        }
        InteractionEffect::SelectionChanged { invocation_id, selected_slice } => {
            log::debug!(
                target: "interaction",
                "component=interaction event_code=selection_changed result_code=ok \
                 config_revision=0 invocation_id={invocation_id} \
                 selected_slice={selected_slice:?}"
            );
            OverlayCommand::SelectionChanged { selected_slice }
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
            OverlayCommand::Close
        }
    };

    match command_sender.try_send(command) {
        Ok(()) => egui_context.request_repaint(),
        Err(TrySendError::Full(
            command @ (OverlayCommand::Open { .. } | OverlayCommand::Close),
        )) => {
            // A selection repaint may be dropped under pressure, but the
            // overlay's visible lifetime must remain balanced. Wake the UI and
            // give it one bounded opportunity to drain without ever blocking
            // the low-level hook callback.
            egui_context.request_repaint();
            match command_sender.send_timeout(command, LIFECYCLE_COMMAND_TIMEOUT) {
                Ok(()) => egui_context.request_repaint(),
                Err(SendTimeoutError::Timeout(_)) => {
                    log::error!(
                        target: "interaction",
                        "component=interaction event_code=overlay_lifecycle_command_timeout \
                         result_code=failure config_revision=0 timeout_ms=50"
                    );
                }
                Err(SendTimeoutError::Disconnected(_)) => {
                    log::warn!(
                        target: "interaction",
                        "component=interaction event_code=overlay_command_queue_closed \
                         result_code=failure config_revision=0"
                    );
                }
            }
        }
        Err(TrySendError::Full(_)) => {
            log::error!(
                target: "interaction",
                "component=interaction event_code=overlay_command_queue_full result_code=failure \
                 config_revision=0 capacity={OVERLAY_COMMAND_CAPACITY}"
            );
        }
        Err(TrySendError::Disconnected(_)) => {
            log::warn!(
                target: "interaction",
                "component=interaction event_code=overlay_command_queue_closed result_code=failure \
                 config_revision=0"
            );
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
