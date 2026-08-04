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
    ContinuousRequest, ContinuousSession,
};
use thiserror::Error;
use uuid::Uuid;

use crate::action_executor::{ActionExecutorHandle, ContinuousActionLease, CoordinatorStopToken};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContinuousCompletionOutcome {
    Committed,
    Cancelled(ContinuousCancelReason),
    BeginRejected,
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
    Handoff { session_id: Uuid, snapshot: ContinuousActionSnapshot, value: f64 },
    Commit { value: f64 },
    Cancel { reason: ContinuousCancelReason },
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
        self.send_control(ContinuousControl::Handoff {
            session_id: Uuid::new_v4(),
            snapshot,
            value,
        })
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

        let join_handle = thread::Builder::new()
            .name("gradial-continuous-executor".to_owned())
            .spawn(move || {
                run_worker(
                    invocation_id,
                    initial,
                    backend,
                    lease,
                    &stop_token,
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
            worker_thread,
        };
        Ok(Self { handle, completions, join_handle: Some(join_handle) })
    }

    pub(crate) fn handle(&self) -> ContinuousInvocationHandle {
        self.handle.clone()
    }

    pub(crate) fn try_completion(&self) -> Option<ContinuousCompletion> {
        self.completions.try_recv().ok()
    }

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

    fn join(&mut self) {
        if let Some(join_handle) = self.join_handle.take() {
            let _worker_result = join_handle.join();
        }
    }
}

impl Drop for ContinuousInvocation {
    fn drop(&mut self) {
        if self.join_handle.is_some() {
            let _cancel = self.handle.cancel(ContinuousCancelReason::Shutdown);
            self.join();
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
    Complete(ContinuousCompletion),
}

#[allow(clippy::too_many_arguments)]
fn run_worker<B: ContinuousBackend>(
    invocation_id: Uuid,
    mut segment: SegmentStart,
    mut backend: B,
    lease: ContinuousActionLease,
    stop_token: &CoordinatorStopToken,
    controls: &Receiver<ContinuousControl>,
    latest: &LatestObservationSlot,
    completions: &Sender<ContinuousCompletion>,
    clock_origin: Instant,
    policy: ContinuousExecutionPolicy,
) {
    debug_assert_eq!(lease.invocation_id(), invocation_id);
    loop {
        match run_segment(
            invocation_id,
            segment,
            &mut backend,
            stop_token,
            controls,
            latest,
            clock_origin,
            policy,
        ) {
            SegmentResult::Handoff(replacement) => segment = replacement,
            SegmentResult::Complete(completion) => {
                let _sent = completions.send(completion);
                break;
            }
        }
    }
    drop(lease);
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
    } else {
        apply_latest(&mut session, latest, clock_origin);
    }

    let mut pending_handoff = None;
    if session.terminal().is_none()
        && let Ok(control) = controls.try_recv()
    {
        match control {
            ContinuousControl::Handoff { session_id, snapshot, value } => {
                let _pending = session.cancel(ContinuousCancelReason::DirectionChanged);
                pending_handoff = Some(SegmentStart { session_id, snapshot, value });
            }
            ContinuousControl::Commit { value } => {
                let _pending = session.commit(value, elapsed(clock_origin));
            }
            ContinuousControl::Cancel { reason } => {
                let _pending = session.cancel(reason);
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
        if let Some(replacement) = pending_handoff {
            if let Some(reason) = stop_reason(stop_token) {
                return SegmentResult::Complete(cancelled_completion(
                    invocation_id,
                    reason,
                    delivery.map(|error| backend_failure(ContinuousBackendStage::Cancel, error)),
                ));
            }
            return SegmentResult::Handoff(replacement);
        }
        return SegmentResult::Complete(terminal_completion(
            invocation_id,
            &request,
            delivery,
            stop_token,
        ));
    }

    loop {
        if let Some(reason) = stop_reason(stop_token) {
            return finish_cancel(invocation_id, &mut session, reason, backend, stop_token, policy);
        }

        if let Ok(control) = controls.try_recv() {
            match control {
                ContinuousControl::Handoff { session_id, snapshot, value } => {
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
                    if matches!(result, SegmentResult::Complete(_)) {
                        return SegmentResult::Handoff(SegmentStart {
                            session_id,
                            snapshot,
                            value,
                        });
                    }
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
            }
        }

        apply_latest(&mut session, latest, clock_origin);
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
                    let _stopped = finish_cancel(
                        invocation_id,
                        &mut session,
                        ContinuousCancelReason::PluginFaulted,
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

fn apply_latest(
    session: &mut ContinuousSession,
    latest: &LatestObservationSlot,
    clock_origin: Instant,
) {
    if let Some(observation) = latest.take() {
        let observed_at = observation.observed_at.saturating_duration_since(clock_origin);
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
        ContinuousCompletionOutcome, ContinuousInvocation,
    };
    use crate::action_executor::{ActionDispatch, ActionExecutor};

    #[derive(Clone, Debug)]
    enum FakeResponse {
        Success,
        Fault,
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
        for (response, expected) in [
            (
                FakeResponse::Stall,
                ContinuousBackendFailure::TimedOut(ContinuousBackendStage::Update),
            ),
            (
                FakeResponse::Fault,
                ContinuousBackendFailure::Faulted(ContinuousBackendStage::Update),
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
            invocation.handle().publish(0.8).expect("publish");
            assert!(matches!(wait_call(&calls), ContinuousRequest::Update(_)));
            let ContinuousRequest::Cancel(cancel) = wait_call(&calls) else {
                panic!("expected fault cancel");
            };
            assert_eq!(cancel.reason(), ContinuousCancelReason::PluginFaulted);
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
