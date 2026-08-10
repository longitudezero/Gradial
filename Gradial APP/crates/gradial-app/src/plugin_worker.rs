use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded, select_biased};
use gradial_core::{
    AppPaths, ContinuousBeginRequest, ContinuousCancelReason, ContinuousCancelRequest,
    ContinuousCommitRequest, ContinuousRequest, ContinuousUpdateRequest, PluginActionId, PluginId,
    PluginLoopbackGrant, PluginManifestAction, PluginPermission,
};
use gradial_lua::{LuaLifecycleAdapter, LuaSandboxError, LuaSandboxLimits};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    capability_broker::{
        LoopbackCapabilityClient, LoopbackCapabilityFailure, LoopbackCapabilityRuntime,
        LuaCapabilityBrokerClient, NotificationCapabilityClient, NotificationCapabilityFailure,
        NotificationCapabilityRuntime, ShortcutCapabilityClient, ShortcutCapabilityFailure,
        ShortcutCapabilityRuntime,
    },
    plugin_discovery::{
        DiscoveredPlugin, PluginDiscoveryReport, PluginPackageStatus, discover_plugins,
        log_discovery_report,
    },
    plugin_state::{
        PluginStateFileErrorCode, PluginStateStore, PluginStateStoreError,
        PluginStateTransitionError, PluginTrustState, PluginTrustStatus,
    },
};

const COMMAND_CAPACITY: usize = 8;
const LIFECYCLE_BEGIN_CAPACITY: usize = 1;
const LIFECYCLE_UPDATE_CAPACITY: usize = 1;
const LIFECYCLE_TERMINAL_CAPACITY: usize = 1;
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);
const CALLBACK_TIMING_BUCKET_UPPER_US: [u64; 15] = [
    25,
    50,
    100,
    250,
    500,
    1_000,
    2_000,
    4_000,
    8_000,
    16_000,
    32_000,
    64_000,
    100_000,
    250_000,
    u64::MAX,
];

type PluginActionCatalog = BTreeMap<(String, String), PluginActionAdmission>;
type SharedPluginActionCatalog = Arc<RwLock<Arc<PluginActionCatalog>>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PluginActionAdmission {
    update_hz: u16,
    permissions: u8,
}

const CONTEXT_APPLICATION_PERMISSION: u8 = 1 << 0;
const CONTEXT_WINDOW_TITLE_PERMISSION: u8 = 1 << 1;
const INPUT_SHORTCUT_PERMISSION: u8 = 1 << 2;
const NOTIFICATION_PERMISSION: u8 = 1 << 3;
const NETWORK_LOOPBACK_PERMISSION: u8 = 1 << 4;

impl PluginActionAdmission {
    fn new(update_hz: u16, permissions: &[PluginPermission]) -> Self {
        let permissions = permissions.iter().fold(0, |mask, permission| {
            mask | match permission {
                PluginPermission::ContextApplication => CONTEXT_APPLICATION_PERMISSION,
                PluginPermission::ContextWindowTitle => CONTEXT_WINDOW_TITLE_PERMISSION,
                PluginPermission::InputShortcut => INPUT_SHORTCUT_PERMISSION,
                PluginPermission::Notification => NOTIFICATION_PERMISSION,
                PluginPermission::NetworkLoopback => NETWORK_LOOPBACK_PERMISSION,
            }
        });
        Self { update_hz, permissions }
    }

    pub(crate) const fn update_hz(self) -> u16 {
        self.update_hz
    }

    pub(crate) const fn permits_context_application(self) -> bool {
        self.permissions & CONTEXT_APPLICATION_PERMISSION != 0
    }

    pub(crate) const fn permits_context_window_title(self) -> bool {
        self.permissions & CONTEXT_WINDOW_TITLE_PERMISSION != 0
    }

    pub(crate) const fn permits_input_shortcut(self) -> bool {
        self.permissions & INPUT_SHORTCUT_PERMISSION != 0
    }

    pub(crate) const fn permits_notification(self) -> bool {
        self.permissions & NOTIFICATION_PERMISSION != 0
    }

    pub(crate) const fn permits_network_loopback(self) -> bool {
        self.permissions & NETWORK_LOOPBACK_PERMISSION != 0
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(update_hz: u16, permissions: &[PluginPermission]) -> Self {
        Self::new(update_hz, permissions)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code, reason = "the Plugins page and coordinator will submit all operations")]
pub(crate) enum PluginWorkerOperation {
    ApprovePermissions,
    Enable,
    Reload,
    Disable,
    MarkFaulted,
}

impl PluginWorkerOperation {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ApprovePermissions => "approve_permissions",
            Self::Enable => "enable",
            Self::Reload => "reload",
            Self::Disable => "disable",
            Self::MarkFaulted => "mark_faulted",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginWorkerReceipt {
    pub(crate) operation: PluginWorkerOperation,
    pub(crate) plugin_id: String,
    pub(crate) state_revision: u64,
    pub(crate) loaded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginInventoryStatus {
    Disabled,
    PermissionReviewRequired,
    Enabled,
    ReloadRequired,
    Faulted,
    Incompatible,
    Unavailable,
    Invalid,
}

impl From<PluginTrustStatus> for PluginInventoryStatus {
    fn from(status: PluginTrustStatus) -> Self {
        match status {
            PluginTrustStatus::Disabled => Self::Disabled,
            PluginTrustStatus::PermissionReviewRequired => Self::PermissionReviewRequired,
            PluginTrustStatus::Enabled => Self::Enabled,
            PluginTrustStatus::ReloadRequired => Self::ReloadRequired,
            PluginTrustStatus::Faulted => Self::Faulted,
            PluginTrustStatus::Incompatible => Self::Incompatible,
            PluginTrustStatus::Unavailable => Self::Unavailable,
            PluginTrustStatus::Invalid => Self::Invalid,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginInventoryEntry {
    pub(crate) plugin_id: String,
    pub(crate) display_name: String,
    pub(crate) version: Option<String>,
    pub(crate) api_version: Option<u16>,
    pub(crate) permissions: Vec<PluginPermission>,
    pub(crate) loopback: Vec<PluginLoopbackGrant>,
    pub(crate) actions: Vec<PluginManifestAction>,
    pub(crate) permissions_approved: bool,
    pub(crate) status: PluginInventoryStatus,
    pub(crate) issue_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginWorkerSnapshot {
    pub(crate) state_revision: u64,
    pub(crate) loaded_plugin_ids: Vec<String>,
    pub(crate) plugins: Vec<PluginInventoryEntry>,
    pub(crate) scan_error_code: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum PluginWorkerSubmitError {
    #[error("plugin_worker_plugin_id_invalid")]
    InvalidPluginId,
    #[error("plugin_worker_queue_full")]
    QueueFull,
    #[error("plugin_worker_unavailable")]
    Unavailable,
    #[error("plugin_worker_shutting_down")]
    ShuttingDown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginLifecycleStage {
    Begin,
    Update,
    Commit,
    Cancel,
}

impl PluginLifecycleStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Update => "update",
            Self::Commit => "commit",
            Self::Cancel => "cancel",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Begin => 0,
            Self::Update => 1,
            Self::Commit => 2,
            Self::Cancel => 3,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct FixedTimingHistogram {
    buckets: [u64; CALLBACK_TIMING_BUCKET_UPPER_US.len()],
    count: u64,
    maximum_us: u64,
}

impl FixedTimingHistogram {
    fn record(&mut self, duration: Duration) {
        let micros = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        let bucket = CALLBACK_TIMING_BUCKET_UPPER_US
            .iter()
            .position(|upper| micros <= *upper)
            .unwrap_or(CALLBACK_TIMING_BUCKET_UPPER_US.len() - 1);
        self.buckets[bucket] = self.buckets[bucket].saturating_add(1);
        self.count = self.count.saturating_add(1);
        self.maximum_us = self.maximum_us.max(micros);
    }

    fn percentile(&self, percentile: u64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let rank = self.count.saturating_mul(percentile).div_ceil(100).max(1);
        let mut cumulative = 0_u64;
        for (index, count) in self.buckets.iter().enumerate() {
            cumulative = cumulative.saturating_add(*count);
            if cumulative >= rank {
                return CALLBACK_TIMING_BUCKET_UPPER_US[index].min(self.maximum_us);
            }
        }
        self.maximum_us
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CallbackStageTiming {
    queue: FixedTimingHistogram,
    callback: FixedTimingHistogram,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PluginCallbackTiming {
    stages: [CallbackStageTiming; 4],
    all: CallbackStageTiming,
}

impl PluginCallbackTiming {
    fn record(
        &mut self,
        stage: PluginLifecycleStage,
        queue_duration: Duration,
        callback_duration: Duration,
    ) {
        let timing = &mut self.stages[stage.index()];
        timing.queue.record(queue_duration);
        timing.callback.record(callback_duration);
        self.all.queue.record(queue_duration);
        self.all.callback.record(callback_duration);
    }

    fn log(&self) {
        Self::log_stage("all", &self.all);
        for stage in [
            PluginLifecycleStage::Begin,
            PluginLifecycleStage::Update,
            PluginLifecycleStage::Commit,
            PluginLifecycleStage::Cancel,
        ] {
            let timing = &self.stages[stage.index()];
            Self::log_stage(stage.as_str(), timing);
        }
    }

    fn log_stage(stage: &str, timing: &CallbackStageTiming) {
        log::info!(
            target: "plugins",
            "component=plugins event_code=plugin_callback_timing result_code=ok \
             stage={} callback_count={} p50_callback_us={} p95_callback_us={} \
             p99_callback_us={} maximum_callback_us={} p50_queue_us={} \
             p95_queue_us={} p99_queue_us={} maximum_queue_us={}",
            stage,
            timing.callback.count,
            timing.callback.percentile(50),
            timing.callback.percentile(95),
            timing.callback.percentile(99),
            timing.callback.maximum_us,
            timing.queue.percentile(50),
            timing.queue.percentile(95),
            timing.queue.percentile(99),
            timing.queue.maximum_us
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum PluginLifecycleSubmitError {
    #[error("plugin_lifecycle_queue_full")]
    QueueFull(PluginLifecycleStage),
    #[error("plugin_worker_unavailable")]
    Unavailable,
    #[error("plugin_worker_shutting_down")]
    ShuttingDown,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum PluginLifecycleError {
    #[error("plugin_lifecycle_plugin_unavailable")]
    PluginUnavailable,
    #[error("plugin_lifecycle_action_unavailable")]
    ActionUnavailable,
    #[error("plugin_lifecycle_parameters_invalid")]
    ParametersInvalid,
    #[error("plugin_lifecycle_begin_aborted")]
    BeginAborted,
    #[error("plugin_lifecycle_session_active")]
    SessionActive,
    #[error("plugin_lifecycle_session_not_active")]
    SessionNotActive,
    #[error("{0}")]
    Lua(LuaSandboxError),
}

impl PluginLifecycleError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::PluginUnavailable => "plugin_unavailable",
            Self::ActionUnavailable => "action_unavailable",
            Self::ParametersInvalid => "parameters_invalid",
            Self::BeginAborted => "begin_aborted",
            Self::SessionActive => "session_active",
            Self::SessionNotActive => "session_not_active",
            Self::Lua(error) => lua_error_code(error),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum PluginWorkerError {
    #[error("plugin_worker_discovery_unavailable")]
    DiscoveryUnavailable,
    #[error("plugin_worker_package_missing")]
    PackageMissing,
    #[error("plugin_worker_already_enabled")]
    AlreadyEnabled,
    #[error("plugin_worker_reload_required")]
    ReloadRequired,
    #[error("plugin_worker_faulted")]
    Faulted,
    #[error("plugin_worker_unavailable")]
    Unavailable,
    #[error("plugin_worker_session_active")]
    SessionActive,
    #[error("{0}")]
    StateTransition(PluginStateTransitionError),
    #[error("{0}")]
    StatePersistence(PluginStateStoreError),
    #[error("{0}")]
    Lua(LuaSandboxError),
}

impl PluginWorkerError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::DiscoveryUnavailable => "discovery_unavailable",
            Self::PackageMissing => "package_missing",
            Self::AlreadyEnabled => "already_enabled",
            Self::ReloadRequired => "reload_required",
            Self::Faulted => "faulted",
            Self::Unavailable => "unavailable",
            Self::SessionActive => "session_active",
            Self::StateTransition(error) => transition_error_code(error),
            Self::StatePersistence(error) => error.code(),
            Self::Lua(error) => lua_error_code(error),
        }
    }
}

const fn transition_error_code(error: PluginStateTransitionError) -> &'static str {
    match error {
        PluginStateTransitionError::InvalidPackage => "package_invalid",
        PluginStateTransitionError::Incompatible => "incompatible",
        PluginStateTransitionError::ApprovalRequired => "approval_required",
        PluginStateTransitionError::UnknownPlugin => "plugin_unknown",
        PluginStateTransitionError::NotEnabled => "not_enabled",
    }
}

const fn lua_error_code(error: LuaSandboxError) -> &'static str {
    match error {
        LuaSandboxError::InvalidLimits => "lua_limits_invalid",
        LuaSandboxError::Initialization => "lua_initialization_failed",
        LuaSandboxError::MemoryLimit => "lua_memory_limit",
        LuaSandboxError::InstructionLimit => "lua_instruction_limit",
        LuaSandboxError::DeadlineExceeded => "lua_deadline_exceeded",
        LuaSandboxError::ModuleNameInvalid => "module_name_invalid",
        LuaSandboxError::ModuleNotFound => "module_not_found",
        LuaSandboxError::ModuleCycle => "module_cycle",
        LuaSandboxError::Syntax => "lua_syntax_error",
        LuaSandboxError::Runtime => "lua_runtime_error",
        LuaSandboxError::EntryNotTable => "entry_not_table",
        LuaSandboxError::CallbackMissing { .. } => "callback_missing",
        LuaSandboxError::CallbackNotFunction { .. } => "callback_not_function",
        LuaSandboxError::BeginStateNotTable => "begin_state_not_table",
        LuaSandboxError::CallbackReturnInvalid => "callback_return_invalid",
        LuaSandboxError::SessionAlreadyActive => "session_already_active",
        LuaSandboxError::SessionNotActive => "session_not_active",
        LuaSandboxError::PluginFaulted => "plugin_faulted",
        LuaSandboxError::HostValueInvalid => "host_value_invalid",
    }
}

#[derive(Debug)]
pub(crate) struct PluginWorkerTicket {
    reply: Receiver<Result<PluginWorkerReceipt, PluginWorkerError>>,
}

impl PluginWorkerTicket {
    #[allow(dead_code, reason = "the Plugins page will poll operation completion")]
    pub(crate) fn try_result(&self) -> Option<Result<PluginWorkerReceipt, PluginWorkerError>> {
        match self.reply.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(PluginWorkerError::Unavailable)),
        }
    }

    #[cfg(test)]
    fn receive(self) -> Result<PluginWorkerReceipt, PluginWorkerError> {
        self.reply.recv_timeout(SHUTDOWN_WAIT).unwrap_or(Err(PluginWorkerError::Unavailable))
    }
}

#[derive(Debug)]
pub(crate) struct PluginWorkerSnapshotTicket {
    reply: Receiver<PluginWorkerSnapshot>,
}

impl PluginWorkerSnapshotTicket {
    #[allow(dead_code, reason = "the Plugins page will poll immutable worker snapshots")]
    pub(crate) fn try_result(&self) -> Option<Result<PluginWorkerSnapshot, PluginWorkerError>> {
        match self.reply.try_recv() {
            Ok(snapshot) => Some(Ok(snapshot)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(PluginWorkerError::Unavailable)),
        }
    }

    #[cfg(test)]
    fn receive(self) -> PluginWorkerSnapshot {
        self.reply.recv_timeout(SHUTDOWN_WAIT).expect("plugin worker snapshot")
    }
}

#[derive(Debug)]
pub(crate) struct PluginLifecycleTicket {
    reply: Receiver<Result<(), PluginLifecycleError>>,
}

impl PluginLifecycleTicket {
    #[allow(dead_code, reason = "the live continuous backend will wait within its deadline")]
    pub(crate) fn try_result(&self) -> Option<Result<(), PluginLifecycleError>> {
        match self.reply.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(PluginLifecycleError::PluginUnavailable)),
        }
    }

    #[cfg(test)]
    fn receive(self) -> Result<(), PluginLifecycleError> {
        self.reply
            .recv_timeout(SHUTDOWN_WAIT)
            .unwrap_or(Err(PluginLifecycleError::PluginUnavailable))
    }
}

#[derive(Debug)]
enum PluginLifecycleCommand {
    Request {
        request: Box<ContinuousRequest>,
        submitted_at: Instant,
        reply: Sender<Result<(), PluginLifecycleError>>,
    },
    AbortBegin {
        session_id: Uuid,
        reason: ContinuousCancelReason,
    },
}

#[derive(Debug)]
enum PluginWorkerCommand {
    Operate {
        operation: PluginWorkerOperation,
        plugin_id: String,
        reply: Sender<Result<PluginWorkerReceipt, PluginWorkerError>>,
    },
    Snapshot {
        reply: Sender<PluginWorkerSnapshot>,
    },
    Rescan {
        reply: Sender<PluginWorkerSnapshot>,
    },
    Shutdown,
}

#[derive(Clone, Debug)]
pub(crate) struct PluginLifecycleClient {
    begins: Sender<PluginLifecycleCommand>,
    updates: Sender<PluginLifecycleCommand>,
    terminals: Sender<PluginLifecycleCommand>,
    action_catalog: SharedPluginActionCatalog,
    shortcut_capabilities: ShortcutCapabilityClient,
    notification_capabilities: NotificationCapabilityClient,
    loopback_capabilities: LoopbackCapabilityClient,
    shutdown_requested: Arc<AtomicBool>,
}

impl PluginLifecycleClient {
    #[cfg(test)]
    pub(crate) fn with_test_action(plugin_id: &str, action_id: &str, update_hz: u16) -> Self {
        Self::with_test_action_and_permissions(plugin_id, action_id, update_hz, &[])
    }

    #[cfg(test)]
    pub(crate) fn with_test_action_and_permissions(
        plugin_id: &str,
        action_id: &str,
        update_hz: u16,
        permissions: &[PluginPermission],
    ) -> Self {
        let (begins, _begin_receiver) = bounded(1);
        let (updates, _update_receiver) = bounded(1);
        let (terminals, _terminal_receiver) = bounded(1);
        let action_catalog = BTreeMap::from([(
            (plugin_id.to_owned(), action_id.to_owned()),
            PluginActionAdmission::new(update_hz, permissions),
        )]);
        Self {
            begins,
            updates,
            terminals,
            action_catalog: Arc::new(RwLock::new(Arc::new(action_catalog))),
            shortcut_capabilities: ShortcutCapabilityClient::inert_for_test(),
            notification_capabilities: NotificationCapabilityClient::inert_for_test(),
            loopback_capabilities: LoopbackCapabilityClient::inert_for_test(),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn request(
        &self,
        request: ContinuousRequest,
    ) -> Result<PluginLifecycleTicket, PluginLifecycleSubmitError> {
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err(PluginLifecycleSubmitError::ShuttingDown);
        }
        let stage = lifecycle_stage(&request);
        let sender = match stage {
            PluginLifecycleStage::Begin => &self.begins,
            PluginLifecycleStage::Update => &self.updates,
            PluginLifecycleStage::Commit | PluginLifecycleStage::Cancel => &self.terminals,
        };
        let (reply, replies) = bounded(1);
        let command = PluginLifecycleCommand::Request {
            request: Box::new(request),
            submitted_at: Instant::now(),
            reply,
        };
        match sender.try_send(command) {
            Ok(()) => Ok(PluginLifecycleTicket { reply: replies }),
            Err(TrySendError::Full(_)) => Err(PluginLifecycleSubmitError::QueueFull(stage)),
            Err(TrySendError::Disconnected(_)) => Err(PluginLifecycleSubmitError::Unavailable),
        }
    }

    pub(crate) fn action_admission(
        &self,
        plugin_id: &PluginId,
        action_id: &PluginActionId,
    ) -> Option<PluginActionAdmission> {
        let catalog = self.action_catalog.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        catalog.get(&(plugin_id.as_str().to_owned(), action_id.as_str().to_owned())).copied()
    }

    pub(crate) fn abort_begin(
        &self,
        session_id: Uuid,
        reason: ContinuousCancelReason,
    ) -> Result<(), PluginLifecycleSubmitError> {
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err(PluginLifecycleSubmitError::ShuttingDown);
        }
        match self.terminals.try_send(PluginLifecycleCommand::AbortBegin { session_id, reason }) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                Err(PluginLifecycleSubmitError::QueueFull(PluginLifecycleStage::Cancel))
            }
            Err(TrySendError::Disconnected(_)) => Err(PluginLifecycleSubmitError::Unavailable),
        }
    }

    pub(crate) fn open_shortcut_session(
        &self,
        session_id: Uuid,
        plugin_id: &PluginId,
        target: Arc<gradial_core::InvocationContext>,
        permitted: bool,
    ) -> Result<(), ShortcutCapabilityFailure> {
        self.shortcut_capabilities.open_session(session_id, plugin_id.as_str(), target, permitted)
    }

    pub(crate) fn shortcut_failure(&self, session_id: Uuid) -> Option<ShortcutCapabilityFailure> {
        self.shortcut_capabilities.failure(session_id)
    }

    pub(crate) fn cancel_shortcut_session(&self, session_id: Uuid) {
        self.shortcut_capabilities.cancel_session(session_id);
    }

    pub(crate) fn finish_shortcut_commit(
        &self,
        session_id: Uuid,
        deadline: std::time::Instant,
    ) -> Result<(), ShortcutCapabilityFailure> {
        self.shortcut_capabilities.finish_commit(session_id, deadline)
    }

    pub(crate) fn open_notification_session(
        &self,
        session_id: Uuid,
        plugin_id: &PluginId,
        permitted: bool,
    ) -> Result<(), NotificationCapabilityFailure> {
        self.notification_capabilities.open_session(session_id, plugin_id.as_str(), permitted)
    }

    pub(crate) fn notification_failure(
        &self,
        session_id: Uuid,
    ) -> Option<NotificationCapabilityFailure> {
        self.notification_capabilities.failure(session_id)
    }

    pub(crate) fn cancel_notification_session(&self, session_id: Uuid) {
        self.notification_capabilities.cancel_session(session_id);
    }

    pub(crate) fn finish_notification_commit(
        &self,
        session_id: Uuid,
    ) -> Result<(), NotificationCapabilityFailure> {
        self.notification_capabilities.finish_commit(session_id)
    }

    pub(crate) fn open_loopback_session(
        &self,
        session_id: Uuid,
        plugin_id: &PluginId,
        permitted: bool,
    ) -> Result<(), LoopbackCapabilityFailure> {
        self.loopback_capabilities.open_session(session_id, plugin_id.as_str(), permitted)
    }

    pub(crate) fn loopback_failure(&self, session_id: Uuid) -> Option<LoopbackCapabilityFailure> {
        self.loopback_capabilities.failure(session_id)
    }

    pub(crate) fn cancel_loopback_session(&self, session_id: Uuid) {
        self.loopback_capabilities.cancel_session(session_id);
    }

    pub(crate) fn finish_loopback_commit(
        &self,
        session_id: Uuid,
        deadline: std::time::Instant,
    ) -> Result<(), LoopbackCapabilityFailure> {
        self.loopback_capabilities.finish_commit(session_id, deadline)
    }
}

#[derive(Debug)]
pub(crate) struct PluginWorkerRuntime {
    commands: Sender<PluginWorkerCommand>,
    lifecycle: PluginLifecycleClient,
    shutdown_requested: Arc<AtomicBool>,
    stopped: Receiver<()>,
    join_handle: Option<JoinHandle<()>>,
    shortcut_capabilities: ShortcutCapabilityRuntime,
    notification_capabilities: NotificationCapabilityRuntime,
    loopback_capabilities: LoopbackCapabilityRuntime,
}

impl PluginWorkerRuntime {
    pub(crate) fn start(paths: AppPaths) -> Result<Self> {
        let shortcut_capabilities = ShortcutCapabilityRuntime::start()
            .context("could not start the shortcut capability broker")?;
        let notification_capabilities = NotificationCapabilityRuntime::start();
        let loopback_capabilities = LoopbackCapabilityRuntime::start()
            .context("could not start the loopback capability broker")?;
        Self::start_with_all_capabilities(
            paths,
            shortcut_capabilities,
            notification_capabilities,
            loopback_capabilities,
        )
    }

    #[cfg(test)]
    fn start_with_shortcut_capabilities(
        paths: AppPaths,
        shortcut_capabilities: ShortcutCapabilityRuntime,
    ) -> Result<Self> {
        Self::start_with_capabilities(
            paths,
            shortcut_capabilities,
            NotificationCapabilityRuntime::start(),
        )
    }

    #[cfg(test)]
    fn start_with_capabilities(
        paths: AppPaths,
        shortcut_capabilities: ShortcutCapabilityRuntime,
        notification_capabilities: NotificationCapabilityRuntime,
    ) -> Result<Self> {
        let loopback_capabilities = LoopbackCapabilityRuntime::start()
            .context("could not start the test loopback capability broker")?;
        Self::start_with_all_capabilities(
            paths,
            shortcut_capabilities,
            notification_capabilities,
            loopback_capabilities,
        )
    }

    fn start_with_all_capabilities(
        paths: AppPaths,
        shortcut_capabilities: ShortcutCapabilityRuntime,
        notification_capabilities: NotificationCapabilityRuntime,
        loopback_capabilities: LoopbackCapabilityRuntime,
    ) -> Result<Self> {
        let shortcut_client = shortcut_capabilities.client();
        let notification_client = notification_capabilities.client();
        let loopback_client = loopback_capabilities.client();
        let (command_sender, commands) = bounded(COMMAND_CAPACITY);
        let (begin_sender, begins) = bounded(LIFECYCLE_BEGIN_CAPACITY);
        let (update_sender, updates) = bounded(LIFECYCLE_UPDATE_CAPACITY);
        let (terminal_sender, terminals) = bounded(LIFECYCLE_TERMINAL_CAPACITY);
        let (stopped_sender, stopped) = bounded(1);
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let action_catalog = Arc::new(RwLock::new(Arc::new(BTreeMap::new())));
        let worker_shutdown = Arc::clone(&shutdown_requested);
        let worker_action_catalog = Arc::clone(&action_catalog);
        let worker_shortcut_client = shortcut_client.clone();
        let worker_notification_client = notification_client.clone();
        let worker_loopback_client = loopback_client.clone();
        let join_handle = thread::Builder::new()
            .name("gradial-plugin-worker".to_owned())
            .spawn(move || {
                let mut worker = PluginWorker::bootstrap(
                    paths,
                    &worker_shutdown,
                    worker_action_catalog,
                    worker_shortcut_client,
                    worker_notification_client,
                    worker_loopback_client,
                );
                worker.run(&commands, &begins, &updates, &terminals, &worker_shutdown);
                worker.callback_timing.log();
                let _sent = stopped_sender.try_send(());
            })
            .context("could not spawn the plugin worker")?;
        log::info!(
            target: "plugins",
            "component=plugins event_code=plugin_worker_started result_code=ok \
             command_capacity={COMMAND_CAPACITY} lifecycle_begin_capacity={LIFECYCLE_BEGIN_CAPACITY} \
             lifecycle_update_capacity={LIFECYCLE_UPDATE_CAPACITY} \
             lifecycle_terminal_capacity={LIFECYCLE_TERMINAL_CAPACITY} shutdown_wait_ms={}",
            SHUTDOWN_WAIT.as_millis()
        );
        let lifecycle = PluginLifecycleClient {
            begins: begin_sender,
            updates: update_sender,
            terminals: terminal_sender,
            action_catalog,
            shortcut_capabilities: shortcut_client,
            notification_capabilities: notification_client,
            loopback_capabilities: loopback_client,
            shutdown_requested: Arc::clone(&shutdown_requested),
        };
        Ok(Self {
            commands: command_sender,
            lifecycle,
            shutdown_requested,
            stopped,
            join_handle: Some(join_handle),
            shortcut_capabilities,
            notification_capabilities,
            loopback_capabilities,
        })
    }

    pub(crate) fn lifecycle_client(&self) -> PluginLifecycleClient {
        self.lifecycle.clone()
    }

    #[allow(dead_code, reason = "the Plugins page and coordinator will submit these operations")]
    pub(crate) fn request(
        &self,
        operation: PluginWorkerOperation,
        plugin_id: impl Into<String>,
    ) -> Result<PluginWorkerTicket, PluginWorkerSubmitError> {
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err(PluginWorkerSubmitError::ShuttingDown);
        }
        let plugin_id = plugin_id.into();
        PluginId::new(plugin_id.clone()).map_err(|_| PluginWorkerSubmitError::InvalidPluginId)?;
        let (reply, replies) = bounded(1);
        let command = PluginWorkerCommand::Operate { operation, plugin_id, reply };
        match self.commands.try_send(command) {
            Ok(()) => Ok(PluginWorkerTicket { reply: replies }),
            Err(TrySendError::Full(_)) => Err(PluginWorkerSubmitError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(PluginWorkerSubmitError::Unavailable),
        }
    }

    #[allow(dead_code, reason = "the Plugins page will request immutable worker snapshots")]
    pub(crate) fn request_snapshot(
        &self,
    ) -> Result<PluginWorkerSnapshotTicket, PluginWorkerSubmitError> {
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err(PluginWorkerSubmitError::ShuttingDown);
        }
        let (reply, replies) = bounded(1);
        match self.commands.try_send(PluginWorkerCommand::Snapshot { reply }) {
            Ok(()) => Ok(PluginWorkerSnapshotTicket { reply: replies }),
            Err(TrySendError::Full(_)) => Err(PluginWorkerSubmitError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(PluginWorkerSubmitError::Unavailable),
        }
    }

    pub(crate) fn request_rescan(
        &self,
    ) -> Result<PluginWorkerSnapshotTicket, PluginWorkerSubmitError> {
        if self.shutdown_requested.load(Ordering::Acquire) {
            return Err(PluginWorkerSubmitError::ShuttingDown);
        }
        let (reply, replies) = bounded(1);
        match self.commands.try_send(PluginWorkerCommand::Rescan { reply }) {
            Ok(()) => Ok(PluginWorkerSnapshotTicket { reply: replies }),
            Err(TrySendError::Full(_)) => Err(PluginWorkerSubmitError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(PluginWorkerSubmitError::Unavailable),
        }
    }

    pub(crate) fn shutdown(&mut self) {
        let Some(join_handle) = self.join_handle.take() else {
            return;
        };
        self.shutdown_requested.store(true, Ordering::Release);
        let _wake = self.commands.try_send(PluginWorkerCommand::Shutdown);
        log::info!(
            target: "plugins",
            "component=plugins event_code=plugin_worker_shutdown_requested result_code=ok \
             wait_ms={}",
            SHUTDOWN_WAIT.as_millis()
        );
        match self.stopped.recv_timeout(SHUTDOWN_WAIT) {
            Ok(()) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                if join_handle.join().is_err() {
                    log::error!(
                        target: "plugins",
                        "component=plugins event_code=plugin_worker_panicked result_code=failure"
                    );
                } else {
                    log::info!(
                        target: "plugins",
                        "component=plugins event_code=plugin_worker_stopped result_code=ok"
                    );
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => log::error!(
                target: "plugins",
                "component=plugins event_code=plugin_worker_shutdown_timeout \
                 result_code=failure error_code=shutdown_timeout wait_ms={}",
                SHUTDOWN_WAIT.as_millis()
            ),
        }
        self.loopback_capabilities.shutdown();
        self.notification_capabilities.shutdown();
        self.shortcut_capabilities.shutdown();
    }
}

impl Drop for PluginWorkerRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct PluginWorker {
    paths: AppPaths,
    store: PluginStateStore,
    state: PluginTrustState,
    adapters: BTreeMap<String, LoadedPlugin>,
    action_catalog: SharedPluginActionCatalog,
    shortcut_capabilities: ShortcutCapabilityClient,
    notification_capabilities: NotificationCapabilityClient,
    loopback_capabilities: LoopbackCapabilityClient,
    callback_timing: PluginCallbackTiming,
    aborted_begins: BTreeSet<Uuid>,
    active_session: Option<ActivePluginSession>,
    reconciliation_deferred: bool,
    inventory: Vec<PluginInventoryEntry>,
    scan_error_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActivePluginSession {
    session_id: Uuid,
    plugin_id: String,
}

struct LoadedPlugin {
    adapter: LuaLifecycleAdapter,
    actions: BTreeMap<String, PluginManifestAction>,
    permissions: Vec<PluginPermission>,
    loopback: Vec<PluginLoopbackGrant>,
}

impl PluginWorker {
    fn bootstrap(
        paths: AppPaths,
        shutdown_requested: &AtomicBool,
        action_catalog: SharedPluginActionCatalog,
        shortcut_capabilities: ShortcutCapabilityClient,
        notification_capabilities: NotificationCapabilityClient,
        loopback_capabilities: LoopbackCapabilityClient,
    ) -> Self {
        let store = PluginStateStore::new(paths.clone());
        let loaded = store.load_or_initialize();
        log::info!(
            target: "plugins",
            "component=plugins event_code=plugin_state_loaded result_code=ok state_revision={} \
             source={} tracked_plugins={}",
            loaded.state.revision(),
            loaded.source.as_str(),
            loaded.state.plugin_count()
        );
        if let Some(warning) = loaded.warning {
            log::error!(
                target: "plugins",
                "component=plugins event_code=plugin_state_recovery result_code=failure \
                 primary_error={} backup_error={}",
                warning.primary_error.as_str(),
                warning
                    .backup_error
                    .map_or("none", PluginStateFileErrorCode::as_str)
            );
        }

        let mut worker = Self {
            paths,
            store,
            state: loaded.state,
            adapters: BTreeMap::new(),
            action_catalog,
            shortcut_capabilities,
            notification_capabilities,
            loopback_capabilities,
            callback_timing: PluginCallbackTiming::default(),
            aborted_begins: BTreeSet::new(),
            active_session: None,
            reconciliation_deferred: false,
            inventory: Vec::new(),
            scan_error_code: None,
        };
        worker.bootstrap_packages(shutdown_requested);
        worker.publish_action_catalog();
        worker
    }

    fn bootstrap_packages(&mut self, shutdown_requested: &AtomicBool) {
        let report = discover_plugins(&self.paths.plugins);
        log_discovery_report(&report);
        if let Some(error) = report.root_error {
            self.scan_error_code = Some(error.to_string());
        }
        let (reconciliation, _persistence_error) = self.reconcile_report(&report);
        let counts = reconciliation.status_counts();
        log::info!(
            target: "plugins",
            "component=plugins event_code=plugin_state_status result_code=ok state_revision={} \
             scan_available={} enabled={} disabled={} reload_required={} faulted={} \
             permission_review_required={} incompatible={} unavailable={} invalid={}",
            self.state.revision(),
            reconciliation.scan_available,
            counts.get("enabled").copied().unwrap_or(0),
            counts.get("disabled").copied().unwrap_or(0),
            counts.get("reload_required").copied().unwrap_or(0),
            counts.get("faulted").copied().unwrap_or(0),
            counts.get("permission_review_required").copied().unwrap_or(0),
            counts.get("incompatible").copied().unwrap_or(0),
            counts.get("unavailable").copied().unwrap_or(0),
            counts.get("invalid").copied().unwrap_or(0)
        );
        if report.root_error.is_none() {
            for package in &report.packages {
                if shutdown_requested.load(Ordering::Acquire) {
                    break;
                }
                if self.state.status(package) != Ok(PluginTrustStatus::Enabled) {
                    continue;
                }
                let plugin_id = package.directory_name.clone();
                match load_plugin(
                    package,
                    &self.shortcut_capabilities,
                    &self.notification_capabilities,
                    &self.loopback_capabilities,
                ) {
                    Ok(plugin) => {
                        self.adapters.insert(plugin_id.clone(), plugin);
                        log_operation(
                            PluginWorkerOperation::Enable,
                            &plugin_id,
                            Ok(()),
                            self.state.revision(),
                            true,
                            "startup",
                        );
                    }
                    Err(error) => {
                        self.persist_adapter_fault(&plugin_id);
                        log_operation(
                            PluginWorkerOperation::Enable,
                            &plugin_id,
                            Err(PluginWorkerError::Lua(error)),
                            self.state.revision(),
                            false,
                            "startup",
                        );
                    }
                }
            }
            self.update_inventory(&report);
        }
        log::info!(
            target: "plugins",
            "component=plugins event_code=plugin_worker_ready result_code=ok \
             state_revision={} loaded_plugins={}",
            self.state.revision(),
            self.adapters.len()
        );
    }

    fn run(
        &mut self,
        commands: &Receiver<PluginWorkerCommand>,
        begins: &Receiver<PluginLifecycleCommand>,
        updates: &Receiver<PluginLifecycleCommand>,
        terminals: &Receiver<PluginLifecycleCommand>,
        shutdown_requested: &AtomicBool,
    ) {
        while !shutdown_requested.load(Ordering::Acquire) {
            select_biased! {
                recv(terminals) -> command => {
                    let Ok(command) = command else { break };
                    self.handle_lifecycle_command(command);
                }
                recv(begins) -> command => {
                    let Ok(command) = command else { break };
                    self.handle_lifecycle_command(command);
                }
                recv(updates) -> command => {
                    let Ok(command) = command else { break };
                    self.handle_lifecycle_command(command);
                }
                recv(commands) -> command => {
                    let Ok(command) = command else { break };
                    if !self.handle_worker_command(command) {
                        break;
                    }
                }
            }
        }
        self.active_session = None;
        self.aborted_begins.clear();
        self.adapters.clear();
        self.publish_action_catalog();
    }

    fn handle_worker_command(&mut self, command: PluginWorkerCommand) -> bool {
        match command {
            PluginWorkerCommand::Operate { operation, plugin_id, reply } => {
                let result = self.operate(operation, &plugin_id);
                if matches!(&result, Err(PluginWorkerError::SessionActive)) {
                    let _deferred = self.rescan();
                } else {
                    self.refresh_inventory_view();
                }
                log_operation(
                    operation,
                    &plugin_id,
                    result.as_ref().map(|_| ()).map_err(|error| *error),
                    self.state.revision(),
                    self.adapters.contains_key(&plugin_id),
                    "request",
                );
                self.publish_action_catalog();
                let _sent = reply.try_send(result);
            }
            PluginWorkerCommand::Snapshot { reply } => {
                let _sent = reply.try_send(self.snapshot());
            }
            PluginWorkerCommand::Rescan { reply } => {
                let result = self.rescan();
                log::info!(
                    target: "plugins",
                    "component=plugins event_code=plugin_rescan result_code={} \
                     state_revision={} plugin_count={} active_session={} error_code={}",
                    if result.is_ok() { "ok" } else { "failure" },
                    self.state.revision(),
                    self.inventory.len(),
                    self.active_session.is_some(),
                    result.err().map_or("none", PluginWorkerError::code)
                );
                self.publish_action_catalog();
                let _sent = reply.try_send(self.snapshot());
            }
            PluginWorkerCommand::Shutdown => return false,
        }
        true
    }

    fn handle_lifecycle_command(&mut self, command: PluginLifecycleCommand) {
        let (request, submitted_at, reply) = match command {
            PluginLifecycleCommand::Request { request, submitted_at, reply } => {
                (*request, submitted_at, reply)
            }
            PluginLifecycleCommand::AbortBegin { session_id, reason } => {
                self.abort_begin(session_id, reason);
                self.publish_action_catalog();
                return;
            }
        };
        let stage = lifecycle_stage(&request);
        let session_id = request.session_id();
        let plugin_id = match &request {
            ContinuousRequest::Begin(request) => request.plugin_id().as_str().to_owned(),
            ContinuousRequest::Update(_)
            | ContinuousRequest::Commit(_)
            | ContinuousRequest::Cancel(_) => self
                .active_session
                .as_ref()
                .map_or_else(|| "none".to_owned(), |active| active.plugin_id.clone()),
        };
        let callback_started = Instant::now();
        let queue_duration = callback_started.saturating_duration_since(submitted_at);
        let result = self.dispatch_lifecycle(&request);
        let callback_duration = callback_started.elapsed();
        self.callback_timing.record(stage, queue_duration, callback_duration);
        log::info!(
            target: "plugins",
            "component=plugins event_code=plugin_lifecycle result_code={} error_code={} \
             plugin_id={} session_id={} stage={} active_session={} queue_wait_us={} \
             callback_duration_us={} total_duration_us={}",
            if result.is_ok() { "ok" } else { "failure" },
            result.as_ref().err().map_or("none", |error| error.code()),
            plugin_id,
            session_id,
            stage.as_str(),
            self.active_session.is_some(),
            queue_duration.as_micros(),
            callback_duration.as_micros(),
            submitted_at.elapsed().as_micros()
        );
        if result.is_err()
            || matches!(stage, PluginLifecycleStage::Commit | PluginLifecycleStage::Cancel)
        {
            self.publish_action_catalog();
        }
        let _sent = reply.try_send(result);
    }

    fn abort_begin(&mut self, session_id: Uuid, reason: ContinuousCancelReason) {
        if self.active_session.as_ref().is_some_and(|active| active.session_id == session_id) {
            let request = ContinuousCancelRequest::new(session_id, reason)
                .expect("worker lifecycle session identifiers are non-nil");
            let result = self.cancel_lifecycle(request);
            log::warn!(
                target: "plugins",
                "component=plugins event_code=plugin_lifecycle_begin_aborted result_code={} \
                 error_code={} session_id={} phase=active",
                if result.is_ok() { "ok" } else { "failure" },
                result.as_ref().err().map_or("none", |error| error.code()),
                session_id
            );
            return;
        }
        self.aborted_begins.insert(session_id);
        log::warn!(
            target: "plugins",
            "component=plugins event_code=plugin_lifecycle_begin_aborted result_code=ok \
             error_code=none session_id={session_id} phase=pending"
        );
    }

    fn dispatch_lifecycle(
        &mut self,
        request: &ContinuousRequest,
    ) -> Result<(), PluginLifecycleError> {
        match request {
            ContinuousRequest::Begin(request) => self.begin_lifecycle(request),
            ContinuousRequest::Update(request) => self.update_lifecycle(*request),
            ContinuousRequest::Commit(request) => self.commit_lifecycle(*request),
            ContinuousRequest::Cancel(request) => self.cancel_lifecycle(*request),
        }
    }

    fn begin_lifecycle(
        &mut self,
        request: &ContinuousBeginRequest,
    ) -> Result<(), PluginLifecycleError> {
        if self.aborted_begins.remove(&request.session_id()) {
            return Err(PluginLifecycleError::BeginAborted);
        }
        if self.active_session.is_some() {
            return Err(PluginLifecycleError::SessionActive);
        }
        let plugin_id = request.plugin_id().as_str().to_owned();
        let plugin =
            self.adapters.get_mut(&plugin_id).ok_or(PluginLifecycleError::PluginUnavailable)?;
        let action = plugin
            .actions
            .get(request.action_id().as_str())
            .ok_or(PluginLifecycleError::ActionUnavailable)?;
        action
            .validate_parameters(request.parameters())
            .map_err(|_error| PluginLifecycleError::ParametersInvalid)?;
        let result = plugin.adapter.begin(request);
        self.map_lua_result(&plugin_id, result)?;
        self.active_session =
            Some(ActivePluginSession { session_id: request.session_id(), plugin_id });
        Ok(())
    }

    fn update_lifecycle(
        &mut self,
        request: ContinuousUpdateRequest,
    ) -> Result<(), PluginLifecycleError> {
        let plugin_id = self.plugin_for_session(request.session_id())?;
        let Some(plugin) = self.adapters.get_mut(&plugin_id) else {
            self.active_session = None;
            self.finish_deferred_reconciliation();
            return Err(PluginLifecycleError::PluginUnavailable);
        };
        let result = plugin.adapter.update(&request);
        self.map_lua_result(&plugin_id, result)
    }

    fn commit_lifecycle(
        &mut self,
        request: ContinuousCommitRequest,
    ) -> Result<(), PluginLifecycleError> {
        let plugin_id = self.plugin_for_session(request.session_id())?;
        self.active_session = None;
        let result = self
            .adapters
            .get_mut(&plugin_id)
            .map_or(Err(PluginLifecycleError::PluginUnavailable), |adapter| {
                adapter.adapter.commit(&request).map_err(PluginLifecycleError::Lua)
            });
        let result = match result {
            Ok(()) => Ok(()),
            Err(PluginLifecycleError::Lua(error)) => self.map_lua_result(&plugin_id, Err(error)),
            Err(error) => Err(error),
        };
        self.finish_deferred_reconciliation();
        result
    }

    fn cancel_lifecycle(
        &mut self,
        request: ContinuousCancelRequest,
    ) -> Result<(), PluginLifecycleError> {
        let plugin_id = self.plugin_for_session(request.session_id())?;
        self.active_session = None;
        let result = self
            .adapters
            .get_mut(&plugin_id)
            .map_or(Err(PluginLifecycleError::PluginUnavailable), |adapter| {
                adapter.adapter.cancel(&request).map_err(PluginLifecycleError::Lua)
            });
        let result = match result {
            Ok(()) => Ok(()),
            Err(PluginLifecycleError::Lua(error)) => self.map_lua_result(&plugin_id, Err(error)),
            Err(error) => Err(error),
        };
        self.finish_deferred_reconciliation();
        result
    }

    fn plugin_for_session(&self, session_id: Uuid) -> Result<String, PluginLifecycleError> {
        self.active_session
            .as_ref()
            .filter(|active| active.session_id == session_id)
            .map(|active| active.plugin_id.clone())
            .ok_or(PluginLifecycleError::SessionNotActive)
    }

    fn map_lua_result(
        &mut self,
        plugin_id: &str,
        result: Result<(), LuaSandboxError>,
    ) -> Result<(), PluginLifecycleError> {
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let faulted =
                    self.adapters.get(plugin_id).is_some_and(|plugin| plugin.adapter.is_faulted());
                if faulted {
                    self.active_session = None;
                    self.persist_adapter_fault(plugin_id);
                    self.refresh_inventory_view();
                    self.finish_deferred_reconciliation();
                }
                Err(PluginLifecycleError::Lua(error))
            }
        }
    }

    fn finish_deferred_reconciliation(&mut self) {
        if !std::mem::take(&mut self.reconciliation_deferred) {
            return;
        }
        if let Err(error) = self.fresh_report() {
            log::error!(
                target: "plugins",
                "component=plugins event_code=plugin_deferred_reconciliation \
                 result_code=failure error_code={} state_revision={}",
                error.code(),
                self.state.revision()
            );
        } else {
            log::info!(
                target: "plugins",
                "component=plugins event_code=plugin_deferred_reconciliation \
                 result_code=ok state_revision={}",
                self.state.revision()
            );
        }
    }

    fn operate(
        &mut self,
        operation: PluginWorkerOperation,
        plugin_id: &str,
    ) -> Result<PluginWorkerReceipt, PluginWorkerError> {
        if self.active_session.is_some() {
            return Err(PluginWorkerError::SessionActive);
        }
        match operation {
            PluginWorkerOperation::ApprovePermissions => self.approve_permissions(plugin_id)?,
            PluginWorkerOperation::Enable => self.enable(plugin_id)?,
            PluginWorkerOperation::Reload => self.reload(plugin_id)?,
            PluginWorkerOperation::Disable => self.disable(plugin_id)?,
            PluginWorkerOperation::MarkFaulted => self.mark_faulted(plugin_id)?,
        }
        Ok(PluginWorkerReceipt {
            operation,
            plugin_id: plugin_id.to_owned(),
            state_revision: self.state.revision(),
            loaded: self.adapters.contains_key(plugin_id),
        })
    }

    fn approve_permissions(&mut self, plugin_id: &str) -> Result<(), PluginWorkerError> {
        let report = self.fresh_report()?;
        let package = find_package(&report, plugin_id)?.clone();
        ensure_compatible(&package)?;
        let mut proposed = self.state.clone();
        proposed.approve_permissions(&package).map_err(PluginWorkerError::StateTransition)?;
        self.persist(&proposed)?;
        self.adapters.remove(plugin_id);
        Ok(())
    }

    fn enable(&mut self, plugin_id: &str) -> Result<(), PluginWorkerError> {
        let report = self.fresh_report()?;
        let package = find_package(&report, plugin_id)?.clone();
        ensure_compatible(&package)?;
        match self.state.status(&package).map_err(PluginWorkerError::StateTransition)? {
            PluginTrustStatus::Disabled => {}
            PluginTrustStatus::PermissionReviewRequired => {
                return Err(PluginWorkerError::StateTransition(
                    PluginStateTransitionError::ApprovalRequired,
                ));
            }
            PluginTrustStatus::Enabled => return Err(PluginWorkerError::AlreadyEnabled),
            PluginTrustStatus::ReloadRequired => return Err(PluginWorkerError::ReloadRequired),
            PluginTrustStatus::Faulted => return Err(PluginWorkerError::Faulted),
            PluginTrustStatus::Incompatible => {
                return Err(PluginWorkerError::StateTransition(
                    PluginStateTransitionError::Incompatible,
                ));
            }
            PluginTrustStatus::Unavailable | PluginTrustStatus::Invalid => {
                return Err(PluginWorkerError::Unavailable);
            }
        }

        if !self.state.permissions_approved(&package).map_err(PluginWorkerError::StateTransition)? {
            return Err(PluginWorkerError::StateTransition(
                PluginStateTransitionError::ApprovalRequired,
            ));
        }

        let candidate = load_plugin(
            &package,
            &self.shortcut_capabilities,
            &self.notification_capabilities,
            &self.loopback_capabilities,
        )
        .map_err(PluginWorkerError::Lua)?;
        let mut proposed = self.state.clone();
        proposed.record_successful_load(&package).map_err(PluginWorkerError::StateTransition)?;
        self.persist(&proposed)?;
        self.adapters.insert(plugin_id.to_owned(), candidate);
        Ok(())
    }

    fn reload(&mut self, plugin_id: &str) -> Result<(), PluginWorkerError> {
        let report = self.fresh_report()?;
        let package = find_package(&report, plugin_id)?.clone();
        ensure_compatible(&package)?;
        match self.state.status(&package).map_err(PluginWorkerError::StateTransition)? {
            PluginTrustStatus::Enabled
            | PluginTrustStatus::ReloadRequired
            | PluginTrustStatus::Faulted => {}
            PluginTrustStatus::Disabled => {
                return Err(PluginWorkerError::StateTransition(
                    PluginStateTransitionError::NotEnabled,
                ));
            }
            PluginTrustStatus::PermissionReviewRequired => {
                return Err(PluginWorkerError::StateTransition(
                    PluginStateTransitionError::ApprovalRequired,
                ));
            }
            PluginTrustStatus::Incompatible => {
                return Err(PluginWorkerError::StateTransition(
                    PluginStateTransitionError::Incompatible,
                ));
            }
            PluginTrustStatus::Unavailable | PluginTrustStatus::Invalid => {
                return Err(PluginWorkerError::Unavailable);
            }
        }

        self.adapters.remove(plugin_id);
        let candidate = match load_plugin(
            &package,
            &self.shortcut_capabilities,
            &self.notification_capabilities,
            &self.loopback_capabilities,
        ) {
            Ok(candidate) => candidate,
            Err(error) => {
                self.persist_adapter_fault(plugin_id);
                return Err(PluginWorkerError::Lua(error));
            }
        };
        let mut proposed = self.state.clone();
        proposed.record_successful_load(&package).map_err(PluginWorkerError::StateTransition)?;
        self.persist(&proposed)?;
        self.adapters.insert(plugin_id.to_owned(), candidate);
        Ok(())
    }

    fn disable(&mut self, plugin_id: &str) -> Result<(), PluginWorkerError> {
        self.adapters.remove(plugin_id);
        let mut proposed = self.state.clone();
        proposed.disable(plugin_id).map_err(PluginWorkerError::StateTransition)?;
        self.persist(&proposed)
    }

    fn mark_faulted(&mut self, plugin_id: &str) -> Result<(), PluginWorkerError> {
        self.adapters.remove(plugin_id);
        let mut proposed = self.state.clone();
        proposed.mark_faulted(plugin_id).map_err(PluginWorkerError::StateTransition)?;
        self.persist(&proposed)
    }

    fn rescan(&mut self) -> Result<(), PluginWorkerError> {
        if self.active_session.is_none() {
            self.fresh_report()?;
            return Ok(());
        }

        self.reconciliation_deferred = true;
        let report = discover_plugins(&self.paths.plugins);
        log_discovery_report(&report);
        if let Some(error) = report.root_error {
            self.scan_error_code = Some(error.to_string());
            return Err(PluginWorkerError::DiscoveryUnavailable);
        }
        self.update_inventory(&report);
        self.scan_error_code = None;
        Ok(())
    }

    fn fresh_report(&mut self) -> Result<PluginDiscoveryReport, PluginWorkerError> {
        let report = discover_plugins(&self.paths.plugins);
        log_discovery_report(&report);
        if let Some(error) = report.root_error {
            self.scan_error_code = Some(error.to_string());
            self.adapters.clear();
            return Err(PluginWorkerError::DiscoveryUnavailable);
        }
        let (_reconciliation, persistence_error) = self.reconcile_report(&report);
        self.prune_unauthorized(&report);
        self.update_inventory(&report);
        if let Some(error) = persistence_error {
            // A failed save leaves `self.state` unchanged. Effective status
            // still prunes changed packages, but no admission may proceed.
            return Err(PluginWorkerError::StatePersistence(error));
        }
        self.scan_error_code = None;
        Ok(report)
    }

    fn reconcile_report(
        &mut self,
        report: &PluginDiscoveryReport,
    ) -> (crate::plugin_state::PluginStateReconciliation, Option<PluginStateStoreError>) {
        let mut proposed = self.state.clone();
        let reconciliation = proposed.reconcile(report);
        let mut persistence_error = None;
        if reconciliation.changed {
            match self.store.save(&self.state, &proposed) {
                Ok(saved) => self.state = saved,
                Err(error) => {
                    persistence_error = Some(error);
                    log::error!(
                        target: "plugins",
                        "component=plugins event_code=plugin_state_reconciled \
                         result_code=failure error_code={} permission_changes={} \
                         unavailable_disabled={}",
                        error.code(),
                        reconciliation.permission_changes,
                        reconciliation.unavailable_disabled
                    );
                }
            }
        }
        (reconciliation, persistence_error)
    }

    fn prune_unauthorized(&mut self, report: &PluginDiscoveryReport) {
        let loaded = self.adapters.keys().cloned().collect::<Vec<_>>();
        for plugin_id in loaded {
            let authorized = find_package(report, &plugin_id)
                .ok()
                .and_then(|package| self.state.status(package).ok())
                == Some(PluginTrustStatus::Enabled);
            if !authorized {
                self.adapters.remove(&plugin_id);
                log::info!(
                    target: "plugins",
                    "component=plugins event_code=plugin_unloaded result_code=ok \
                     plugin_id={} reason=trust_reconciliation state_revision={}",
                    plugin_id,
                    self.state.revision()
                );
            }
        }
    }

    fn persist(&mut self, proposed: &PluginTrustState) -> Result<(), PluginWorkerError> {
        self.state =
            self.store.save(&self.state, proposed).map_err(PluginWorkerError::StatePersistence)?;
        Ok(())
    }

    fn persist_adapter_fault(&mut self, plugin_id: &str) {
        self.adapters.remove(plugin_id);
        let mut proposed = self.state.clone();
        if proposed.mark_faulted(plugin_id).is_err() {
            return;
        }
        if let Err(error) = self.persist(&proposed) {
            log::error!(
                target: "plugins",
                "component=plugins event_code=plugin_fault_persisted result_code=failure \
                 plugin_id={} error_code={} state_revision={}",
                plugin_id,
                error.code(),
                self.state.revision()
            );
        }
    }

    fn snapshot(&self) -> PluginWorkerSnapshot {
        PluginWorkerSnapshot {
            state_revision: self.state.revision(),
            loaded_plugin_ids: self.adapters.keys().cloned().collect(),
            plugins: self.inventory.clone(),
            scan_error_code: self.scan_error_code.clone(),
        }
    }

    fn publish_action_catalog(&self) {
        let catalog = self
            .adapters
            .iter()
            .flat_map(|(plugin_id, plugin)| {
                plugin.actions.iter().map(|(action_id, action)| {
                    (
                        (plugin_id.clone(), action_id.clone()),
                        PluginActionAdmission::new(action.update_hz, &plugin.permissions),
                    )
                })
            })
            .collect();
        let shortcut_permissions = self
            .adapters
            .iter()
            .map(|(plugin_id, plugin)| {
                (plugin_id.clone(), plugin.permissions.contains(&PluginPermission::InputShortcut))
            })
            .collect();
        self.shortcut_capabilities.reconcile_permissions(&shortcut_permissions);
        let notification_permissions = self
            .adapters
            .iter()
            .map(|(plugin_id, plugin)| {
                (plugin_id.clone(), plugin.permissions.contains(&PluginPermission::Notification))
            })
            .collect();
        self.notification_capabilities.reconcile_permissions(&notification_permissions);
        let loopback_grants = self
            .adapters
            .iter()
            .filter(|(_plugin_id, plugin)| {
                plugin.permissions.contains(&PluginPermission::NetworkLoopback)
            })
            .map(|(plugin_id, plugin)| (plugin_id.clone(), plugin.loopback.clone()))
            .collect();
        self.loopback_capabilities.reconcile_grants(&loopback_grants);
        let mut published =
            self.action_catalog.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        *published = Arc::new(catalog);
    }

    fn update_inventory(&mut self, report: &PluginDiscoveryReport) {
        let mut observed = self.state.clone();
        let statuses = observed.reconcile(report).statuses;
        self.inventory = statuses
            .into_iter()
            .map(|(plugin_id, status)| {
                let package =
                    report.packages.iter().find(|package| package.directory_name == plugin_id);
                match package.map(|package| &package.status) {
                    Some(PluginPackageStatus::Compatible { manifest, .. }) => {
                        let permissions_approved = package.is_some_and(|package| {
                            self.state.permissions_approved(package).unwrap_or(false)
                        });
                        PluginInventoryEntry {
                            plugin_id,
                            display_name: manifest.name.clone(),
                            version: Some(manifest.version.clone()),
                            api_version: Some(manifest.api_version),
                            permissions: manifest.permissions.clone(),
                            loopback: manifest.loopback.clone(),
                            actions: manifest.actions.clone(),
                            permissions_approved,
                            status: status.into(),
                            issue_code: None,
                        }
                    }
                    Some(PluginPackageStatus::IncompatibleApi { manifest, .. }) => {
                        PluginInventoryEntry {
                            plugin_id,
                            display_name: manifest.name.clone(),
                            version: Some(manifest.version.clone()),
                            api_version: Some(manifest.api_version),
                            permissions: manifest.permissions.clone(),
                            loopback: manifest.loopback.clone(),
                            actions: Vec::new(),
                            permissions_approved: false,
                            status: status.into(),
                            issue_code: None,
                        }
                    }
                    Some(PluginPackageStatus::Invalid(error)) => PluginInventoryEntry {
                        display_name: plugin_id.clone(),
                        plugin_id,
                        version: None,
                        api_version: None,
                        permissions: Vec::new(),
                        loopback: Vec::new(),
                        actions: Vec::new(),
                        permissions_approved: false,
                        status: PluginInventoryStatus::Invalid,
                        issue_code: Some(error.to_string()),
                    },
                    None => PluginInventoryEntry {
                        display_name: plugin_id.clone(),
                        plugin_id,
                        version: None,
                        api_version: None,
                        permissions: Vec::new(),
                        loopback: Vec::new(),
                        actions: Vec::new(),
                        permissions_approved: false,
                        status: PluginInventoryStatus::Unavailable,
                        issue_code: Some("package_missing".to_owned()),
                    },
                }
            })
            .collect();
    }

    fn refresh_inventory_view(&mut self) {
        let report = discover_plugins(&self.paths.plugins);
        if let Some(error) = report.root_error {
            self.scan_error_code = Some(error.to_string());
        } else {
            self.update_inventory(&report);
            self.scan_error_code = None;
        }
    }
}

const fn lifecycle_stage(request: &ContinuousRequest) -> PluginLifecycleStage {
    match request {
        ContinuousRequest::Begin(_) => PluginLifecycleStage::Begin,
        ContinuousRequest::Update(_) => PluginLifecycleStage::Update,
        ContinuousRequest::Commit(_) => PluginLifecycleStage::Commit,
        ContinuousRequest::Cancel(_) => PluginLifecycleStage::Cancel,
    }
}

fn find_package<'a>(
    report: &'a PluginDiscoveryReport,
    plugin_id: &str,
) -> Result<&'a DiscoveredPlugin, PluginWorkerError> {
    report
        .packages
        .iter()
        .find(|package| package.directory_name == plugin_id)
        .ok_or(PluginWorkerError::PackageMissing)
}

fn ensure_compatible(package: &DiscoveredPlugin) -> Result<(), PluginWorkerError> {
    match &package.status {
        PluginPackageStatus::Compatible { .. } => Ok(()),
        PluginPackageStatus::IncompatibleApi { .. } => {
            Err(PluginWorkerError::StateTransition(PluginStateTransitionError::Incompatible))
        }
        PluginPackageStatus::Invalid(_) => {
            Err(PluginWorkerError::StateTransition(PluginStateTransitionError::InvalidPackage))
        }
    }
}

fn load_plugin(
    package: &DiscoveredPlugin,
    shortcut_capabilities: &ShortcutCapabilityClient,
    notification_capabilities: &NotificationCapabilityClient,
    loopback_capabilities: &LoopbackCapabilityClient,
) -> Result<LoadedPlugin, LuaSandboxError> {
    let PluginPackageStatus::Compatible { manifest, sources, .. } = &package.status else {
        return Err(LuaSandboxError::Initialization);
    };
    let adapter = LuaLifecycleAdapter::load_with_capability_host(
        sources.clone(),
        LuaSandboxLimits::default(),
        Arc::new(LuaCapabilityBrokerClient::new(
            shortcut_capabilities.clone(),
            notification_capabilities.clone(),
            loopback_capabilities.clone(),
        )),
    )?;
    let actions =
        manifest.actions.iter().cloned().map(|action| (action.id.clone(), action)).collect();
    Ok(LoadedPlugin {
        adapter,
        actions,
        permissions: manifest.permissions.clone(),
        loopback: manifest.loopback.clone(),
    })
}

fn log_operation(
    operation: PluginWorkerOperation,
    plugin_id: &str,
    result: Result<(), PluginWorkerError>,
    state_revision: u64,
    loaded: bool,
    source: &str,
) {
    match result {
        Ok(()) => log::info!(
            target: "plugins",
            "component=plugins event_code=plugin_worker_operation result_code=ok \
             operation={} plugin_id={} state_revision={} loaded={} source={}",
            operation.as_str(),
            plugin_id,
            state_revision,
            loaded,
            source
        ),
        Err(error) => log::warn!(
            target: "plugins",
            "component=plugins event_code=plugin_worker_operation result_code=failure \
             operation={} plugin_id={} error_code={} state_revision={} loaded={} source={}",
            operation.as_str(),
            plugin_id,
            error.code(),
            state_revision,
            loaded,
            source
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path, thread, time::Instant};

    use gradial_core::{
        ContinuousActionSnapshot, ContinuousCancelReason, ContinuousSession, PermittedContext,
        PluginActionId, PluginValue,
    };

    use super::*;
    use crate::plugin_discovery::discover_plugins;

    const FIRST_PLUGIN: &str = "example.first";
    const SECOND_PLUGIN: &str = "example.second";
    const METER_PLUGIN: &str = "org.gradial.example.meter";
    const METER_MANIFEST: &str =
        include_str!("../../../examples/plugins/org.gradial.example.meter/plugin.toml");
    const METER_SOURCE: &str =
        include_str!("../../../examples/plugins/org.gradial.example.meter/main.lua");
    const SHORTCUT_BANDS_PLUGIN: &str = "org.gradial.example.shortcut-bands";
    const SHORTCUT_BANDS_MANIFEST: &str =
        include_str!("../../../examples/plugins/org.gradial.example.shortcut-bands/plugin.toml");
    const SHORTCUT_BANDS_SOURCE: &str =
        include_str!("../../../examples/plugins/org.gradial.example.shortcut-bands/main.lua");
    const NOTIFICATION_PLUGIN: &str = "org.gradial.example.notification";
    const NOTIFICATION_MANIFEST: &str =
        include_str!("../../../examples/plugins/org.gradial.example.notification/plugin.toml");
    const NOTIFICATION_SOURCE: &str =
        include_str!("../../../examples/plugins/org.gradial.example.notification/main.lua");
    const LOOPBACK_PLUGIN: &str = "org.gradial.example.loopback";
    const LOOPBACK_MANIFEST: &str =
        include_str!("../../../examples/plugins/org.gradial.example.loopback/plugin.toml");
    const LOOPBACK_SOURCE: &str =
        include_str!("../../../examples/plugins/org.gradial.example.loopback/main.lua");
    const VALID_SOURCE: &str = r"
        return {
            begin = function(context, parameters, sample) return {} end,
            update = function(session, sample) end,
            commit = function(session, sample) end,
            cancel = function(session, reason) end,
        }
    ";

    #[test]
    fn callback_timing_histogram_is_fixed_and_uses_nearest_rank_buckets() {
        let mut histogram = FixedTimingHistogram::default();
        for micros in [1, 25, 26, 100, 101, 999, 1_001, 20_000] {
            histogram.record(Duration::from_micros(micros));
        }

        assert_eq!(histogram.count, 8);
        assert_eq!(histogram.percentile(50), 100);
        assert_eq!(histogram.percentile(95), 20_000);
        assert_eq!(histogram.percentile(99), 20_000);
        assert_eq!(histogram.maximum_us, 20_000);
        assert_eq!(histogram.buckets.iter().sum::<u64>(), histogram.count);
    }

    fn setup() -> (tempfile::TempDir, AppPaths) {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let paths = AppPaths::from_local_app_data(temporary.path());
        paths.ensure_directories().expect("application directories");
        (temporary, paths)
    }

    fn write_package(root: &Path, plugin_id: &str, permissions: &[&str], source: &str) {
        let package = root.join(plugin_id);
        fs::create_dir_all(&package).expect("plugin package");
        let permissions = permissions
            .iter()
            .map(|permission| format!("\"{permission}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let manifest = format!(
            "manifest_version = 1\nid = \"{plugin_id}\"\nname = \"Test plugin\"\n\
             version = \"0.1.0\"\napi_version = 1\nentry = \"main.lua\"\n\
             permissions = [{permissions}]\n\n[[actions]]\nid = \"measure\"\n\
             name = \"Measure\"\nkind = \"continuous\"\nupdate_hz = 30\n"
        );
        fs::write(package.join("plugin.toml"), manifest).expect("plugin manifest");
        fs::write(package.join("main.lua"), source).expect("plugin source");
    }

    fn write_meter_example(root: &Path) {
        let package = root.join(METER_PLUGIN);
        fs::create_dir_all(&package).expect("example package");
        fs::write(package.join("plugin.toml"), METER_MANIFEST).expect("example manifest");
        fs::write(package.join("main.lua"), METER_SOURCE).expect("example source");
    }

    fn write_shortcut_bands_example(root: &Path) {
        let package = root.join(SHORTCUT_BANDS_PLUGIN);
        fs::create_dir_all(&package).expect("example package");
        fs::write(package.join("plugin.toml"), SHORTCUT_BANDS_MANIFEST).expect("example manifest");
        fs::write(package.join("main.lua"), SHORTCUT_BANDS_SOURCE).expect("example source");
    }

    fn write_notification_example(root: &Path) {
        let package = root.join(NOTIFICATION_PLUGIN);
        fs::create_dir_all(&package).expect("example package");
        fs::write(package.join("plugin.toml"), NOTIFICATION_MANIFEST).expect("example manifest");
        fs::write(package.join("main.lua"), NOTIFICATION_SOURCE).expect("example source");
    }

    fn write_loopback_example(root: &Path) {
        let package = root.join(LOOPBACK_PLUGIN);
        fs::create_dir_all(&package).expect("example package");
        fs::write(package.join("plugin.toml"), LOOPBACK_MANIFEST).expect("example manifest");
        fs::write(package.join("main.lua"), LOOPBACK_SOURCE).expect("example source");
    }

    fn write_required_parameter_package(root: &Path, plugin_id: &str) {
        let package = root.join(plugin_id);
        fs::create_dir_all(&package).expect("plugin package");
        let manifest = format!(
            "manifest_version = 1\nid = \"{plugin_id}\"\nname = \"Parameterized plugin\"\n\
             version = \"0.1.0\"\napi_version = 1\nentry = \"main.lua\"\npermissions = []\n\n\
             [[actions]]\nid = \"measure\"\nname = \"Measure\"\nkind = \"continuous\"\n\
             update_hz = 30\n\n[[actions.parameters]]\nkey = \"scale\"\nlabel = \"Scale\"\n\
             type = \"number\"\nrequired = true\nminimum = 0.1\nmaximum = 2.0\n"
        );
        fs::write(package.join("plugin.toml"), manifest).expect("plugin manifest");
        fs::write(
            package.join("main.lua"),
            VALID_SOURCE.replace("return {}", "error('begin must not run')"),
        )
        .expect("plugin source");
    }

    fn begin_session(plugin_id: &str, session_id: Uuid) -> (ContinuousSession, ContinuousRequest) {
        let action = ContinuousActionSnapshot::new(
            PluginId::new(plugin_id).expect("plugin ID"),
            PluginActionId::new("measure").expect("action ID"),
            PermittedContext::new(None, None),
            BTreeMap::new(),
            30,
        )
        .expect("continuous action");
        ContinuousSession::begin(session_id, action, 0.25).expect("begin session")
    }

    fn begin_shortcut_bands_session(session_id: Uuid) -> (ContinuousSession, ContinuousRequest) {
        let parameters = BTreeMap::from([
            ("bands".to_owned(), PluginValue::Integer(6)),
            ("increase_shortcut".to_owned(), PluginValue::Choice("right".to_owned())),
            ("decrease_shortcut".to_owned(), PluginValue::Choice("left".to_owned())),
        ]);
        let action = ContinuousActionSnapshot::new(
            PluginId::new(SHORTCUT_BANDS_PLUGIN).expect("plugin ID"),
            PluginActionId::new("step-between-bands").expect("action ID"),
            PermittedContext::new(None, None),
            parameters,
            20,
        )
        .expect("continuous action");
        ContinuousSession::begin(session_id, action, 0.25).expect("begin session")
    }

    fn begin_notification_session(session_id: Uuid) -> (ContinuousSession, ContinuousRequest) {
        let action = ContinuousActionSnapshot::new(
            PluginId::new(NOTIFICATION_PLUGIN).expect("plugin ID"),
            PluginActionId::new("notify-on-commit").expect("action ID"),
            PermittedContext::new(None, None),
            BTreeMap::new(),
            10,
        )
        .expect("continuous action");
        ContinuousSession::begin(session_id, action, 0.25).expect("begin session")
    }

    fn begin_loopback_session(session_id: Uuid) -> (ContinuousSession, ContinuousRequest) {
        let action = ContinuousActionSnapshot::new(
            PluginId::new(LOOPBACK_PLUGIN).expect("plugin ID"),
            PluginActionId::new("post-on-commit").expect("action ID"),
            PermittedContext::new(None, None),
            BTreeMap::new(),
            10,
        )
        .expect("continuous action");
        ContinuousSession::begin(session_id, action, 0.25).expect("begin session")
    }

    fn accept_begin(session: &mut ContinuousSession) {
        assert!(session.accept_begin(Duration::ZERO).expect("accept begin").is_none());
    }

    fn update_request(session: &mut ContinuousSession, value: f64) -> ContinuousRequest {
        update_request_at(session, value, Duration::from_millis(40))
    }

    fn update_request_at(
        session: &mut ContinuousSession,
        value: f64,
        elapsed: Duration,
    ) -> ContinuousRequest {
        session.observe(value, elapsed).expect("observe");
        session.take_due_update(elapsed).expect("take update").expect("due update")
    }

    fn cancel_request(
        session: &mut ContinuousSession,
        reason: ContinuousCancelReason,
    ) -> ContinuousRequest {
        session.cancel(reason).expect("cancel").expect("cancel request")
    }

    fn commit_request(session: &mut ContinuousSession, value: f64) -> ContinuousRequest {
        session.commit(value, Duration::from_millis(60)).expect("commit").expect("commit request")
    }

    fn dispatch(
        client: &PluginLifecycleClient,
        request: ContinuousRequest,
    ) -> Result<(), PluginLifecycleError> {
        client.request(request).expect("accepted lifecycle request").receive()
    }

    fn prepare_state(paths: &AppPaths, approvals: &[(&str, bool)]) {
        let store = PluginStateStore::new(paths.clone());
        let loaded = store.load_or_initialize();
        let report = discover_plugins(&paths.plugins);
        let mut proposed = loaded.state.clone();
        for (plugin_id, enabled) in approvals {
            let package = find_package(&report, plugin_id).expect("prepared package");
            proposed.approve_permissions(package).expect("prepared approval");
            if *enabled {
                proposed.record_successful_load(package).expect("prepared enabled state");
            }
        }
        store.save(&loaded.state, &proposed).expect("prepared trust state");
    }

    fn operate(
        runtime: &PluginWorkerRuntime,
        operation: PluginWorkerOperation,
        plugin_id: &str,
    ) -> Result<PluginWorkerReceipt, PluginWorkerError> {
        runtime.request(operation, plugin_id).expect("accepted operation").receive()
    }

    fn snapshot(runtime: &PluginWorkerRuntime) -> PluginWorkerSnapshot {
        runtime.request_snapshot().expect("accepted snapshot").receive()
    }

    fn rescan(runtime: &PluginWorkerRuntime) -> PluginWorkerSnapshot {
        runtime.request_rescan().expect("accepted rescan").receive()
    }

    fn persisted_status(paths: &AppPaths, plugin_id: &str) -> PluginTrustStatus {
        let store = PluginStateStore::new(paths.clone());
        let state = store.load_or_initialize().state;
        let report = discover_plugins(&paths.plugins);
        state.status(find_package(&report, plugin_id).expect("package")).expect("status")
    }

    #[test]
    fn startup_constructs_only_effectively_enabled_adapters() {
        let (_temporary, paths) = setup();
        write_package(
            &paths.plugins,
            FIRST_PLUGIN,
            &["context.application", "context.window_title", "input.shortcut", "notification"],
            VALID_SOURCE,
        );
        write_package(&paths.plugins, SECOND_PLUGIN, &[], "this is invalid Lua");
        prepare_state(&paths, &[(FIRST_PLUGIN, true), (SECOND_PLUGIN, false)]);

        let runtime = PluginWorkerRuntime::start(paths).expect("plugin worker");
        let snapshot = snapshot(&runtime);
        let client = runtime.lifecycle_client();

        assert_eq!(snapshot.loaded_plugin_ids, vec![FIRST_PLUGIN]);
        assert_eq!(snapshot.plugins[0].actions[0].id, "measure");
        assert_eq!(snapshot.plugins[0].actions[0].update_hz, 30);
        let admission = client
            .action_admission(
                &PluginId::new(FIRST_PLUGIN).expect("plugin ID"),
                &PluginActionId::new("measure").expect("action ID"),
            )
            .expect("admitted action");
        assert_eq!(admission.update_hz(), 30);
        assert!(admission.permits_context_application());
        assert!(admission.permits_context_window_title());
        assert!(admission.permits_input_shortcut());
        assert!(admission.permits_notification());
        assert_eq!(
            client.action_admission(
                &PluginId::new(SECOND_PLUGIN).expect("plugin ID"),
                &PluginActionId::new("measure").expect("action ID"),
            ),
            None
        );
    }

    #[test]
    fn no_side_effect_example_completes_the_worker_lifecycle() {
        let (_temporary, paths) = setup();
        write_meter_example(&paths.plugins);
        prepare_state(&paths, &[(METER_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths).expect("plugin worker");
        let client = runtime.lifecycle_client();
        let (mut session, begin) = begin_session(METER_PLUGIN, Uuid::from_u128(1));

        dispatch(&client, begin).expect("begin");
        accept_begin(&mut session);
        let update = update_request(&mut session, 0.55);
        dispatch(&client, update.clone()).expect("update");
        dispatch(&client, commit_request(&mut session, 0.75)).expect("commit");

        assert_eq!(dispatch(&client, update), Err(PluginLifecycleError::SessionNotActive));
        assert_eq!(snapshot(&runtime).loaded_plugin_ids, vec![METER_PLUGIN]);
    }

    #[test]
    fn admitted_shortcut_request_crosses_lua_worker_and_bounded_broker() {
        let (_temporary, paths) = setup();
        let source = VALID_SOURCE.replace(
            "begin = function(context, parameters, sample) return {} end",
            r#"begin = function(context, parameters, sample)
                local accepted, reason = gradial.input.shortcut("left_control+right")
                assert(accepted and reason == nil)
                return {}
            end"#,
        );
        write_package(&paths.plugins, FIRST_PLUGIN, &["input.shortcut"], &source);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let (shortcut_capabilities, calls) = ShortcutCapabilityRuntime::successful_for_test();
        let runtime =
            PluginWorkerRuntime::start_with_shortcut_capabilities(paths, shortcut_capabilities)
                .expect("plugin worker");
        assert_eq!(snapshot(&runtime).loaded_plugin_ids, vec![FIRST_PLUGIN]);
        let client = runtime.lifecycle_client();
        let plugin_id = PluginId::new(FIRST_PLUGIN).expect("plugin ID");
        let action_id = PluginActionId::new("measure").expect("action ID");
        let admission = client.action_admission(&plugin_id, &action_id).expect("admission");
        let session_id = Uuid::from_u128(101);
        client
            .open_shortcut_session(
                session_id,
                &plugin_id,
                Arc::new(gradial_core::InvocationContext::default()),
                admission.permits_input_shortcut(),
            )
            .expect("capability session");
        let (mut session, begin) = begin_session(FIRST_PLUGIN, session_id);

        dispatch(&client, begin).expect("begin");
        client
            .finish_shortcut_commit(session_id, Instant::now() + Duration::from_secs(1))
            .expect("shortcut completion");
        assert_eq!(calls.load(Ordering::Acquire), 1);
        accept_begin(&mut session);
        dispatch(&client, cancel_request(&mut session, ContinuousCancelReason::ReturnedToDiscrete))
            .expect("cancel");
    }

    #[test]
    fn checked_in_shortcut_example_crosses_worker_and_broker() {
        let (_temporary, paths) = setup();
        write_shortcut_bands_example(&paths.plugins);
        prepare_state(&paths, &[(SHORTCUT_BANDS_PLUGIN, true)]);
        let (shortcut_capabilities, calls) = ShortcutCapabilityRuntime::successful_for_test();
        let runtime =
            PluginWorkerRuntime::start_with_shortcut_capabilities(paths, shortcut_capabilities)
                .expect("plugin worker");
        assert_eq!(snapshot(&runtime).loaded_plugin_ids, vec![SHORTCUT_BANDS_PLUGIN]);
        let client = runtime.lifecycle_client();
        let plugin_id = PluginId::new(SHORTCUT_BANDS_PLUGIN).expect("plugin ID");
        let action_id = PluginActionId::new("step-between-bands").expect("action ID");
        let admission = client.action_admission(&plugin_id, &action_id).expect("admission");
        assert!(admission.permits_input_shortcut());
        let session_id = Uuid::from_u128(103);
        client
            .open_shortcut_session(
                session_id,
                &plugin_id,
                Arc::new(gradial_core::InvocationContext::default()),
                admission.permits_input_shortcut(),
            )
            .expect("capability session");
        let (mut session, begin) = begin_shortcut_bands_session(session_id);

        dispatch(&client, begin).expect("begin");
        accept_begin(&mut session);
        dispatch(&client, update_request_at(&mut session, 0.55, Duration::from_millis(60)))
            .expect("update");
        let commit = session
            .commit(0.75, Duration::from_millis(80))
            .expect("commit")
            .expect("commit request");
        dispatch(&client, commit).expect("commit");
        client
            .finish_shortcut_commit(session_id, Instant::now() + Duration::from_secs(1))
            .expect("shortcut completion");

        assert_eq!(calls.load(Ordering::Acquire), 2);
    }

    #[test]
    fn checked_in_notification_example_crosses_worker_and_broker() {
        let (_temporary, paths) = setup();
        write_notification_example(&paths.plugins);
        prepare_state(&paths, &[(NOTIFICATION_PLUGIN, true)]);
        let (shortcut_capabilities, _shortcut_calls) =
            ShortcutCapabilityRuntime::successful_for_test();
        let (notification_capabilities, notifications) =
            NotificationCapabilityRuntime::successful_for_test();
        let runtime = PluginWorkerRuntime::start_with_capabilities(
            paths,
            shortcut_capabilities,
            notification_capabilities,
        )
        .expect("plugin worker");
        assert_eq!(snapshot(&runtime).loaded_plugin_ids, vec![NOTIFICATION_PLUGIN]);
        let client = runtime.lifecycle_client();
        let plugin_id = PluginId::new(NOTIFICATION_PLUGIN).expect("plugin ID");
        let action_id = PluginActionId::new("notify-on-commit").expect("action ID");
        let admission = client.action_admission(&plugin_id, &action_id).expect("admission");
        assert!(admission.permits_notification());
        let session_id = Uuid::from_u128(104);
        client
            .open_notification_session(session_id, &plugin_id, admission.permits_notification())
            .expect("capability session");
        let (mut session, begin) = begin_notification_session(session_id);

        dispatch(&client, begin).expect("begin");
        accept_begin(&mut session);
        dispatch(&client, commit_request(&mut session, 0.75)).expect("commit");
        client.finish_notification_commit(session_id).expect("notification completion");

        let notifications = notifications.lock().expect("notifications");
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].0, NOTIFICATION_PLUGIN);
        assert_eq!(notifications[0].1, "Continuous gesture committed.");
        assert!(!notifications[0].2.load(Ordering::Acquire));
    }

    #[test]
    fn undeclared_notification_permission_fails_without_broker_delivery() {
        let (_temporary, paths) = setup();
        let source = VALID_SOURCE.replace(
            "commit = function(session, sample) end",
            r#"commit = function(session, sample)
                local accepted, reason = gradial.notification("Must not appear")
                assert(not accepted and reason == "permission_denied")
            end"#,
        );
        write_package(&paths.plugins, FIRST_PLUGIN, &[], &source);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let (shortcut_capabilities, _shortcut_calls) =
            ShortcutCapabilityRuntime::successful_for_test();
        let (notification_capabilities, notifications) =
            NotificationCapabilityRuntime::successful_for_test();
        let runtime = PluginWorkerRuntime::start_with_capabilities(
            paths,
            shortcut_capabilities,
            notification_capabilities,
        )
        .expect("plugin worker");
        assert_eq!(snapshot(&runtime).loaded_plugin_ids, vec![FIRST_PLUGIN]);
        let client = runtime.lifecycle_client();
        let plugin_id = PluginId::new(FIRST_PLUGIN).expect("plugin ID");
        let action_id = PluginActionId::new("measure").expect("action ID");
        let admission = client.action_admission(&plugin_id, &action_id).expect("admission");
        assert!(!admission.permits_notification());
        let session_id = Uuid::from_u128(105);
        client
            .open_notification_session(session_id, &plugin_id, admission.permits_notification())
            .expect("capability session");
        let (mut session, begin) = begin_session(FIRST_PLUGIN, session_id);

        dispatch(&client, begin).expect("begin");
        accept_begin(&mut session);
        dispatch(&client, commit_request(&mut session, 0.75)).expect("handled denial");
        assert_eq!(
            client.finish_notification_commit(session_id),
            Err(NotificationCapabilityFailure::PermissionDenied)
        );
        assert!(notifications.lock().expect("notifications").is_empty());
    }

    #[test]
    fn checked_in_loopback_example_crosses_worker_and_broker() {
        let (_temporary, paths) = setup();
        write_loopback_example(&paths.plugins);
        prepare_state(&paths, &[(LOOPBACK_PLUGIN, true)]);
        let (shortcut_capabilities, _shortcut_calls) =
            ShortcutCapabilityRuntime::successful_for_test();
        let (notification_capabilities, _notifications) =
            NotificationCapabilityRuntime::successful_for_test();
        let (loopback_capabilities, requests) = LoopbackCapabilityRuntime::successful_for_test();
        let runtime = PluginWorkerRuntime::start_with_all_capabilities(
            paths,
            shortcut_capabilities,
            notification_capabilities,
            loopback_capabilities,
        )
        .expect("plugin worker");
        let snapshot = snapshot(&runtime);
        assert_eq!(snapshot.loaded_plugin_ids, vec![LOOPBACK_PLUGIN]);
        assert_eq!(snapshot.plugins[0].loopback.len(), 1);
        assert_eq!(snapshot.plugins[0].loopback[0].origin.as_str(), "http://127.0.0.1:32123");
        assert_eq!(
            snapshot.plugins[0].loopback[0].methods,
            vec![gradial_core::PluginLoopbackMethod::Post]
        );
        let client = runtime.lifecycle_client();
        let plugin_id = PluginId::new(LOOPBACK_PLUGIN).expect("plugin ID");
        let action_id = PluginActionId::new("post-on-commit").expect("action ID");
        let admission = client.action_admission(&plugin_id, &action_id).expect("admission");
        assert!(admission.permits_network_loopback());
        let session_id = Uuid::from_u128(106);
        client
            .open_loopback_session(session_id, &plugin_id, admission.permits_network_loopback())
            .expect("capability session");
        let (mut session, begin) = begin_loopback_session(session_id);

        dispatch(&client, begin).expect("begin");
        accept_begin(&mut session);
        dispatch(&client, commit_request(&mut session, 0.75)).expect("commit");
        client
            .finish_loopback_commit(session_id, Instant::now() + Duration::from_secs(1))
            .expect("loopback completion");

        let requests = requests.lock().expect("requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, "POST");
        assert_eq!(requests[0].1, "http://127.0.0.1:32123");
        assert_eq!(requests[0].2, "/gradial/example");
        assert_eq!(requests[0].3, br#"{"value":0.750000}"#);
    }

    #[test]
    fn undeclared_loopback_permission_fails_without_broker_delivery() {
        let (_temporary, paths) = setup();
        let source = VALID_SOURCE.replace(
            "commit = function(session, sample) end",
            r#"commit = function(session, sample)
                local accepted, reason = gradial.network.loopback({
                    method = "POST",
                    origin = "http://127.0.0.1:32123",
                    path = "/must-not-run",
                    body = "private",
                })
                assert(not accepted and reason == "permission_denied")
            end"#,
        );
        write_package(&paths.plugins, FIRST_PLUGIN, &[], &source);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let (shortcut_capabilities, _shortcut_calls) =
            ShortcutCapabilityRuntime::successful_for_test();
        let (notification_capabilities, _notifications) =
            NotificationCapabilityRuntime::successful_for_test();
        let (loopback_capabilities, requests) = LoopbackCapabilityRuntime::successful_for_test();
        let runtime = PluginWorkerRuntime::start_with_all_capabilities(
            paths,
            shortcut_capabilities,
            notification_capabilities,
            loopback_capabilities,
        )
        .expect("plugin worker");
        assert_eq!(snapshot(&runtime).loaded_plugin_ids, vec![FIRST_PLUGIN]);
        let client = runtime.lifecycle_client();
        let plugin_id = PluginId::new(FIRST_PLUGIN).expect("plugin ID");
        let action_id = PluginActionId::new("measure").expect("action ID");
        let admission = client.action_admission(&plugin_id, &action_id).expect("admission");
        assert!(!admission.permits_network_loopback());
        let session_id = Uuid::from_u128(107);
        client
            .open_loopback_session(session_id, &plugin_id, admission.permits_network_loopback())
            .expect("capability session");
        let (mut session, begin) = begin_session(FIRST_PLUGIN, session_id);

        dispatch(&client, begin).expect("begin");
        accept_begin(&mut session);
        dispatch(&client, commit_request(&mut session, 0.75)).expect("handled denial");
        assert_eq!(
            client.finish_loopback_commit(session_id, Instant::now() + Duration::from_secs(1)),
            Err(LoopbackCapabilityFailure::PermissionDenied)
        );
        assert!(requests.lock().expect("requests").is_empty());
    }

    #[test]
    fn undeclared_shortcut_permission_fails_session_without_broker_work() {
        let (_temporary, paths) = setup();
        let source = VALID_SOURCE.replace(
            "begin = function(context, parameters, sample) return {} end",
            r#"begin = function(context, parameters, sample)
                local accepted, reason = gradial.input.shortcut("a")
                assert(not accepted and reason == "permission_denied")
                return {}
            end"#,
        );
        write_package(&paths.plugins, FIRST_PLUGIN, &[], &source);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let (shortcut_capabilities, calls) = ShortcutCapabilityRuntime::successful_for_test();
        let runtime =
            PluginWorkerRuntime::start_with_shortcut_capabilities(paths, shortcut_capabilities)
                .expect("plugin worker");
        assert_eq!(snapshot(&runtime).loaded_plugin_ids, vec![FIRST_PLUGIN]);
        let client = runtime.lifecycle_client();
        let plugin_id = PluginId::new(FIRST_PLUGIN).expect("plugin ID");
        let action_id = PluginActionId::new("measure").expect("action ID");
        let admission = client.action_admission(&plugin_id, &action_id).expect("admission");
        assert!(!admission.permits_input_shortcut());
        let session_id = Uuid::from_u128(102);
        client
            .open_shortcut_session(
                session_id,
                &plugin_id,
                Arc::new(gradial_core::InvocationContext::default()),
                admission.permits_input_shortcut(),
            )
            .expect("capability session");
        let (mut session, begin) = begin_session(FIRST_PLUGIN, session_id);

        dispatch(&client, begin).expect("begin with handled denial");
        assert_eq!(
            client.shortcut_failure(session_id),
            Some(ShortcutCapabilityFailure::PermissionDenied)
        );
        assert_eq!(calls.load(Ordering::Acquire), 0);
        client.cancel_shortcut_session(session_id);
        accept_begin(&mut session);
        dispatch(&client, cancel_request(&mut session, ContinuousCancelReason::PluginFaulted))
            .expect("cancel");
    }

    #[test]
    fn manifest_parameter_validation_rejects_begin_before_lua() {
        let (_temporary, paths) = setup();
        write_required_parameter_package(&paths.plugins, FIRST_PLUGIN);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths).expect("plugin worker");
        let client = runtime.lifecycle_client();
        let (_session, begin) = begin_session(FIRST_PLUGIN, Uuid::from_u128(2));

        assert_eq!(dispatch(&client, begin), Err(PluginLifecycleError::ParametersInvalid));
        let snapshot = snapshot(&runtime);
        assert_eq!(snapshot.loaded_plugin_ids, vec![FIRST_PLUGIN]);
        assert_eq!(snapshot.plugins[0].actions[0].parameters.len(), 1);
    }

    #[test]
    fn worker_owns_one_session_and_defers_changed_package_teardown_until_terminal() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");
        let client = runtime.lifecycle_client();
        let (mut first, begin) = begin_session(FIRST_PLUGIN, Uuid::from_u128(11));
        dispatch(&client, begin).expect("first begin");
        accept_begin(&mut first);

        let (_second, second_begin) = begin_session(FIRST_PLUGIN, Uuid::from_u128(12));
        assert_eq!(dispatch(&client, second_begin), Err(PluginLifecycleError::SessionActive));
        assert_eq!(
            operate(&runtime, PluginWorkerOperation::Disable, FIRST_PLUGIN),
            Err(PluginWorkerError::SessionActive)
        );

        let changed_source = VALID_SOURCE.replace("return {}", "return { changed = true }");
        write_package(&paths.plugins, FIRST_PLUGIN, &[], &changed_source);
        let during_session = rescan(&runtime);
        assert_eq!(during_session.loaded_plugin_ids, vec![FIRST_PLUGIN]);
        assert_eq!(during_session.plugins[0].status, PluginInventoryStatus::ReloadRequired);

        dispatch(&client, cancel_request(&mut first, ContinuousCancelReason::ReturnedToDiscrete))
            .expect("cancel");
        let after_terminal = snapshot(&runtime);
        assert!(after_terminal.loaded_plugin_ids.is_empty());
        assert_eq!(after_terminal.plugins[0].status, PluginInventoryStatus::ReloadRequired);
    }

    #[test]
    fn callback_failure_faults_and_unloads_the_owned_plugin() {
        let (_temporary, paths) = setup();
        let source = VALID_SOURCE.replace(
            "update = function(session, sample) end",
            "update = function(session, sample) error('private failure') end",
        );
        write_package(&paths.plugins, FIRST_PLUGIN, &[], &source);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");
        let client = runtime.lifecycle_client();
        let (mut session, begin) = begin_session(FIRST_PLUGIN, Uuid::from_u128(21));
        dispatch(&client, begin).expect("begin");
        accept_begin(&mut session);

        assert_eq!(
            dispatch(&client, update_request(&mut session, 0.5)),
            Err(PluginLifecycleError::Lua(LuaSandboxError::Runtime))
        );
        assert!(snapshot(&runtime).loaded_plugin_ids.is_empty());
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Faulted);
    }

    #[test]
    fn terminal_lane_remains_available_when_the_update_lane_is_full() {
        let (_temporary, paths) = setup();
        let slow_begin = VALID_SOURCE.replace(
            "begin = function(context, parameters, sample) return {} end",
            "begin = function(context, parameters, sample) while true do end end",
        );
        write_package(&paths.plugins, FIRST_PLUGIN, &[], &slow_begin);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths).expect("plugin worker");
        let client = runtime.lifecycle_client();
        let (mut session, begin) = begin_session(FIRST_PLUGIN, Uuid::from_u128(31));
        accept_begin(&mut session);
        let update = update_request(&mut session, 0.5);
        let terminal = cancel_request(&mut session, ContinuousCancelReason::UserRequested);

        let begin_ticket = client.request(begin).expect("begin accepted");
        thread::sleep(Duration::from_millis(10));
        let update_ticket = client.request(update.clone()).expect("first update accepted");
        assert!(matches!(
            client.request(update),
            Err(PluginLifecycleSubmitError::QueueFull(PluginLifecycleStage::Update))
        ));
        let terminal_ticket = client.request(terminal).expect("reserved terminal accepted");

        assert!(matches!(
            begin_ticket.receive(),
            Err(PluginLifecycleError::Lua(
                LuaSandboxError::InstructionLimit | LuaSandboxError::DeadlineExceeded
            ))
        ));
        assert_eq!(terminal_ticket.receive(), Err(PluginLifecycleError::SessionNotActive));
        assert_eq!(update_ticket.receive(), Err(PluginLifecycleError::SessionNotActive));
    }

    #[test]
    fn timed_out_begin_can_be_aborted_before_lua_observes_it() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths).expect("plugin worker");
        let client = runtime.lifecycle_client();
        let session_id = Uuid::from_u128(32);

        client.abort_begin(session_id, ContinuousCancelReason::PluginFaulted).expect("abort lane");
        thread::sleep(Duration::from_millis(10));
        let (_session, begin) = begin_session(FIRST_PLUGIN, session_id);
        assert_eq!(dispatch(&client, begin), Err(PluginLifecycleError::BeginAborted));
        assert_eq!(snapshot(&runtime).loaded_plugin_ids, vec![FIRST_PLUGIN]);
    }

    #[test]
    fn startup_load_failure_faults_the_persisted_enabled_record() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], "return {}");
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);

        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");

        assert!(snapshot(&runtime).loaded_plugin_ids.is_empty());
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Faulted);
    }

    #[test]
    fn approval_and_successful_enable_remain_separate_worker_transactions() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");

        assert_eq!(
            operate(&runtime, PluginWorkerOperation::Enable, FIRST_PLUGIN),
            Err(PluginWorkerError::StateTransition(PluginStateTransitionError::ApprovalRequired))
        );
        let approval = operate(&runtime, PluginWorkerOperation::ApprovePermissions, FIRST_PLUGIN)
            .expect("approval");
        assert!(!approval.loaded);
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Disabled);

        let enabled = operate(&runtime, PluginWorkerOperation::Enable, FIRST_PLUGIN)
            .expect("validated enable");
        assert!(enabled.loaded);
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Enabled);
    }

    #[test]
    fn unapproved_source_is_never_evaluated_by_enable() {
        let (_temporary, paths) = setup();
        write_package(
            &paths.plugins,
            FIRST_PLUGIN,
            &["notification"],
            "error('this source must stay inert')",
        );
        let runtime = PluginWorkerRuntime::start(paths).expect("plugin worker");

        assert_eq!(
            operate(&runtime, PluginWorkerOperation::Enable, FIRST_PLUGIN),
            Err(PluginWorkerError::StateTransition(PluginStateTransitionError::ApprovalRequired))
        );
        let snapshot = snapshot(&runtime);
        assert!(snapshot.loaded_plugin_ids.is_empty());
        assert_eq!(snapshot.plugins.len(), 1);
        assert!(!snapshot.plugins[0].permissions_approved);
        assert_eq!(snapshot.plugins[0].status, PluginInventoryStatus::Disabled);
    }

    #[test]
    fn enable_uses_fresh_source_and_failed_validation_records_nothing() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");
        operate(&runtime, PluginWorkerOperation::ApprovePermissions, FIRST_PLUGIN)
            .expect("approval");
        write_package(&paths.plugins, FIRST_PLUGIN, &[], "return {}");

        assert_eq!(
            operate(&runtime, PluginWorkerOperation::Enable, FIRST_PLUGIN),
            Err(PluginWorkerError::Lua(LuaSandboxError::CallbackMissing { callback: "begin" }))
        );
        assert!(snapshot(&runtime).loaded_plugin_ids.is_empty());
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Disabled);
    }

    #[test]
    fn persistence_failure_drops_a_validated_enable_candidate() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");
        operate(&runtime, PluginWorkerOperation::ApprovePermissions, FIRST_PLUGIN)
            .expect("approval");
        fs::create_dir(&paths.plugins_state_temporary).expect("blocking temporary directory");

        assert_eq!(
            operate(&runtime, PluginWorkerOperation::Enable, FIRST_PLUGIN),
            Err(PluginWorkerError::StatePersistence(PluginStateStoreError::Write))
        );
        assert!(snapshot(&runtime).loaded_plugin_ids.is_empty());
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Disabled);
        fs::remove_dir(&paths.plugins_state_temporary).expect("remove blocking directory");
    }

    #[test]
    fn permission_change_unloads_and_disables_before_reapproval() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");
        assert_eq!(snapshot(&runtime).loaded_plugin_ids, vec![FIRST_PLUGIN]);
        let client = runtime.lifecycle_client();
        let plugin_id = PluginId::new(FIRST_PLUGIN).expect("plugin ID");
        let action_id = PluginActionId::new("measure").expect("action ID");
        let admission = client.action_admission(&plugin_id, &action_id).expect("admission");
        assert!(!admission.permits_context_application());
        write_package(&paths.plugins, FIRST_PLUGIN, &["context.application"], VALID_SOURCE);

        assert_eq!(
            operate(&runtime, PluginWorkerOperation::Reload, FIRST_PLUGIN),
            Err(PluginWorkerError::StateTransition(PluginStateTransitionError::ApprovalRequired))
        );
        assert!(snapshot(&runtime).loaded_plugin_ids.is_empty());
        assert_eq!(client.action_admission(&plugin_id, &action_id), None);
        assert_eq!(
            persisted_status(&paths, FIRST_PLUGIN),
            PluginTrustStatus::PermissionReviewRequired
        );
    }

    #[test]
    fn failed_reload_faults_and_unloads_until_a_valid_manual_reload() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");
        write_package(&paths.plugins, FIRST_PLUGIN, &[], "return {");

        assert_eq!(
            operate(&runtime, PluginWorkerOperation::Reload, FIRST_PLUGIN),
            Err(PluginWorkerError::Lua(LuaSandboxError::Syntax))
        );
        assert!(snapshot(&runtime).loaded_plugin_ids.is_empty());
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Faulted);

        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        let receipt = operate(&runtime, PluginWorkerOperation::Reload, FIRST_PLUGIN)
            .expect("recovered reload");
        assert!(receipt.loaded);
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Enabled);
    }

    #[test]
    fn valid_source_edit_becomes_authoritative_only_after_manual_reload() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");
        let changed_source = VALID_SOURCE.replace("return {}", "return { changed = true }");
        write_package(&paths.plugins, FIRST_PLUGIN, &[], &changed_source);

        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::ReloadRequired);
        let reloaded = operate(&runtime, PluginWorkerOperation::Reload, FIRST_PLUGIN)
            .expect("manual source reload");

        assert!(reloaded.loaded);
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Enabled);
    }

    #[test]
    fn explicit_rescan_is_code_free_and_unloads_a_changed_package() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");
        write_package(&paths.plugins, FIRST_PLUGIN, &[], "error('rescan must not execute this')");

        let snapshot = rescan(&runtime);

        assert!(snapshot.loaded_plugin_ids.is_empty());
        assert_eq!(snapshot.plugins[0].status, PluginInventoryStatus::ReloadRequired);
    }

    #[test]
    fn explicit_fault_reload_and_disable_have_durable_teardown() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        let runtime = PluginWorkerRuntime::start(paths.clone()).expect("plugin worker");

        let fault = operate(&runtime, PluginWorkerOperation::MarkFaulted, FIRST_PLUGIN)
            .expect("fault transition");
        assert!(!fault.loaded);
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Faulted);
        operate(&runtime, PluginWorkerOperation::Reload, FIRST_PLUGIN)
            .expect("manual fault recovery");
        let disabled =
            operate(&runtime, PluginWorkerOperation::Disable, FIRST_PLUGIN).expect("disable");
        assert!(!disabled.loaded);
        assert_eq!(persisted_status(&paths, FIRST_PLUGIN), PluginTrustStatus::Disabled);
    }

    #[test]
    fn invalid_identifiers_and_bounded_queue_fail_before_worker_execution() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], "while true do end");
        prepare_state(&paths, &[(FIRST_PLUGIN, false)]);
        let mut runtime = PluginWorkerRuntime::start(paths).expect("plugin worker");
        assert!(matches!(
            runtime.request(PluginWorkerOperation::Enable, "../outside"),
            Err(PluginWorkerSubmitError::InvalidPluginId)
        ));

        let _first = runtime
            .request(PluginWorkerOperation::Enable, FIRST_PLUGIN)
            .expect("first queued request");
        let mut queue_full = false;
        let mut tickets = Vec::new();
        for _ in 0..(COMMAND_CAPACITY * 8) {
            match runtime.request_snapshot() {
                Ok(ticket) => tickets.push(ticket),
                Err(PluginWorkerSubmitError::QueueFull) => {
                    queue_full = true;
                    break;
                }
                Err(error) => panic!("unexpected submission error: {error}"),
            }
        }
        assert!(queue_full);
        drop(tickets);

        let started = Instant::now();
        runtime.shutdown();
        assert!(started.elapsed() < SHUTDOWN_WAIT);
        assert!(matches!(runtime.request_snapshot(), Err(PluginWorkerSubmitError::ShuttingDown)));
    }

    #[test]
    fn root_discovery_failure_never_admits_persisted_enabled_state() {
        let (_temporary, paths) = setup();
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        prepare_state(&paths, &[(FIRST_PLUGIN, true)]);
        fs::remove_dir_all(&paths.plugins).expect("remove plugin root");
        fs::write(&paths.plugins, b"not a directory").expect("replace root with file");

        let runtime = PluginWorkerRuntime::start(paths).expect("plugin worker");

        assert!(snapshot(&runtime).loaded_plugin_ids.is_empty());
    }
}
