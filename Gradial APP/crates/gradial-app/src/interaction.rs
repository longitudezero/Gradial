use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[cfg(debug_assertions)]
use std::sync::atomic::AtomicU8;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use gradial_core::{
    CancelReason, GestureConfig, InputEvent, InputEventKind, InteractionController,
    InteractionEffect, InteractionOutcome, InteractionState, InvocationSnapshot, SliceCount,
    monitor_index_for_point,
};
use rtrb::{Consumer, RingBuffer};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use crate::{
    diagnostics::TimingMarkers,
    input::{HookThread, HookThreadMetrics, InputConfiguration},
    overlay::OverlayRoute,
    platform,
};

const INPUT_QUEUE_CAPACITY: usize = 4_096;
const RUNTIME_CHANNEL_CAPACITY: usize = 4;
const HOOK_UNRESPONSIVE_AFTER_MS: i64 = 750;
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(1);

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
    egui_context: &'a eframe::egui::Context,
    timing_markers: &'a TimingMarkers,
    routes: &'a [OverlayRoute],
    slice_count: SliceCount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeControl {
    DisplayTopologyChanged,
    ReregisterInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeEvent {
    DisplayRebuildReady,
    InputRecoveryReady,
    InputFaulted,
}

#[derive(Debug)]
pub(crate) struct InteractionRuntime {
    hook_thread: HookThread,
    accepting_input: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
    control_sender: Sender<RuntimeControl>,
    events: Receiver<RuntimeEvent>,
    #[cfg(debug_assertions)]
    debug_failure: Arc<AtomicU8>,
}

impl InteractionRuntime {
    pub(crate) fn start(
        egui_context: eframe::egui::Context,
        input_configuration: InputConfiguration,
        timing_markers: Arc<TimingMarkers>,
        routes: Vec<OverlayRoute>,
        slice_count: SliceCount,
    ) -> Result<Self> {
        let (producer, consumer) = RingBuffer::new(INPUT_QUEUE_CAPACITY);
        let accepting_input = Arc::new(AtomicBool::new(true));
        let hook_thread =
            HookThread::spawn(producer, input_configuration, Arc::clone(&accepting_input))?;
        let hook_metrics = hook_thread.metrics.clone();
        let thread_accepting_input = Arc::clone(&accepting_input);
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let (control_sender, controls) = bounded(RUNTIME_CHANNEL_CAPACITY);
        let (event_sender, events) = bounded(RUNTIME_CHANNEL_CAPACITY);
        #[cfg(debug_assertions)]
        let debug_failure = Arc::new(AtomicU8::new(0));
        #[cfg(debug_assertions)]
        let thread_debug_failure = Arc::clone(&debug_failure);

        let join_handle = thread::Builder::new()
            .name("gradial-interaction".to_owned())
            .spawn(move || {
                run_interaction_thread(
                    consumer,
                    &controls,
                    &event_sender,
                    &egui_context,
                    &hook_metrics,
                    &thread_accepting_input,
                    #[cfg(debug_assertions)]
                    &thread_debug_failure,
                    &thread_shutdown,
                    &timing_markers,
                    &routes,
                    slice_count,
                );
            })
            .context("could not spawn the interaction thread")?;

        Ok(Self {
            hook_thread,
            accepting_input,
            shutdown,
            join_handle: Some(join_handle),
            control_sender,
            events,
            #[cfg(debug_assertions)]
            debug_failure,
        })
    }

    pub(crate) fn begin_display_rebuild(&self) -> Result<()> {
        self.accepting_input.store(false, Ordering::Release);
        self.control_sender
            .try_send(RuntimeControl::DisplayTopologyChanged)
            .context("could not request display-topology cancellation")
    }

    pub(crate) fn begin_input_recovery(&self) -> Result<()> {
        self.accepting_input.store(false, Ordering::Release);
        self.control_sender
            .try_send(RuntimeControl::ReregisterInput)
            .context("could not request input re-registration")
    }

    pub(crate) fn try_event(&self) -> Option<RuntimeEvent> {
        self.events.try_recv().ok()
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_simulate_hook_unresponsive(&self) {
        self.accepting_input.store(false, Ordering::Release);
        self.debug_failure.store(1, Ordering::Release);
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_simulate_queue_overflow(&self) {
        self.accepting_input.store(false, Ordering::Release);
        self.debug_failure.store(2, Ordering::Release);
    }
}

impl Drop for InteractionRuntime {
    fn drop(&mut self) {
        self.accepting_input.store(false, Ordering::Release);
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

#[allow(clippy::too_many_arguments)]
fn run_interaction_thread(
    mut consumer: Consumer<InputEvent>,
    controls: &Receiver<RuntimeControl>,
    event_sender: &Sender<RuntimeEvent>,
    egui_context: &eframe::egui::Context,
    metrics: &HookThreadMetrics,
    accepting_input: &Arc<AtomicBool>,
    #[cfg(debug_assertions)] debug_failure: &Arc<AtomicU8>,
    shutdown: &Arc<AtomicBool>,
    timing_markers: &TimingMarkers,
    routes: &[OverlayRoute],
    slice_count: SliceCount,
) {
    let gesture_config = GestureConfig { slice_count, ..GestureConfig::default() };
    let mut controller = InteractionController::new(gesture_config);
    let mut active_snapshot = None;
    let mut active_route = None;
    let mut counters = EventCounters::default();
    let dispatcher = EffectDispatcher { egui_context, timing_markers, routes, slice_count };
    let qpc_frequency = query_qpc_frequency();
    let hook_timeout_qpc = qpc_frequency.saturating_mul(HOOK_UNRESPONSIVE_AFTER_MS) / 1_000;
    let mut runtime_stop_pending = false;

    while !shutdown.load(Ordering::Acquire) {
        #[cfg(debug_assertions)]
        let simulated_failure = debug_failure.swap(0, Ordering::AcqRel);
        #[cfg(not(debug_assertions))]
        let simulated_failure = 0_u8;

        if let Ok(control) = controls.try_recv() {
            handle_runtime_control(
                control,
                &mut controller,
                &mut active_snapshot,
                &mut active_route,
                event_sender,
                &dispatcher,
            );
            runtime_stop_pending = true;
        }

        if runtime_stop_pending {
            thread::sleep(IDLE_POLL_INTERVAL);
            continue;
        }

        if simulated_failure == 2 || metrics.queue_overflow.swap(false, Ordering::AcqRel) {
            accepting_input.store(false, Ordering::Release);
            if let Some(effect) = controller.fault(CancelReason::InputQueueOverflow) {
                dispatch_effect(effect, query_qpc(), active_snapshot, active_route, &dispatcher);
            }
            active_snapshot = None;
            active_route = None;
            runtime_stop_pending = true;
            log::error!(
                target: "input",
                "component=input event_code=input_queue_overflow result_code=failure \
                 config_revision=0 capacity={INPUT_QUEUE_CAPACITY}"
            );
            if let Err(error) = event_sender.try_send(RuntimeEvent::InputFaulted) {
                log_runtime_event_error(error);
            }
            platform::request_controller_repaint();
            continue;
        }

        let now = query_qpc();
        let heartbeat = metrics.heartbeat_qpc.load(Ordering::Acquire);
        if simulated_failure == 1
            || heartbeat_is_unresponsive(qpc_frequency, now, heartbeat, hook_timeout_qpc)
        {
            accepting_input.store(false, Ordering::Release);
            if let Some(effect) = controller.fault(CancelReason::HookUnresponsive) {
                dispatch_effect(effect, now, active_snapshot, active_route, &dispatcher);
            }
            active_snapshot = None;
            active_route = None;
            runtime_stop_pending = true;
            log::error!(
                target: "input",
                "component=input event_code=hook_thread_unresponsive result_code=failure \
                 config_revision=0 timeout_ms={HOOK_UNRESPONSIVE_AFTER_MS}"
            );
            if let Err(error) = event_sender.try_send(RuntimeEvent::InputRecoveryReady) {
                log_runtime_event_error(error);
            }
            platform::request_controller_repaint();
            continue;
        }

        let mut handled_event = false;
        while let Ok(event) = consumer.pop() {
            handled_event = true;
            handle_input_event(
                event,
                &mut controller,
                &mut active_snapshot,
                &mut active_route,
                &mut counters,
                &dispatcher,
            );
        }

        if !handled_event {
            thread::sleep(IDLE_POLL_INTERVAL);
        }
    }

    if let Some(effect) = controller.fault(CancelReason::Shutdown) {
        dispatch_effect(effect, query_qpc(), active_snapshot, active_route, &dispatcher);
    }
    log_interaction_stopped(&counters);
}

fn log_interaction_stopped(counters: &EventCounters) {
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

fn handle_runtime_control(
    control: RuntimeControl,
    controller: &mut InteractionController,
    active_snapshot: &mut Option<InvocationSnapshot>,
    active_route: &mut Option<usize>,
    event_sender: &Sender<RuntimeEvent>,
    dispatcher: &EffectDispatcher<'_>,
) {
    let (reason, event, event_code) = match control {
        RuntimeControl::DisplayTopologyChanged => (
            CancelReason::DisplayTopologyChange,
            RuntimeEvent::DisplayRebuildReady,
            "display_rebuild_requested",
        ),
        RuntimeControl::ReregisterInput => (
            CancelReason::InputReregistration,
            RuntimeEvent::InputRecoveryReady,
            "input_reregistration_started",
        ),
    };
    if let Some(effect) = controller.fault(reason) {
        dispatch_effect(effect, query_qpc(), *active_snapshot, *active_route, dispatcher);
    }
    *active_snapshot = None;
    *active_route = None;
    log::info!(
        target: "interaction",
        "component=interaction event_code={event_code} result_code=ok config_revision=0"
    );
    if let Err(error) = event_sender.try_send(event) {
        log_runtime_event_error(error);
    }
    platform::request_controller_repaint();
}

fn handle_input_event(
    event: InputEvent,
    controller: &mut InteractionController,
    active_snapshot: &mut Option<InvocationSnapshot>,
    active_route: &mut Option<usize>,
    counters: &mut EventCounters,
    dispatcher: &EffectDispatcher<'_>,
) {
    let source_qpc = event.qpc_timestamp;
    let effect = match event.kind {
        InputEventKind::ActivationDown(geometry) => {
            counters.activation_down = counters.activation_down.saturating_add(1);
            let snapshot = capture_snapshot(event, geometry);
            *active_snapshot = Some(snapshot);
            *active_route = route_for_point(snapshot.cursor_physical_px, dispatcher.routes);
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
        dispatch_effect(effect, source_qpc, *active_snapshot, *active_route, dispatcher);
    }
    if matches!(controller.state(), InteractionState::Idle | InteractionState::Faulted) {
        *active_snapshot = None;
        *active_route = None;
    }
}

fn capture_snapshot(
    event: InputEvent,
    geometry: gradial_core::InvocationGeometry,
) -> InvocationSnapshot {
    let foreground = platform::foreground_window_identity();
    InvocationSnapshot {
        invocation_id: uuid::Uuid::new_v4(),
        qpc_timestamp: event.qpc_timestamp,
        foreground_window: foreground.window,
        foreground_pid: foreground.process_id,
        cursor_physical_px: event.point,
        monitor_dpi: geometry.monitor_dpi,
        geometry,
    }
}

fn route_for_point(point: gradial_core::ScreenPoint, routes: &[OverlayRoute]) -> Option<usize> {
    let monitors: Vec<_> = routes.iter().map(OverlayRoute::monitor).collect();
    monitor_index_for_point(point, &monitors)
}

fn dispatch_effect(
    effect: InteractionEffect,
    source_qpc: i64,
    active_snapshot: Option<InvocationSnapshot>,
    active_route: Option<usize>,
    dispatcher: &EffectDispatcher<'_>,
) {
    let Some((route_index, route)) = active_overlay_route(active_route, dispatcher.routes) else {
        return;
    };

    match effect {
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
                 config_revision=0 invocation_id={invocation_id} monitor_index={route_index} \
                 geometry={} slice_count={}",
                snapshot.geometry.behavior.as_str(),
                dispatcher.slice_count.as_str()
            );
            dispatcher.timing_markers.emit(
                "open_requested",
                Some(invocation_id),
                source_qpc,
                &format!("geometry={}", snapshot.geometry.behavior.as_str()),
            );
            if let Err(error) =
                route.open(dispatcher.egui_context, dispatcher.timing_markers, snapshot)
            {
                log::error!(
                    target: "renderer",
                    "component=renderer event_code=overlay_wake_request_failed \
                     result_code=failure config_revision=0 invocation_id={invocation_id} \
                     error={error}"
                );
            }
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
            route.set_selection(dispatcher.egui_context, invocation_id, selected_slice);
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
            route.close(dispatcher.egui_context, dispatcher.timing_markers, invocation_id);
        }
    }
}

fn active_overlay_route(
    route_index: Option<usize>,
    routes: &[OverlayRoute],
) -> Option<(usize, &OverlayRoute)> {
    let Some(route_index) = route_index else {
        log::error!(
            target: "renderer",
            "component=renderer event_code=overlay_route_missing result_code=failure \
             config_revision=0"
        );
        return None;
    };
    let route = routes.get(route_index);
    if route.is_none() {
        log::error!(
            target: "renderer",
            "component=renderer event_code=overlay_route_missing result_code=failure \
             config_revision=0 monitor_index={route_index}"
        );
    }
    route.map(|route| (route_index, route))
}

fn log_runtime_event_error(error: TrySendError<RuntimeEvent>) {
    let reason = if matches!(error, TrySendError::Full(_)) { "queue_full" } else { "queue_closed" };
    log::error!(
        target: "interaction",
        "component=interaction event_code=runtime_signal_failed result_code=failure \
         config_revision=0 reason={reason}"
    );
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

const fn heartbeat_is_unresponsive(
    qpc_frequency: i64,
    now: i64,
    heartbeat: i64,
    timeout_qpc: i64,
) -> bool {
    qpc_frequency > 0 && heartbeat > 0 && now.saturating_sub(heartbeat) > timeout_qpc
}

#[cfg(test)]
mod tests {
    use super::heartbeat_is_unresponsive;

    #[test]
    fn heartbeat_faults_only_after_three_missed_intervals() {
        let frequency = 10_000_i64;
        let timeout = frequency * 750 / 1_000;
        assert!(!heartbeat_is_unresponsive(frequency, 8_500, 1_000, timeout));
        assert!(heartbeat_is_unresponsive(frequency, 8_501, 1_000, timeout));
    }

    #[test]
    fn heartbeat_does_not_fault_before_initial_sample() {
        assert!(!heartbeat_is_unresponsive(10_000, 100_000, 0, 7_500));
        assert!(!heartbeat_is_unresponsive(0, 100_000, 1, 7_500));
    }
}
