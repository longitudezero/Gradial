use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use gradial_core::{ActionErrorCode, ActionResult, InvocationContext, PluginShortcutChord};
use gradial_lua::{
    LuaCallbackPhase, LuaCapabilityHost, LuaCapabilityRejection, LuaCapabilityStatus,
};
use uuid::Uuid;

use crate::shortcut;

const SHORTCUT_QUEUE_CAPACITY: usize = 32;
const SHORTCUT_REQUESTS_PER_SECOND: usize = 30;
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShortcutCapabilityFailure {
    PermissionDenied,
    RateLimited,
    Busy,
    TargetStale,
    TargetElevated,
    InjectionFailed,
    Interrupted,
    DeadlineExceeded,
}

impl ShortcutCapabilityFailure {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::PermissionDenied => "plugin_permission_denied",
            Self::RateLimited => "plugin_rate_limited",
            Self::Busy => "plugin_capability_busy",
            Self::TargetStale => "continuous_target_lost",
            Self::TargetElevated => "elevated_target",
            Self::InjectionFailed => "plugin_capability_failed",
            Self::Interrupted => "plugin_capability_cancelled",
            Self::DeadlineExceeded => "plugin_capability_timeout",
        }
    }
}

struct ShortcutSession {
    plugin_id: String,
    target: Arc<InvocationContext>,
    permitted: bool,
    accepting: bool,
    accepted_at: VecDeque<Instant>,
    pending: usize,
    cancellation: Arc<AtomicBool>,
    failure: Option<ShortcutCapabilityFailure>,
}

#[derive(Default)]
struct BrokerState {
    sessions: BTreeMap<Uuid, ShortcutSession>,
}

struct SharedBrokerState {
    state: Mutex<BrokerState>,
    changed: Condvar,
    shutdown: Arc<AtomicBool>,
}

impl SharedBrokerState {
    fn new(shutdown: Arc<AtomicBool>) -> Self {
        Self { state: Mutex::new(BrokerState::default()), changed: Condvar::new(), shutdown }
    }
}

struct ShortcutWork {
    session_id: Uuid,
    target: Arc<InvocationContext>,
    shortcut: PluginShortcutChord,
    cancellation: Arc<AtomicBool>,
}

trait ShortcutRunner: Send + 'static {
    fn run(
        &mut self,
        target: &InvocationContext,
        shortcut: &PluginShortcutChord,
        cancellation: &AtomicBool,
        shutdown: &AtomicBool,
    ) -> ActionResult;
}

struct WindowsShortcutRunner;

impl ShortcutRunner for WindowsShortcutRunner {
    fn run(
        &mut self,
        target: &InvocationContext,
        shortcut: &PluginShortcutChord,
        cancellation: &AtomicBool,
        shutdown: &AtomicBool,
    ) -> ActionResult {
        shortcut::execute(target, shortcut.modifiers(), shortcut.key(), cancellation, shutdown)
    }
}

#[derive(Clone)]
pub(crate) struct ShortcutCapabilityClient {
    work: Sender<ShortcutWork>,
    shared: Arc<SharedBrokerState>,
}

impl std::fmt::Debug for ShortcutCapabilityClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ShortcutCapabilityClient").finish_non_exhaustive()
    }
}

impl ShortcutCapabilityClient {
    pub(crate) fn open_session(
        &self,
        session_id: Uuid,
        plugin_id: &str,
        target: Arc<InvocationContext>,
        permitted: bool,
    ) -> Result<(), ShortcutCapabilityFailure> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(ShortcutCapabilityFailure::Interrupted);
        }
        let mut state = lock_state(&self.shared);
        if state.sessions.contains_key(&session_id) || !state.sessions.is_empty() {
            return Err(ShortcutCapabilityFailure::Busy);
        }
        state.sessions.insert(
            session_id,
            ShortcutSession {
                plugin_id: plugin_id.to_owned(),
                target,
                permitted,
                accepting: true,
                accepted_at: VecDeque::with_capacity(SHORTCUT_REQUESTS_PER_SECOND),
                pending: 0,
                cancellation: Arc::new(AtomicBool::new(false)),
                failure: None,
            },
        );
        Ok(())
    }

    pub(crate) fn failure(&self, session_id: Uuid) -> Option<ShortcutCapabilityFailure> {
        lock_state(&self.shared).sessions.get(&session_id).and_then(|session| session.failure)
    }

    pub(crate) fn cancel_session(&self, session_id: Uuid) {
        let session = lock_state(&self.shared).sessions.remove(&session_id);
        if let Some(session) = session {
            session.cancellation.store(true, Ordering::Release);
        }
        self.shared.changed.notify_all();
    }

    pub(crate) fn finish_commit(
        &self,
        session_id: Uuid,
        deadline: Instant,
    ) -> Result<(), ShortcutCapabilityFailure> {
        let mut state = lock_state(&self.shared);
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return Err(ShortcutCapabilityFailure::Interrupted);
        };
        session.accepting = false;
        loop {
            let Some(session) = state.sessions.get(&session_id) else {
                return Err(ShortcutCapabilityFailure::Interrupted);
            };
            if session.pending == 0 {
                break;
            }
            let now = Instant::now();
            if now >= deadline {
                if let Some(session) = state.sessions.remove(&session_id) {
                    session.cancellation.store(true, Ordering::Release);
                }
                return Err(ShortcutCapabilityFailure::DeadlineExceeded);
            }
            let wait = deadline.saturating_duration_since(now);
            let (next, _timeout) = self
                .shared
                .changed
                .wait_timeout(state, wait)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
        }
        let session = state.sessions.remove(&session_id).expect("session remains while locked");
        session.cancellation.store(true, Ordering::Release);
        session.failure.map_or(Ok(()), Err)
    }

    pub(crate) fn reconcile_permissions(&self, permissions: &BTreeMap<String, bool>) {
        let mut state = lock_state(&self.shared);
        for session in state.sessions.values_mut() {
            let permitted = permissions.get(&session.plugin_id).copied().unwrap_or(false);
            session.permitted = permitted;
            if !permitted {
                session.accepting = false;
                session.failure.get_or_insert(ShortcutCapabilityFailure::PermissionDenied);
                session.cancellation.store(true, Ordering::Release);
            }
        }
        drop(state);
        self.shared.changed.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn inert_for_test() -> Self {
        let (work, _works) = bounded(1);
        let shutdown = Arc::new(AtomicBool::new(false));
        Self { work, shared: Arc::new(SharedBrokerState::new(shutdown)) }
    }

    fn submit(
        &self,
        session_id: Uuid,
        phase: LuaCallbackPhase,
        shortcut: PluginShortcutChord,
    ) -> LuaCapabilityStatus {
        if phase == LuaCallbackPhase::Cancel || self.shared.shutdown.load(Ordering::Acquire) {
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Cancelled);
        }
        let mut state = lock_state(&self.shared);
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InactiveSession);
        };
        if !session.accepting || session.cancellation.load(Ordering::Acquire) {
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Cancelled);
        }
        if !session.permitted {
            session.failure.get_or_insert(ShortcutCapabilityFailure::PermissionDenied);
            session.accepting = false;
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::PermissionDenied);
        }

        let now = Instant::now();
        while session.accepted_at.front().is_some_and(|accepted| {
            now.saturating_duration_since(*accepted) >= Duration::from_secs(1)
        }) {
            session.accepted_at.pop_front();
        }
        if session.accepted_at.len() >= SHORTCUT_REQUESTS_PER_SECOND {
            session.failure.get_or_insert(ShortcutCapabilityFailure::RateLimited);
            session.accepting = false;
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::RateLimited);
        }

        let work = ShortcutWork {
            session_id,
            target: Arc::clone(&session.target),
            shortcut,
            cancellation: Arc::clone(&session.cancellation),
        };
        session.pending = session.pending.saturating_add(1);
        match self.work.try_send(work) {
            Ok(()) => {
                session.accepted_at.push_back(now);
                LuaCapabilityStatus::Accepted
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                session.pending = session.pending.saturating_sub(1);
                session.failure.get_or_insert(ShortcutCapabilityFailure::Busy);
                session.accepting = false;
                LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Busy)
            }
        }
    }
}

impl LuaCapabilityHost for ShortcutCapabilityClient {
    fn request_shortcut(
        &self,
        session_id: Uuid,
        phase: LuaCallbackPhase,
        shortcut: PluginShortcutChord,
    ) -> LuaCapabilityStatus {
        self.submit(session_id, phase, shortcut)
    }
}

#[derive(Debug)]
pub(crate) struct ShortcutCapabilityRuntime {
    client: ShortcutCapabilityClient,
    stopped: Receiver<()>,
    join_handle: Option<JoinHandle<()>>,
}

impl ShortcutCapabilityRuntime {
    pub(crate) fn start() -> Result<Self> {
        Self::start_with_runner(Box::new(WindowsShortcutRunner))
    }

    fn start_with_runner(mut runner: Box<dyn ShortcutRunner>) -> Result<Self> {
        let (work, works) = bounded(SHORTCUT_QUEUE_CAPACITY);
        let (stopped_sender, stopped) = bounded(1);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(SharedBrokerState::new(Arc::clone(&shutdown)));
        let worker_shared = Arc::clone(&shared);
        let join_handle = thread::Builder::new()
            .name("gradial-shortcut-broker".to_owned())
            .spawn(move || {
                run_broker(&works, &worker_shared, runner.as_mut());
                let _sent = stopped_sender.try_send(());
            })
            .context("could not spawn the shortcut capability broker")?;
        log::info!(
            target: "plugins",
            "component=capabilities event_code=shortcut_broker_started result_code=ok \
             queue_capacity={SHORTCUT_QUEUE_CAPACITY} session_rate_per_second={SHORTCUT_REQUESTS_PER_SECOND}"
        );
        Ok(Self {
            client: ShortcutCapabilityClient { work, shared },
            stopped,
            join_handle: Some(join_handle),
        })
    }

    pub(crate) fn client(&self) -> ShortcutCapabilityClient {
        self.client.clone()
    }

    #[cfg(test)]
    pub(crate) fn successful_for_test() -> (Self, Arc<std::sync::atomic::AtomicUsize>) {
        struct CountingRunner(Arc<std::sync::atomic::AtomicUsize>);

        impl ShortcutRunner for CountingRunner {
            fn run(
                &mut self,
                _target: &InvocationContext,
                _shortcut: &PluginShortcutChord,
                cancellation: &AtomicBool,
                shutdown: &AtomicBool,
            ) -> ActionResult {
                if cancellation.load(Ordering::Acquire) || shutdown.load(Ordering::Acquire) {
                    return ActionResult::Cancelled;
                }
                self.0.fetch_add(1, Ordering::AcqRel);
                ActionResult::Success
            }
        }

        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let runtime = Self::start_with_runner(Box::new(CountingRunner(Arc::clone(&calls))))
            .expect("test shortcut broker");
        (runtime, calls)
    }

    pub(crate) fn shutdown(&mut self) {
        let Some(join_handle) = self.join_handle.take() else {
            return;
        };
        self.client.shared.shutdown.store(true, Ordering::Release);
        {
            let mut state = lock_state(&self.client.shared);
            for session in state.sessions.values_mut() {
                session.accepting = false;
                session.cancellation.store(true, Ordering::Release);
            }
        }
        drop(self.client.work.clone());
        self.client.shared.changed.notify_all();
        match self.stopped.recv_timeout(SHUTDOWN_WAIT) {
            Ok(()) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                if join_handle.join().is_err() {
                    log::error!(
                        target: "plugins",
                        "component=capabilities event_code=shortcut_broker_panicked result_code=failure"
                    );
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => log::error!(
                target: "plugins",
                "component=capabilities event_code=shortcut_broker_shutdown_timeout \
                 result_code=failure wait_ms={}",
                SHUTDOWN_WAIT.as_millis()
            ),
        }
    }
}

impl Drop for ShortcutCapabilityRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_broker(
    works: &Receiver<ShortcutWork>,
    shared: &SharedBrokerState,
    runner: &mut dyn ShortcutRunner,
) {
    while !shared.shutdown.load(Ordering::Acquire) {
        let Ok(work) = works.recv_timeout(Duration::from_millis(25)) else {
            continue;
        };
        let result = runner.run(&work.target, &work.shortcut, &work.cancellation, &shared.shutdown);
        complete_work(shared, work.session_id, &result);
    }
}

fn complete_work(shared: &SharedBrokerState, session_id: Uuid, result: &ActionResult) {
    let mut state = lock_state(shared);
    let Some(session) = state.sessions.get_mut(&session_id) else {
        return;
    };
    session.pending = session.pending.saturating_sub(1);
    if !session.cancellation.load(Ordering::Acquire)
        && let Some(failure) = result_failure(result)
    {
        session.failure.get_or_insert(failure);
        session.accepting = false;
        session.cancellation.store(true, Ordering::Release);
        log::warn!(
            target: "plugins",
            "component=capabilities event_code=shortcut_request_completed result_code=failure \
             error_code={} plugin_id={} session_id={session_id}",
            failure.code(),
            session.plugin_id
        );
    }
    drop(state);
    shared.changed.notify_all();
}

const fn result_failure(result: &ActionResult) -> Option<ShortcutCapabilityFailure> {
    match result {
        ActionResult::Success => None,
        ActionResult::Cancelled => Some(ShortcutCapabilityFailure::Interrupted),
        ActionResult::Failure { code: ActionErrorCode::StaleTarget, .. } => {
            Some(ShortcutCapabilityFailure::TargetStale)
        }
        ActionResult::Failure { code: ActionErrorCode::ElevatedTarget, .. } => {
            Some(ShortcutCapabilityFailure::TargetElevated)
        }
        ActionResult::Failure { .. } => Some(ShortcutCapabilityFailure::InjectionFailed),
    }
}

fn lock_state(shared: &SharedBrokerState) -> std::sync::MutexGuard<'_, BrokerState> {
    shared.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    type RecordedCalls = Arc<Mutex<Vec<(usize, u16, bool)>>>;

    #[derive(Clone)]
    struct RecordingRunner {
        calls: Arc<Mutex<Vec<(usize, u16, bool)>>>,
        result: ActionResult,
    }

    impl ShortcutRunner for RecordingRunner {
        fn run(
            &mut self,
            target: &InvocationContext,
            shortcut: &PluginShortcutChord,
            cancellation: &AtomicBool,
            shutdown: &AtomicBool,
        ) -> ActionResult {
            if cancellation.load(Ordering::Acquire) || shutdown.load(Ordering::Acquire) {
                return ActionResult::Cancelled;
            }
            self.calls.lock().expect("calls").push((
                target.foreground_window,
                shortcut.key().scan_code,
                shortcut.key().extended,
            ));
            self.result.clone()
        }
    }

    fn runtime(result: ActionResult) -> (ShortcutCapabilityRuntime, RecordedCalls) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner = RecordingRunner { calls: Arc::clone(&calls), result };
        (ShortcutCapabilityRuntime::start_with_runner(Box::new(runner)).expect("runtime"), calls)
    }

    fn target() -> Arc<InvocationContext> {
        Arc::new(InvocationContext {
            foreground_window: 44,
            foreground_pid: 55,
            process_start_time: 66,
            config_revision: 7,
            ..InvocationContext::default()
        })
    }

    fn chord() -> PluginShortcutChord {
        "left_control+right".parse().expect("chord")
    }

    #[test]
    fn permitted_request_runs_asynchronously_and_commit_waits_for_it() {
        let (mut runtime, calls) = runtime(ActionResult::Success);
        let client = runtime.client();
        let session_id = Uuid::from_u128(1);
        client.open_session(session_id, "org.gradial.test", target(), true).expect("open");

        assert_eq!(
            client.request_shortcut(session_id, LuaCallbackPhase::Update, chord()),
            LuaCapabilityStatus::Accepted
        );
        client
            .finish_commit(session_id, Instant::now() + Duration::from_secs(1))
            .expect("drained commit");
        assert_eq!(*calls.lock().expect("calls"), vec![(44, 0x4D, true)]);
        assert_eq!(client.failure(session_id), None);
        runtime.shutdown();
    }

    #[test]
    fn missing_permission_fails_the_session_without_enqueuing_input() {
        let (mut runtime, calls) = runtime(ActionResult::Success);
        let client = runtime.client();
        let session_id = Uuid::from_u128(2);
        client.open_session(session_id, "org.gradial.test", target(), false).expect("open");

        assert_eq!(
            client.request_shortcut(session_id, LuaCallbackPhase::Begin, chord()),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::PermissionDenied)
        );
        assert_eq!(client.failure(session_id), Some(ShortcutCapabilityFailure::PermissionDenied));
        assert!(calls.lock().expect("calls").is_empty());
        client.cancel_session(session_id);
        runtime.shutdown();
    }

    #[test]
    fn unavailable_queue_returns_busy_and_fails_closed() {
        let client = ShortcutCapabilityClient::inert_for_test();
        let session_id = Uuid::from_u128(7);
        client.open_session(session_id, "org.gradial.test", target(), true).expect("open");

        assert_eq!(
            client.request_shortcut(session_id, LuaCallbackPhase::Update, chord()),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Busy)
        );
        assert_eq!(client.failure(session_id), Some(ShortcutCapabilityFailure::Busy));
        client.cancel_session(session_id);
    }

    #[test]
    fn rate_limit_accepts_thirty_requests_and_rejects_the_next() {
        let (mut runtime, _calls) = runtime(ActionResult::Success);
        let client = runtime.client();
        let session_id = Uuid::from_u128(3);
        client.open_session(session_id, "org.gradial.test", target(), true).expect("open");
        for _ in 0..SHORTCUT_REQUESTS_PER_SECOND {
            assert_eq!(
                client.request_shortcut(session_id, LuaCallbackPhase::Update, chord()),
                LuaCapabilityStatus::Accepted
            );
        }
        assert_eq!(
            client.request_shortcut(session_id, LuaCallbackPhase::Update, chord()),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::RateLimited)
        );
        assert_eq!(client.failure(session_id), Some(ShortcutCapabilityFailure::RateLimited));
        client.cancel_session(session_id);
        runtime.shutdown();
    }

    #[test]
    fn revocation_and_cancel_close_the_request_epoch() {
        let (mut runtime, calls) = runtime(ActionResult::Success);
        let client = runtime.client();
        let session_id = Uuid::from_u128(4);
        client.open_session(session_id, "org.gradial.test", target(), true).expect("open");
        client.reconcile_permissions(&BTreeMap::new());
        assert_eq!(client.failure(session_id), Some(ShortcutCapabilityFailure::PermissionDenied));
        assert_eq!(
            client.request_shortcut(session_id, LuaCallbackPhase::Update, chord()),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Cancelled)
        );
        client.cancel_session(session_id);
        assert_eq!(
            client.request_shortcut(session_id, LuaCallbackPhase::Commit, chord()),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InactiveSession)
        );
        assert!(calls.lock().expect("calls").is_empty());
        runtime.shutdown();
    }

    #[test]
    fn target_and_injection_failures_are_retained_until_terminal_handling() {
        for (code, expected) in [
            (ActionErrorCode::StaleTarget, ShortcutCapabilityFailure::TargetStale),
            (ActionErrorCode::ElevatedTarget, ShortcutCapabilityFailure::TargetElevated),
            (ActionErrorCode::InputInjectionFailed, ShortcutCapabilityFailure::InjectionFailed),
        ] {
            let (mut runtime, _calls) =
                runtime(ActionResult::Failure { code, safe_message: "redacted".to_owned() });
            let client = runtime.client();
            let session_id = Uuid::new_v4();
            client.open_session(session_id, "org.gradial.test", target(), true).expect("open");
            assert_eq!(
                client.request_shortcut(session_id, LuaCallbackPhase::Update, chord()),
                LuaCapabilityStatus::Accepted
            );
            let failure = client
                .finish_commit(session_id, Instant::now() + Duration::from_secs(1))
                .expect_err("capability failure");
            assert_eq!(failure, expected);
            runtime.shutdown();
        }
    }

    #[test]
    fn terminal_wait_has_a_deadline_and_cancels_stalled_work() {
        struct StalledRunner;

        impl ShortcutRunner for StalledRunner {
            fn run(
                &mut self,
                _target: &InvocationContext,
                _shortcut: &PluginShortcutChord,
                cancellation: &AtomicBool,
                shutdown: &AtomicBool,
            ) -> ActionResult {
                while !cancellation.load(Ordering::Acquire) && !shutdown.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                ActionResult::Cancelled
            }
        }

        let mut runtime =
            ShortcutCapabilityRuntime::start_with_runner(Box::new(StalledRunner)).expect("runtime");
        let client = runtime.client();
        let session_id = Uuid::from_u128(5);
        client.open_session(session_id, "org.gradial.test", target(), true).expect("open");
        assert_eq!(
            client.request_shortcut(session_id, LuaCallbackPhase::Commit, chord()),
            LuaCapabilityStatus::Accepted
        );

        assert_eq!(
            client.finish_commit(session_id, Instant::now() + Duration::from_millis(10)),
            Err(ShortcutCapabilityFailure::DeadlineExceeded)
        );
        assert_eq!(
            client.request_shortcut(session_id, LuaCallbackPhase::Update, chord()),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InactiveSession)
        );
        runtime.shutdown();
    }

    #[test]
    fn cancel_callback_cannot_submit_an_external_effect() {
        let (mut runtime, calls) = runtime(ActionResult::Success);
        let client = runtime.client();
        let session_id = Uuid::from_u128(6);
        client.open_session(session_id, "org.gradial.test", target(), true).expect("open");

        assert_eq!(
            client.request_shortcut(session_id, LuaCallbackPhase::Cancel, chord()),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Cancelled)
        );
        assert!(calls.lock().expect("calls").is_empty());
        client.cancel_session(session_id);
        runtime.shutdown();
    }
}
