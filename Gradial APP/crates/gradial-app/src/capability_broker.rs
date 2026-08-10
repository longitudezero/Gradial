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
use gradial_core::{
    ActionErrorCode, ActionResult, InvocationContext, PluginLoopbackGrant, PluginLoopbackRequest,
    PluginNotificationText, PluginShortcutChord,
};
use gradial_lua::{
    LuaCallbackPhase, LuaCapabilityHost, LuaCapabilityRejection, LuaCapabilityStatus,
};
use uuid::Uuid;

use crate::{loopback, shortcut, tray};

const SHORTCUT_QUEUE_CAPACITY: usize = 32;
const SHORTCUT_REQUESTS_PER_SECOND: usize = 30;
const NOTIFICATIONS_PER_PLUGIN_PER_MINUTE: usize = 5;
const NOTIFICATION_RATE_WINDOW: Duration = Duration::from_mins(1);
const LOOPBACK_QUEUE_CAPACITY: usize = 32;
const LOOPBACK_REQUESTS_PER_SECOND: usize = 30;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NotificationCapabilityFailure {
    PermissionDenied,
    RateLimited,
    Busy,
    Interrupted,
}

impl NotificationCapabilityFailure {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::PermissionDenied => "plugin_permission_denied",
            Self::RateLimited => "plugin_rate_limited",
            Self::Busy => "plugin_capability_busy",
            Self::Interrupted => "plugin_capability_cancelled",
        }
    }
}

struct NotificationSession {
    plugin_id: String,
    permitted: bool,
    accepting: bool,
    accepted: bool,
    cancellation: Arc<AtomicBool>,
    failure: Option<NotificationCapabilityFailure>,
}

#[derive(Default)]
struct NotificationState {
    sessions: BTreeMap<Uuid, NotificationSession>,
    accepted_by_plugin: BTreeMap<String, VecDeque<Instant>>,
}

trait NotificationSink: Send + Sync + 'static {
    fn enqueue(
        &self,
        plugin_id: &str,
        text: PluginNotificationText,
        cancellation: Arc<AtomicBool>,
    ) -> bool;
}

struct TrayNotificationSink;

impl NotificationSink for TrayNotificationSink {
    fn enqueue(
        &self,
        plugin_id: &str,
        text: PluginNotificationText,
        cancellation: Arc<AtomicBool>,
    ) -> bool {
        tray::enqueue_plugin_notification(plugin_id, text, cancellation)
    }
}

struct SharedNotificationState {
    state: Mutex<NotificationState>,
    sink: Arc<dyn NotificationSink>,
    shutdown: AtomicBool,
}

#[derive(Clone)]
pub(crate) struct NotificationCapabilityClient {
    shared: Arc<SharedNotificationState>,
}

impl std::fmt::Debug for NotificationCapabilityClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("NotificationCapabilityClient").finish_non_exhaustive()
    }
}

impl NotificationCapabilityClient {
    #[cfg(test)]
    pub(crate) fn inert_for_test() -> Self {
        let shared = Arc::new(SharedNotificationState {
            state: Mutex::new(NotificationState::default()),
            sink: Arc::new(RecordingNotificationSink {
                calls: Arc::new(Mutex::new(Vec::new())),
                accepts: false,
            }),
            shutdown: AtomicBool::new(false),
        });
        Self { shared }
    }

    pub(crate) fn open_session(
        &self,
        session_id: Uuid,
        plugin_id: &str,
        permitted: bool,
    ) -> Result<(), NotificationCapabilityFailure> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(NotificationCapabilityFailure::Interrupted);
        }
        let mut state = self.shared.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.sessions.contains_key(&session_id) || !state.sessions.is_empty() {
            return Err(NotificationCapabilityFailure::Busy);
        }
        state.sessions.insert(
            session_id,
            NotificationSession {
                plugin_id: plugin_id.to_owned(),
                permitted,
                accepting: true,
                accepted: false,
                cancellation: Arc::new(AtomicBool::new(false)),
                failure: None,
            },
        );
        Ok(())
    }

    pub(crate) fn failure(&self, session_id: Uuid) -> Option<NotificationCapabilityFailure> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sessions
            .get(&session_id)
            .and_then(|session| session.failure)
    }

    pub(crate) fn cancel_session(&self, session_id: Uuid) {
        let session = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sessions
            .remove(&session_id);
        if let Some(session) = session {
            session.cancellation.store(true, Ordering::Release);
        }
    }

    pub(crate) fn finish_commit(
        &self,
        session_id: Uuid,
    ) -> Result<(), NotificationCapabilityFailure> {
        let session = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sessions
            .remove(&session_id)
            .ok_or(NotificationCapabilityFailure::Interrupted)?;
        session.failure.map_or(Ok(()), Err)
    }

    pub(crate) fn reconcile_permissions(&self, permissions: &BTreeMap<String, bool>) {
        let mut state = self.shared.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for session in state.sessions.values_mut() {
            let permitted = permissions.get(&session.plugin_id).copied().unwrap_or(false);
            session.permitted = permitted;
            if !permitted {
                fail_notification_session(session, NotificationCapabilityFailure::PermissionDenied);
            }
        }
    }

    fn submit(
        &self,
        session_id: Uuid,
        phase: LuaCallbackPhase,
        text: PluginNotificationText,
    ) -> LuaCapabilityStatus {
        if phase == LuaCallbackPhase::Cancel || self.shared.shutdown.load(Ordering::Acquire) {
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Cancelled);
        }
        let mut state = self.shared.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InactiveSession);
        };
        if !session.accepting || session.cancellation.load(Ordering::Acquire) {
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Cancelled);
        }
        if !session.permitted {
            fail_notification_session(session, NotificationCapabilityFailure::PermissionDenied);
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::PermissionDenied);
        }
        if session.accepted {
            fail_notification_session(session, NotificationCapabilityFailure::RateLimited);
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::RateLimited);
        }
        let plugin_id = session.plugin_id.clone();
        let cancellation = Arc::clone(&session.cancellation);

        let now = Instant::now();
        prune_notification_history(&mut state, now);
        if state
            .accepted_by_plugin
            .get(&plugin_id)
            .is_some_and(|accepted| accepted.len() >= NOTIFICATIONS_PER_PLUGIN_PER_MINUTE)
        {
            let session = state.sessions.get_mut(&session_id).expect("session remains locked");
            fail_notification_session(session, NotificationCapabilityFailure::RateLimited);
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::RateLimited);
        }
        if !self.shared.sink.enqueue(&plugin_id, text, Arc::clone(&cancellation)) {
            let session = state.sessions.get_mut(&session_id).expect("session remains locked");
            fail_notification_session(session, NotificationCapabilityFailure::Busy);
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Busy);
        }

        state.sessions.get_mut(&session_id).expect("session remains locked").accepted = true;
        state.accepted_by_plugin.entry(plugin_id.clone()).or_default().push_back(now);
        log::info!(
            target: "plugins",
            "component=capabilities event_code=plugin_notification_queued result_code=ok \
             plugin_id={plugin_id} session_id={session_id}"
        );
        LuaCapabilityStatus::Accepted
    }
}

fn fail_notification_session(
    session: &mut NotificationSession,
    failure: NotificationCapabilityFailure,
) {
    session.failure.get_or_insert(failure);
    session.accepting = false;
    session.cancellation.store(true, Ordering::Release);
}

fn prune_notification_history(state: &mut NotificationState, now: Instant) {
    for accepted in state.accepted_by_plugin.values_mut() {
        while accepted
            .front()
            .is_some_and(|time| now.saturating_duration_since(*time) >= NOTIFICATION_RATE_WINDOW)
        {
            accepted.pop_front();
        }
    }
    state.accepted_by_plugin.retain(|_, accepted| !accepted.is_empty());
}

#[derive(Clone)]
pub(crate) struct LuaCapabilityBrokerClient {
    shortcuts: ShortcutCapabilityClient,
    notifications: NotificationCapabilityClient,
    loopback: LoopbackCapabilityClient,
}

impl LuaCapabilityBrokerClient {
    pub(crate) fn new(
        shortcuts: ShortcutCapabilityClient,
        notifications: NotificationCapabilityClient,
        loopback: LoopbackCapabilityClient,
    ) -> Self {
        Self { shortcuts, notifications, loopback }
    }
}

impl LuaCapabilityHost for LuaCapabilityBrokerClient {
    fn request_shortcut(
        &self,
        session_id: Uuid,
        phase: LuaCallbackPhase,
        shortcut: PluginShortcutChord,
    ) -> LuaCapabilityStatus {
        self.shortcuts.submit(session_id, phase, shortcut)
    }

    fn request_notification(
        &self,
        session_id: Uuid,
        phase: LuaCallbackPhase,
        text: PluginNotificationText,
    ) -> LuaCapabilityStatus {
        self.notifications.submit(session_id, phase, text)
    }

    fn request_loopback(
        &self,
        session_id: Uuid,
        phase: LuaCallbackPhase,
        request: PluginLoopbackRequest,
    ) -> LuaCapabilityStatus {
        self.loopback.submit(session_id, phase, request)
    }
}

#[derive(Debug)]
pub(crate) struct NotificationCapabilityRuntime {
    client: NotificationCapabilityClient,
}

#[cfg(test)]
pub(crate) type RecordedNotifications = Arc<Mutex<Vec<(String, String, Arc<AtomicBool>)>>>;

#[cfg(test)]
struct RecordingNotificationSink {
    calls: RecordedNotifications,
    accepts: bool,
}

#[cfg(test)]
impl NotificationSink for RecordingNotificationSink {
    fn enqueue(
        &self,
        plugin_id: &str,
        text: PluginNotificationText,
        cancellation: Arc<AtomicBool>,
    ) -> bool {
        if !self.accepts {
            return false;
        }
        self.calls.lock().expect("notifications").push((
            plugin_id.to_owned(),
            text.as_str().to_owned(),
            cancellation,
        ));
        true
    }
}

impl NotificationCapabilityRuntime {
    pub(crate) fn start() -> Self {
        Self::start_with_sink(Arc::new(TrayNotificationSink))
    }

    fn start_with_sink(sink: Arc<dyn NotificationSink>) -> Self {
        let shared = Arc::new(SharedNotificationState {
            state: Mutex::new(NotificationState::default()),
            sink,
            shutdown: AtomicBool::new(false),
        });
        log::info!(
            target: "plugins",
            "component=capabilities event_code=notification_broker_started result_code=ok \
             session_limit=1 plugin_limit_per_minute={NOTIFICATIONS_PER_PLUGIN_PER_MINUTE}"
        );
        Self { client: NotificationCapabilityClient { shared } }
    }

    pub(crate) fn client(&self) -> NotificationCapabilityClient {
        self.client.clone()
    }

    #[cfg(test)]
    pub(crate) fn successful_for_test() -> (Self, RecordedNotifications) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runtime = Self::start_with_sink(Arc::new(RecordingNotificationSink {
            calls: Arc::clone(&calls),
            accepts: true,
        }));
        (runtime, calls)
    }

    #[cfg(test)]
    fn unavailable_for_test() -> Self {
        Self::start_with_sink(Arc::new(RecordingNotificationSink {
            calls: Arc::new(Mutex::new(Vec::new())),
            accepts: false,
        }))
    }

    pub(crate) fn shutdown(&mut self) {
        if self.client.shared.shutdown.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut state =
            self.client.shared.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for session in state.sessions.values() {
            session.cancellation.store(true, Ordering::Release);
        }
        state.sessions.clear();
    }
}

impl Drop for NotificationCapabilityRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LoopbackCapabilityFailure {
    PermissionDenied,
    GrantDenied,
    RateLimited,
    Busy,
    RequestFailed,
    Interrupted,
    DeadlineExceeded,
}

impl LoopbackCapabilityFailure {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::PermissionDenied => "plugin_permission_denied",
            Self::GrantDenied => "plugin_loopback_grant_denied",
            Self::RateLimited => "plugin_rate_limited",
            Self::Busy => "plugin_capability_busy",
            Self::RequestFailed => "plugin_capability_failed",
            Self::Interrupted => "plugin_capability_cancelled",
            Self::DeadlineExceeded => "plugin_capability_timeout",
        }
    }
}

struct LoopbackSession {
    plugin_id: String,
    permitted: bool,
    grants: Vec<PluginLoopbackGrant>,
    accepting: bool,
    accepted_at: VecDeque<Instant>,
    pending: usize,
    cancellation: Arc<AtomicBool>,
    failure: Option<LoopbackCapabilityFailure>,
}

#[derive(Default)]
struct LoopbackState {
    sessions: BTreeMap<Uuid, LoopbackSession>,
    grants_by_plugin: BTreeMap<String, Vec<PluginLoopbackGrant>>,
}

struct SharedLoopbackState {
    state: Mutex<LoopbackState>,
    changed: Condvar,
    shutdown: Arc<AtomicBool>,
}

struct LoopbackWork {
    session_id: Uuid,
    accepted_at: Instant,
    request: PluginLoopbackRequest,
    cancellation: Arc<AtomicBool>,
}

trait LoopbackRunner: Send + 'static {
    fn run(
        &mut self,
        request: &PluginLoopbackRequest,
        accepted_at: Instant,
        cancellation: &AtomicBool,
        shutdown: &AtomicBool,
    ) -> Result<(), loopback::LoopbackTransportFailure>;
}

struct DirectLoopbackRunner;

impl LoopbackRunner for DirectLoopbackRunner {
    fn run(
        &mut self,
        request: &PluginLoopbackRequest,
        accepted_at: Instant,
        cancellation: &AtomicBool,
        shutdown: &AtomicBool,
    ) -> Result<(), loopback::LoopbackTransportFailure> {
        loopback::execute(request, accepted_at, cancellation, shutdown)
    }
}

#[derive(Clone)]
pub(crate) struct LoopbackCapabilityClient {
    work: Sender<LoopbackWork>,
    shared: Arc<SharedLoopbackState>,
}

impl std::fmt::Debug for LoopbackCapabilityClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LoopbackCapabilityClient").finish_non_exhaustive()
    }
}

impl LoopbackCapabilityClient {
    pub(crate) fn open_session(
        &self,
        session_id: Uuid,
        plugin_id: &str,
        permitted: bool,
    ) -> Result<(), LoopbackCapabilityFailure> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(LoopbackCapabilityFailure::Interrupted);
        }
        let mut state = lock_loopback_state(&self.shared);
        if state.sessions.contains_key(&session_id) || !state.sessions.is_empty() {
            return Err(LoopbackCapabilityFailure::Busy);
        }
        let grants = state.grants_by_plugin.get(plugin_id).cloned().unwrap_or_default();
        state.sessions.insert(
            session_id,
            LoopbackSession {
                plugin_id: plugin_id.to_owned(),
                permitted: permitted && !grants.is_empty(),
                grants,
                accepting: true,
                accepted_at: VecDeque::with_capacity(LOOPBACK_REQUESTS_PER_SECOND),
                pending: 0,
                cancellation: Arc::new(AtomicBool::new(false)),
                failure: None,
            },
        );
        Ok(())
    }

    pub(crate) fn failure(&self, session_id: Uuid) -> Option<LoopbackCapabilityFailure> {
        lock_loopback_state(&self.shared)
            .sessions
            .get(&session_id)
            .and_then(|session| session.failure)
    }

    pub(crate) fn cancel_session(&self, session_id: Uuid) {
        let session = lock_loopback_state(&self.shared).sessions.remove(&session_id);
        if let Some(session) = session {
            session.cancellation.store(true, Ordering::Release);
        }
        self.shared.changed.notify_all();
    }

    pub(crate) fn finish_commit(
        &self,
        session_id: Uuid,
        deadline: Instant,
    ) -> Result<(), LoopbackCapabilityFailure> {
        let mut state = lock_loopback_state(&self.shared);
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return Err(LoopbackCapabilityFailure::Interrupted);
        };
        session.accepting = false;
        loop {
            let Some(session) = state.sessions.get(&session_id) else {
                return Err(LoopbackCapabilityFailure::Interrupted);
            };
            if session.pending == 0 {
                break;
            }
            let now = Instant::now();
            if now >= deadline {
                if let Some(session) = state.sessions.remove(&session_id) {
                    session.cancellation.store(true, Ordering::Release);
                }
                return Err(LoopbackCapabilityFailure::DeadlineExceeded);
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

    pub(crate) fn reconcile_grants(
        &self,
        grants_by_plugin: &BTreeMap<String, Vec<PluginLoopbackGrant>>,
    ) {
        let mut state = lock_loopback_state(&self.shared);
        state.grants_by_plugin.clone_from(grants_by_plugin);
        for session in state.sessions.values_mut() {
            let grants = grants_by_plugin.get(&session.plugin_id).cloned().unwrap_or_default();
            if !session.permitted || grants != session.grants {
                fail_loopback_session(session, LoopbackCapabilityFailure::PermissionDenied);
            }
        }
        drop(state);
        self.shared.changed.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn inert_for_test() -> Self {
        let (work, _works) = bounded(1);
        let shutdown = Arc::new(AtomicBool::new(false));
        Self {
            work,
            shared: Arc::new(SharedLoopbackState {
                state: Mutex::new(LoopbackState::default()),
                changed: Condvar::new(),
                shutdown,
            }),
        }
    }

    fn submit(
        &self,
        session_id: Uuid,
        phase: LuaCallbackPhase,
        request: PluginLoopbackRequest,
    ) -> LuaCapabilityStatus {
        if phase == LuaCallbackPhase::Cancel || self.shared.shutdown.load(Ordering::Acquire) {
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Cancelled);
        }
        let mut state = lock_loopback_state(&self.shared);
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InactiveSession);
        };
        if !session.accepting || session.cancellation.load(Ordering::Acquire) {
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Cancelled);
        }
        if !session.permitted {
            fail_loopback_session(session, LoopbackCapabilityFailure::PermissionDenied);
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::PermissionDenied);
        }
        if !session.grants.iter().any(|grant| grant.permits(&request)) {
            fail_loopback_session(session, LoopbackCapabilityFailure::GrantDenied);
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::AllowlistDenied);
        }

        let now = Instant::now();
        while session.accepted_at.front().is_some_and(|accepted| {
            now.saturating_duration_since(*accepted) >= Duration::from_secs(1)
        }) {
            session.accepted_at.pop_front();
        }
        if session.accepted_at.len() >= LOOPBACK_REQUESTS_PER_SECOND {
            fail_loopback_session(session, LoopbackCapabilityFailure::RateLimited);
            return LuaCapabilityStatus::Rejected(LuaCapabilityRejection::RateLimited);
        }

        let work = LoopbackWork {
            session_id,
            accepted_at: now,
            request,
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
                fail_loopback_session(session, LoopbackCapabilityFailure::Busy);
                LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Busy)
            }
        }
    }
}

fn fail_loopback_session(session: &mut LoopbackSession, failure: LoopbackCapabilityFailure) {
    session.failure.get_or_insert(failure);
    session.accepting = false;
    session.cancellation.store(true, Ordering::Release);
}

#[derive(Debug)]
pub(crate) struct LoopbackCapabilityRuntime {
    client: LoopbackCapabilityClient,
    stopped: Receiver<()>,
    join_handle: Option<JoinHandle<()>>,
}

#[cfg(test)]
pub(crate) type RecordedLoopbackRequests = Arc<Mutex<Vec<(String, String, String, Vec<u8>)>>>;

#[cfg(test)]
struct CapturingLoopbackRunner(RecordedLoopbackRequests);

#[cfg(test)]
impl LoopbackRunner for CapturingLoopbackRunner {
    fn run(
        &mut self,
        request: &PluginLoopbackRequest,
        _accepted_at: Instant,
        cancellation: &AtomicBool,
        shutdown: &AtomicBool,
    ) -> Result<(), loopback::LoopbackTransportFailure> {
        if cancellation.load(Ordering::Acquire) || shutdown.load(Ordering::Acquire) {
            return Err(loopback::LoopbackTransportFailure::Interrupted);
        }
        self.0.lock().expect("loopback requests").push((
            request.method().as_str().to_owned(),
            request.origin().as_str().to_owned(),
            request.path().to_owned(),
            request.body().to_vec(),
        ));
        Ok(())
    }
}

impl LoopbackCapabilityRuntime {
    pub(crate) fn start() -> Result<Self> {
        Self::start_with_runner(Box::new(DirectLoopbackRunner))
    }

    fn start_with_runner(mut runner: Box<dyn LoopbackRunner>) -> Result<Self> {
        let (work, works) = bounded(LOOPBACK_QUEUE_CAPACITY);
        let (stopped_sender, stopped) = bounded(1);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(SharedLoopbackState {
            state: Mutex::new(LoopbackState::default()),
            changed: Condvar::new(),
            shutdown,
        });
        let worker_shared = Arc::clone(&shared);
        let join_handle = thread::Builder::new()
            .name("gradial-loopback-broker".to_owned())
            .spawn(move || {
                run_loopback_broker(&works, &worker_shared, runner.as_mut());
                let _sent = stopped_sender.try_send(());
            })
            .context("could not spawn the loopback capability broker")?;
        log::info!(
            target: "plugins",
            "component=capabilities event_code=loopback_broker_started result_code=ok \
             queue_capacity={LOOPBACK_QUEUE_CAPACITY} \
             session_rate_per_second={LOOPBACK_REQUESTS_PER_SECOND} timeout_ms={}",
            loopback::LOOPBACK_OPERATION_TIMEOUT.as_millis()
        );
        Ok(Self {
            client: LoopbackCapabilityClient { work, shared },
            stopped,
            join_handle: Some(join_handle),
        })
    }

    pub(crate) fn client(&self) -> LoopbackCapabilityClient {
        self.client.clone()
    }

    #[cfg(test)]
    pub(crate) fn successful_for_test() -> (Self, RecordedLoopbackRequests) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runtime =
            Self::start_with_runner(Box::new(CapturingLoopbackRunner(Arc::clone(&calls))))
                .expect("test loopback broker");
        (runtime, calls)
    }

    pub(crate) fn shutdown(&mut self) {
        let Some(join_handle) = self.join_handle.take() else {
            return;
        };
        self.client.shared.shutdown.store(true, Ordering::Release);
        {
            let mut state = lock_loopback_state(&self.client.shared);
            for session in state.sessions.values_mut() {
                fail_loopback_session(session, LoopbackCapabilityFailure::Interrupted);
            }
        }
        self.client.shared.changed.notify_all();
        match self.stopped.recv_timeout(SHUTDOWN_WAIT) {
            Ok(()) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                if join_handle.join().is_err() {
                    log::error!(
                        target: "plugins",
                        "component=capabilities event_code=loopback_broker_panicked \
                         result_code=failure"
                    );
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => log::error!(
                target: "plugins",
                "component=capabilities event_code=loopback_broker_shutdown_timeout \
                 result_code=failure wait_ms={}",
                SHUTDOWN_WAIT.as_millis()
            ),
        }
    }
}

impl Drop for LoopbackCapabilityRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_loopback_broker(
    works: &Receiver<LoopbackWork>,
    shared: &SharedLoopbackState,
    runner: &mut dyn LoopbackRunner,
) {
    while !shared.shutdown.load(Ordering::Acquire) {
        let Ok(work) = works.recv_timeout(Duration::from_millis(25)) else {
            continue;
        };
        let result =
            runner.run(&work.request, work.accepted_at, &work.cancellation, &shared.shutdown);
        complete_loopback_work(shared, work.session_id, result);
    }
}

fn complete_loopback_work(
    shared: &SharedLoopbackState,
    session_id: Uuid,
    result: Result<(), loopback::LoopbackTransportFailure>,
) {
    let mut state = lock_loopback_state(shared);
    let Some(session) = state.sessions.get_mut(&session_id) else {
        return;
    };
    session.pending = session.pending.saturating_sub(1);
    if !session.cancellation.load(Ordering::Acquire)
        && let Err(transport_failure) = result
    {
        let failure = match transport_failure {
            loopback::LoopbackTransportFailure::Interrupted => {
                LoopbackCapabilityFailure::Interrupted
            }
            loopback::LoopbackTransportFailure::TimedOut => {
                LoopbackCapabilityFailure::DeadlineExceeded
            }
            loopback::LoopbackTransportFailure::Connection
            | loopback::LoopbackTransportFailure::Protocol
            | loopback::LoopbackTransportFailure::ResponseTooLarge
            | loopback::LoopbackTransportFailure::HttpStatus => {
                LoopbackCapabilityFailure::RequestFailed
            }
        };
        fail_loopback_session(session, failure);
        log::warn!(
            target: "plugins",
            "component=capabilities event_code=loopback_request_completed result_code=failure \
             error_code={} plugin_id={} session_id={session_id}",
            failure.code(),
            session.plugin_id
        );
    }
    drop(state);
    shared.changed.notify_all();
}

fn lock_loopback_state(shared: &SharedLoopbackState) -> std::sync::MutexGuard<'_, LoopbackState> {
    shared.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
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

    fn notification_text(value: &str) -> PluginNotificationText {
        value.parse().expect("notification text")
    }

    #[test]
    fn permitted_notification_is_bounded_and_commit_keeps_it_deliverable() {
        let (mut runtime, calls) = NotificationCapabilityRuntime::successful_for_test();
        let client = runtime.client();
        let session_id = Uuid::from_u128(201);
        client.open_session(session_id, "org.gradial.test", true).expect("open");

        assert_eq!(
            client.submit(
                session_id,
                LuaCallbackPhase::Commit,
                notification_text("Gesture complete")
            ),
            LuaCapabilityStatus::Accepted
        );
        client.finish_commit(session_id).expect("commit");

        let calls = calls.lock().expect("notifications");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "org.gradial.test");
        assert_eq!(calls[0].1, "Gesture complete");
        assert!(!calls[0].2.load(Ordering::Acquire));
        drop(calls);
        runtime.shutdown();
    }

    #[test]
    fn notification_permission_denial_never_reaches_the_sink() {
        let (mut runtime, calls) = NotificationCapabilityRuntime::successful_for_test();
        let client = runtime.client();
        let session_id = Uuid::from_u128(202);
        client.open_session(session_id, "org.gradial.test", false).expect("open");

        assert_eq!(
            client.submit(
                session_id,
                LuaCallbackPhase::Update,
                notification_text("Must not appear")
            ),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::PermissionDenied)
        );
        assert_eq!(
            client.failure(session_id),
            Some(NotificationCapabilityFailure::PermissionDenied)
        );
        assert!(calls.lock().expect("notifications").is_empty());
        client.cancel_session(session_id);
        runtime.shutdown();
    }

    #[test]
    fn notification_session_limit_cancels_its_already_queued_item() {
        let (mut runtime, calls) = NotificationCapabilityRuntime::successful_for_test();
        let client = runtime.client();
        let session_id = Uuid::from_u128(203);
        client.open_session(session_id, "org.gradial.test", true).expect("open");
        assert_eq!(
            client.submit(session_id, LuaCallbackPhase::Update, notification_text("First")),
            LuaCapabilityStatus::Accepted
        );
        assert_eq!(
            client.submit(session_id, LuaCallbackPhase::Update, notification_text("Second")),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::RateLimited)
        );
        assert_eq!(
            client.finish_commit(session_id),
            Err(NotificationCapabilityFailure::RateLimited)
        );
        assert!(calls.lock().expect("notifications")[0].2.load(Ordering::Acquire));
        runtime.shutdown();
    }

    #[test]
    fn notification_plugin_limit_is_shared_across_sessions() {
        let (mut runtime, calls) = NotificationCapabilityRuntime::successful_for_test();
        let client = runtime.client();
        for index in 0..NOTIFICATIONS_PER_PLUGIN_PER_MINUTE {
            let session_id = Uuid::from_u128(210 + index as u128);
            client.open_session(session_id, "org.gradial.test", true).expect("open");
            assert_eq!(
                client.submit(
                    session_id,
                    LuaCallbackPhase::Commit,
                    notification_text("Within limit")
                ),
                LuaCapabilityStatus::Accepted
            );
            client.finish_commit(session_id).expect("commit");
        }
        let rejected = Uuid::from_u128(220);
        client.open_session(rejected, "org.gradial.test", true).expect("open");
        assert_eq!(
            client.submit(rejected, LuaCallbackPhase::Commit, notification_text("Above limit")),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::RateLimited)
        );
        assert_eq!(calls.lock().expect("notifications").len(), 5);
        client.cancel_session(rejected);
        runtime.shutdown();
    }

    #[test]
    fn notification_revocation_cancel_and_shutdown_drop_pending_delivery() {
        let (mut runtime, calls) = NotificationCapabilityRuntime::successful_for_test();
        let client = runtime.client();
        let revoked = Uuid::from_u128(204);
        client.open_session(revoked, "org.gradial.test", true).expect("open");
        assert_eq!(
            client.submit(revoked, LuaCallbackPhase::Begin, notification_text("Pending")),
            LuaCapabilityStatus::Accepted
        );
        client.reconcile_permissions(&BTreeMap::new());
        assert!(calls.lock().expect("notifications")[0].2.load(Ordering::Acquire));
        client.cancel_session(revoked);

        let shutdown = Uuid::from_u128(205);
        client.open_session(shutdown, "org.gradial.test", true).expect("open");
        assert_eq!(
            client.submit(shutdown, LuaCallbackPhase::Update, notification_text("Also pending")),
            LuaCapabilityStatus::Accepted
        );
        runtime.shutdown();
        assert!(calls.lock().expect("notifications")[1].2.load(Ordering::Acquire));
    }

    #[test]
    fn unavailable_notification_sink_fails_closed_as_busy() {
        let mut runtime = NotificationCapabilityRuntime::unavailable_for_test();
        let client = runtime.client();
        let session_id = Uuid::from_u128(206);
        client.open_session(session_id, "org.gradial.test", true).expect("open");

        assert_eq!(
            client.submit(session_id, LuaCallbackPhase::Update, notification_text("Cannot queue")),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Busy)
        );
        assert_eq!(client.failure(session_id), Some(NotificationCapabilityFailure::Busy));
        client.cancel_session(session_id);
        runtime.shutdown();
    }

    #[test]
    fn cancel_callback_cannot_queue_a_notification() {
        let (mut runtime, calls) = NotificationCapabilityRuntime::successful_for_test();
        let client = runtime.client();
        let session_id = Uuid::from_u128(207);
        client.open_session(session_id, "org.gradial.test", true).expect("open");

        assert_eq!(
            client.submit(
                session_id,
                LuaCallbackPhase::Cancel,
                notification_text("Must not appear")
            ),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Cancelled)
        );
        assert!(calls.lock().expect("notifications").is_empty());
        client.cancel_session(session_id);
        runtime.shutdown();
    }

    type RecordedLoopbackCalls = Arc<Mutex<Vec<(String, String, usize)>>>;

    struct RecordingLoopbackRunner {
        calls: RecordedLoopbackCalls,
        result: Result<(), loopback::LoopbackTransportFailure>,
    }

    impl LoopbackRunner for RecordingLoopbackRunner {
        fn run(
            &mut self,
            request: &PluginLoopbackRequest,
            _accepted_at: Instant,
            cancellation: &AtomicBool,
            shutdown: &AtomicBool,
        ) -> Result<(), loopback::LoopbackTransportFailure> {
            if cancellation.load(Ordering::Acquire) || shutdown.load(Ordering::Acquire) {
                return Err(loopback::LoopbackTransportFailure::Interrupted);
            }
            self.calls.lock().expect("loopback calls").push((
                request.method().as_str().to_owned(),
                request.origin().as_str().to_owned(),
                request.body().len(),
            ));
            self.result
        }
    }

    fn loopback_runtime(
        result: Result<(), loopback::LoopbackTransportFailure>,
    ) -> (LoopbackCapabilityRuntime, RecordedLoopbackCalls) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner = RecordingLoopbackRunner { calls: Arc::clone(&calls), result };
        (LoopbackCapabilityRuntime::start_with_runner(Box::new(runner)).expect("runtime"), calls)
    }

    fn loopback_grant(port: u16) -> PluginLoopbackGrant {
        PluginLoopbackGrant {
            origin: format!("http://127.0.0.1:{port}").parse().expect("origin"),
            methods: vec![
                gradial_core::PluginLoopbackMethod::Get,
                gradial_core::PluginLoopbackMethod::Post,
            ],
        }
    }

    fn loopback_request(
        port: u16,
        method: gradial_core::PluginLoopbackMethod,
    ) -> PluginLoopbackRequest {
        PluginLoopbackRequest::new(
            method,
            format!("http://127.0.0.1:{port}").parse().expect("origin"),
            "/private?value=42",
            if method == gradial_core::PluginLoopbackMethod::Post {
                b"private body".to_vec()
            } else {
                Vec::new()
            },
            gradial_core::PluginLoopbackContentType::Text,
        )
        .expect("request")
    }

    fn approve_loopback(client: &LoopbackCapabilityClient, plugin_id: &str, port: u16) {
        client.reconcile_grants(&BTreeMap::from([(
            plugin_id.to_owned(),
            vec![loopback_grant(port)],
        )]));
    }

    #[test]
    fn permitted_loopback_request_runs_off_lua_and_commit_waits() {
        let (mut runtime, calls) = loopback_runtime(Ok(()));
        let client = runtime.client();
        approve_loopback(&client, "org.gradial.test", 18080);
        let session_id = Uuid::from_u128(301);
        client.open_session(session_id, "org.gradial.test", true).expect("open");

        assert_eq!(
            client.submit(
                session_id,
                LuaCallbackPhase::Commit,
                loopback_request(18080, gradial_core::PluginLoopbackMethod::Post),
            ),
            LuaCapabilityStatus::Accepted
        );
        client.finish_commit(session_id, Instant::now() + Duration::from_secs(1)).expect("commit");
        assert_eq!(
            *calls.lock().expect("calls"),
            vec![("POST".to_owned(), "http://127.0.0.1:18080".to_owned(), 12)]
        );
        runtime.shutdown();
    }

    #[test]
    fn loopback_permission_and_allowlist_denials_never_reach_transport() {
        let (mut runtime, calls) = loopback_runtime(Ok(()));
        let client = runtime.client();
        approve_loopback(&client, "org.gradial.test", 18080);

        let denied = Uuid::from_u128(302);
        client.open_session(denied, "org.gradial.test", false).expect("open");
        assert_eq!(
            client.submit(
                denied,
                LuaCallbackPhase::Update,
                loopback_request(18080, gradial_core::PluginLoopbackMethod::Get),
            ),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::PermissionDenied)
        );
        client.cancel_session(denied);

        let wrong_origin = Uuid::from_u128(303);
        client.open_session(wrong_origin, "org.gradial.test", true).expect("open");
        assert_eq!(
            client.submit(
                wrong_origin,
                LuaCallbackPhase::Update,
                loopback_request(18081, gradial_core::PluginLoopbackMethod::Get),
            ),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::AllowlistDenied)
        );
        assert_eq!(client.failure(wrong_origin), Some(LoopbackCapabilityFailure::GrantDenied));
        assert!(calls.lock().expect("calls").is_empty());
        client.cancel_session(wrong_origin);

        client.reconcile_grants(&BTreeMap::from([(
            "org.gradial.test".to_owned(),
            vec![PluginLoopbackGrant {
                origin: "http://127.0.0.1:18080".parse().expect("origin"),
                methods: vec![gradial_core::PluginLoopbackMethod::Get],
            }],
        )]));
        let wrong_method = Uuid::from_u128(309);
        client.open_session(wrong_method, "org.gradial.test", true).expect("open");
        assert_eq!(
            client.submit(
                wrong_method,
                LuaCallbackPhase::Update,
                loopback_request(18080, gradial_core::PluginLoopbackMethod::Post),
            ),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::AllowlistDenied)
        );
        assert!(calls.lock().expect("calls").is_empty());
        client.cancel_session(wrong_method);
        runtime.shutdown();
    }

    #[test]
    fn loopback_rate_revocation_cancel_and_shutdown_fail_closed() {
        let (mut runtime, _calls) = loopback_runtime(Ok(()));
        let client = runtime.client();
        approve_loopback(&client, "org.gradial.test", 18080);
        let session_id = Uuid::from_u128(304);
        client.open_session(session_id, "org.gradial.test", true).expect("open");
        for _ in 0..LOOPBACK_REQUESTS_PER_SECOND {
            assert_eq!(
                client.submit(
                    session_id,
                    LuaCallbackPhase::Update,
                    loopback_request(18080, gradial_core::PluginLoopbackMethod::Get),
                ),
                LuaCapabilityStatus::Accepted
            );
        }
        assert_eq!(
            client.submit(
                session_id,
                LuaCallbackPhase::Update,
                loopback_request(18080, gradial_core::PluginLoopbackMethod::Get),
            ),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::RateLimited)
        );
        client.cancel_session(session_id);

        let revoked = Uuid::from_u128(305);
        client.open_session(revoked, "org.gradial.test", true).expect("open");
        client.reconcile_grants(&BTreeMap::new());
        assert_eq!(client.failure(revoked), Some(LoopbackCapabilityFailure::PermissionDenied));
        client.cancel_session(revoked);

        approve_loopback(&client, "org.gradial.test", 18080);
        let shutdown = Uuid::from_u128(306);
        client.open_session(shutdown, "org.gradial.test", true).expect("open");
        runtime.shutdown();
        assert_eq!(client.failure(shutdown), Some(LoopbackCapabilityFailure::Interrupted));
    }

    #[test]
    fn loopback_transport_timeout_busy_and_cancel_phase_have_stable_results() {
        for (transport, expected) in [
            (
                loopback::LoopbackTransportFailure::Connection,
                LoopbackCapabilityFailure::RequestFailed,
            ),
            (
                loopback::LoopbackTransportFailure::TimedOut,
                LoopbackCapabilityFailure::DeadlineExceeded,
            ),
        ] {
            let (mut runtime, _calls) = loopback_runtime(Err(transport));
            let client = runtime.client();
            approve_loopback(&client, "org.gradial.test", 18080);
            let session_id = Uuid::new_v4();
            client.open_session(session_id, "org.gradial.test", true).expect("open");
            assert_eq!(
                client.submit(
                    session_id,
                    LuaCallbackPhase::Commit,
                    loopback_request(18080, gradial_core::PluginLoopbackMethod::Get),
                ),
                LuaCapabilityStatus::Accepted
            );
            assert_eq!(
                client.finish_commit(session_id, Instant::now() + Duration::from_secs(1)),
                Err(expected)
            );
            runtime.shutdown();
        }

        let client = LoopbackCapabilityClient::inert_for_test();
        approve_loopback(&client, "org.gradial.test", 18080);
        let busy = Uuid::from_u128(307);
        client.open_session(busy, "org.gradial.test", true).expect("open");
        assert_eq!(
            client.submit(
                busy,
                LuaCallbackPhase::Update,
                loopback_request(18080, gradial_core::PluginLoopbackMethod::Get),
            ),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Busy)
        );
        client.cancel_session(busy);

        let (mut runtime, calls) = loopback_runtime(Ok(()));
        let client = runtime.client();
        approve_loopback(&client, "org.gradial.test", 18080);
        let cancelled = Uuid::from_u128(308);
        client.open_session(cancelled, "org.gradial.test", true).expect("open");
        assert_eq!(
            client.submit(
                cancelled,
                LuaCallbackPhase::Cancel,
                loopback_request(18080, gradial_core::PluginLoopbackMethod::Get),
            ),
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::Cancelled)
        );
        assert!(calls.lock().expect("calls").is_empty());
        client.cancel_session(cancelled);
        runtime.shutdown();
    }
}
