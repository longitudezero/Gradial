use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};
use gradial_core::{AppPaths, PluginId, PluginPermission};
use gradial_lua::{LuaLifecycleAdapter, LuaSandboxError, LuaSandboxLimits};
use thiserror::Error;

use crate::{
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
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

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
    const fn as_str(self) -> &'static str {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginInventoryEntry {
    pub(crate) plugin_id: String,
    pub(crate) display_name: String,
    pub(crate) version: Option<String>,
    pub(crate) api_version: Option<u16>,
    pub(crate) permissions: Vec<PluginPermission>,
    pub(crate) permissions_approved: bool,
    pub(crate) status: PluginInventoryStatus,
    pub(crate) issue_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

#[derive(Debug)]
pub(crate) struct PluginWorkerRuntime {
    commands: Sender<PluginWorkerCommand>,
    shutdown_requested: Arc<AtomicBool>,
    stopped: Receiver<()>,
    join_handle: Option<JoinHandle<()>>,
}

impl PluginWorkerRuntime {
    pub(crate) fn start(paths: AppPaths) -> Result<Self> {
        let (command_sender, commands) = bounded(COMMAND_CAPACITY);
        let (stopped_sender, stopped) = bounded(1);
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutdown_requested);
        let join_handle = thread::Builder::new()
            .name("gradial-plugin-worker".to_owned())
            .spawn(move || {
                let mut worker = PluginWorker::bootstrap(paths, &worker_shutdown);
                worker.run(&commands, &worker_shutdown);
                let _sent = stopped_sender.try_send(());
            })
            .context("could not spawn the plugin worker")?;
        log::info!(
            target: "plugins",
            "component=plugins event_code=plugin_worker_started result_code=ok \
             command_capacity={COMMAND_CAPACITY} shutdown_wait_ms={}",
            SHUTDOWN_WAIT.as_millis()
        );
        Ok(Self {
            commands: command_sender,
            shutdown_requested,
            stopped,
            join_handle: Some(join_handle),
        })
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
    adapters: BTreeMap<String, LuaLifecycleAdapter>,
    inventory: Vec<PluginInventoryEntry>,
    scan_error_code: Option<String>,
}

impl PluginWorker {
    fn bootstrap(paths: AppPaths, shutdown_requested: &AtomicBool) -> Self {
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
            inventory: Vec::new(),
            scan_error_code: None,
        };
        worker.bootstrap_packages(shutdown_requested);
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
                match load_adapter(package) {
                    Ok(adapter) => {
                        self.adapters.insert(plugin_id.clone(), adapter);
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
                        self.persist_fault_after_load_failure(&plugin_id);
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

    fn run(&mut self, commands: &Receiver<PluginWorkerCommand>, shutdown_requested: &AtomicBool) {
        while !shutdown_requested.load(Ordering::Acquire) {
            let Ok(command) = commands.recv() else {
                break;
            };
            match command {
                PluginWorkerCommand::Operate { operation, plugin_id, reply } => {
                    let result = self.operate(operation, &plugin_id);
                    self.refresh_inventory_view();
                    log_operation(
                        operation,
                        &plugin_id,
                        result.as_ref().map(|_| ()).map_err(|error| *error),
                        self.state.revision(),
                        self.adapters.contains_key(&plugin_id),
                        "request",
                    );
                    let _sent = reply.try_send(result);
                }
                PluginWorkerCommand::Snapshot { reply } => {
                    let _sent = reply.try_send(self.snapshot());
                }
                PluginWorkerCommand::Rescan { reply } => {
                    let result = self.fresh_report();
                    log::info!(
                        target: "plugins",
                        "component=plugins event_code=plugin_rescan result_code={} \
                         state_revision={} plugin_count={} error_code={}",
                        if result.is_ok() { "ok" } else { "failure" },
                        self.state.revision(),
                        self.inventory.len(),
                        result.err().map_or("none", PluginWorkerError::code)
                    );
                    let _sent = reply.try_send(self.snapshot());
                }
                PluginWorkerCommand::Shutdown => break,
            }
        }
        self.adapters.clear();
    }

    fn operate(
        &mut self,
        operation: PluginWorkerOperation,
        plugin_id: &str,
    ) -> Result<PluginWorkerReceipt, PluginWorkerError> {
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

        let candidate = load_adapter(&package).map_err(PluginWorkerError::Lua)?;
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
        let candidate = match load_adapter(&package) {
            Ok(candidate) => candidate,
            Err(error) => {
                self.persist_fault_after_load_failure(plugin_id);
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

    fn fresh_report(&mut self) -> Result<PluginDiscoveryReport, PluginWorkerError> {
        let report = discover_plugins(&self.paths.plugins);
        log_discovery_report(&report);
        if let Some(error) = report.root_error {
            self.scan_error_code = Some(error.to_string());
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

    fn persist_fault_after_load_failure(&mut self, plugin_id: &str) {
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

    fn update_inventory(&mut self, report: &PluginDiscoveryReport) {
        let mut observed = self.state.clone();
        let statuses = observed.reconcile(report).statuses;
        self.inventory = statuses
            .into_iter()
            .map(|(plugin_id, status)| {
                let package =
                    report.packages.iter().find(|package| package.directory_name == plugin_id);
                match package.map(|package| &package.status) {
                    Some(
                        PluginPackageStatus::Compatible { manifest, .. }
                        | PluginPackageStatus::IncompatibleApi { manifest, .. },
                    ) => {
                        let permissions_approved = package.is_some_and(|package| {
                            self.state.permissions_approved(package).unwrap_or(false)
                        });
                        PluginInventoryEntry {
                            plugin_id,
                            display_name: manifest.name.clone(),
                            version: Some(manifest.version.clone()),
                            api_version: Some(manifest.api_version),
                            permissions: manifest.permissions.clone(),
                            permissions_approved,
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

fn load_adapter(package: &DiscoveredPlugin) -> Result<LuaLifecycleAdapter, LuaSandboxError> {
    let PluginPackageStatus::Compatible { sources, .. } = &package.status else {
        return Err(LuaSandboxError::Initialization);
    };
    LuaLifecycleAdapter::load(sources.clone(), LuaSandboxLimits::default())
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
    use std::{fs, path::Path, time::Instant};

    use super::*;
    use crate::plugin_discovery::discover_plugins;

    const FIRST_PLUGIN: &str = "example.first";
    const SECOND_PLUGIN: &str = "example.second";
    const VALID_SOURCE: &str = r"
        return {
            begin = function(context, parameters, sample) return {} end,
            update = function(session, sample) end,
            commit = function(session, sample) end,
            cancel = function(session, reason) end,
        }
    ";

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
             permissions = [{permissions}]\n"
        );
        fs::write(package.join("plugin.toml"), manifest).expect("plugin manifest");
        fs::write(package.join("main.lua"), source).expect("plugin source");
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
        write_package(&paths.plugins, FIRST_PLUGIN, &[], VALID_SOURCE);
        write_package(&paths.plugins, SECOND_PLUGIN, &[], "this is invalid Lua");
        prepare_state(&paths, &[(FIRST_PLUGIN, true), (SECOND_PLUGIN, false)]);

        let runtime = PluginWorkerRuntime::start(paths).expect("plugin worker");
        let snapshot = snapshot(&runtime);

        assert_eq!(snapshot.loaded_plugin_ids, vec![FIRST_PLUGIN]);
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
        write_package(&paths.plugins, FIRST_PLUGIN, &["context.application"], VALID_SOURCE);

        assert_eq!(
            operate(&runtime, PluginWorkerOperation::Reload, FIRST_PLUGIN),
            Err(PluginWorkerError::StateTransition(PluginStateTransitionError::ApprovalRequired))
        );
        assert!(snapshot(&runtime).loaded_plugin_ids.is_empty());
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
