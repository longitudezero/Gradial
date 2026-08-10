use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle, Thread},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use gradial_core::{
    ActionErrorCode, ActionResult, ContinuousActionSnapshot, ContinuousCancelReason,
    ContinuousRequest, ContinuousSession, ExecutionPlan, InvocationContext,
};
use gradial_lua::LuaSandboxError;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    action_executor::{
        ActionDispatch, ActionExecutorHandle, ContinuousActionLease, CoordinatorStopToken,
    },
    action_target,
    plugin_worker::{PluginLifecycleClient, PluginLifecycleError, PluginLifecycleSubmitError},
    tray,
};

const CONTROL_CAPACITY: usize = 8;
const COMPLETION_CAPACITY: usize = 1;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(1);
const BEGIN_TIMEOUT: Duration = Duration::from_millis(100);
const UPDATE_TIMEOUT: Duration = Duration::from_millis(16);
const COMMIT_TIMEOUT: Duration = Duration::from_millis(100);
const CANCEL_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContinuousBackendStage {
    Begin,
    Update,
    Commit,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContinuousBackendFailure {
    TimedOut(ContinuousBackendStage),
    Faulted(ContinuousBackendStage),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ContinuousCompletionOutcome {
    Committed,
    Cancelled(ContinuousCancelReason),
    BeginRejected,
    DiscreteDispatched,
    DiscreteRejected(ActionResult),
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContinuousCompletion {
    pub(crate) invocation_id: Uuid,
    pub(crate) outcome: ContinuousCompletionOutcome,
    pub(crate) backend_failure: Option<ContinuousBackendFailure>,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum ContinuousBackendError {
    #[error("continuous Begin was rejected")]
    Rejected,
    #[error("continuous callback timed out")]
    TimedOut,
    #[error("continuous callback faulted")]
    Faulted,
    #[error("the captured continuous target became stale")]
    TargetStale,
    #[error("continuous callback was interrupted")]
    Interrupted,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ContinuousBackendContext<'a> {
    deadline: Instant,
    stop_token: &'a CoordinatorStopToken,
}

impl ContinuousBackendContext<'_> {
    pub(crate) const fn deadline(self) -> Instant {
        self.deadline
    }

    pub(crate) fn cancellation_requested(self) -> bool {
        self.stop_token.cancellation_requested()
    }

    pub(crate) fn shutdown_requested(self) -> bool {
        self.stop_token.shutdown_requested()
    }

    pub(crate) fn should_stop(self) -> bool {
        self.cancellation_requested() || self.shutdown_requested()
    }
}

pub(crate) trait ContinuousBackend: Send + 'static {
    fn dispatch(
        &mut self,
        request: &ContinuousRequest,
        context: ContinuousBackendContext<'_>,
    ) -> Result<(), ContinuousBackendError>;
}

#[derive(Clone, Debug)]
pub(crate) struct ValidatingInertBackend {
    invocation_id: Uuid,
    target: Arc<InvocationContext>,
}

impl ValidatingInertBackend {
    pub(crate) fn new(invocation_id: Uuid, target: Arc<InvocationContext>) -> Self {
        Self { invocation_id, target }
    }
}

impl ContinuousBackend for ValidatingInertBackend {
    fn dispatch(
        &mut self,
        request: &ContinuousRequest,
        context: ContinuousBackendContext<'_>,
    ) -> Result<(), ContinuousBackendError> {
        if !matches!(request, ContinuousRequest::Cancel(_)) && context.should_stop() {
            return Err(ContinuousBackendError::Interrupted);
        }
        if Instant::now() >= context.deadline() {
            return Err(ContinuousBackendError::TimedOut);
        }
        let validation = match request {
            ContinuousRequest::Begin(_) => action_target::validate(&self.target),
            ContinuousRequest::Update(_) | ContinuousRequest::Commit(_) => {
                action_target::validate_freshness(&self.target)
            }
            ContinuousRequest::Cancel(_) => Ok(()),
        };
        validation.map_err(|failure| {
            let stage = stage_for(request);
            log::warn!(
                target: "continuous",
                "component=continuous event_code=inert_target_validation_failed \
                 result_code=failure config_revision={} invocation_id={} \
                 stage={stage:?} reason={failure:?}",
                self.target.config_revision,
                self.invocation_id
            );
            if stage == ContinuousBackendStage::Begin {
                ContinuousBackendError::Rejected
            } else if failure == action_target::TargetValidationFailure::StaleTarget {
                ContinuousBackendError::TargetStale
            } else {
                ContinuousBackendError::Faulted
            }
        })?;

        match request {
            ContinuousRequest::Update(_) => log::debug!(
                target: "continuous",
                "component=continuous event_code=inert_backend_request_validated \
                 result_code=ok config_revision={} invocation_id={} stage=Update",
                self.target.config_revision,
                self.invocation_id
            ),
            ContinuousRequest::Cancel(cancel) => log::info!(
                target: "continuous",
                "component=continuous event_code=inert_backend_request_validated \
                 result_code=ok config_revision={} invocation_id={} stage=Cancel \
                 cancel_reason={:?}",
                self.target.config_revision,
                self.invocation_id,
                cancel.reason()
            ),
            ContinuousRequest::Begin(_) | ContinuousRequest::Commit(_) => log::info!(
                target: "continuous",
                "component=continuous event_code=inert_backend_request_validated \
                 result_code=ok config_revision={} invocation_id={} stage={:?}",
                self.target.config_revision,
                self.invocation_id,
                stage_for(request)
            ),
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PluginContinuousBackend {
    invocation_id: Uuid,
    target: Arc<InvocationContext>,
    client: PluginLifecycleClient,
}

impl PluginContinuousBackend {
    pub(crate) fn new(
        invocation_id: Uuid,
        target: Arc<InvocationContext>,
        client: PluginLifecycleClient,
    ) -> Self {
        Self { invocation_id, target, client }
    }

    fn abort_pending_begin(&self, request: &ContinuousRequest, reason: ContinuousCancelReason) {
        if !matches!(request, ContinuousRequest::Begin(_)) {
            return;
        }
        if let Err(error) = self.client.abort_begin(request.session_id(), reason) {
            log::error!(
                target: "continuous",
                "component=continuous event_code=plugin_begin_abort_submit_failed \
                 result_code=failure error_code={error} config_revision={} \
                 invocation_id={} session_id={}",
                self.target.config_revision,
                self.invocation_id,
                request.session_id()
            );
        }
    }
}

impl ContinuousBackend for PluginContinuousBackend {
    fn dispatch(
        &mut self,
        request: &ContinuousRequest,
        context: ContinuousBackendContext<'_>,
    ) -> Result<(), ContinuousBackendError> {
        if !matches!(request, ContinuousRequest::Cancel(_)) && context.should_stop() {
            return Err(ContinuousBackendError::Interrupted);
        }
        if Instant::now() >= context.deadline() {
            return Err(ContinuousBackendError::TimedOut);
        }
        validate_plugin_target(self.invocation_id, &self.target, request)?;

        let ticket = self
            .client
            .request(request.clone())
            .map_err(|error| map_plugin_submit_error(error, request, context))?;
        loop {
            if let Some(result) = ticket.try_result() {
                return result.map_err(|error| map_plugin_lifecycle_error(error, request));
            }
            if !matches!(request, ContinuousRequest::Cancel(_)) && context.should_stop() {
                let reason = if context.shutdown_requested() {
                    ContinuousCancelReason::Shutdown
                } else {
                    ContinuousCancelReason::UserRequested
                };
                self.abort_pending_begin(request, reason);
                return Err(ContinuousBackendError::Interrupted);
            }
            let now = Instant::now();
            if now >= context.deadline() {
                self.abort_pending_begin(request, ContinuousCancelReason::PluginFaulted);
                return Err(ContinuousBackendError::TimedOut);
            }
            thread::park_timeout(
                WORKER_POLL_INTERVAL.min(context.deadline().saturating_duration_since(now)),
            );
        }
    }
}

fn validate_plugin_target(
    invocation_id: Uuid,
    target: &InvocationContext,
    request: &ContinuousRequest,
) -> Result<(), ContinuousBackendError> {
    let validation = match request {
        ContinuousRequest::Begin(_) => action_target::validate(target),
        ContinuousRequest::Update(_) | ContinuousRequest::Commit(_) => {
            action_target::validate_freshness(target)
        }
        ContinuousRequest::Cancel(_) => Ok(()),
    };
    validation.map_err(|failure| {
        let stage = stage_for(request);
        log::warn!(
            target: "continuous",
            "component=continuous event_code=plugin_target_validation_failed \
             result_code=failure config_revision={} invocation_id={} \
             stage={stage:?} reason={failure:?}",
            target.config_revision,
            invocation_id
        );
        if stage == ContinuousBackendStage::Begin {
            ContinuousBackendError::Rejected
        } else if failure == action_target::TargetValidationFailure::StaleTarget {
            ContinuousBackendError::TargetStale
        } else {
            ContinuousBackendError::Faulted
        }
    })
}

fn map_plugin_submit_error(
    error: PluginLifecycleSubmitError,
    request: &ContinuousRequest,
    context: ContinuousBackendContext<'_>,
) -> ContinuousBackendError {
    match error {
        PluginLifecycleSubmitError::ShuttingDown if context.shutdown_requested() => {
            ContinuousBackendError::Interrupted
        }
        PluginLifecycleSubmitError::QueueFull(_)
            if matches!(request, ContinuousRequest::Begin(_)) =>
        {
            ContinuousBackendError::Rejected
        }
        PluginLifecycleSubmitError::QueueFull(_)
        | PluginLifecycleSubmitError::Unavailable
        | PluginLifecycleSubmitError::ShuttingDown => ContinuousBackendError::Faulted,
    }
}

fn map_plugin_lifecycle_error(
    error: PluginLifecycleError,
    request: &ContinuousRequest,
) -> ContinuousBackendError {
    match error {
        PluginLifecycleError::PluginUnavailable
        | PluginLifecycleError::ActionUnavailable
        | PluginLifecycleError::ParametersInvalid
        | PluginLifecycleError::SessionActive
            if matches!(request, ContinuousRequest::Begin(_)) =>
        {
            ContinuousBackendError::Rejected
        }
        PluginLifecycleError::BeginAborted => ContinuousBackendError::Interrupted,
        PluginLifecycleError::Lua(LuaSandboxError::DeadlineExceeded) => {
            ContinuousBackendError::TimedOut
        }
        PluginLifecycleError::PluginUnavailable
        | PluginLifecycleError::ActionUnavailable
        | PluginLifecycleError::ParametersInvalid
        | PluginLifecycleError::SessionActive
        | PluginLifecycleError::SessionNotActive
        | PluginLifecycleError::Lua(_) => ContinuousBackendError::Faulted,
    }
}

#[derive(Clone, Copy, Debug)]
struct ContinuousExecutionPolicy {
    begin_timeout: Duration,
    update_timeout: Duration,
    commit_timeout: Duration,
    cancel_timeout: Duration,
    poll_interval: Duration,
}

impl ContinuousExecutionPolicy {
    const fn production() -> Self {
        Self {
            begin_timeout: BEGIN_TIMEOUT,
            update_timeout: UPDATE_TIMEOUT,
            commit_timeout: COMMIT_TIMEOUT,
            cancel_timeout: CANCEL_TIMEOUT,
            poll_interval: WORKER_POLL_INTERVAL,
        }
    }

    const fn timeout_for(self, stage: ContinuousBackendStage) -> Duration {
        match stage {
            ContinuousBackendStage::Begin => self.begin_timeout,
            ContinuousBackendStage::Update => self.update_timeout,
            ContinuousBackendStage::Commit => self.commit_timeout,
            ContinuousBackendStage::Cancel => self.cancel_timeout,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct LatestObservation {
    value: f64,
    observed_at: Instant,
    count: u64,
}

#[derive(Debug, Default)]
struct LatestObservationSlot {
    pending: Mutex<Option<LatestObservation>>,
}

impl LatestObservationSlot {
    fn publish(&self, value: f64, observed_at: Instant) {
        let mut pending = self.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = pending.map_or(1, |observation| observation.count.saturating_add(1));
        *pending = Some(LatestObservation { value, observed_at, count });
    }

    fn take(&self) -> Option<LatestObservation> {
        self.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }
}

#[derive(Debug)]
enum ContinuousControl {
    Resume { session_id: Uuid, snapshot: ContinuousActionSnapshot, value: f64 },
    Suspend { reason: ContinuousCancelReason },
    Commit { value: f64 },
    Cancel { reason: ContinuousCancelReason },
    FinishDiscrete { plan: Arc<ExecutionPlan>, target: Arc<InvocationContext> },
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum ContinuousSubmitError {
    #[error("continuous invocation is closed")]
    Closed,
    #[error("continuous control queue is full")]
    QueueFull,
    #[error("continuous value must be finite and between zero and one")]
    InvalidValue,
}

#[derive(Clone, Debug)]
pub(crate) struct ContinuousInvocationHandle {
    controls: Sender<ContinuousControl>,
    latest: Arc<LatestObservationSlot>,
    terminal_requested: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    stop_token: CoordinatorStopToken,
    worker_thread: Thread,
}

impl ContinuousInvocationHandle {
    pub(crate) fn publish(&self, value: f64) -> Result<(), ContinuousSubmitError> {
        validate_value(value)?;
        if self.is_closed_for_input() {
            return Err(ContinuousSubmitError::Closed);
        }
        self.latest.publish(value, Instant::now());
        self.worker_thread.unpark();
        Ok(())
    }

    pub(crate) fn handoff(
        &self,
        snapshot: ContinuousActionSnapshot,
        value: f64,
    ) -> Result<(), ContinuousSubmitError> {
        validate_value(value)?;
        if self.is_closed_for_input() {
            return Err(ContinuousSubmitError::Closed);
        }
        self.send_control(ContinuousControl::Resume { session_id: Uuid::new_v4(), snapshot, value })
    }

    pub(crate) fn resume(
        &self,
        snapshot: ContinuousActionSnapshot,
        value: f64,
    ) -> Result<(), ContinuousSubmitError> {
        self.handoff(snapshot, value)
    }

    pub(crate) fn suspend(
        &self,
        reason: ContinuousCancelReason,
    ) -> Result<(), ContinuousSubmitError> {
        if self.is_closed_for_input() {
            return Err(ContinuousSubmitError::Closed);
        }
        self.send_control(ContinuousControl::Suspend { reason })
    }

    pub(crate) fn commit(&self, value: f64) -> Result<(), ContinuousSubmitError> {
        validate_value(value)?;
        self.send_terminal(ContinuousControl::Commit { value })
    }

    pub(crate) fn cancel(
        &self,
        reason: ContinuousCancelReason,
    ) -> Result<(), ContinuousSubmitError> {
        self.send_terminal(ContinuousControl::Cancel { reason })
    }

    pub(crate) fn finish_discrete(
        &self,
        plan: Arc<ExecutionPlan>,
        target: Arc<InvocationContext>,
    ) -> Result<(), ContinuousSubmitError> {
        self.send_terminal(ContinuousControl::FinishDiscrete { plan, target })
    }

    fn send_terminal(&self, control: ContinuousControl) -> Result<(), ContinuousSubmitError> {
        if self.closed.load(Ordering::Acquire)
            || self
                .terminal_requested
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(ContinuousSubmitError::Closed);
        }
        if let Err(error) = self.send_control(control) {
            self.terminal_requested.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(())
    }

    fn send_control(&self, control: ContinuousControl) -> Result<(), ContinuousSubmitError> {
        match self.controls.try_send(control) {
            Ok(()) => {
                self.worker_thread.unpark();
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(ContinuousSubmitError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(ContinuousSubmitError::Closed),
        }
    }

    fn is_closed_for_input(&self) -> bool {
        self.closed.load(Ordering::Acquire) || self.terminal_requested.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub(crate) struct ContinuousInvocation {
    handle: ContinuousInvocationHandle,
    completions: Receiver<ContinuousCompletion>,
    join_handle: Option<JoinHandle<()>>,
}

impl ContinuousInvocation {
    pub(crate) fn start<B: ContinuousBackend>(
        action_executor: &ActionExecutorHandle,
        invocation_id: Uuid,
        snapshot: ContinuousActionSnapshot,
        initial_value: f64,
        backend: B,
    ) -> Result<Self, ActionResult> {
        Self::start_with_policy(
            action_executor,
            invocation_id,
            snapshot,
            initial_value,
            backend,
            ContinuousExecutionPolicy::production(),
        )
    }

    fn start_with_policy<B: ContinuousBackend>(
        action_executor: &ActionExecutorHandle,
        invocation_id: Uuid,
        snapshot: ContinuousActionSnapshot,
        initial_value: f64,
        backend: B,
        policy: ContinuousExecutionPolicy,
    ) -> Result<Self, ActionResult> {
        validate_value(initial_value).map_err(|_| {
            failure(ActionErrorCode::UnsupportedAction, "continuous initial value is invalid")
        })?;
        let lease = action_executor.try_acquire_continuous(invocation_id)?;
        let stop_token = lease.stop_token();
        let (control_sender, controls) = bounded(CONTROL_CAPACITY);
        let (completion_sender, completions) = bounded(COMPLETION_CAPACITY);
        let latest = Arc::new(LatestObservationSlot::default());
        let worker_latest = Arc::clone(&latest);
        let terminal_requested = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let worker_closed = Arc::clone(&closed);
        let initial = SegmentStart { session_id: Uuid::new_v4(), snapshot, value: initial_value };
        let clock_origin = Instant::now();
        let worker_action_executor = action_executor.clone();
        let worker_stop_token = stop_token.clone();

        let join_handle = thread::Builder::new()
            .name("gradial-continuous-executor".to_owned())
            .spawn(move || {
                run_worker(
                    invocation_id,
                    initial,
                    backend,
                    lease,
                    &worker_action_executor,
                    &worker_stop_token,
                    &controls,
                    &worker_latest,
                    &completion_sender,
                    clock_origin,
                    policy,
                );
                worker_closed.store(true, Ordering::Release);
            })
            .map_err(|_| {
                failure(
                    ActionErrorCode::ExecutorUnavailable,
                    "the continuous executor could not start",
                )
            })?;
        let worker_thread = join_handle.thread().clone();
        let handle = ContinuousInvocationHandle {
            controls: control_sender,
            latest,
            terminal_requested,
            closed,
            stop_token,
            worker_thread,
        };
        Ok(Self { handle, completions, join_handle: Some(join_handle) })
    }

    pub(crate) fn handle(&self) -> ContinuousInvocationHandle {
        self.handle.clone()
    }

    pub(crate) fn try_completion(&mut self) -> Option<ContinuousCompletion> {
        let completion = self.completions.try_recv().ok();
        if completion.is_some() {
            let _detached = self.join_handle.take();
        }
        completion
    }

    #[cfg(test)]
    pub(crate) fn wait_for_completion(
        &mut self,
        timeout: Duration,
    ) -> Option<ContinuousCompletion> {
        let completion = self.completions.recv_timeout(timeout).ok();
        if completion.is_some() {
            self.join();
        }
        completion
    }

    #[cfg(test)]
    fn join(&mut self) {
        if let Some(join_handle) = self.join_handle.take() {
            let _worker_result = join_handle.join();
        }
    }
}

impl Drop for ContinuousInvocation {
    fn drop(&mut self) {
        if self.join_handle.is_some() {
            if matches!(
                self.handle.cancel(ContinuousCancelReason::Shutdown),
                Err(ContinuousSubmitError::QueueFull)
            ) {
                self.handle.stop_token.request_cancellation();
                self.handle.worker_thread.unpark();
            }
            let _detached = self.join_handle.take();
        }
    }
}

#[derive(Debug)]
struct SegmentStart {
    session_id: Uuid,
    snapshot: ContinuousActionSnapshot,
    value: f64,
}

#[derive(Debug)]
enum SegmentResult {
    Handoff(SegmentStart),
    Inactive,
    DispatchDiscrete { plan: Arc<ExecutionPlan>, target: Arc<InvocationContext> },
    Complete(ContinuousCompletion),
}

#[allow(clippy::too_many_arguments)]
fn run_worker<B: ContinuousBackend>(
    invocation_id: Uuid,
    segment: SegmentStart,
    mut backend: B,
    lease: ContinuousActionLease,
    action_executor: &ActionExecutorHandle,
    stop_token: &CoordinatorStopToken,
    controls: &Receiver<ContinuousControl>,
    latest: &LatestObservationSlot,
    completions: &Sender<ContinuousCompletion>,
    clock_origin: Instant,
    policy: ContinuousExecutionPolicy,
) {
    debug_assert_eq!(lease.invocation_id(), invocation_id);
    let mut lease = Some(lease);
    let mut active_segment = Some(segment);
    loop {
        let result = active_segment.take().map_or_else(
            || run_inactive(invocation_id, stop_token, controls, latest, policy),
            |current| {
                run_segment(
                    invocation_id,
                    current,
                    &mut backend,
                    stop_token,
                    controls,
                    latest,
                    clock_origin,
                    policy,
                )
            },
        );
        match result {
            SegmentResult::Handoff(replacement) => active_segment = Some(replacement),
            SegmentResult::Inactive => {
                let _discarded = latest.take();
            }
            SegmentResult::DispatchDiscrete { plan, target } => {
                let config_revision = plan.config_revision();
                let action_id = plan.root_action_id();
                let dispatch = lease.take().map_or_else(
                    || {
                        ActionDispatch::Rejected(failure(
                            ActionErrorCode::ExecutorUnavailable,
                            "the continuous action lease is unavailable",
                        ))
                    },
                    |owned_lease| {
                        action_executor.try_dispatch_from_continuous(
                            owned_lease,
                            invocation_id,
                            plan,
                            target,
                        )
                    },
                );
                let outcome = match dispatch {
                    ActionDispatch::Accepted => {
                        log::info!(
                            target: "continuous",
                            "component=continuous event_code=discrete_lease_transferred \
                             result_code=ok config_revision={config_revision} \
                             invocation_id={invocation_id} action_id={action_id}"
                        );
                        ContinuousCompletionOutcome::DiscreteDispatched
                    }
                    ActionDispatch::Rejected(result) => {
                        let error_code = match &result {
                            ActionResult::Failure { code, .. } => code.as_str(),
                            ActionResult::Cancelled => "cancelled",
                            ActionResult::Success => "invalid_executor_result",
                        };
                        log::warn!(
                            target: "continuous",
                            "component=continuous event_code=discrete_lease_transfer_rejected \
                             result_code=failure error_code={error_code} \
                             config_revision={config_revision} invocation_id={invocation_id} \
                             action_id={action_id}"
                        );
                        tray::record_action_result(&result, config_revision);
                        ContinuousCompletionOutcome::DiscreteRejected(result)
                    }
                };
                let completion =
                    ContinuousCompletion { invocation_id, outcome, backend_failure: None };
                let _sent = completions.send(completion);
                break;
            }
            SegmentResult::Complete(completion) => {
                drop(lease.take());
                let _sent = completions.send(completion);
                break;
            }
        }
    }
    drop(lease);
}

fn run_inactive(
    invocation_id: Uuid,
    stop_token: &CoordinatorStopToken,
    controls: &Receiver<ContinuousControl>,
    latest: &LatestObservationSlot,
    policy: ContinuousExecutionPolicy,
) -> SegmentResult {
    loop {
        if let Some(reason) = stop_reason(stop_token) {
            return SegmentResult::Complete(cancelled_completion(invocation_id, reason, None));
        }
        if let Ok(control) = controls.try_recv() {
            return match control {
                ContinuousControl::Resume { session_id, snapshot, value } => {
                    SegmentResult::Handoff(SegmentStart { session_id, snapshot, value })
                }
                ContinuousControl::Suspend { .. } => SegmentResult::Inactive,
                ContinuousControl::Commit { .. } => SegmentResult::Complete(failed_completion(
                    invocation_id,
                    ContinuousBackendFailure::Faulted(ContinuousBackendStage::Commit),
                )),
                ContinuousControl::Cancel { reason } => {
                    SegmentResult::Complete(cancelled_completion(invocation_id, reason, None))
                }
                ContinuousControl::FinishDiscrete { plan, target } => {
                    SegmentResult::DispatchDiscrete { plan, target }
                }
            };
        }
        let _discarded = latest.take();
        thread::park_timeout(policy.poll_interval);
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn run_segment<B: ContinuousBackend>(
    invocation_id: Uuid,
    segment: SegmentStart,
    backend: &mut B,
    stop_token: &CoordinatorStopToken,
    controls: &Receiver<ContinuousControl>,
    latest: &LatestObservationSlot,
    clock_origin: Instant,
    policy: ContinuousExecutionPolicy,
) -> SegmentResult {
    if let Some(reason) = stop_reason(stop_token) {
        return SegmentResult::Complete(cancelled_completion(invocation_id, reason, None));
    }
    let Ok((mut session, begin)) =
        ContinuousSession::begin(segment.session_id, segment.snapshot, segment.value)
    else {
        return SegmentResult::Complete(failed_completion(
            invocation_id,
            ContinuousBackendFailure::Faulted(ContinuousBackendStage::Begin),
        ));
    };
    if let Err(error) = call_backend(backend, &begin, stop_token, policy) {
        let _rejected = session.reject_begin();
        return SegmentResult::Complete(begin_failure(invocation_id, error, stop_token));
    }

    if let Some(reason) = stop_reason(stop_token) {
        let _pending = session.cancel(reason);
    }

    let mut pending_transition = None;
    if session.terminal().is_none()
        && let Ok(control) = controls.try_recv()
    {
        match control {
            ContinuousControl::Resume { session_id, snapshot, value } => {
                let _pending = session.cancel(ContinuousCancelReason::DirectionChanged);
                pending_transition =
                    Some(SegmentResult::Handoff(SegmentStart { session_id, snapshot, value }));
            }
            ContinuousControl::Suspend { reason } => {
                let _pending = session.cancel(reason);
                pending_transition = Some(SegmentResult::Inactive);
            }
            ContinuousControl::Commit { value } => {
                let _pending = session.commit(value, elapsed(clock_origin));
            }
            ContinuousControl::Cancel { reason } => {
                let _pending = session.cancel(reason);
            }
            ContinuousControl::FinishDiscrete { plan, target } => {
                let _pending = session.cancel(ContinuousCancelReason::ReturnedToDiscrete);
                pending_transition = Some(SegmentResult::DispatchDiscrete { plan, target });
            }
        }
    }

    let accepted_at = elapsed(clock_origin);
    let Ok(terminal) = session.accept_begin(accepted_at) else {
        return SegmentResult::Complete(failed_completion(
            invocation_id,
            ContinuousBackendFailure::Faulted(ContinuousBackendStage::Begin),
        ));
    };
    if let Some(request) = terminal {
        let delivery = call_backend(backend, &request, stop_token, policy).err();
        if let Some(transition) = pending_transition {
            if let Some(reason) = stop_reason(stop_token) {
                return SegmentResult::Complete(cancelled_completion(
                    invocation_id,
                    reason,
                    delivery.map(|error| backend_failure(ContinuousBackendStage::Cancel, error)),
                ));
            }
            if let Some(error) = delivery {
                return SegmentResult::Complete(cancelled_completion(
                    invocation_id,
                    cancel_reason_for_request(&request),
                    Some(backend_failure(ContinuousBackendStage::Cancel, error)),
                ));
            }
            return transition;
        }
        return SegmentResult::Complete(terminal_completion(
            invocation_id,
            &request,
            delivery,
            stop_token,
        ));
    }

    apply_latest(&mut session, latest, clock_origin, accepted_at);

    loop {
        if let Some(reason) = stop_reason(stop_token) {
            return finish_cancel(invocation_id, &mut session, reason, backend, stop_token, policy);
        }

        if let Ok(control) = controls.try_recv() {
            match control {
                ContinuousControl::Resume { session_id, snapshot, value } => {
                    let result = finish_cancel(
                        invocation_id,
                        &mut session,
                        ContinuousCancelReason::DirectionChanged,
                        backend,
                        stop_token,
                        policy,
                    );
                    if stop_reason(stop_token).is_some() {
                        return result;
                    }
                    if cancellation_delivery_succeeded(&result) {
                        return SegmentResult::Handoff(SegmentStart {
                            session_id,
                            snapshot,
                            value,
                        });
                    }
                    return result;
                }
                ContinuousControl::Suspend { reason } => {
                    let result = finish_cancel(
                        invocation_id,
                        &mut session,
                        reason,
                        backend,
                        stop_token,
                        policy,
                    );
                    if stop_reason(stop_token).is_some() {
                        return result;
                    }
                    if cancellation_delivery_succeeded(&result) {
                        return SegmentResult::Inactive;
                    }
                    return result;
                }
                ContinuousControl::Commit { value } => {
                    let Ok(Some(request)) = session.commit(value, elapsed(clock_origin)) else {
                        return SegmentResult::Complete(failed_completion(
                            invocation_id,
                            ContinuousBackendFailure::Faulted(ContinuousBackendStage::Commit),
                        ));
                    };
                    let delivery = call_backend(backend, &request, stop_token, policy).err();
                    return SegmentResult::Complete(terminal_completion(
                        invocation_id,
                        &request,
                        delivery,
                        stop_token,
                    ));
                }
                ContinuousControl::Cancel { reason } => {
                    return finish_cancel(
                        invocation_id,
                        &mut session,
                        reason,
                        backend,
                        stop_token,
                        policy,
                    );
                }
                ContinuousControl::FinishDiscrete { plan, target } => {
                    let result = finish_cancel(
                        invocation_id,
                        &mut session,
                        ContinuousCancelReason::ReturnedToDiscrete,
                        backend,
                        stop_token,
                        policy,
                    );
                    if stop_reason(stop_token).is_some() {
                        return result;
                    }
                    if cancellation_delivery_succeeded(&result) {
                        return SegmentResult::DispatchDiscrete { plan, target };
                    }
                    return result;
                }
            }
        }

        apply_latest(&mut session, latest, clock_origin, accepted_at);
        match session.take_due_update(elapsed(clock_origin)) {
            Ok(Some(request)) => {
                if let Err(error) = call_backend(backend, &request, stop_token, policy) {
                    if let Some(reason) = stop_reason(stop_token) {
                        return finish_cancel(
                            invocation_id,
                            &mut session,
                            reason,
                            backend,
                            stop_token,
                            policy,
                        );
                    }
                    let failure = backend_failure(ContinuousBackendStage::Update, error);
                    let cancel_reason = if error == ContinuousBackendError::TargetStale {
                        ContinuousCancelReason::TargetStale
                    } else {
                        ContinuousCancelReason::PluginFaulted
                    };
                    let _stopped = finish_cancel(
                        invocation_id,
                        &mut session,
                        cancel_reason,
                        backend,
                        stop_token,
                        policy,
                    );
                    return SegmentResult::Complete(failed_completion(invocation_id, failure));
                }
            }
            Ok(None) => {}
            Err(_) => {
                return SegmentResult::Complete(failed_completion(
                    invocation_id,
                    ContinuousBackendFailure::Faulted(ContinuousBackendStage::Update),
                ));
            }
        }
        thread::park_timeout(policy.poll_interval);
    }
}

fn cancellation_delivery_succeeded(result: &SegmentResult) -> bool {
    matches!(result, SegmentResult::Complete(ContinuousCompletion { backend_failure: None, .. }))
}

fn cancel_reason_for_request(request: &ContinuousRequest) -> ContinuousCancelReason {
    match request {
        ContinuousRequest::Cancel(cancel) => cancel.reason(),
        ContinuousRequest::Begin(_)
        | ContinuousRequest::Update(_)
        | ContinuousRequest::Commit(_) => ContinuousCancelReason::PluginFaulted,
    }
}

fn apply_latest(
    session: &mut ContinuousSession,
    latest: &LatestObservationSlot,
    clock_origin: Instant,
    accepted_at: Duration,
) {
    if let Some(observation) = latest.take() {
        let observed_at =
            observation.observed_at.saturating_duration_since(clock_origin).max(accepted_at);
        let _observed =
            session.observe_coalesced(observation.value, observed_at, observation.count);
    }
}

fn finish_cancel<B: ContinuousBackend>(
    invocation_id: Uuid,
    session: &mut ContinuousSession,
    reason: ContinuousCancelReason,
    backend: &mut B,
    stop_token: &CoordinatorStopToken,
    policy: ContinuousExecutionPolicy,
) -> SegmentResult {
    let Ok(Some(request)) = session.cancel(reason) else {
        return SegmentResult::Complete(failed_completion(
            invocation_id,
            ContinuousBackendFailure::Faulted(ContinuousBackendStage::Cancel),
        ));
    };
    let delivery = call_backend(backend, &request, stop_token, policy).err();
    SegmentResult::Complete(terminal_completion(invocation_id, &request, delivery, stop_token))
}

fn call_backend<B: ContinuousBackend>(
    backend: &mut B,
    request: &ContinuousRequest,
    stop_token: &CoordinatorStopToken,
    policy: ContinuousExecutionPolicy,
) -> Result<(), ContinuousBackendError> {
    let stage = stage_for(request);
    let deadline =
        Instant::now().checked_add(policy.timeout_for(stage)).unwrap_or_else(Instant::now);
    let result = backend.dispatch(request, ContinuousBackendContext { deadline, stop_token });
    if result.is_ok() && Instant::now() > deadline {
        Err(ContinuousBackendError::TimedOut)
    } else {
        result
    }
}

const fn stage_for(request: &ContinuousRequest) -> ContinuousBackendStage {
    match request {
        ContinuousRequest::Begin(_) => ContinuousBackendStage::Begin,
        ContinuousRequest::Update(_) => ContinuousBackendStage::Update,
        ContinuousRequest::Commit(_) => ContinuousBackendStage::Commit,
        ContinuousRequest::Cancel(_) => ContinuousBackendStage::Cancel,
    }
}

fn stop_reason(stop_token: &CoordinatorStopToken) -> Option<ContinuousCancelReason> {
    if stop_token.shutdown_requested() {
        Some(ContinuousCancelReason::Shutdown)
    } else if stop_token.cancellation_requested() {
        Some(ContinuousCancelReason::UserRequested)
    } else {
        None
    }
}

fn begin_failure(
    invocation_id: Uuid,
    error: ContinuousBackendError,
    stop_token: &CoordinatorStopToken,
) -> ContinuousCompletion {
    if let Some(reason) = stop_reason(stop_token) {
        return cancelled_completion(invocation_id, reason, None);
    }
    match error {
        ContinuousBackendError::Rejected => ContinuousCompletion {
            invocation_id,
            outcome: ContinuousCompletionOutcome::BeginRejected,
            backend_failure: None,
        },
        error => {
            failed_completion(invocation_id, backend_failure(ContinuousBackendStage::Begin, error))
        }
    }
}

fn terminal_completion(
    invocation_id: Uuid,
    request: &ContinuousRequest,
    delivery: Option<ContinuousBackendError>,
    stop_token: &CoordinatorStopToken,
) -> ContinuousCompletion {
    if let Some(reason) = stop_reason(stop_token) {
        return cancelled_completion(
            invocation_id,
            reason,
            delivery.map(|error| backend_failure(stage_for(request), error)),
        );
    }
    match request {
        ContinuousRequest::Commit(_) => delivery.map_or(
            ContinuousCompletion {
                invocation_id,
                outcome: ContinuousCompletionOutcome::Committed,
                backend_failure: None,
            },
            |error| {
                failed_completion(
                    invocation_id,
                    backend_failure(ContinuousBackendStage::Commit, error),
                )
            },
        ),
        ContinuousRequest::Cancel(cancel) => cancelled_completion(
            invocation_id,
            cancel.reason(),
            delivery.map(|error| backend_failure(ContinuousBackendStage::Cancel, error)),
        ),
        ContinuousRequest::Begin(_) | ContinuousRequest::Update(_) => {
            failed_completion(invocation_id, ContinuousBackendFailure::Faulted(stage_for(request)))
        }
    }
}

fn backend_failure(
    stage: ContinuousBackendStage,
    error: ContinuousBackendError,
) -> ContinuousBackendFailure {
    match error {
        ContinuousBackendError::TimedOut => ContinuousBackendFailure::TimedOut(stage),
        ContinuousBackendError::Rejected
        | ContinuousBackendError::Faulted
        | ContinuousBackendError::TargetStale
        | ContinuousBackendError::Interrupted => ContinuousBackendFailure::Faulted(stage),
    }
}

const fn failed_completion(
    invocation_id: Uuid,
    backend_failure: ContinuousBackendFailure,
) -> ContinuousCompletion {
    ContinuousCompletion {
        invocation_id,
        outcome: ContinuousCompletionOutcome::Failed,
        backend_failure: Some(backend_failure),
    }
}

const fn cancelled_completion(
    invocation_id: Uuid,
    reason: ContinuousCancelReason,
    backend_failure: Option<ContinuousBackendFailure>,
) -> ContinuousCompletion {
    ContinuousCompletion {
        invocation_id,
        outcome: ContinuousCompletionOutcome::Cancelled(reason),
        backend_failure,
    }
}

fn validate_value(value: f64) -> Result<(), ContinuousSubmitError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(ContinuousSubmitError::InvalidValue)
    }
}

fn elapsed(origin: Instant) -> Duration {
    Instant::now().saturating_duration_since(origin)
}

fn failure(code: ActionErrorCode, safe_message: &str) -> ActionResult {
    ActionResult::Failure { code, safe_message: safe_message.to_owned() }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, VecDeque},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    use crossbeam_channel::{Receiver, Sender, bounded};
    use gradial_core::{
        ActionDefinition, ActionErrorCode, ActionKind, ActionPlanCompiler, Configuration,
        ContinuousActionSnapshot, ContinuousCancelReason, ContinuousRequest, PermittedContext,
        PluginActionId, PluginId,
    };
    use uuid::Uuid;

    use super::{
        ContinuousBackend, ContinuousBackendContext, ContinuousBackendError,
        ContinuousBackendFailure, ContinuousBackendStage, ContinuousCompletion,
        ContinuousCompletionOutcome, ContinuousInvocation, ValidatingInertBackend,
        map_plugin_lifecycle_error, map_plugin_submit_error,
    };
    use crate::action_executor::{ActionDispatch, ActionExecutor};
    use crate::plugin_worker::{
        PluginLifecycleError, PluginLifecycleStage, PluginLifecycleSubmitError,
    };
    use gradial_lua::LuaSandboxError;

    #[derive(Clone, Debug)]
    enum FakeResponse {
        Success,
        Fault,
        TargetStale,
        Stall,
        Wait(Arc<AtomicBool>),
        WaitReject(Arc<AtomicBool>),
    }

    #[derive(Debug)]
    struct FakeBackend {
        responses: VecDeque<FakeResponse>,
        calls: Sender<ContinuousRequest>,
    }

    impl ContinuousBackend for FakeBackend {
        fn dispatch(
            &mut self,
            request: &ContinuousRequest,
            context: ContinuousBackendContext<'_>,
        ) -> Result<(), ContinuousBackendError> {
            let _recorded = self.calls.send(request.clone());
            match self.responses.pop_front().unwrap_or(FakeResponse::Success) {
                FakeResponse::Success => Ok(()),
                FakeResponse::Fault => Err(ContinuousBackendError::Faulted),
                FakeResponse::TargetStale => Err(ContinuousBackendError::TargetStale),
                FakeResponse::Stall => loop {
                    if context.should_stop() {
                        return Err(ContinuousBackendError::Interrupted);
                    }
                    if Instant::now() >= context.deadline() {
                        return Err(ContinuousBackendError::TimedOut);
                    }
                    thread::sleep(Duration::from_millis(1));
                },
                FakeResponse::Wait(gate) => loop {
                    if gate.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    if context.should_stop() {
                        return Err(ContinuousBackendError::Interrupted);
                    }
                    if Instant::now() >= context.deadline() {
                        return Err(ContinuousBackendError::TimedOut);
                    }
                    thread::sleep(Duration::from_millis(1));
                },
                FakeResponse::WaitReject(gate) => loop {
                    if gate.load(Ordering::Acquire) {
                        return Err(ContinuousBackendError::Rejected);
                    }
                    if context.should_stop() {
                        return Err(ContinuousBackendError::Interrupted);
                    }
                    if Instant::now() >= context.deadline() {
                        return Err(ContinuousBackendError::TimedOut);
                    }
                    thread::sleep(Duration::from_millis(1));
                },
            }
        }
    }

    fn fake_backend(
        responses: impl IntoIterator<Item = FakeResponse>,
    ) -> (FakeBackend, Receiver<ContinuousRequest>) {
        let (sender, receiver) = bounded(128);
        (FakeBackend { responses: responses.into_iter().collect(), calls: sender }, receiver)
    }

    fn snapshot(action_id: &str) -> ContinuousActionSnapshot {
        ContinuousActionSnapshot::new(
            PluginId::new("org.gradial.fake").expect("plugin"),
            PluginActionId::new(action_id).expect("action"),
            PermittedContext::default(),
            BTreeMap::new(),
            30,
        )
        .expect("snapshot")
    }

    #[test]
    fn plugin_bridge_errors_map_to_the_coordinator_contract() {
        let (_session, begin) =
            gradial_core::ContinuousSession::begin(Uuid::from_u128(400), snapshot("measure"), 0.25)
                .expect("begin request");
        let mut executor = ActionExecutor::start(1).expect("executor");
        let lease = executor
            .handle()
            .try_acquire_continuous(Uuid::from_u128(401))
            .expect("continuous lease");
        let stop_token = lease.stop_token();
        let context = ContinuousBackendContext {
            deadline: Instant::now() + Duration::from_secs(1),
            stop_token: &stop_token,
        };

        assert_eq!(
            map_plugin_lifecycle_error(PluginLifecycleError::ActionUnavailable, &begin),
            ContinuousBackendError::Rejected
        );
        assert_eq!(
            map_plugin_lifecycle_error(
                PluginLifecycleError::Lua(LuaSandboxError::DeadlineExceeded),
                &begin,
            ),
            ContinuousBackendError::TimedOut
        );
        assert_eq!(
            map_plugin_submit_error(
                PluginLifecycleSubmitError::QueueFull(PluginLifecycleStage::Begin),
                &begin,
                context,
            ),
            ContinuousBackendError::Rejected
        );
        drop(lease);
        executor.shutdown();
    }

    fn delay_plan(milliseconds: u32) -> Arc<gradial_core::ExecutionPlan> {
        let configuration = Configuration {
            config_revision: 1,
            actions: vec![ActionDefinition {
                id: Uuid::from_u128(1),
                name: "Delay".to_owned(),
                kind: ActionKind::Delay { milliseconds },
            }],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        Arc::new(compiler.compile_plan(Uuid::from_u128(1), 1).expect("plan"))
    }

    fn wait_call(calls: &Receiver<ContinuousRequest>) -> ContinuousRequest {
        calls.recv_timeout(Duration::from_secs(1)).expect("backend call")
    }

    fn finish(invocation: &mut ContinuousInvocation) -> ContinuousCompletion {
        invocation.wait_for_completion(Duration::from_secs(1)).expect("continuous completion")
    }

    #[test]
    fn continuous_lease_rejects_discrete_work_and_releases_after_commit() {
        let executor = ActionExecutor::start(1).expect("executor");
        let (backend, calls) = fake_backend([]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(10),
            snapshot("first"),
            0.2,
            backend,
        )
        .expect("continuous invocation");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));
        assert!(matches!(
            executor.handle().try_dispatch(
                Uuid::from_u128(11),
                delay_plan(10),
                Arc::new(gradial_core::InvocationContext::default()),
            ),
            ActionDispatch::Rejected(gradial_core::ActionResult::Failure {
                code: ActionErrorCode::ActionBusy,
                ..
            })
        ));

        invocation.handle().commit(0.7).expect("commit");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Commit(_)));
        assert_eq!(finish(&mut invocation).outcome, ContinuousCompletionOutcome::Committed);

        assert_eq!(
            executor.handle().try_dispatch(
                Uuid::from_u128(12),
                delay_plan(10),
                Arc::new(gradial_core::InvocationContext::default()),
            ),
            ActionDispatch::Accepted
        );
    }

    #[test]
    fn discrete_work_prevents_a_continuous_lease() {
        let executor = ActionExecutor::start(1).expect("executor");
        assert_eq!(
            executor.handle().try_dispatch(
                Uuid::from_u128(20),
                delay_plan(100),
                Arc::new(gradial_core::InvocationContext::default()),
            ),
            ActionDispatch::Accepted
        );
        let (backend, _) = fake_backend([]);
        let rejected = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(21),
            snapshot("blocked"),
            0.2,
            backend,
        );
        assert!(matches!(
            rejected,
            Err(gradial_core::ActionResult::Failure { code: ActionErrorCode::ActionBusy, .. })
        ));
        assert!(executor.cancel_current());
    }

    #[test]
    fn inert_backend_rejects_a_stale_target_before_begin_is_accepted() {
        let executor = ActionExecutor::start(1).expect("executor");
        let invocation_id = Uuid::from_u128(25);
        let target = Arc::new(gradial_core::InvocationContext::default());
        let backend = ValidatingInertBackend::new(invocation_id, Arc::clone(&target));
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            invocation_id,
            snapshot("stale"),
            0.2,
            backend,
        )
        .expect("continuous invocation");

        assert_eq!(finish(&mut invocation).outcome, ContinuousCompletionOutcome::BeginRejected);
        assert!(!executor.handle().is_busy());
    }

    #[test]
    fn handoff_cancels_old_segment_before_new_begin_without_releasing_lease() {
        let executor = ActionExecutor::start(1).expect("executor");
        let (backend, calls) = fake_backend([]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(30),
            snapshot("west"),
            0.3,
            backend,
        )
        .expect("continuous invocation");
        let ContinuousRequest::Begin(first) = wait_call(&calls) else {
            panic!("expected first begin");
        };
        invocation.handle().handoff(snapshot("south"), 0.6).expect("handoff");
        let ContinuousRequest::Cancel(cancel) = wait_call(&calls) else {
            panic!("expected old cancel");
        };
        assert_eq!(cancel.session_id(), first.session_id());
        assert_eq!(cancel.reason(), ContinuousCancelReason::DirectionChanged);
        let ContinuousRequest::Begin(second) = wait_call(&calls) else {
            panic!("expected replacement begin");
        };
        assert_ne!(second.session_id(), first.session_id());
        assert_eq!(second.action_id().as_str(), "south");
        assert!(executor.handle().is_busy());

        invocation.handle().commit(0.8).expect("commit replacement");
        let ContinuousRequest::Commit(commit) = wait_call(&calls) else {
            panic!("expected replacement commit");
        };
        assert_eq!(commit.session_id(), second.session_id());
        assert_eq!(finish(&mut invocation).outcome, ContinuousCompletionOutcome::Committed);
    }

    #[test]
    fn failed_handoff_cancel_releases_the_lease_without_starting_a_replacement() {
        let executor = ActionExecutor::start(1).expect("executor");
        let (backend, calls) = fake_backend([FakeResponse::Success, FakeResponse::Fault]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(34),
            snapshot("west"),
            0.3,
            backend,
        )
        .expect("continuous invocation");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));

        invocation.handle().handoff(snapshot("south"), 0.6).expect("handoff");
        let ContinuousRequest::Cancel(cancel) = wait_call(&calls) else {
            panic!("expected old cancel");
        };
        assert_eq!(cancel.reason(), ContinuousCancelReason::DirectionChanged);
        let completion = finish(&mut invocation);
        assert_eq!(
            completion.outcome,
            ContinuousCompletionOutcome::Cancelled(ContinuousCancelReason::DirectionChanged)
        );
        assert_eq!(
            completion.backend_failure,
            Some(ContinuousBackendFailure::Faulted(ContinuousBackendStage::Cancel))
        );
        assert!(calls.try_recv().is_err());
        assert!(!executor.handle().is_busy());
    }

    #[test]
    fn inward_suspend_keeps_the_lease_and_can_resume_on_an_unlocked_wedge() {
        let executor = ActionExecutor::start(1).expect("executor");
        let (backend, calls) = fake_backend([]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(35),
            snapshot("west"),
            0.3,
            backend,
        )
        .expect("continuous invocation");
        let ContinuousRequest::Begin(first) = wait_call(&calls) else {
            panic!("expected first begin");
        };

        invocation.handle().suspend(ContinuousCancelReason::ReturnedToDiscrete).expect("suspend");
        let ContinuousRequest::Cancel(cancel) = wait_call(&calls) else {
            panic!("expected inward cancel");
        };
        assert_eq!(cancel.session_id(), first.session_id());
        assert_eq!(cancel.reason(), ContinuousCancelReason::ReturnedToDiscrete);
        assert!(executor.handle().is_busy());
        assert!(invocation.try_completion().is_none());
        assert!(matches!(
            executor.handle().try_dispatch(
                Uuid::from_u128(36),
                delay_plan(10),
                Arc::new(gradial_core::InvocationContext::default()),
            ),
            ActionDispatch::Rejected(gradial_core::ActionResult::Failure {
                code: ActionErrorCode::ActionBusy,
                ..
            })
        ));

        invocation.handle().resume(snapshot("south"), 0.6).expect("resume");
        let ContinuousRequest::Begin(second) = wait_call(&calls) else {
            panic!("expected resumed begin");
        };
        assert_ne!(second.session_id(), first.session_id());
        assert_eq!(second.action_id().as_str(), "south");

        invocation.handle().commit(0.8).expect("commit resumed segment");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Commit(_)));
        assert_eq!(finish(&mut invocation).outcome, ContinuousCompletionOutcome::Committed);
    }

    #[test]
    fn inactive_invocation_transfers_its_lease_to_the_discrete_worker() {
        let executor = ActionExecutor::start(1).expect("executor");
        let (backend, calls) = fake_backend([]);
        let invocation_id = Uuid::from_u128(37);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            invocation_id,
            snapshot("west"),
            0.3,
            backend,
        )
        .expect("continuous invocation");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));
        invocation.handle().suspend(ContinuousCancelReason::ReturnedToDiscrete).expect("suspend");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Cancel(_)));

        invocation
            .handle()
            .finish_discrete(delay_plan(10), Arc::new(gradial_core::InvocationContext::default()))
            .expect("transfer discrete action");
        assert_eq!(
            finish(&mut invocation).outcome,
            ContinuousCompletionOutcome::DiscreteDispatched
        );

        for _ in 0..100 {
            if let Some(completion) = executor.try_completion() {
                assert_eq!(completion.invocation_id, invocation_id);
                assert_eq!(completion.result, gradial_core::ActionResult::Success);
                assert!(!executor.handle().is_busy());
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("transferred discrete action did not complete");
    }

    #[test]
    fn terminal_queued_during_begin_is_delivered_immediately_after_acceptance() {
        let executor = ActionExecutor::start(1).expect("executor");
        let gate = Arc::new(AtomicBool::new(false));
        let (backend, calls) = fake_backend([FakeResponse::Wait(Arc::clone(&gate))]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(40),
            snapshot("pending"),
            0.2,
            backend,
        )
        .expect("continuous invocation");
        let ContinuousRequest::Begin(begin) = wait_call(&calls) else {
            panic!("expected begin");
        };
        invocation.handle().commit(0.9).expect("pending commit");
        gate.store(true, Ordering::Release);
        let ContinuousRequest::Commit(commit) = wait_call(&calls) else {
            panic!("expected commit after begin");
        };
        assert_eq!(commit.session_id(), begin.session_id());
        assert_eq!(finish(&mut invocation).outcome, ContinuousCompletionOutcome::Committed);
    }

    #[test]
    fn latest_value_published_during_begin_is_observed_after_acceptance() {
        let executor = ActionExecutor::start(1).expect("executor");
        let gate = Arc::new(AtomicBool::new(false));
        let (backend, calls) = fake_backend([FakeResponse::Wait(Arc::clone(&gate))]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(41),
            snapshot("pending-value"),
            0.2,
            backend,
        )
        .expect("continuous invocation");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));
        invocation.handle().publish(0.8).expect("pending value");
        gate.store(true, Ordering::Release);
        let ContinuousRequest::Update(update) = wait_call(&calls) else {
            panic!("expected update after Begin acceptance");
        };
        assert!((update.sample().value() - 0.8).abs() < f64::EPSILON);
        invocation.handle().commit(0.8).expect("commit");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Commit(_)));
        assert_eq!(finish(&mut invocation).outcome, ContinuousCompletionOutcome::Committed);
    }

    #[test]
    fn begin_rejection_has_no_terminal_callback_and_releases_the_lease() {
        let executor = ActionExecutor::start(1).expect("executor");
        let gate = Arc::new(AtomicBool::new(false));
        let (backend, calls) = fake_backend([FakeResponse::WaitReject(Arc::clone(&gate))]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(50),
            snapshot("rejected"),
            0.2,
            backend,
        )
        .expect("continuous invocation");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));
        invocation.handle().commit(0.7).expect("pending commit");
        gate.store(true, Ordering::Release);
        assert_eq!(finish(&mut invocation).outcome, ContinuousCompletionOutcome::BeginRejected);
        assert!(calls.try_recv().is_err());
        assert!(!executor.handle().is_busy());
    }

    #[test]
    fn stalled_begin_times_out_without_a_terminal_callback() {
        let executor = ActionExecutor::start(1).expect("executor");
        let (backend, calls) = fake_backend([FakeResponse::Stall]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(52),
            snapshot("stalled-begin"),
            0.2,
            backend,
        )
        .expect("continuous invocation");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));
        let completion = finish(&mut invocation);
        assert_eq!(completion.outcome, ContinuousCompletionOutcome::Failed);
        assert_eq!(
            completion.backend_failure,
            Some(ContinuousBackendFailure::TimedOut(ContinuousBackendStage::Begin))
        );
        assert!(calls.try_recv().is_err());
        assert!(!executor.handle().is_busy());
    }

    #[test]
    fn dropping_a_stalled_invocation_never_joins_on_the_interaction_owner() {
        let executor = ActionExecutor::start(1).expect("executor");
        let (backend, calls) = fake_backend([FakeResponse::Stall]);
        let invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(53),
            snapshot("detached"),
            0.2,
            backend,
        )
        .expect("continuous invocation");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));

        let drop_started = Instant::now();
        drop(invocation);
        assert!(drop_started.elapsed() < Duration::from_millis(50));

        for _ in 0..200 {
            if !executor.handle().is_busy() {
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("detached stalled invocation did not release its lease");
    }

    #[test]
    fn rapid_publications_deliver_only_latest_value_with_a_sequence_gap() {
        let executor = ActionExecutor::start(1).expect("executor");
        let (backend, calls) = fake_backend([]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(55),
            snapshot("coalesced"),
            0.1,
            backend,
        )
        .expect("continuous invocation");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));
        let handle = invocation.handle();
        handle.publish(0.2).expect("first");
        handle.publish(0.4).expect("second");
        handle.publish(0.8).expect("latest");
        let ContinuousRequest::Update(update) = wait_call(&calls) else {
            panic!("expected coalesced update");
        };
        assert_eq!(update.sample().sequence(), 3);
        assert!((update.sample().value() - 0.8).abs() < f64::EPSILON);
        assert!(calls.try_recv().is_err());

        handle.commit(0.8).expect("commit");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Commit(_)));
        assert_eq!(finish(&mut invocation).outcome, ContinuousCompletionOutcome::Committed);
    }

    #[test]
    fn stalled_and_faulted_updates_cancel_the_session_and_release_the_lease() {
        for (response, expected, cancel_reason) in [
            (
                FakeResponse::Stall,
                ContinuousBackendFailure::TimedOut(ContinuousBackendStage::Update),
                ContinuousCancelReason::PluginFaulted,
            ),
            (
                FakeResponse::Fault,
                ContinuousBackendFailure::Faulted(ContinuousBackendStage::Update),
                ContinuousCancelReason::PluginFaulted,
            ),
            (
                FakeResponse::TargetStale,
                ContinuousBackendFailure::Faulted(ContinuousBackendStage::Update),
                ContinuousCancelReason::TargetStale,
            ),
        ] {
            let executor = ActionExecutor::start(1).expect("executor");
            let (backend, calls) = fake_backend([FakeResponse::Success, response]);
            let mut invocation = ContinuousInvocation::start(
                &executor.handle(),
                Uuid::new_v4(),
                snapshot("update"),
                0.1,
                backend,
            )
            .expect("continuous invocation");
            assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));
            thread::sleep(Duration::from_millis(5));
            invocation.handle().publish(0.8).expect("publish");
            assert!(matches!(wait_call(&calls), ContinuousRequest::Update(_)));
            let ContinuousRequest::Cancel(cancel) = wait_call(&calls) else {
                panic!("expected fault cancel");
            };
            assert_eq!(cancel.reason(), cancel_reason);
            let completion = finish(&mut invocation);
            assert_eq!(completion.outcome, ContinuousCompletionOutcome::Failed);
            assert_eq!(completion.backend_failure, Some(expected));
            assert!(!executor.handle().is_busy());
        }
    }

    #[test]
    fn user_cancellation_and_shutdown_send_bounded_terminal_requests() {
        let executor = ActionExecutor::start(1).expect("executor");
        let (backend, calls) = fake_backend([]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(60),
            snapshot("cancel"),
            0.2,
            backend,
        )
        .expect("continuous invocation");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));
        assert!(executor.cancel_current());
        let ContinuousRequest::Cancel(cancel) = wait_call(&calls) else {
            panic!("expected user cancel");
        };
        assert_eq!(cancel.reason(), ContinuousCancelReason::UserRequested);
        assert_eq!(
            finish(&mut invocation).outcome,
            ContinuousCompletionOutcome::Cancelled(ContinuousCancelReason::UserRequested)
        );

        let mut executor = ActionExecutor::start(1).expect("executor");
        let (backend, calls) = fake_backend([]);
        let mut invocation = ContinuousInvocation::start(
            &executor.handle(),
            Uuid::from_u128(61),
            snapshot("shutdown"),
            0.2,
            backend,
        )
        .expect("continuous invocation");
        assert!(matches!(wait_call(&calls), ContinuousRequest::Begin(_)));
        let started = Instant::now();
        executor.shutdown();
        assert!(started.elapsed() < Duration::from_secs(1));
        let ContinuousRequest::Cancel(cancel) = wait_call(&calls) else {
            panic!("expected shutdown cancel");
        };
        assert_eq!(cancel.reason(), ContinuousCancelReason::Shutdown);
        assert_eq!(
            finish(&mut invocation).outcome,
            ContinuousCompletionOutcome::Cancelled(ContinuousCancelReason::Shutdown)
        );
    }
}
