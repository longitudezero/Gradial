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
    InteractionEffect, InteractionOutcome, InteractionState, InvocationSnapshot, ProfileResolver,
    ProfileTarget, ResolvedMenu, monitor_index_for_point,
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

#[derive(Debug, Default)]
struct ActiveInvocation {
    snapshot: Option<InvocationSnapshot>,
    profile_id: Option<uuid::Uuid>,
    menu: Option<Arc<ResolvedMenu>>,
    route: Option<usize>,
}

impl ActiveInvocation {
    fn clear(&mut self) {
        self.snapshot = None;
        self.profile_id = None;
        self.menu = None;
        self.route = None;
    }
}

#[derive(Debug)]
struct EffectDispatcher<'a> {
    egui_context: &'a eframe::egui::Context,
    timing_markers: &'a TimingMarkers,
    routes: &'a [OverlayRoute],
    config_revision: u64,
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
    config_revision: u64,
    #[cfg(debug_assertions)]
    debug_failure: Arc<AtomicU8>,
}

impl InteractionRuntime {
    pub(crate) fn start(
        egui_context: eframe::egui::Context,
        input_configuration: InputConfiguration,
        timing_markers: Arc<TimingMarkers>,
        routes: Vec<OverlayRoute>,
        profile_resolver: Arc<ProfileResolver>,
        debug_menu_override: Option<Arc<ResolvedMenu>>,
        config_revision: u64,
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
                    &profile_resolver,
                    debug_menu_override.as_ref(),
                    config_revision,
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
            config_revision,
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
                 result_code=failure config_revision={}",
                self.config_revision
            );
        }

        let frequency = query_qpc_frequency();
        let maximum_qpc = self.hook_thread.metrics.maximum_callback_qpc.load(Ordering::Acquire);
        let maximum_microseconds =
            if frequency > 0 { maximum_qpc.saturating_mul(1_000_000) / frequency } else { 0 };
        #[cfg(debug_assertions)]
        log::info!(
            target: "input",
            "component=input event_code=hook_debug_summary result_code=ok config_revision={} \
             mouse_callback_count={} last_mouse_message={} last_mouse_data={}",
            self.config_revision,
            self.hook_thread.metrics.debug.mouse_callback_count.load(Ordering::Acquire),
            self.hook_thread.metrics.debug.last_mouse_message.load(Ordering::Acquire),
            self.hook_thread.metrics.debug.last_mouse_data.load(Ordering::Acquire),
        );
        log::info!(
            target: "input",
            "component=input event_code=hook_thread_stopped result_code=ok config_revision={} \
             maximum_callback_us={maximum_microseconds}",
            self.config_revision
        );
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
    profile_resolver: &ProfileResolver,
    debug_menu_override: Option<&Arc<ResolvedMenu>>,
    config_revision: u64,
) {
    let mut controller = InteractionController::new(GestureConfig::default());
    let mut active = ActiveInvocation::default();
    let mut counters = EventCounters::default();
    let dispatcher = EffectDispatcher { egui_context, timing_markers, routes, config_revision };
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
                &mut active,
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
                dispatch_effect(
                    effect,
                    query_qpc(),
                    active.snapshot.as_ref(),
                    active.profile_id,
                    active.menu.as_ref(),
                    active.route,
                    &dispatcher,
                );
            }
            active.clear();
            runtime_stop_pending = true;
            log::error!(
                target: "input",
                "component=input event_code=input_queue_overflow result_code=failure \
                 config_revision={config_revision} capacity={INPUT_QUEUE_CAPACITY}"
            );
            if let Err(error) = event_sender.try_send(RuntimeEvent::InputFaulted) {
                log_runtime_event_error(error, config_revision);
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
                dispatch_effect(
                    effect,
                    now,
                    active.snapshot.as_ref(),
                    active.profile_id,
                    active.menu.as_ref(),
                    active.route,
                    &dispatcher,
                );
            }
            active.clear();
            runtime_stop_pending = true;
            log::error!(
                target: "input",
                "component=input event_code=hook_thread_unresponsive result_code=failure \
                 config_revision={config_revision} timeout_ms={HOOK_UNRESPONSIVE_AFTER_MS}"
            );
            if let Err(error) = event_sender.try_send(RuntimeEvent::InputRecoveryReady) {
                log_runtime_event_error(error, config_revision);
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
                &mut active,
                &mut counters,
                &dispatcher,
                profile_resolver,
                debug_menu_override,
            );
        }

        if !handled_event {
            thread::sleep(IDLE_POLL_INTERVAL);
        }
    }

    if let Some(effect) = controller.fault(CancelReason::Shutdown) {
        dispatch_effect(
            effect,
            query_qpc(),
            active.snapshot.as_ref(),
            active.profile_id,
            active.menu.as_ref(),
            active.route,
            &dispatcher,
        );
    }
    log_interaction_stopped(&counters, config_revision);
}

fn log_interaction_stopped(counters: &EventCounters, config_revision: u64) {
    log::info!(
        target: "interaction",
        "component=interaction event_code=interaction_thread_stopped result_code=ok \
         config_revision={config_revision} activation_down_events={} pointer_moved_events={} \
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
    active: &mut ActiveInvocation,
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
        dispatch_effect(
            effect,
            query_qpc(),
            active.snapshot.as_ref(),
            active.profile_id,
            active.menu.as_ref(),
            active.route,
            dispatcher,
        );
    }
    active.clear();
    log::info!(
        target: "interaction",
        "component=interaction event_code={event_code} result_code=ok config_revision={}",
        dispatcher.config_revision
    );
    if let Err(error) = event_sender.try_send(event) {
        log_runtime_event_error(error, dispatcher.config_revision);
    }
    platform::request_controller_repaint();
}

fn handle_input_event(
    event: InputEvent,
    controller: &mut InteractionController,
    active: &mut ActiveInvocation,
    counters: &mut EventCounters,
    dispatcher: &EffectDispatcher<'_>,
    profile_resolver: &ProfileResolver,
    debug_menu_override: Option<&Arc<ResolvedMenu>>,
) {
    let source_qpc = event.qpc_timestamp;
    let effect = match event.kind {
        InputEventKind::ActivationDown { geometry, physical_modifiers } => {
            counters.activation_down = counters.activation_down.saturating_add(1);
            begin_invocation(
                event,
                geometry,
                physical_modifiers,
                controller,
                active,
                dispatcher,
                profile_resolver,
                debug_menu_override,
            )
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
                active.snapshot.as_ref().map(|snapshot| snapshot.invocation_id),
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
        dispatch_effect(
            effect,
            source_qpc,
            active.snapshot.as_ref(),
            active.profile_id,
            active.menu.as_ref(),
            active.route,
            dispatcher,
        );
    }
    if matches!(controller.state(), InteractionState::Idle | InteractionState::Faulted) {
        active.clear();
    }
}

#[allow(clippy::too_many_arguments)]
fn begin_invocation(
    event: InputEvent,
    geometry: gradial_core::InvocationGeometry,
    physical_modifiers: gradial_core::PhysicalModifiers,
    controller: &mut InteractionController,
    active: &mut ActiveInvocation,
    dispatcher: &EffectDispatcher<'_>,
    profile_resolver: &ProfileResolver,
    debug_menu_override: Option<&Arc<ResolvedMenu>>,
) -> Option<InteractionEffect> {
    let snapshot =
        capture_snapshot(event, geometry, physical_modifiers, dispatcher.config_revision);
    log::info!(
        target: "interaction",
        "component=interaction event_code=invocation_snapshot_captured result_code=ok \
         config_revision={} invocation_id={} foreground_pid={} \
         process_start_time_available={} executable_path_available={} \
         window_class_available={} window_title_available={} \
         monitor_device_available={} modifier_control={} modifier_alt={} \
         modifier_shift={} activation_binding={:?}",
        snapshot.context.config_revision,
        snapshot.invocation_id,
        snapshot.context.foreground_pid,
        snapshot.context.process_start_time != 0,
        snapshot.context.executable_path.is_some(),
        !snapshot.context.window_class.is_empty(),
        !snapshot.context.window_title.is_empty(),
        !snapshot.context.monitor_device_name.is_empty(),
        snapshot.context.physical_modifiers.control,
        snapshot.context.physical_modifiers.alt,
        snapshot.context.physical_modifiers.shift,
        snapshot.context.activation_binding
    );
    let resolution = profile_resolver.resolve(ProfileTarget {
        executable_path: snapshot.context.executable_path.as_deref(),
        window_class: &snapshot.context.window_class,
        window_title: &snapshot.context.window_title,
    });
    let (menu, menu_source) = debug_menu_override.map_or_else(
        || (Arc::clone(&resolution.menu), "configuration"),
        |menu| (Arc::clone(menu), "debug_override"),
    );
    log::info!(
        target: "profiles",
        "component=profiles event_code=profile_resolved result_code=ok \
         config_revision={} invocation_id={} profile_id={} used_global={} \
         decision={} matching_profile_count={} configured_slice_count={} \
         menu_slice_count={} menu_label_count={} menu_source={menu_source}",
        snapshot.context.config_revision,
        snapshot.invocation_id,
        resolution.profile_id,
        resolution.used_global_profile,
        resolution.decision.as_str(),
        resolution.matching_profiles.len(),
        resolution.menu.slice_count().as_str(),
        menu.slice_count().as_str(),
        menu.labels().len()
    );
    dispatcher.timing_markers.emit(
        "activation_dequeued",
        Some(snapshot.invocation_id),
        event.qpc_timestamp,
        &format!("geometry={}", geometry.behavior.as_str()),
    );
    let effect = controller.activation_down_with_slice_count(snapshot.clone(), menu.slice_count());
    if effect.is_some() {
        active.profile_id = Some(resolution.profile_id);
        active.route = route_for_point(snapshot.cursor_physical_px, dispatcher.routes);
        active.snapshot = Some(snapshot);
        active.menu = Some(menu);
    }
    effect
}

fn capture_snapshot(
    event: InputEvent,
    geometry: gradial_core::InvocationGeometry,
    physical_modifiers: gradial_core::PhysicalModifiers,
    config_revision: u64,
) -> InvocationSnapshot {
    InvocationSnapshot {
        invocation_id: uuid::Uuid::new_v4(),
        qpc_timestamp: event.qpc_timestamp,
        cursor_physical_px: event.point,
        monitor_dpi: geometry.monitor_dpi,
        geometry,
        context: Arc::new(platform::capture_invocation_context(
            event.point,
            physical_modifiers,
            config_revision,
        )),
    }
}

fn route_for_point(point: gradial_core::ScreenPoint, routes: &[OverlayRoute]) -> Option<usize> {
    let monitors: Vec<_> = routes.iter().map(OverlayRoute::monitor).collect();
    monitor_index_for_point(point, &monitors)
}

fn dispatch_effect(
    effect: InteractionEffect,
    source_qpc: i64,
    active_snapshot: Option<&InvocationSnapshot>,
    active_profile_id: Option<uuid::Uuid>,
    active_menu: Option<&Arc<ResolvedMenu>>,
    active_route: Option<usize>,
    dispatcher: &EffectDispatcher<'_>,
) {
    let Some((route_index, route)) =
        active_overlay_route(active_route, dispatcher.routes, dispatcher.config_revision)
    else {
        return;
    };

    match effect {
        InteractionEffect::Open { invocation_id, .. } => {
            dispatch_open(
                invocation_id,
                source_qpc,
                active_snapshot,
                active_profile_id,
                active_menu,
                route_index,
                route,
                dispatcher,
            );
        }
        InteractionEffect::SelectionChanged { invocation_id, selected_slice } => {
            log::debug!(
                target: "interaction",
                "component=interaction event_code=selection_changed result_code=ok \
                 config_revision={} invocation_id={invocation_id} \
                 selected_slice={selected_slice:?}",
                active_snapshot.map_or(dispatcher.config_revision, |snapshot| {
                    snapshot.context.config_revision
                })
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
                         config_revision={} invocation_id={invocation_id} profile_id={} \
                         slice_index={slice_index} action_executed=false",
                        active_snapshot.map_or(dispatcher.config_revision, |snapshot| {
                            snapshot.context.config_revision
                        }),
                        active_profile_id.unwrap_or_default()
                    );
                }
                InteractionOutcome::Cancelled { reason } => {
                    log::info!(
                        target: "interaction",
                        "component=interaction event_code=invocation_cancelled result_code=ok \
                         config_revision={} invocation_id={invocation_id} reason={reason:?}",
                        active_snapshot.map_or(dispatcher.config_revision, |snapshot| {
                            snapshot.context.config_revision
                        })
                    );
                }
            }
            dispatcher.timing_markers.emit("close_requested", Some(invocation_id), source_qpc, "");
            route.close(dispatcher.egui_context, dispatcher.timing_markers, invocation_id);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_open(
    invocation_id: uuid::Uuid,
    source_qpc: i64,
    active_snapshot: Option<&InvocationSnapshot>,
    active_profile_id: Option<uuid::Uuid>,
    active_menu: Option<&Arc<ResolvedMenu>>,
    route_index: usize,
    route: &OverlayRoute,
    dispatcher: &EffectDispatcher<'_>,
) {
    let Some(snapshot) = active_snapshot.filter(|snapshot| snapshot.invocation_id == invocation_id)
    else {
        log::error!(
            target: "interaction",
            "component=interaction event_code=invocation_snapshot_missing \
             result_code=failure config_revision={} invocation_id={invocation_id}",
            dispatcher.config_revision
        );
        return;
    };
    let Some(menu) = active_menu else {
        log::error!(
            target: "interaction",
            "component=interaction event_code=invocation_menu_missing \
             result_code=failure config_revision={} invocation_id={invocation_id}",
            dispatcher.config_revision
        );
        return;
    };
    log::info!(
        target: "interaction",
        "component=interaction event_code=invocation_started result_code=ok \
         config_revision={} invocation_id={invocation_id} profile_id={} \
         monitor_index={route_index} geometry={} slice_count={}",
        snapshot.context.config_revision,
        active_profile_id.unwrap_or_default(),
        snapshot.geometry.behavior.as_str(),
        menu.slice_count().as_str()
    );
    dispatcher.timing_markers.emit(
        "open_requested",
        Some(invocation_id),
        source_qpc,
        &format!("geometry={}", snapshot.geometry.behavior.as_str()),
    );
    if let Err(error) =
        route.open(dispatcher.egui_context, dispatcher.timing_markers, snapshot, Arc::clone(menu))
    {
        log::error!(
            target: "renderer",
            "component=renderer event_code=overlay_wake_request_failed \
             result_code=failure config_revision={} invocation_id={invocation_id} \
             error={error}",
            snapshot.context.config_revision
        );
    }
}

fn active_overlay_route(
    route_index: Option<usize>,
    routes: &[OverlayRoute],
    config_revision: u64,
) -> Option<(usize, &OverlayRoute)> {
    let Some(route_index) = route_index else {
        log::error!(
            target: "renderer",
            "component=renderer event_code=overlay_route_missing result_code=failure \
             config_revision={config_revision}"
        );
        return None;
    };
    let route = routes.get(route_index);
    if route.is_none() {
        log::error!(
            target: "renderer",
            "component=renderer event_code=overlay_route_missing result_code=failure \
             config_revision={config_revision} monitor_index={route_index}"
        );
    }
    route.map(|route| (route_index, route))
}

fn log_runtime_event_error(error: TrySendError<RuntimeEvent>, config_revision: u64) {
    let reason = if matches!(error, TrySendError::Full(_)) { "queue_full" } else { "queue_closed" };
    log::error!(
        target: "interaction",
        "component=interaction event_code=runtime_signal_failed result_code=failure \
         config_revision={config_revision} reason={reason}"
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
