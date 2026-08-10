use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(debug_assertions)]
use std::sync::atomic::AtomicU8;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use gradial_core::{
    ActionPlanCompiler, ActionResult, CancelReason, ContinuousActionSnapshot,
    ContinuousCancelReason, ContinuousSliceMask, ContinuousVisualState,
    DEFAULT_CONTINUOUS_FULL_SCALE_RADIUS_DIP, ExecutionPlan, GestureConfig, InputEvent,
    InputEventKind, InteractionController, InteractionEffect, InteractionOutcome, InteractionState,
    InvocationContext, InvocationSnapshot, MAX_CONTINUOUS_UPDATE_HZ, PermittedApplicationContext,
    PermittedContext, PluginActionId, PluginId, ProfileResolver, ProfileTarget, ResolvedMenu,
    monitor_index_for_point,
};
use rtrb::{Consumer, RingBuffer};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use crate::{
    action_executor::{ActionDispatch, ActionExecutorHandle},
    continuous_executor::{
        ContinuousCompletion, ContinuousCompletionOutcome, ContinuousInvocation,
        ContinuousSubmitError, PluginContinuousBackend, ValidatingInertBackend,
    },
    continuous_study::ContinuousStudySink,
    diagnostics::TimingMarkers,
    input::{HookThread, HookThreadMetrics, InputConfiguration},
    onboarding::OnboardingPractice,
    overlay::OverlayRoute,
    platform,
    plugin_worker::{PluginActionAdmission, PluginLifecycleClient},
    tray,
};

const INPUT_QUEUE_CAPACITY: usize = 4_096;
const RUNTIME_CHANNEL_CAPACITY: usize = 4;
const HOOK_UNRESPONSIVE_AFTER_MS: i64 = 750;
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(1);
const CONTINUOUS_VISUAL_INTERVAL: Duration = Duration::from_micros(4_167);

#[derive(Debug, Default)]
struct EventCounters {
    activation_down: u64,
    pointer_moved: u64,
    raw_pointer_delta: u64,
    continuous_updates_dispatched: u64,
    continuous_updates_prioritized: u64,
    continuous_updates_coalesced: u64,
    activation_up: u64,
    cancel: u64,
}

#[derive(Clone, Copy, Debug)]
struct PendingContinuousUpdate {
    invocation_id: uuid::Uuid,
    visual: ContinuousVisualState,
    source_qpc: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ContinuousVisualKey {
    invocation_id: uuid::Uuid,
    slice_index: usize,
    armed: bool,
}

impl From<PendingContinuousUpdate> for ContinuousVisualKey {
    fn from(update: PendingContinuousUpdate) -> Self {
        Self {
            invocation_id: update.invocation_id,
            slice_index: update.visual.selection.slice_index,
            armed: update.visual.armed,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ReadyContinuousUpdate {
    update: PendingContinuousUpdate,
    transition: bool,
}

#[derive(Debug, Default)]
struct ContinuousUpdateBatch {
    pending: Option<PendingContinuousUpdate>,
    last_dispatched: Option<ContinuousVisualKey>,
    last_dispatched_at: Option<Instant>,
}

impl ContinuousUpdateBatch {
    fn defer(
        &mut self,
        invocation_id: uuid::Uuid,
        visual: ContinuousVisualState,
        source_qpc: i64,
    ) -> bool {
        self.pending
            .replace(PendingContinuousUpdate { invocation_id, visual, source_qpc })
            .is_some()
    }

    fn pending_is_transition(&self) -> bool {
        self.pending
            .is_some_and(|pending| self.last_dispatched != Some(ContinuousVisualKey::from(pending)))
    }

    fn take_ready(&mut self, now: Instant, force: bool) -> Option<ReadyContinuousUpdate> {
        let transition = self.pending_is_transition();
        let interval_elapsed = self
            .last_dispatched_at
            .is_none_or(|last| now.saturating_duration_since(last) >= CONTINUOUS_VISUAL_INTERVAL);
        if !force && !transition && !interval_elapsed {
            return None;
        }
        let update = self.pending.take()?;
        self.last_dispatched = Some(ContinuousVisualKey::from(update));
        self.last_dispatched_at = Some(now);
        Some(ReadyContinuousUpdate { update, transition })
    }

    fn discard_pending(&mut self) -> bool {
        self.pending.take().is_some()
    }

    fn reset(&mut self) -> bool {
        let discarded = self.discard_pending();
        self.last_dispatched = None;
        self.last_dispatched_at = None;
        discarded
    }
}

#[derive(Debug, Default)]
struct ActiveInvocation {
    snapshot: Option<InvocationSnapshot>,
    profile_id: Option<uuid::Uuid>,
    menu: Option<Arc<ResolvedMenu>>,
    action_id: Option<uuid::Uuid>,
    execution_plan: Option<Arc<ExecutionPlan>>,
    route: Option<usize>,
    practice: bool,
    continuous_meter: bool,
    continuous_full_scale_radius_dip: Option<f64>,
    study_trial_id: Option<u32>,
    live_continuous: Option<LiveContinuousInvocation>,
    continuous_actions: Vec<Option<ContinuousActionSnapshot>>,
    continuous_unavailable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveContinuousPhase {
    Active { slice_index: usize },
    Inactive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveContinuousChange {
    None,
    Start,
    Publish,
    Handoff,
    Suspend,
    Resume,
}

#[derive(Debug)]
struct LiveContinuousInvocation {
    invocation: ContinuousInvocation,
    phase: LiveContinuousPhase,
}

#[derive(Debug)]
struct PendingRuntimeCancellation {
    invocation: Option<ContinuousInvocation>,
    reason: ContinuousCancelReason,
    terminal_submitted: bool,
    completion_ready: bool,
}

#[derive(Clone, Copy)]
struct DebugInteractionOptions<'a> {
    menu_override: Option<&'a Arc<ResolvedMenu>>,
    continuous_meter: bool,
    continuous_full_scale_radius_dip: f64,
}

impl ActiveInvocation {
    fn clear(&mut self) {
        self.live_continuous = None;
        self.continuous_actions.clear();
        self.snapshot = None;
        self.profile_id = None;
        self.menu = None;
        self.action_id = None;
        self.execution_plan = None;
        self.route = None;
        self.practice = false;
        self.continuous_meter = false;
        self.continuous_full_scale_radius_dip = None;
        self.study_trial_id = None;
        self.continuous_unavailable = false;
    }
}

#[derive(Debug)]
struct EffectDispatcher<'a> {
    egui_context: &'a eframe::egui::Context,
    timing_markers: &'a TimingMarkers,
    routes: &'a [OverlayRoute],
    action_plan_compiler: &'a ActionPlanCompiler,
    action_executor: &'a ActionExecutorHandle,
    onboarding_practice: &'a OnboardingPractice,
    continuous_study: &'a ContinuousStudySink,
    plugin_lifecycle: &'a PluginLifecycleClient,
    qpc_frequency: i64,
    config_revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeControl {
    DisplayTopologyChanged,
    ReregisterInput,
    CancelContinuous(ContinuousCancelReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeEvent {
    DisplayRebuildReady,
    InputRecoveryReady,
    InputFaulted,
    ContinuousCancellationReady(ContinuousCancelReason),
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        egui_context: eframe::egui::Context,
        input_configuration: InputConfiguration,
        timing_markers: Arc<TimingMarkers>,
        routes: Vec<OverlayRoute>,
        profile_resolver: Arc<ProfileResolver>,
        action_plan_compiler: Arc<ActionPlanCompiler>,
        action_executor: ActionExecutorHandle,
        plugin_lifecycle: PluginLifecycleClient,
        onboarding_practice: OnboardingPractice,
        continuous_study: ContinuousStudySink,
        debug_menu_override: Option<Arc<ResolvedMenu>>,
        debug_continuous_meter: bool,
        gesture_config: GestureConfig,
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
                    &action_plan_compiler,
                    &action_executor,
                    &plugin_lifecycle,
                    &onboarding_practice,
                    &continuous_study,
                    debug_menu_override.as_ref(),
                    debug_continuous_meter,
                    gesture_config,
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

    pub(crate) fn cancel_continuous(&self, reason: ContinuousCancelReason) -> Result<()> {
        self.control_sender
            .try_send(RuntimeControl::CancelContinuous(reason))
            .context("could not request continuous cancellation")
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
        let maximum_microseconds = if frequency > 0 {
            maximum_qpc.saturating_mul(1_000_000).saturating_add(frequency - 1) / frequency
        } else {
            0
        };
        let callback_summary = self.hook_thread.metrics.callback_histogram.summary();
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
             callback_count={} p99_callback_us={} maximum_callback_us={maximum_microseconds}",
            self.config_revision,
            callback_summary.count,
            callback_summary.p99_microseconds,
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
    action_plan_compiler: &ActionPlanCompiler,
    action_executor: &ActionExecutorHandle,
    plugin_lifecycle: &PluginLifecycleClient,
    onboarding_practice: &OnboardingPractice,
    continuous_study: &ContinuousStudySink,
    debug_menu_override: Option<&Arc<ResolvedMenu>>,
    debug_continuous_meter: bool,
    gesture_config: GestureConfig,
    config_revision: u64,
) {
    let default_continuous_full_scale_radius_dip = gesture_config.continuous_full_scale_radius_dip;
    let mut controller = InteractionController::new(gesture_config);
    let mut active = ActiveInvocation::default();
    let mut counters = EventCounters::default();
    let qpc_frequency = query_qpc_frequency();
    let dispatcher = EffectDispatcher {
        egui_context,
        timing_markers,
        routes,
        action_plan_compiler,
        action_executor,
        onboarding_practice,
        continuous_study,
        plugin_lifecycle,
        qpc_frequency,
        config_revision,
    };
    let debug_options = DebugInteractionOptions {
        menu_override: debug_menu_override,
        continuous_meter: debug_continuous_meter,
        continuous_full_scale_radius_dip: default_continuous_full_scale_radius_dip,
    };
    let hook_timeout_qpc = qpc_frequency.saturating_mul(HOOK_UNRESPONSIVE_AFTER_MS) / 1_000;
    let mut runtime_stop_pending = false;
    let mut pending_runtime_cancellation = None;
    let mut continuous_batch = ContinuousUpdateBatch::default();

    while !shutdown.load(Ordering::Acquire) {
        #[cfg(debug_assertions)]
        let simulated_failure = debug_failure.swap(0, Ordering::AcqRel);
        #[cfg(not(debug_assertions))]
        let simulated_failure = 0_u8;

        if let Ok(control) = controls.try_recv() {
            let stops_runtime = handle_runtime_control(
                control,
                &mut controller,
                &mut active,
                &mut pending_runtime_cancellation,
                event_sender,
                &dispatcher,
            );
            if stops_runtime {
                if continuous_batch.reset() {
                    counters.continuous_updates_coalesced =
                        counters.continuous_updates_coalesced.saturating_add(1);
                }
                runtime_stop_pending = true;
            }
        }

        poll_runtime_continuous_cancellation(
            &mut pending_runtime_cancellation,
            event_sender,
            config_revision,
        );

        if runtime_stop_pending {
            thread::sleep(IDLE_POLL_INTERVAL);
            continue;
        }

        if simulated_failure == 2 || metrics.queue_overflow.swap(false, Ordering::AcqRel) {
            accepting_input.store(false, Ordering::Release);
            if let Some(effect) = controller.fault(CancelReason::InputQueueOverflow) {
                dispatch_effect(effect, query_qpc(), &mut active, &dispatcher);
            }
            if continuous_batch.reset() {
                counters.continuous_updates_coalesced =
                    counters.continuous_updates_coalesced.saturating_add(1);
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
                dispatch_effect(effect, now, &mut active, &dispatcher);
            }
            if continuous_batch.reset() {
                counters.continuous_updates_coalesced =
                    counters.continuous_updates_coalesced.saturating_add(1);
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
            let source_qpc = event.qpc_timestamp;
            let effect = process_input_event(
                event,
                &mut controller,
                &mut active,
                &mut counters,
                &dispatcher,
                profile_resolver,
                debug_options,
            );
            if let Some(effect) = effect {
                match effect {
                    InteractionEffect::ContinuousChanged { invocation_id, visual } => {
                        if continuous_batch.defer(invocation_id, visual, source_qpc) {
                            counters.continuous_updates_coalesced =
                                counters.continuous_updates_coalesced.saturating_add(1);
                        }
                        if continuous_batch.pending_is_transition() {
                            dispatch_pending_continuous(
                                &mut continuous_batch,
                                &mut active,
                                &dispatcher,
                                &mut counters,
                                false,
                            );
                        }
                    }
                    InteractionEffect::SelectionChanged { .. } => {
                        if continuous_batch.discard_pending() {
                            counters.continuous_updates_coalesced =
                                counters.continuous_updates_coalesced.saturating_add(1);
                        }
                        dispatch_effect(effect, source_qpc, &mut active, &dispatcher);
                    }
                    _ => {
                        dispatch_pending_continuous(
                            &mut continuous_batch,
                            &mut active,
                            &dispatcher,
                            &mut counters,
                            true,
                        );
                        dispatch_effect(effect, source_qpc, &mut active, &dispatcher);
                    }
                }
            }
            if matches!(controller.state(), InteractionState::Idle | InteractionState::Faulted) {
                if continuous_batch.reset() {
                    counters.continuous_updates_coalesced =
                        counters.continuous_updates_coalesced.saturating_add(1);
                }
                active.clear();
            }
        }
        dispatch_pending_continuous(
            &mut continuous_batch,
            &mut active,
            &dispatcher,
            &mut counters,
            false,
        );

        if !handled_event {
            thread::sleep(IDLE_POLL_INTERVAL);
        }
    }

    dispatch_pending_continuous(
        &mut continuous_batch,
        &mut active,
        &dispatcher,
        &mut counters,
        true,
    );
    if let Some(effect) = controller.fault(CancelReason::Shutdown) {
        dispatch_effect(effect, query_qpc(), &mut active, &dispatcher);
    }
    log_interaction_stopped(&counters, config_revision);
}

fn log_interaction_stopped(counters: &EventCounters, config_revision: u64) {
    log::info!(
        target: "interaction",
        "component=interaction event_code=interaction_thread_stopped result_code=ok \
         config_revision={config_revision} activation_down_events={} pointer_moved_events={} \
         activation_up_events={} raw_pointer_delta_events={} \
         continuous_updates_dispatched={} continuous_updates_prioritized={} \
         continuous_updates_coalesced={} cancel_events={}",
        counters.activation_down,
        counters.pointer_moved,
        counters.activation_up,
        counters.raw_pointer_delta,
        counters.continuous_updates_dispatched,
        counters.continuous_updates_prioritized,
        counters.continuous_updates_coalesced,
        counters.cancel
    );
}

fn dispatch_pending_continuous(
    batch: &mut ContinuousUpdateBatch,
    active: &mut ActiveInvocation,
    dispatcher: &EffectDispatcher<'_>,
    counters: &mut EventCounters,
    force: bool,
) {
    let Some(ready) = batch.take_ready(Instant::now(), force) else {
        return;
    };
    counters.continuous_updates_dispatched =
        counters.continuous_updates_dispatched.saturating_add(1);
    if ready.transition {
        counters.continuous_updates_prioritized =
            counters.continuous_updates_prioritized.saturating_add(1);
    }
    dispatch_continuous_effect(ready.update, ready.transition, active, dispatcher);
}

fn handle_runtime_control(
    control: RuntimeControl,
    controller: &mut InteractionController,
    active: &mut ActiveInvocation,
    pending_runtime_cancellation: &mut Option<PendingRuntimeCancellation>,
    event_sender: &Sender<RuntimeEvent>,
    dispatcher: &EffectDispatcher<'_>,
) -> bool {
    if let RuntimeControl::CancelContinuous(reason) = control {
        begin_runtime_continuous_cancellation(
            reason,
            active,
            pending_runtime_cancellation,
            dispatcher.config_revision,
        );
        return false;
    }
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
        RuntimeControl::CancelContinuous(_) => unreachable!(),
    };
    if let Some(effect) = controller.fault(reason) {
        dispatch_effect(effect, query_qpc(), active, dispatcher);
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
    true
}

fn begin_runtime_continuous_cancellation(
    reason: ContinuousCancelReason,
    active: &mut ActiveInvocation,
    pending: &mut Option<PendingRuntimeCancellation>,
    config_revision: u64,
) {
    reap_live_continuous(active, config_revision);
    active.continuous_unavailable = true;
    let Some(live) = active.live_continuous.take() else {
        *pending = Some(PendingRuntimeCancellation {
            invocation: None,
            reason,
            terminal_submitted: true,
            completion_ready: true,
        });
        return;
    };
    let terminal_submitted = match live.invocation.handle().cancel(reason) {
        Ok(()) | Err(ContinuousSubmitError::Closed) => true,
        Err(ContinuousSubmitError::QueueFull) => false,
        Err(ContinuousSubmitError::InvalidValue) => unreachable!(),
    };
    *pending = Some(PendingRuntimeCancellation {
        invocation: Some(live.invocation),
        reason,
        terminal_submitted,
        completion_ready: false,
    });
}

fn poll_runtime_continuous_cancellation(
    pending: &mut Option<PendingRuntimeCancellation>,
    event_sender: &Sender<RuntimeEvent>,
    config_revision: u64,
) {
    let Some(cancellation) = pending.as_mut() else {
        return;
    };
    if !cancellation.terminal_submitted {
        let invocation = cancellation.invocation.as_ref().expect("active cancellation");
        cancellation.terminal_submitted = match invocation.handle().cancel(cancellation.reason) {
            Ok(()) | Err(ContinuousSubmitError::Closed) => true,
            Err(ContinuousSubmitError::QueueFull) => false,
            Err(ContinuousSubmitError::InvalidValue) => unreachable!(),
        };
    }
    if !cancellation.completion_ready {
        let invocation = cancellation.invocation.as_mut().expect("active cancellation");
        let Some(completion) = invocation.try_completion() else {
            return;
        };
        log_live_continuous_completion(&completion, config_revision);
        cancellation.completion_ready = true;
    }
    match event_sender.try_send(RuntimeEvent::ContinuousCancellationReady(cancellation.reason)) {
        Ok(()) => *pending = None,
        Err(TrySendError::Full(_)) => {}
        Err(error @ TrySendError::Disconnected(_)) => {
            log_runtime_event_error(error, config_revision);
            *pending = None;
        }
    }
}

fn process_input_event(
    event: InputEvent,
    controller: &mut InteractionController,
    active: &mut ActiveInvocation,
    counters: &mut EventCounters,
    dispatcher: &EffectDispatcher<'_>,
    profile_resolver: &ProfileResolver,
    debug_options: DebugInteractionOptions<'_>,
) -> Option<InteractionEffect> {
    let source_qpc = event.qpc_timestamp;
    match event.kind {
        InputEventKind::ActivationDown { geometry, physical_modifiers, activation_binding } => {
            counters.activation_down = counters.activation_down.saturating_add(1);
            begin_invocation(
                event,
                geometry,
                physical_modifiers,
                activation_binding,
                controller,
                active,
                dispatcher,
                profile_resolver,
                debug_options.menu_override,
                debug_options.continuous_meter,
                debug_options.continuous_full_scale_radius_dip,
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
            let effect = controller.activation_up();
            prepare_selected_action(
                effect,
                active,
                dispatcher.action_plan_compiler,
                dispatcher.config_revision,
            );
            effect
        }
        InputEventKind::Cancel(reason) => {
            counters.cancel = counters.cancel.saturating_add(1);
            controller.cancel(reason)
        }
    }
}

fn prepare_selected_action(
    effect: Option<InteractionEffect>,
    active: &mut ActiveInvocation,
    action_plan_compiler: &ActionPlanCompiler,
    active_config_revision: u64,
) {
    if active.practice || active.continuous_meter {
        return;
    }
    let Some(InteractionEffect::Close {
        outcome: InteractionOutcome::Selected { slice_index },
        ..
    }) = effect
    else {
        return;
    };
    let Some(action_id) = active.menu.as_ref().and_then(|menu| menu.action_id(slice_index)) else {
        return;
    };
    active.action_id = Some(action_id);
    let invocation_config_revision = active
        .snapshot
        .as_ref()
        .map_or(active_config_revision, |snapshot| snapshot.context.config_revision);
    match action_plan_compiler.compile_plan(action_id, invocation_config_revision) {
        Ok(plan) => active.execution_plan = Some(Arc::new(plan)),
        Err(error) => {
            log::error!(
                target: "actions",
                "component=actions event_code=action_plan_compile_failed \
                 result_code=failure error_code=config_invalid \
                 config_revision={invocation_config_revision} action_id={action_id} error={error}"
            );
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn begin_invocation(
    event: InputEvent,
    geometry: gradial_core::InvocationGeometry,
    physical_modifiers: gradial_core::PhysicalModifiers,
    activation_binding: gradial_core::ActivationBindingId,
    controller: &mut InteractionController,
    active: &mut ActiveInvocation,
    dispatcher: &EffectDispatcher<'_>,
    profile_resolver: &ProfileResolver,
    debug_menu_override: Option<&Arc<ResolvedMenu>>,
    debug_continuous_meter: bool,
    debug_continuous_full_scale_radius_dip: f64,
) -> Option<InteractionEffect> {
    let snapshot = capture_snapshot(
        event,
        geometry,
        physical_modifiers,
        activation_binding,
        dispatcher.config_revision,
    );
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
    let study_setup = dispatcher.continuous_study.invocation_if_enabled();
    let study_trial_id = study_setup.as_ref().map(|setup| setup.trial_id);
    let continuous_full_scale_radius_dip = study_setup
        .as_ref()
        .map_or(debug_continuous_full_scale_radius_dip, |setup| setup.full_scale_radius_dip);
    let practice_menu = dispatcher.onboarding_practice.menu_if_enabled();
    let (menu, menu_source, practice_active) = if let Some(setup) = study_setup {
        (setup.menu, "continuous_study", false)
    } else if let Some(menu) = practice_menu {
        (menu, "onboarding_practice", true)
    } else if let Some(menu) = debug_menu_override {
        (Arc::clone(menu), "debug_override", false)
    } else {
        (Arc::clone(&resolution.menu), "configuration", false)
    };
    let continuous_meter = debug_continuous_meter && !practice_active;
    if !controller.set_continuous_full_scale_radius_dip(continuous_full_scale_radius_dip) {
        log::error!(
            target: "interaction",
            "component=interaction event_code=continuous_geometry_rejected result_code=failure \
             config_revision={} invocation_id={} \
             continuous_full_scale_radius_dip={continuous_full_scale_radius_dip}",
            snapshot.context.config_revision,
            snapshot.invocation_id
        );
        return None;
    }
    log::info!(
        target: "profiles",
        "component=profiles event_code=profile_resolved result_code=ok \
         config_revision={} invocation_id={} profile_id={} used_global={} \
         decision={} matching_profile_count={} configured_slice_count={} \
         menu_slice_count={} menu_label_count={} menu_source={menu_source} \
         onboarding_practice={practice_active} continuous_meter={continuous_meter} \
         continuous_study_trial={study_trial_id:?} \
         continuous_full_scale_radius_dip={continuous_full_scale_radius_dip}",
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
    let continuous_actions = freeze_continuous_actions(
        &menu,
        dispatcher.action_plan_compiler,
        dispatcher.plugin_lifecycle,
        snapshot.context.as_ref(),
        snapshot.context.config_revision,
        continuous_meter,
    );
    let continuous_bits =
        continuous_actions.iter().enumerate().fold(0_u8, |bits, (slice_index, action)| {
            if action.is_some() { bits | (1_u8 << slice_index) } else { bits }
        });
    let continuous_slices = ContinuousSliceMask::from_bits(menu.slice_count(), continuous_bits);
    let effect = controller.activation_down_with_behavior(
        snapshot.clone(),
        menu.slice_count(),
        continuous_slices,
    );
    if effect.is_some() {
        active.profile_id = Some(resolution.profile_id);
        active.route = route_for_point(snapshot.cursor_physical_px, dispatcher.routes);
        active.snapshot = Some(snapshot);
        active.menu = Some(menu);
        active.continuous_actions = continuous_actions;
        active.practice = practice_active;
        active.continuous_meter = continuous_meter;
        active.continuous_full_scale_radius_dip = Some(continuous_full_scale_radius_dip);
        active.study_trial_id = study_trial_id;
    }
    effect
}

fn freeze_continuous_actions(
    menu: &ResolvedMenu,
    compiler: &ActionPlanCompiler,
    plugin_lifecycle: &PluginLifecycleClient,
    invocation_context: &InvocationContext,
    config_revision: u64,
    continuous_meter: bool,
) -> Vec<Option<ContinuousActionSnapshot>> {
    (0..menu.slice_count().get())
        .map(|slice_index| {
            if continuous_meter {
                return inert_action_snapshot(slice_index).ok();
            }
            let action_id = menu.continuous_action_id(slice_index)?;
            let binding = match compiler.compile_continuous_action(action_id, config_revision) {
                Ok(binding) => binding,
                Err(error) => {
                    log::error!(
                        target: "continuous",
                        "component=continuous event_code=continuous_action_compile_failed \
                         result_code=failure error_code=config_invalid \
                         config_revision={config_revision} action_id={action_id} error={error}"
                    );
                    return None;
                }
            };
            let Some(admission) =
                plugin_lifecycle.action_admission(binding.plugin_id(), binding.action_id())
            else {
                log::debug!(
                    target: "continuous",
                    "component=continuous event_code=continuous_action_unavailable \
                     result_code=unavailable config_revision={config_revision} \
                     action_id={action_id} plugin_id={} plugin_action_id={}",
                    binding.plugin_id().as_str(),
                    binding.action_id().as_str()
                );
                return None;
            };
            let context = permitted_plugin_context(invocation_context, admission);
            match binding.into_snapshot(context, admission.update_hz()) {
                Ok(snapshot) => Some(snapshot),
                Err(error) => {
                    log::error!(
                        target: "continuous",
                        "component=continuous event_code=continuous_snapshot_failed \
                         result_code=failure error_code=config_invalid \
                         config_revision={config_revision} action_id={action_id} error={error}"
                    );
                    None
                }
            }
        })
        .collect()
}

fn permitted_plugin_context(
    invocation: &InvocationContext,
    admission: PluginActionAdmission,
) -> PermittedContext {
    let application = if admission.permits_context_application() {
        invocation.executable_path.clone().map(|executable_path| {
            PermittedApplicationContext::new(executable_path, invocation.window_class.clone())
        })
    } else {
        None
    };
    let window_title =
        admission.permits_context_window_title().then(|| invocation.window_title.clone());
    PermittedContext::new(application, window_title)
}

fn capture_snapshot(
    event: InputEvent,
    geometry: gradial_core::InvocationGeometry,
    physical_modifiers: gradial_core::PhysicalModifiers,
    activation_binding: gradial_core::ActivationBindingId,
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
            activation_binding,
            config_revision,
        )),
    }
}

fn route_for_point(point: gradial_core::ScreenPoint, routes: &[OverlayRoute]) -> Option<usize> {
    let monitors: Vec<_> = routes.iter().map(OverlayRoute::monitor).collect();
    monitor_index_for_point(point, &monitors)
}

fn dispatch_continuous_effect(
    update: PendingContinuousUpdate,
    transition: bool,
    active: &mut ActiveInvocation,
    dispatcher: &EffectDispatcher<'_>,
) {
    let Some((_, route)) =
        active_overlay_route(active.route, dispatcher.routes, dispatcher.config_revision)
    else {
        return;
    };
    update_live_continuous(update.invocation_id, update.visual, active, dispatcher);
    dispatch_continuous_changed(
        update.invocation_id,
        update.visual,
        update.source_qpc,
        active.snapshot.as_ref(),
        route,
        dispatcher,
        transition,
    );
}

fn dispatch_effect(
    effect: InteractionEffect,
    source_qpc: i64,
    active: &mut ActiveInvocation,
    dispatcher: &EffectDispatcher<'_>,
) {
    let Some((route_index, route)) =
        active_overlay_route(active.route, dispatcher.routes, dispatcher.config_revision)
    else {
        return;
    };

    match effect {
        InteractionEffect::Open { invocation_id, .. } => {
            dispatch_open(
                invocation_id,
                source_qpc,
                active.snapshot.as_ref(),
                active.profile_id,
                active.menu.as_ref(),
                active.continuous_meter,
                active.continuous_full_scale_radius_dip,
                route_index,
                route,
                dispatcher,
            );
            if active.practice {
                dispatcher.onboarding_practice.opened();
                platform::request_controller_repaint();
            }
        }
        InteractionEffect::SelectionChanged { invocation_id, selected_slice } => {
            suspend_live_continuous(active, ContinuousCancelReason::ReturnedToDiscrete, dispatcher);
            dispatch_selection_changed(
                invocation_id,
                selected_slice,
                source_qpc,
                active.snapshot.as_ref(),
                route,
                dispatcher,
            );
        }
        InteractionEffect::ContinuousChanged { invocation_id, visual } => {
            update_live_continuous(invocation_id, visual, active, dispatcher);
            dispatch_continuous_changed(
                invocation_id,
                visual,
                source_qpc,
                active.snapshot.as_ref(),
                route,
                dispatcher,
                true,
            );
        }
        InteractionEffect::Close { invocation_id, outcome } => {
            dispatch_close(invocation_id, outcome, source_qpc, active, route, dispatcher);
        }
    }
}

fn update_live_continuous(
    invocation_id: uuid::Uuid,
    visual: ContinuousVisualState,
    active: &mut ActiveInvocation,
    dispatcher: &EffectDispatcher<'_>,
) {
    reap_live_continuous(active, dispatcher.config_revision);
    if active.continuous_unavailable {
        return;
    }
    let phase = active.live_continuous.as_ref().map(|live| live.phase);
    let change = classify_live_continuous_change(phase, visual);
    match change {
        LiveContinuousChange::None => return,
        LiveContinuousChange::Suspend => {
            suspend_live_continuous(active, ContinuousCancelReason::ReturnedToDiscrete, dispatcher);
            return;
        }
        LiveContinuousChange::Start
        | LiveContinuousChange::Publish
        | LiveContinuousChange::Handoff
        | LiveContinuousChange::Resume => {}
    }

    let Some(invocation_snapshot) = active.snapshot.as_ref() else {
        active.continuous_unavailable = true;
        return;
    };
    let target = Arc::clone(&invocation_snapshot.context);
    let config_revision = target.config_revision;
    if change == LiveContinuousChange::Publish {
        if let Some(live) = active.live_continuous.as_ref()
            && let Err(error) = live.invocation.handle().publish(visual.selection.value)
        {
            log_continuous_submit_failure(invocation_id, config_revision, "value", error);
        }
        return;
    }

    let Some(action_snapshot) =
        active.continuous_actions.get(visual.selection.slice_index).cloned().flatten()
    else {
        active.continuous_unavailable = true;
        log::error!(
            target: "continuous",
            "component=continuous event_code=frozen_snapshot_missing result_code=failure \
             error_code=config_invalid config_revision={} invocation_id={invocation_id} \
             slice_index={}",
            invocation_snapshot.context.config_revision,
            visual.selection.slice_index
        );
        return;
    };
    if change == LiveContinuousChange::Start {
        start_live_continuous(
            invocation_id,
            visual,
            action_snapshot,
            target,
            config_revision,
            active,
            dispatcher,
        );
        return;
    }
    let submit_result =
        match change {
            LiveContinuousChange::Handoff => active.live_continuous.as_ref().map(|live| {
                live.invocation.handle().handoff(action_snapshot, visual.selection.value)
            }),
            LiveContinuousChange::Resume => active.live_continuous.as_ref().map(|live| {
                live.invocation.handle().resume(action_snapshot, visual.selection.value)
            }),
            LiveContinuousChange::None
            | LiveContinuousChange::Start
            | LiveContinuousChange::Publish
            | LiveContinuousChange::Suspend => None,
        };

    let Some(submit_result) = submit_result else {
        return;
    };
    match submit_result {
        Ok(()) => {
            if let Some(live) = active.live_continuous.as_mut() {
                live.phase =
                    LiveContinuousPhase::Active { slice_index: visual.selection.slice_index };
            }
        }
        Err(error) => log_continuous_submit_failure(
            invocation_id,
            config_revision,
            "value_or_direction",
            error,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn start_live_continuous(
    invocation_id: uuid::Uuid,
    visual: ContinuousVisualState,
    action_snapshot: ContinuousActionSnapshot,
    target: Arc<gradial_core::InvocationContext>,
    config_revision: u64,
    active: &mut ActiveInvocation,
    dispatcher: &EffectDispatcher<'_>,
) {
    let start = if active.continuous_meter {
        ContinuousInvocation::start(
            dispatcher.action_executor,
            invocation_id,
            action_snapshot,
            visual.selection.value,
            ValidatingInertBackend::new(invocation_id, target),
        )
    } else {
        ContinuousInvocation::start(
            dispatcher.action_executor,
            invocation_id,
            action_snapshot,
            visual.selection.value,
            PluginContinuousBackend::new(
                invocation_id,
                target,
                dispatcher.plugin_lifecycle.clone(),
            ),
        )
    };
    match start {
        Ok(invocation) => {
            active.live_continuous = Some(LiveContinuousInvocation {
                invocation,
                phase: LiveContinuousPhase::Active { slice_index: visual.selection.slice_index },
            });
            log::info!(
                target: "continuous",
                "component=continuous event_code=live_session_started result_code=ok \
                 config_revision={config_revision} invocation_id={invocation_id} \
                 slice_index={} normalized_value={:.6} backend={}",
                visual.selection.slice_index,
                visual.selection.value,
                if active.continuous_meter { "inert" } else { "plugin" }
            );
        }
        Err(result) => {
            active.continuous_unavailable = true;
            log_continuous_start_rejection(invocation_id, config_revision, &result);
        }
    }
}

const fn classify_live_continuous_change(
    phase: Option<LiveContinuousPhase>,
    visual: ContinuousVisualState,
) -> LiveContinuousChange {
    if !visual.armed {
        return match phase {
            Some(LiveContinuousPhase::Active { .. }) => LiveContinuousChange::Suspend,
            None | Some(LiveContinuousPhase::Inactive) => LiveContinuousChange::None,
        };
    }
    match phase {
        None => LiveContinuousChange::Start,
        Some(LiveContinuousPhase::Inactive) => LiveContinuousChange::Resume,
        Some(LiveContinuousPhase::Active { slice_index })
            if slice_index == visual.selection.slice_index =>
        {
            LiveContinuousChange::Publish
        }
        Some(LiveContinuousPhase::Active { .. }) => LiveContinuousChange::Handoff,
    }
}

fn suspend_live_continuous(
    active: &mut ActiveInvocation,
    reason: ContinuousCancelReason,
    dispatcher: &EffectDispatcher<'_>,
) {
    reap_live_continuous(active, dispatcher.config_revision);
    let Some(live) = active.live_continuous.as_mut() else {
        return;
    };
    if live.phase == LiveContinuousPhase::Inactive {
        return;
    }
    let handle = live.invocation.handle();
    match handle.suspend(reason) {
        Ok(()) => live.phase = LiveContinuousPhase::Inactive,
        Err(error) => log_continuous_submit_failure(
            active.snapshot.as_ref().map_or(uuid::Uuid::nil(), |snapshot| snapshot.invocation_id),
            active
                .snapshot
                .as_ref()
                .map_or(dispatcher.config_revision, |snapshot| snapshot.context.config_revision),
            "suspend",
            error,
        ),
    }
}

fn finalize_live_continuous(
    outcome: InteractionOutcome,
    active: &mut ActiveInvocation,
    dispatcher: &EffectDispatcher<'_>,
) -> bool {
    reap_live_continuous(active, dispatcher.config_revision);
    let Some(live) = active.live_continuous.as_ref() else {
        return false;
    };
    let handle = live.invocation.handle();
    let result = match outcome {
        InteractionOutcome::ContinuousCommitted { value, .. } => handle.commit(value),
        InteractionOutcome::Selected { .. } => {
            match (active.execution_plan.as_ref(), active.snapshot.as_ref()) {
                (Some(plan), Some(snapshot)) if !active.continuous_meter => {
                    handle.finish_discrete(Arc::clone(plan), Arc::clone(&snapshot.context))
                }
                _ => handle.cancel(ContinuousCancelReason::ReturnedToDiscrete),
            }
        }
        InteractionOutcome::Cancelled { reason } => handle.cancel(map_cancel_reason(reason)),
    };
    let transferred = matches!(outcome, InteractionOutcome::Selected { .. })
        && active.execution_plan.is_some()
        && !active.continuous_meter
        && result.is_ok();
    if let Err(error) = result {
        let invocation_id =
            active.snapshot.as_ref().map_or(uuid::Uuid::nil(), |snapshot| snapshot.invocation_id);
        let config_revision = active
            .snapshot
            .as_ref()
            .map_or(dispatcher.config_revision, |snapshot| snapshot.context.config_revision);
        log_continuous_submit_failure(invocation_id, config_revision, "terminal", error);
    }
    transferred
}

fn reap_live_continuous(active: &mut ActiveInvocation, fallback_revision: u64) {
    let completion =
        active.live_continuous.as_mut().and_then(|live| live.invocation.try_completion());
    let Some(completion) = completion else {
        return;
    };
    log_live_continuous_completion(&completion, fallback_revision);
    active.live_continuous = None;
    active.continuous_unavailable = true;
}

fn inert_action_snapshot(
    slice_index: usize,
) -> Result<ContinuousActionSnapshot, gradial_core::ContinuousProtocolError> {
    ContinuousActionSnapshot::new(
        PluginId::new("org.gradial.inert-meter")?,
        PluginActionId::new(format!("slice-{slice_index}"))?,
        PermittedContext::default(),
        BTreeMap::default(),
        MAX_CONTINUOUS_UPDATE_HZ,
    )
}

fn map_cancel_reason(reason: CancelReason) -> ContinuousCancelReason {
    match reason {
        CancelReason::Escape => ContinuousCancelReason::Escape,
        CancelReason::OtherMouseButton => ContinuousCancelReason::OtherMouseButton,
        CancelReason::NoSelection | CancelReason::ReturnedToCenter => {
            ContinuousCancelReason::ReturnedToDiscrete
        }
        CancelReason::InputQueueOverflow => ContinuousCancelReason::InputQueueOverflow,
        CancelReason::HookUnresponsive => ContinuousCancelReason::HookUnresponsive,
        CancelReason::InputReregistration => ContinuousCancelReason::InputReregistration,
        CancelReason::DisplayTopologyChange => ContinuousCancelReason::DisplayTopologyChange,
        CancelReason::Shutdown => ContinuousCancelReason::Shutdown,
    }
}

fn log_continuous_start_rejection(
    invocation_id: uuid::Uuid,
    config_revision: u64,
    result: &ActionResult,
) {
    let error_code = match result {
        ActionResult::Failure { code, .. } => code.as_str(),
        ActionResult::Cancelled => "cancelled",
        ActionResult::Success => "invalid_executor_result",
    };
    log::warn!(
        target: "continuous",
        "component=continuous event_code=inert_session_start_rejected result_code=failure \
         error_code={error_code} config_revision={config_revision} \
         invocation_id={invocation_id}"
    );
}

fn log_continuous_submit_failure(
    invocation_id: uuid::Uuid,
    config_revision: u64,
    operation: &str,
    error: ContinuousSubmitError,
) {
    log::warn!(
        target: "continuous",
        "component=continuous event_code=inert_session_submit_failed result_code=failure \
         config_revision={config_revision} invocation_id={invocation_id} \
         operation={operation} error={error}"
    );
}

fn log_live_continuous_completion(completion: &ContinuousCompletion, fallback_revision: u64) {
    let result_code = match &completion.outcome {
        ContinuousCompletionOutcome::Committed
        | ContinuousCompletionOutcome::DiscreteDispatched => "ok",
        ContinuousCompletionOutcome::Cancelled(_) => "cancelled",
        ContinuousCompletionOutcome::BeginRejected
        | ContinuousCompletionOutcome::DiscreteRejected(_)
        | ContinuousCompletionOutcome::Failed => "failure",
    };
    log::info!(
        target: "continuous",
        "component=continuous event_code=live_session_completed result_code={result_code} \
         config_revision={fallback_revision} invocation_id={} outcome={:?} \
         backend_failure={:?}",
        completion.invocation_id,
        completion.outcome,
        completion.backend_failure
    );
}

fn dispatch_close(
    invocation_id: uuid::Uuid,
    outcome: InteractionOutcome,
    source_qpc: i64,
    active: &mut ActiveInvocation,
    route: &OverlayRoute,
    dispatcher: &EffectDispatcher<'_>,
) {
    let live_discrete_transfer = finalize_live_continuous(outcome, active, dispatcher);
    let active_snapshot = active.snapshot.as_ref();
    let config_revision = active_snapshot
        .map_or(dispatcher.config_revision, |snapshot| snapshot.context.config_revision);
    if let Some(snapshot) = active_snapshot {
        dispatcher.continuous_study.completed(
            active.study_trial_id,
            snapshot,
            outcome,
            qpc_elapsed_milliseconds(snapshot.qpc_timestamp, source_qpc, dispatcher.qpc_frequency),
        );
    }
    match outcome {
        InteractionOutcome::Selected { slice_index } => {
            let action_assigned = active.action_id.is_some();
            let plan_compiled = active.execution_plan.is_some();
            log::info!(
                target: "interaction",
                "component=interaction event_code=slice_selected result_code=ok \
                 config_revision={config_revision} invocation_id={invocation_id} profile_id={} \
                 slice_index={slice_index} action_assigned={action_assigned} \
                 plan_compiled={plan_compiled} action_executed=false",
                active.profile_id.unwrap_or_default()
            );
            if active.practice {
                dispatcher.onboarding_practice.selected(slice_index);
                platform::request_controller_repaint();
            } else if !live_discrete_transfer {
                log_selected_action_outcome(
                    invocation_id,
                    active.action_id,
                    active.execution_plan.as_ref(),
                    active_snapshot.map(|snapshot| &snapshot.context),
                    dispatcher.action_executor,
                    ActionDispatchTrace {
                        timing_markers: dispatcher.timing_markers,
                        release_qpc: source_qpc,
                    },
                    config_revision,
                );
            } else {
                log::info!(
                    target: "actions",
                    "component=actions event_code=continuous_discrete_transfer_requested \
                     result_code=ok config_revision={config_revision} \
                     invocation_id={invocation_id} slice_index={slice_index} \
                     action_executed=false"
                );
            }
        }
        InteractionOutcome::ContinuousCommitted { slice_index, value } => {
            log::info!(
                target: "interaction",
                "component=interaction event_code=continuous_meter_committed \
                 result_code=ok config_revision={config_revision} invocation_id={invocation_id} \
                 profile_id={} slice_index={slice_index} normalized_value={value:.6} \
                 continuous_meter={} action_executed=false",
                active.profile_id.unwrap_or_default(),
                active.continuous_meter
            );
        }
        InteractionOutcome::Cancelled { reason } => {
            log::info!(
                target: "interaction",
                "component=interaction event_code=invocation_cancelled result_code=ok \
                 config_revision={config_revision} invocation_id={invocation_id} reason={reason:?}"
            );
            if active.practice {
                dispatcher.onboarding_practice.cancelled(reason);
                platform::request_controller_repaint();
            }
        }
    }
    dispatcher.timing_markers.emit("close_requested", Some(invocation_id), source_qpc, "");
    route.close(dispatcher.egui_context, dispatcher.timing_markers, invocation_id);
}

fn dispatch_selection_changed(
    invocation_id: uuid::Uuid,
    selected_slice: Option<usize>,
    source_qpc: i64,
    active_snapshot: Option<&InvocationSnapshot>,
    route: &OverlayRoute,
    dispatcher: &EffectDispatcher<'_>,
) {
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

fn dispatch_continuous_changed(
    invocation_id: uuid::Uuid,
    visual: ContinuousVisualState,
    source_qpc: i64,
    active_snapshot: Option<&InvocationSnapshot>,
    route: &OverlayRoute,
    dispatcher: &EffectDispatcher<'_>,
    transition: bool,
) {
    let config_revision = active_snapshot
        .map_or(dispatcher.config_revision, |snapshot| snapshot.context.config_revision);
    if transition {
        log::info!(
            target: "interaction",
            "component=interaction event_code=continuous_meter_updated result_code=ok \
             config_revision={config_revision} invocation_id={invocation_id} change_kind=transition \
             slice_index={} normalized_value={:.6} armed={} action_executed=false",
            visual.selection.slice_index,
            visual.selection.value,
            visual.armed
        );
    } else {
        log::debug!(
            target: "interaction",
            "component=interaction event_code=continuous_meter_updated result_code=ok \
             config_revision={config_revision} invocation_id={invocation_id} change_kind=value \
             slice_index={} normalized_value={:.6} armed={} action_executed=false",
            visual.selection.slice_index,
            visual.selection.value,
            visual.armed
        );
    }
    dispatcher.timing_markers.emit(
        "continuous_value_requested",
        Some(invocation_id),
        source_qpc,
        &format!(
            "slice_index={} normalized_value={:.6} armed={}",
            visual.selection.slice_index, visual.selection.value, visual.armed
        ),
    );
    route.set_continuous(dispatcher.egui_context, invocation_id, visual);
}

#[derive(Clone, Copy)]
struct ActionDispatchTrace<'a> {
    timing_markers: &'a TimingMarkers,
    release_qpc: i64,
}

fn log_selected_action_outcome(
    invocation_id: uuid::Uuid,
    action_id: Option<uuid::Uuid>,
    execution_plan: Option<&Arc<ExecutionPlan>>,
    target: Option<&Arc<gradial_core::InvocationContext>>,
    action_executor: &ActionExecutorHandle,
    trace: ActionDispatchTrace<'_>,
    config_revision: u64,
) {
    match (action_id, execution_plan, target) {
        (Some(action_id), Some(plan), Some(target)) => {
            log::info!(
                target: "actions",
                "component=actions event_code=action_plan_compiled result_code=ok \
                 config_revision={} invocation_id={invocation_id} action_id={action_id} \
                 step_count={} per_step_timeout_ms={} maximum_duration_ms={} \
                 action_executed=false",
                plan.config_revision(),
                plan.steps().len(),
                plan.per_step_timeout_ms(),
                plan.maximum_duration_ms()
            );
            match action_executor.try_dispatch(invocation_id, Arc::clone(plan), Arc::clone(target))
            {
                ActionDispatch::Accepted => {
                    trace.timing_markers.emit(
                        "action_dispatched",
                        Some(invocation_id),
                        trace.release_qpc,
                        &format!("step_count={}", plan.steps().len()),
                    );
                    log::info!(
                        target: "actions",
                        "component=actions event_code=action_dispatched result_code=ok \
                         config_revision={} invocation_id={invocation_id} \
                         action_id={action_id} step_count={} action_executed=false",
                        plan.config_revision(),
                        plan.steps().len()
                    );
                }
                ActionDispatch::Rejected(result) => {
                    log_dispatch_rejection(
                        invocation_id,
                        action_id,
                        plan.config_revision(),
                        &result,
                    );
                    tray::record_action_result(&result, plan.config_revision());
                }
            }
        }
        (Some(action_id), _, _) => {
            log::error!(
                target: "actions",
                "component=actions event_code=invocation_cancelled result_code=cancelled \
                 error_code=config_invalid config_revision={config_revision} \
                 invocation_id={invocation_id} action_id={action_id} \
                 reason=ActionPlanInvalid action_executed=false"
            );
        }
        (None, _, _) => {
            log::info!(
                target: "actions",
                "component=actions event_code=invocation_cancelled result_code=cancelled \
                 config_revision={config_revision} invocation_id={invocation_id} \
                 reason=EmptySlice action_executed=false"
            );
        }
    }
}

fn log_dispatch_rejection(
    invocation_id: uuid::Uuid,
    action_id: uuid::Uuid,
    config_revision: u64,
    result: &ActionResult,
) {
    match result {
        ActionResult::Failure { code, .. } => {
            log::error!(
                target: "actions",
                "component=actions event_code=action_dispatch_rejected result_code=failure \
                 error_code={} config_revision={config_revision} \
                 invocation_id={invocation_id} action_id={action_id} action_executed=false",
                code.as_str()
            );
        }
        ActionResult::Cancelled => {
            log::info!(
                target: "actions",
                "component=actions event_code=action_dispatch_rejected result_code=cancelled \
                 config_revision={config_revision} invocation_id={invocation_id} \
                 action_id={action_id} action_executed=false"
            );
        }
        ActionResult::Success => {
            log::error!(
                target: "actions",
                "component=actions event_code=action_dispatch_rejected result_code=failure \
                 error_code=invalid_executor_result config_revision={config_revision} \
                 invocation_id={invocation_id} action_id={action_id} action_executed=false"
            );
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
    continuous_meter: bool,
    continuous_full_scale_radius_dip: Option<f64>,
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
         monitor_index={route_index} geometry={} slice_count={} \
         continuous_meter={continuous_meter}",
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
    if let Err(error) = route.open(
        dispatcher.egui_context,
        dispatcher.timing_markers,
        snapshot,
        Arc::clone(menu),
        continuous_full_scale_radius_dip
            .unwrap_or(f64::from(DEFAULT_CONTINUOUS_FULL_SCALE_RADIUS_DIP)),
    ) {
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

fn qpc_elapsed_milliseconds(start: i64, end: i64, frequency: i64) -> u32 {
    if frequency <= 0 || end < start {
        return 0;
    }
    let elapsed = end.saturating_sub(start);
    u32::try_from(elapsed.saturating_mul(1_000) / frequency).unwrap_or(u32::MAX)
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
    use std::{
        collections::BTreeMap,
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };

    use gradial_core::{
        ActionDefinition, ActionKind, ActionPlanCompiler, ActionResult, Configuration,
        ContinuousActionSnapshot, ContinuousBeginRequest, ContinuousRequest, ContinuousSelection,
        ContinuousSession, ContinuousVisualState, InteractionEffect, InteractionOutcome,
        InvocationContext, InvocationGeometry, InvocationSnapshot, PermittedContext,
        PluginPermission, PluginValue, Point, ProfileResolver, ProfileTarget, ResolvedMenu,
        ScreenPoint, SliceCount,
    };
    use uuid::Uuid;

    use crate::action_executor::ActionExecutor;
    use crate::diagnostics::TimingMarkers;
    use crate::plugin_worker::{PluginActionAdmission, PluginLifecycleClient};

    use super::{
        ActionDispatchTrace, ActiveInvocation, CONTINUOUS_VISUAL_INTERVAL, ContinuousUpdateBatch,
        LiveContinuousChange, LiveContinuousPhase, classify_live_continuous_change,
        freeze_continuous_actions, heartbeat_is_unresponsive, log_selected_action_outcome,
        prepare_selected_action,
    };

    fn begin_from_snapshot(
        snapshot: ContinuousActionSnapshot,
        session_id: u128,
    ) -> ContinuousBeginRequest {
        let (_session, request) =
            ContinuousSession::begin(Uuid::from_u128(session_id), snapshot, 0.0)
                .expect("continuous session");
        let ContinuousRequest::Begin(begin) = request else {
            panic!("expected Begin");
        };
        begin
    }

    #[test]
    fn continuous_update_batch_retains_only_the_latest_visual_sample() {
        let invocation_id = Uuid::from_u128(31);
        let mut batch = ContinuousUpdateBatch::default();
        let mut coalesced = 0_u64;

        for index in 0_u16..1_000 {
            let visual = ContinuousVisualState {
                selection: ContinuousSelection {
                    slice_index: usize::from(index % 8),
                    value: f64::from(index) / 999.0,
                },
                pointer_offset_dip: Point::new(f32::from(index), -f32::from(index)),
                armed: true,
            };
            if batch.defer(invocation_id, visual, i64::from(index)) {
                coalesced += 1;
            }
        }

        assert_eq!(coalesced, 999);
        let latest =
            batch.take_ready(Instant::now(), true).expect("latest continuous update").update;
        assert_eq!(latest.invocation_id, invocation_id);
        assert_eq!(latest.source_qpc, 999);
        assert_eq!(latest.visual.selection.slice_index, 7);
        assert!((latest.visual.selection.value - 1.0).abs() <= f64::EPSILON);
        assert_eq!(latest.visual.pointer_offset_dip, Point::new(999.0, -999.0));
        assert!(batch.take_ready(Instant::now(), true).is_none());
    }

    #[test]
    fn continuous_batch_prioritizes_direction_and_mode_but_throttles_value_only_updates() {
        let invocation_id = Uuid::from_u128(32);
        let start = Instant::now();
        let mut batch = ContinuousUpdateBatch::default();
        let visual = |slice_index, value, armed| ContinuousVisualState {
            selection: ContinuousSelection { slice_index, value },
            pointer_offset_dip: Point::new(0.0, 0.0),
            armed,
        };

        batch.defer(invocation_id, visual(2, 0.0, false), 1);
        let initial = batch.take_ready(start, false).expect("initial transition");
        assert!(initial.transition);

        batch.defer(invocation_id, visual(2, 0.25, false), 2);
        assert!(batch.take_ready(start + Duration::from_millis(1), false).is_none());
        batch.defer(invocation_id, visual(2, 0.5, false), 3);
        let value = batch
            .take_ready(start + CONTINUOUS_VISUAL_INTERVAL, false)
            .expect("latest value after interval");
        assert!(!value.transition);
        assert!((value.update.visual.selection.value - 0.5).abs() <= f64::EPSILON);

        batch.defer(invocation_id, visual(3, 0.5, false), 4);
        let direction = batch
            .take_ready(start + CONTINUOUS_VISUAL_INTERVAL, false)
            .expect("direction transition bypasses interval");
        assert!(direction.transition);

        batch.defer(invocation_id, visual(3, 0.5, true), 5);
        let mode = batch
            .take_ready(start + CONTINUOUS_VISUAL_INTERVAL, false)
            .expect("mode transition bypasses interval");
        assert!(mode.transition);
    }

    #[test]
    fn live_continuous_bridge_tracks_unlocked_hybrid_transitions() {
        let visual = |slice_index, value, armed| ContinuousVisualState {
            selection: ContinuousSelection { slice_index, value },
            pointer_offset_dip: Point::new(0.0, 0.0),
            armed,
        };

        assert_eq!(
            classify_live_continuous_change(None, visual(2, 0.0, false)),
            LiveContinuousChange::None
        );
        assert_eq!(
            classify_live_continuous_change(None, visual(2, 0.25, true)),
            LiveContinuousChange::Start
        );
        assert_eq!(
            classify_live_continuous_change(
                Some(LiveContinuousPhase::Active { slice_index: 2 }),
                visual(2, 0.4, true),
            ),
            LiveContinuousChange::Publish
        );
        assert_eq!(
            classify_live_continuous_change(
                Some(LiveContinuousPhase::Active { slice_index: 2 }),
                visual(4, 0.4, true),
            ),
            LiveContinuousChange::Handoff
        );
        assert_eq!(
            classify_live_continuous_change(
                Some(LiveContinuousPhase::Active { slice_index: 4 }),
                visual(4, 0.0, false),
            ),
            LiveContinuousChange::Suspend
        );
        assert_eq!(
            classify_live_continuous_change(
                Some(LiveContinuousPhase::Inactive),
                visual(4, 0.0, false),
            ),
            LiveContinuousChange::None
        );
        assert_eq!(
            classify_live_continuous_change(
                Some(LiveContinuousPhase::Inactive),
                visual(6, 0.7, true),
            ),
            LiveContinuousChange::Resume
        );
    }

    #[test]
    fn invocation_freezes_only_enabled_manifest_continuous_actions() {
        let action_id = Uuid::from_u128(80);
        let mut parameters = BTreeMap::new();
        parameters.insert("scale".to_owned(), PluginValue::Number(0.75));
        let mut configuration = Configuration {
            config_revision: 9,
            actions: vec![ActionDefinition {
                id: action_id,
                name: "Meter".to_owned(),
                kind: ActionKind::PluginContinuous {
                    plugin_id: "org.gradial.meter".to_owned(),
                    plugin_action_id: "measure".to_owned(),
                    parameters,
                },
            }],
            ..Configuration::default()
        };
        configuration.global_profile.menu.slices[0].continuous_action_id = Some(action_id);
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        let resolver = ProfileResolver::compile(&configuration).expect("profiles");
        let resolution = resolver.resolve(ProfileTarget {
            executable_path: None,
            window_class: "",
            window_title: "",
        });
        let enabled = PluginLifecycleClient::with_test_action("org.gradial.meter", "measure", 24);

        let snapshots = freeze_continuous_actions(
            &resolution.menu,
            &compiler,
            &enabled,
            &InvocationContext::default(),
            configuration.config_revision,
            false,
        );
        assert!(snapshots[0].is_some());
        assert!(snapshots[1..].iter().all(Option::is_none));
        let (session, begin) = ContinuousSession::begin(
            Uuid::from_u128(81),
            snapshots[0].clone().expect("enabled snapshot"),
            0.25,
        )
        .expect("session");
        assert_eq!(session.update_interval(), Duration::from_nanos(41_666_667));
        let gradial_core::ContinuousRequest::Begin(begin) = begin else {
            panic!("expected Begin");
        };
        assert_eq!(begin.parameters().get("scale"), Some(&PluginValue::Number(0.75)));

        let unavailable =
            PluginLifecycleClient::with_test_action("org.gradial.other", "measure", 30);
        assert!(
            freeze_continuous_actions(
                &resolution.menu,
                &compiler,
                &unavailable,
                &InvocationContext::default(),
                configuration.config_revision,
                false,
            )
            .iter()
            .all(Option::is_none)
        );
    }

    #[test]
    fn invocation_context_exposes_only_admitted_manifest_fields() {
        let action_id = Uuid::from_u128(82);
        let mut configuration = Configuration {
            config_revision: 10,
            actions: vec![ActionDefinition {
                id: action_id,
                name: "Context meter".to_owned(),
                kind: ActionKind::PluginContinuous {
                    plugin_id: "org.gradial.context-meter".to_owned(),
                    plugin_action_id: "measure".to_owned(),
                    parameters: BTreeMap::new(),
                },
            }],
            ..Configuration::default()
        };
        configuration.global_profile.menu.slices[0].continuous_action_id = Some(action_id);
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        let resolver = ProfileResolver::compile(&configuration).expect("profiles");
        let resolution = resolver.resolve(ProfileTarget {
            executable_path: None,
            window_class: "",
            window_title: "",
        });
        let invocation = InvocationContext {
            foreground_window: 44,
            foreground_pid: 55,
            process_start_time: 66,
            executable_path: Some(r"C:\Program Files\Example\example.exe".into()),
            window_class: "ExampleWindow".to_owned(),
            window_title: "Private project title".to_owned(),
            monitor_device_name: r"\\.\DISPLAY1".to_owned(),
            config_revision: configuration.config_revision,
            ..InvocationContext::default()
        };

        let application_only = PluginLifecycleClient::with_test_action_and_permissions(
            "org.gradial.context-meter",
            "measure",
            30,
            &[PluginPermission::ContextApplication],
        );
        let application_begin = begin_from_snapshot(
            freeze_continuous_actions(
                &resolution.menu,
                &compiler,
                &application_only,
                &invocation,
                configuration.config_revision,
                false,
            )[0]
            .clone()
            .expect("application snapshot"),
            83,
        );
        let application = application_begin.context().application().expect("application");
        assert_eq!(
            application.executable_path(),
            std::path::Path::new(r"C:\Program Files\Example\example.exe")
        );
        assert_eq!(application.window_class(), "ExampleWindow");
        assert_eq!(application_begin.context().window_title(), None);

        let title_only = PluginLifecycleClient::with_test_action_and_permissions(
            "org.gradial.context-meter",
            "measure",
            30,
            &[PluginPermission::ContextWindowTitle],
        );
        let title_begin = begin_from_snapshot(
            freeze_continuous_actions(
                &resolution.menu,
                &compiler,
                &title_only,
                &invocation,
                configuration.config_revision,
                false,
            )[0]
            .clone()
            .expect("title snapshot"),
            84,
        );
        assert!(title_begin.context().application().is_none());
        assert_eq!(title_begin.context().window_title(), Some("Private project title"));

        let no_context =
            PluginLifecycleClient::with_test_action("org.gradial.context-meter", "measure", 30);
        let empty_begin = begin_from_snapshot(
            freeze_continuous_actions(
                &resolution.menu,
                &compiler,
                &no_context,
                &invocation,
                configuration.config_revision,
                false,
            )[0]
            .clone()
            .expect("context-free snapshot"),
            85,
        );
        assert_eq!(empty_begin.context(), &PermittedContext::default());
    }

    #[test]
    fn unavailable_application_path_is_omitted_even_when_permitted() {
        let admission = PluginActionAdmission::new_for_test(
            30,
            &[PluginPermission::ContextApplication, PluginPermission::ContextWindowTitle],
        );
        let context = super::permitted_plugin_context(
            &InvocationContext {
                executable_path: None,
                window_class: "ExampleWindow".to_owned(),
                window_title: String::new(),
                ..InvocationContext::default()
            },
            admission,
        );
        assert!(context.application().is_none());
        assert_eq!(context.window_title(), Some(""));
    }

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

    #[test]
    fn release_compiles_and_dispatches_the_frozen_action_for_the_invocation_revision() {
        let action_id = Uuid::from_u128(41);
        let invocation_id = Uuid::from_u128(42);
        let configuration = Configuration {
            config_revision: 9,
            actions: vec![ActionDefinition {
                id: action_id,
                name: "Pause".to_owned(),
                kind: ActionKind::Delay { milliseconds: 10 },
            }],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        let menu = ResolvedMenu::new(
            SliceCount::Four,
            ["North", "East", "South", "West"].map(str::to_owned).into(),
            vec![Some(action_id), None, None, None].into(),
        )
        .expect("menu");
        let mut active = ActiveInvocation {
            snapshot: Some(InvocationSnapshot {
                invocation_id,
                qpc_timestamp: 1,
                cursor_physical_px: ScreenPoint::new(10, 20),
                monitor_dpi: 96,
                geometry: InvocationGeometry::fixed(ScreenPoint::new(10, 20), 96),
                context: Arc::new(InvocationContext {
                    config_revision: 9,
                    ..InvocationContext::default()
                }),
            }),
            menu: Some(Arc::new(menu)),
            ..ActiveInvocation::default()
        };

        prepare_selected_action(
            Some(InteractionEffect::Close {
                invocation_id,
                outcome: InteractionOutcome::Selected { slice_index: 0 },
            }),
            &mut active,
            &compiler,
            99,
        );

        assert_eq!(active.action_id, Some(action_id));
        let plan = Arc::clone(active.execution_plan.as_ref().expect("immutable release plan"));
        assert_eq!(plan.root_action_id(), action_id);
        assert_eq!(plan.config_revision(), 9);
        assert_eq!(plan.steps().len(), 1);

        let executor = ActionExecutor::start(9).expect("executor");
        let timing_markers = TimingMarkers::register().expect("timing markers");
        let target = active.snapshot.as_ref().map(|snapshot| &snapshot.context);
        log_selected_action_outcome(
            invocation_id,
            active.action_id,
            Some(&plan),
            target,
            &executor.handle(),
            ActionDispatchTrace { timing_markers: &timing_markers, release_qpc: 1 },
            9,
        );
        for _ in 0..100 {
            if let Some(completion) = executor.try_completion() {
                assert_eq!(completion.invocation_id, invocation_id);
                assert_eq!(completion.result, ActionResult::Success);
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("release-time action did not complete");
    }

    #[test]
    fn continuous_meter_discrete_release_never_compiles_a_saved_action() {
        let action_id = Uuid::from_u128(51);
        let invocation_id = Uuid::from_u128(52);
        let configuration = Configuration {
            config_revision: 10,
            actions: vec![ActionDefinition {
                id: action_id,
                name: "Must stay inert".to_owned(),
                kind: ActionKind::Delay { milliseconds: 10 },
            }],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        let menu = ResolvedMenu::new(
            SliceCount::Four,
            ["North", "East", "South", "West"].map(str::to_owned).into(),
            vec![Some(action_id), None, None, None].into(),
        )
        .expect("menu");
        let mut active = ActiveInvocation {
            menu: Some(Arc::new(menu)),
            continuous_meter: true,
            ..ActiveInvocation::default()
        };

        prepare_selected_action(
            Some(InteractionEffect::Close {
                invocation_id,
                outcome: InteractionOutcome::Selected { slice_index: 0 },
            }),
            &mut active,
            &compiler,
            10,
        );

        assert_eq!(active.action_id, None);
        assert!(active.execution_plan.is_none());
    }
}
