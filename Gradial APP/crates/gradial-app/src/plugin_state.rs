use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::ffi::OsStrExt,
    path::Path,
};

use gradial_core::{AppPaths, PLUGIN_MANIFEST_VERSION, PluginId, PluginManifest, PluginPermission};
use gradial_lua::MAX_PLUGIN_ROOT_ENTRIES;
use serde::{Deserialize, Serialize};
use windows::{
    Win32::Storage::FileSystem::{
        MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_WRITE_THROUGH, ReplaceFileW,
    },
    core::PCWSTR,
};

use crate::plugin_discovery::{
    DiscoveredPlugin, PluginDiscoveryReport, PluginPackageFingerprint, PluginPackageStatus,
};

pub(crate) const PLUGIN_STATE_SCHEMA_VERSION: u16 = 1;
const MAX_PLUGIN_STATE_BYTES: usize = 512 * 1024;
const SHA256_HEX_LENGTH: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginTrustState {
    schema_version: u16,
    revision: u64,
    plugins: BTreeMap<String, PluginTrustRecord>,
}

impl Default for PluginTrustState {
    fn default() -> Self {
        Self { schema_version: PLUGIN_STATE_SCHEMA_VERSION, revision: 0, plugins: BTreeMap::new() }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginTrustRecord {
    enabled: bool,
    approved_permissions: Vec<PluginPermission>,
    last_loaded_manifest: Option<LoadedPluginManifest>,
    faulted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadedPluginManifest {
    plugin_id: String,
    manifest_version: u16,
    plugin_version: String,
    api_version: u16,
    package_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginTrustStatus {
    Disabled,
    PermissionReviewRequired,
    Enabled,
    ReloadRequired,
    Faulted,
    Incompatible,
    Unavailable,
    Invalid,
}

impl PluginTrustStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::PermissionReviewRequired => "permission_review_required",
            Self::Enabled => "enabled",
            Self::ReloadRequired => "reload_required",
            Self::Faulted => "faulted",
            Self::Incompatible => "incompatible",
            Self::Unavailable => "unavailable",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PluginStateReconciliation {
    pub(crate) scan_available: bool,
    pub(crate) changed: bool,
    pub(crate) permission_changes: usize,
    pub(crate) unavailable_disabled: usize,
    pub(crate) statuses: BTreeMap<String, PluginTrustStatus>,
}

impl PluginStateReconciliation {
    pub(crate) fn status_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for status in self.statuses.values() {
            *counts.entry(status.as_str()).or_default() += 1;
        }
        counts
    }
}

#[derive(Clone, Debug)]
struct PluginObservation {
    plugin_id: String,
    manifest_version: u16,
    plugin_version: String,
    api_version: u16,
    permissions: Vec<PluginPermission>,
    package_fingerprint: PluginPackageFingerprint,
    compatible: bool,
}

impl PluginObservation {
    fn from_package(package: &DiscoveredPlugin) -> Result<Self, PluginStateTransitionError> {
        match &package.status {
            PluginPackageStatus::Compatible { manifest, fingerprint, .. } => {
                Ok(Self::new(manifest, *fingerprint, true))
            }
            PluginPackageStatus::IncompatibleApi { manifest, fingerprint, .. } => {
                Ok(Self::new(manifest, *fingerprint, false))
            }
            PluginPackageStatus::Invalid(_) => Err(PluginStateTransitionError::InvalidPackage),
        }
    }

    fn new(
        manifest: &PluginManifest,
        package_fingerprint: PluginPackageFingerprint,
        compatible: bool,
    ) -> Self {
        let mut permissions = manifest.permissions.clone();
        permissions.sort_unstable();
        Self {
            plugin_id: manifest.id.clone(),
            manifest_version: manifest.manifest_version,
            plugin_version: manifest.version.clone(),
            api_version: manifest.api_version,
            permissions,
            package_fingerprint,
            compatible,
        }
    }

    fn loaded_manifest(&self) -> LoadedPluginManifest {
        LoadedPluginManifest {
            plugin_id: self.plugin_id.clone(),
            manifest_version: self.manifest_version,
            plugin_version: self.plugin_version.clone(),
            api_version: self.api_version,
            package_sha256: self.package_fingerprint.to_hex(),
        }
    }
}

impl PluginTrustState {
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    pub(crate) fn status(
        &self,
        package: &DiscoveredPlugin,
    ) -> Result<PluginTrustStatus, PluginStateTransitionError> {
        let observation = PluginObservation::from_package(package)?;
        Ok(self.observation_status(&observation))
    }

    pub(crate) fn approve_permissions(
        &mut self,
        package: &DiscoveredPlugin,
    ) -> Result<(), PluginStateTransitionError> {
        let observation = PluginObservation::from_package(package)?;
        if !observation.compatible {
            return Err(PluginStateTransitionError::Incompatible);
        }
        let record = self.plugins.entry(observation.plugin_id).or_default();
        record.enabled = false;
        record.approved_permissions = observation.permissions;
        record.faulted = false;
        Ok(())
    }

    /// Records a completed load only after the caller has repeated discovery,
    /// compatibility checks, sandbox loading, and callback validation.
    pub(crate) fn record_successful_load(
        &mut self,
        package: &DiscoveredPlugin,
    ) -> Result<(), PluginStateTransitionError> {
        let observation = PluginObservation::from_package(package)?;
        if !observation.compatible {
            return Err(PluginStateTransitionError::Incompatible);
        }
        let record = self
            .plugins
            .get_mut(&observation.plugin_id)
            .ok_or(PluginStateTransitionError::ApprovalRequired)?;
        if record.approved_permissions != observation.permissions {
            return Err(PluginStateTransitionError::ApprovalRequired);
        }
        record.enabled = true;
        record.last_loaded_manifest = Some(observation.loaded_manifest());
        record.faulted = false;
        Ok(())
    }

    pub(crate) fn disable(&mut self, plugin_id: &str) -> Result<(), PluginStateTransitionError> {
        let record =
            self.plugins.get_mut(plugin_id).ok_or(PluginStateTransitionError::UnknownPlugin)?;
        record.enabled = false;
        record.faulted = false;
        Ok(())
    }

    pub(crate) fn mark_faulted(
        &mut self,
        plugin_id: &str,
    ) -> Result<(), PluginStateTransitionError> {
        let record =
            self.plugins.get_mut(plugin_id).ok_or(PluginStateTransitionError::UnknownPlugin)?;
        if !record.enabled || record.last_loaded_manifest.is_none() {
            return Err(PluginStateTransitionError::NotEnabled);
        }
        record.faulted = true;
        Ok(())
    }

    pub(crate) fn reconcile(
        &mut self,
        report: &PluginDiscoveryReport,
    ) -> PluginStateReconciliation {
        if report.root_error.is_some() {
            return PluginStateReconciliation::default();
        }

        let mut result = PluginStateReconciliation { scan_available: true, ..Default::default() };
        let observations = report
            .packages
            .iter()
            .filter_map(|package| PluginObservation::from_package(package).ok())
            .map(|observation| (observation.plugin_id.clone(), observation))
            .collect::<BTreeMap<_, _>>();
        let present_directories = report
            .packages
            .iter()
            .map(|package| package.directory_name.as_str())
            .collect::<BTreeSet<_>>();

        for (plugin_id, record) in &mut self.plugins {
            let Some(observation) = observations.get(plugin_id) else {
                if record.enabled {
                    record.enabled = false;
                    record.faulted = false;
                    result.changed = true;
                    result.unavailable_disabled += 1;
                }
                continue;
            };
            if !observation.compatible {
                if record.enabled {
                    record.enabled = false;
                    record.faulted = false;
                    result.changed = true;
                    result.unavailable_disabled += 1;
                }
                continue;
            }
            if record.approved_permissions != observation.permissions && record.enabled {
                record.enabled = false;
                record.faulted = false;
                result.changed = true;
                result.permission_changes += 1;
            }
        }

        for package in &report.packages {
            let status = match PluginObservation::from_package(package) {
                Ok(observation) => self.observation_status(&observation),
                Err(_) => PluginTrustStatus::Invalid,
            };
            result.statuses.insert(package.directory_name.clone(), status);
        }
        for plugin_id in self.plugins.keys() {
            if !present_directories.contains(plugin_id.as_str()) {
                result.statuses.entry(plugin_id.clone()).or_insert(PluginTrustStatus::Unavailable);
            }
        }
        result
    }

    fn observation_status(&self, observation: &PluginObservation) -> PluginTrustStatus {
        if !observation.compatible {
            return PluginTrustStatus::Incompatible;
        }
        let Some(record) = self.plugins.get(&observation.plugin_id) else {
            return PluginTrustStatus::Disabled;
        };
        if record.approved_permissions != observation.permissions {
            return PluginTrustStatus::PermissionReviewRequired;
        }
        if !record.enabled {
            return PluginTrustStatus::Disabled;
        }
        if record.faulted {
            return PluginTrustStatus::Faulted;
        }
        let Some(loaded) = &record.last_loaded_manifest else {
            return PluginTrustStatus::Disabled;
        };
        if loaded != &observation.loaded_manifest() {
            return PluginTrustStatus::ReloadRequired;
        }
        PluginTrustStatus::Enabled
    }

    fn validate(&self) -> Result<(), PluginStateValidationError> {
        if self.schema_version != PLUGIN_STATE_SCHEMA_VERSION {
            return Err(PluginStateValidationError::UnsupportedSchema);
        }
        if self.plugins.len() > MAX_PLUGIN_ROOT_ENTRIES {
            return Err(PluginStateValidationError::TooManyPlugins);
        }
        for (plugin_id, record) in &self.plugins {
            PluginId::new(plugin_id).map_err(|_| PluginStateValidationError::InvalidPluginId)?;
            if !record.approved_permissions.windows(2).all(|pair| pair[0] < pair[1]) {
                return Err(PluginStateValidationError::InvalidPermissions);
            }
            if record.faulted && !record.enabled
                || record.enabled && record.last_loaded_manifest.is_none()
            {
                return Err(PluginStateValidationError::InvalidLifecycleState);
            }
            if let Some(loaded) = &record.last_loaded_manifest
                && (loaded.plugin_id != *plugin_id
                    || loaded.manifest_version != PLUGIN_MANIFEST_VERSION
                    || semver::Version::parse(&loaded.plugin_version).is_err()
                    || !valid_sha256(&loaded.package_sha256))
            {
                return Err(PluginStateValidationError::InvalidManifestIdentity);
            }
        }
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == SHA256_HEX_LENGTH
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum PluginStateTransitionError {
    #[error("plugin_state_package_invalid")]
    InvalidPackage,
    #[error("plugin_state_plugin_incompatible")]
    Incompatible,
    #[error("plugin_state_approval_required")]
    ApprovalRequired,
    #[error("plugin_state_plugin_unknown")]
    UnknownPlugin,
    #[error("plugin_state_plugin_not_enabled")]
    NotEnabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginStateSource {
    Primary,
    InitializedDefault,
    BackupRecovery,
    InMemoryRecoveryDefault,
}

impl PluginStateSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::InitializedDefault => "initialized_default",
            Self::BackupRecovery => "backup_recovery",
            Self::InMemoryRecoveryDefault => "in_memory_recovery_default",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PluginStateWarning {
    pub(crate) primary_error: PluginStateFileErrorCode,
    pub(crate) backup_error: Option<PluginStateFileErrorCode>,
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedPluginState {
    pub(crate) state: PluginTrustState,
    pub(crate) source: PluginStateSource,
    pub(crate) warning: Option<PluginStateWarning>,
}

#[derive(Clone, Debug)]
pub(crate) struct PluginStateStore {
    paths: AppPaths,
}

impl PluginStateStore {
    pub(crate) const fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub(crate) fn load_or_initialize(&self) -> LoadedPluginState {
        match read_plugin_state(&self.paths.plugins_state) {
            Ok(state) => {
                LoadedPluginState { state, source: PluginStateSource::Primary, warning: None }
            }
            Err(primary_error @ PluginStateFileError::Missing) => {
                match read_plugin_state(&self.paths.plugins_state_backup) {
                    Ok(state) => LoadedPluginState {
                        state,
                        source: PluginStateSource::BackupRecovery,
                        warning: Some(PluginStateWarning {
                            primary_error: primary_error.code(),
                            backup_error: None,
                        }),
                    },
                    Err(PluginStateFileError::Missing) => {
                        let default = PluginTrustState::default();
                        match self.save(&default, &default) {
                            Ok(state) => LoadedPluginState {
                                state,
                                source: PluginStateSource::InitializedDefault,
                                warning: None,
                            },
                            Err(_) => LoadedPluginState {
                                state: default,
                                source: PluginStateSource::InMemoryRecoveryDefault,
                                warning: Some(PluginStateWarning {
                                    primary_error: PluginStateFileErrorCode::Write,
                                    backup_error: None,
                                }),
                            },
                        }
                    }
                    Err(backup_error) => recovery_default(primary_error, backup_error),
                }
            }
            Err(primary_error) => match read_plugin_state(&self.paths.plugins_state_backup) {
                Ok(state) => LoadedPluginState {
                    state,
                    source: PluginStateSource::BackupRecovery,
                    warning: Some(PluginStateWarning {
                        primary_error: primary_error.code(),
                        backup_error: None,
                    }),
                },
                Err(backup_error) => recovery_default(primary_error, backup_error),
            },
        }
    }

    pub(crate) fn save(
        &self,
        current: &PluginTrustState,
        proposed: &PluginTrustState,
    ) -> Result<PluginTrustState, PluginStateStoreError> {
        self.save_internal(current, proposed, SaveInterruption::None)
    }

    fn save_internal(
        &self,
        current: &PluginTrustState,
        proposed: &PluginTrustState,
        interruption: SaveInterruption,
    ) -> Result<PluginTrustState, PluginStateStoreError> {
        current.validate().map_err(|_| PluginStateStoreError::InvalidState)?;
        let mut next = proposed.clone();
        next.schema_version = PLUGIN_STATE_SCHEMA_VERSION;
        next.revision =
            current.revision.checked_add(1).ok_or(PluginStateStoreError::RevisionExhausted)?;
        next.validate().map_err(|_| PluginStateStoreError::InvalidState)?;

        let preserve_backup = match read_plugin_state(&self.paths.plugins_state) {
            Ok(on_disk) if on_disk != *current => return Err(PluginStateStoreError::StaleState),
            Ok(_) | Err(PluginStateFileError::Missing) => false,
            Err(_) => true,
        };
        let mut serialized =
            serde_json::to_vec_pretty(&next).map_err(|_| PluginStateStoreError::Serialize)?;
        serialized.push(b'\n');
        if serialized.len() > MAX_PLUGIN_STATE_BYTES {
            return Err(PluginStateStoreError::StateTooLarge);
        }
        write_temporary(&self.paths.plugins_state_temporary, &serialized)?;
        if interruption == SaveInterruption::AfterTemporaryFlush {
            return Err(PluginStateStoreError::Interrupted);
        }
        let result = replace_state(
            &self.paths.plugins_state,
            &self.paths.plugins_state_temporary,
            &self.paths.plugins_state_backup,
            preserve_backup,
        );
        if result.is_err() {
            let _removed = fs::remove_file(&self.paths.plugins_state_temporary);
        }
        result?;
        Ok(next)
    }
}

fn recovery_default(
    primary_error: PluginStateFileError,
    backup_error: PluginStateFileError,
) -> LoadedPluginState {
    LoadedPluginState {
        state: PluginTrustState::default(),
        source: PluginStateSource::InMemoryRecoveryDefault,
        warning: Some(PluginStateWarning {
            primary_error: primary_error.code(),
            backup_error: Some(backup_error.code()),
        }),
    }
}

fn read_plugin_state(path: &Path) -> Result<PluginTrustState, PluginStateFileError> {
    let file = File::open(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            PluginStateFileError::Missing
        } else {
            PluginStateFileError::Read
        }
    })?;
    let mut bytes = Vec::with_capacity(16 * 1024);
    file.take(u64::try_from(MAX_PLUGIN_STATE_BYTES).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| PluginStateFileError::Read)?;
    if bytes.len() > MAX_PLUGIN_STATE_BYTES {
        return Err(PluginStateFileError::TooLarge);
    }
    let schema_probe: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| PluginStateFileError::Parse)?;
    let schema = schema_probe.get("schema_version").and_then(serde_json::Value::as_u64);
    if schema != Some(u64::from(PLUGIN_STATE_SCHEMA_VERSION)) {
        return Err(PluginStateFileError::UnsupportedSchema);
    }
    let state: PluginTrustState =
        serde_json::from_slice(&bytes).map_err(|_| PluginStateFileError::Parse)?;
    state.validate().map_err(|_| PluginStateFileError::Invalid)?;
    Ok(state)
}

fn write_temporary(path: &Path, bytes: &[u8]) -> Result<(), PluginStateStoreError> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(PluginStateStoreError::Write),
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|_| PluginStateStoreError::Write)?;
    file.write_all(bytes).map_err(|_| PluginStateStoreError::Write)?;
    file.sync_all().map_err(|_| PluginStateStoreError::Write)?;
    Ok(())
}

fn replace_state(
    primary: &Path,
    temporary: &Path,
    backup: &Path,
    preserve_backup: bool,
) -> Result<(), PluginStateStoreError> {
    let primary_wide = wide_path(primary);
    let temporary_wide = wide_path(temporary);
    if primary.exists() {
        if preserve_backup {
            // SAFETY: both buffers are live null-terminated paths in the same
            // Gradial-owned directory. The valid recovery backup is retained.
            unsafe {
                ReplaceFileW(
                    PCWSTR::from_raw(primary_wide.as_ptr()),
                    PCWSTR::from_raw(temporary_wide.as_ptr()),
                    PCWSTR::null(),
                    REPLACEFILE_WRITE_THROUGH,
                    None,
                    None,
                )
                .map_err(|_| PluginStateStoreError::Replace)?;
            }
        } else {
            match fs::remove_file(backup) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(PluginStateStoreError::Replace),
            }
            let backup_wide = wide_path(backup);
            // SAFETY: all buffers are live null-terminated same-volume paths.
            // Windows atomically installs the new state and preserves the
            // previous valid primary as the recovery backup.
            unsafe {
                ReplaceFileW(
                    PCWSTR::from_raw(primary_wide.as_ptr()),
                    PCWSTR::from_raw(temporary_wide.as_ptr()),
                    PCWSTR::from_raw(backup_wide.as_ptr()),
                    REPLACEFILE_WRITE_THROUGH,
                    None,
                    None,
                )
                .map_err(|_| PluginStateStoreError::Replace)?;
            }
        }
    } else {
        // SAFETY: both buffers are live null-terminated same-volume paths and
        // no primary exists during first initialization or explicit repair.
        unsafe {
            MoveFileExW(
                PCWSTR::from_raw(temporary_wide.as_ptr()),
                PCWSTR::from_raw(primary_wide.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
            .map_err(|_| PluginStateStoreError::Replace)?;
        }
    }
    Ok(())
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SaveInterruption {
    None,
    AfterTemporaryFlush,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum PluginStateStoreError {
    #[error("plugin_state_invalid")]
    InvalidState,
    #[error("plugin_state_revision_exhausted")]
    RevisionExhausted,
    #[error("plugin_state_stale")]
    StaleState,
    #[error("plugin_state_serialize_failed")]
    Serialize,
    #[error("plugin_state_too_large")]
    StateTooLarge,
    #[error("plugin_state_write_failed")]
    Write,
    #[error("plugin_state_replace_failed")]
    Replace,
    #[error("plugin_state_interrupted")]
    Interrupted,
}

impl PluginStateStoreError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::InvalidState => "invalid",
            Self::RevisionExhausted => "revision_exhausted",
            Self::StaleState => "stale",
            Self::Serialize => "serialize_failed",
            Self::StateTooLarge => "too_large",
            Self::Write => "write_failed",
            Self::Replace => "replace_failed",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PluginStateFileErrorCode {
    Missing,
    Read,
    TooLarge,
    Parse,
    UnsupportedSchema,
    Invalid,
    Write,
}

impl PluginStateFileErrorCode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Read => "read_failed",
            Self::TooLarge => "too_large",
            Self::Parse => "parse_failed",
            Self::UnsupportedSchema => "unsupported_schema",
            Self::Invalid => "invalid",
            Self::Write => "write_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PluginStateFileError {
    Missing,
    Read,
    TooLarge,
    Parse,
    UnsupportedSchema,
    Invalid,
}

impl PluginStateFileError {
    const fn code(self) -> PluginStateFileErrorCode {
        match self {
            Self::Missing => PluginStateFileErrorCode::Missing,
            Self::Read => PluginStateFileErrorCode::Read,
            Self::TooLarge => PluginStateFileErrorCode::TooLarge,
            Self::Parse => PluginStateFileErrorCode::Parse,
            Self::UnsupportedSchema => PluginStateFileErrorCode::UnsupportedSchema,
            Self::Invalid => PluginStateFileErrorCode::Invalid,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PluginStateValidationError {
    UnsupportedSchema,
    TooManyPlugins,
    InvalidPluginId,
    InvalidPermissions,
    InvalidLifecycleState,
    InvalidManifestIdentity,
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;
    use crate::plugin_discovery::discover_plugins;

    const PLUGIN_ID: &str = "example.media-controls";
    const SOURCE_SENTINEL: &str = "private-source-sentinel";

    fn setup() -> (tempfile::TempDir, AppPaths, PluginStateStore) {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let paths = AppPaths::from_local_app_data(temporary.path());
        paths.ensure_directories().expect("directories");
        let store = PluginStateStore::new(paths.clone());
        (temporary, paths, store)
    }

    fn write_package(
        root: &Path,
        api_version: u16,
        permissions: &[&str],
        source: &str,
        manifest_suffix: &str,
    ) {
        let package = root.join(PLUGIN_ID);
        fs::create_dir_all(&package).expect("package directory");
        let permissions = permissions
            .iter()
            .map(|permission| format!("\"{permission}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let manifest = format!(
            "manifest_version = 1\nid = \"{PLUGIN_ID}\"\nname = \"Media controls\"\n\
             version = \"0.1.0\"\napi_version = {api_version}\nentry = \"main.lua\"\n\
             permissions = [{permissions}]\n{manifest_suffix}\n"
        );
        fs::write(package.join("plugin.toml"), manifest).expect("manifest");
        fs::write(package.join("main.lua"), source).expect("source");
    }

    fn discovered(paths: &AppPaths) -> PluginDiscoveryReport {
        discover_plugins(&paths.plugins)
    }

    fn only_package(report: &PluginDiscoveryReport) -> &DiscoveredPlugin {
        assert_eq!(report.packages.len(), 1);
        &report.packages[0]
    }

    fn enabled_state(paths: &AppPaths) -> PluginTrustState {
        write_package(&paths.plugins, 1, &["context.application"], SOURCE_SENTINEL, "");
        let report = discovered(paths);
        let mut state = PluginTrustState::default();
        state.approve_permissions(only_package(&report)).expect("approval");
        state.record_successful_load(only_package(&report)).expect("successful load record");
        state
    }

    #[test]
    fn first_load_atomically_initializes_an_empty_versioned_file() {
        let (_temporary, paths, store) = setup();
        let loaded = store.load_or_initialize();

        assert_eq!(loaded.source, PluginStateSource::InitializedDefault);
        assert_eq!(loaded.state.revision(), 1);
        assert_eq!(loaded.state.plugin_count(), 0);
        assert!(loaded.warning.is_none());
        assert!(paths.plugins_state.is_file());
        assert!(!paths.plugins_state_temporary.exists());
        assert_eq!(read_plugin_state(&paths.plugins_state).expect("primary"), loaded.state);
    }

    #[test]
    fn approval_and_successful_load_are_separate_explicit_transitions() {
        let (_temporary, paths, _store) = setup();
        write_package(
            &paths.plugins,
            1,
            &["input.shortcut", "context.application"],
            "this is deliberately not valid Lua",
            "",
        );
        let report = discovered(&paths);
        let package = only_package(&report);
        let mut state = PluginTrustState::default();

        assert_eq!(state.status(package), Ok(PluginTrustStatus::Disabled));
        assert_eq!(
            state.record_successful_load(package),
            Err(PluginStateTransitionError::ApprovalRequired)
        );
        state.approve_permissions(package).expect("approve current permissions");
        assert_eq!(state.status(package), Ok(PluginTrustStatus::Disabled));
        state.record_successful_load(package).expect("record validated load");
        assert_eq!(state.status(package), Ok(PluginTrustStatus::Enabled));

        let record = state.plugins.get(PLUGIN_ID).expect("record");
        assert_eq!(
            record.approved_permissions,
            vec![PluginPermission::ContextApplication, PluginPermission::InputShortcut]
        );
        assert!(record.enabled);
        assert!(!record.faulted);
    }

    #[test]
    fn permission_changes_disable_until_the_new_set_is_approved() {
        let (_temporary, paths, _store) = setup();
        let mut state = enabled_state(&paths);
        write_package(
            &paths.plugins,
            1,
            &["context.application", "input.shortcut"],
            SOURCE_SENTINEL,
            "",
        );
        let report = discovered(&paths);

        let reconciliation = state.reconcile(&report);
        assert!(reconciliation.changed);
        assert_eq!(reconciliation.permission_changes, 1);
        assert_eq!(
            state.status(only_package(&report)),
            Ok(PluginTrustStatus::PermissionReviewRequired)
        );
        assert_eq!(
            state.record_successful_load(only_package(&report)),
            Err(PluginStateTransitionError::ApprovalRequired)
        );

        state.approve_permissions(only_package(&report)).expect("approve changed permissions");
        state.record_successful_load(only_package(&report)).expect("record reload");
        assert_eq!(state.status(only_package(&report)), Ok(PluginTrustStatus::Enabled));
    }

    #[test]
    fn source_and_non_permission_manifest_edits_require_manual_reload_only() {
        let (_temporary, paths, _store) = setup();
        let mut state = enabled_state(&paths);
        write_package(
            &paths.plugins,
            1,
            &["context.application"],
            "changed source that is still not executed",
            "",
        );
        let changed_source = discovered(&paths);
        let reconciliation = state.reconcile(&changed_source);

        assert!(!reconciliation.changed);
        assert_eq!(
            state.status(only_package(&changed_source)),
            Ok(PluginTrustStatus::ReloadRequired)
        );
        state.record_successful_load(only_package(&changed_source)).expect("manual source reload");
        assert_eq!(state.status(only_package(&changed_source)), Ok(PluginTrustStatus::Enabled));

        write_package(
            &paths.plugins,
            1,
            &["context.application"],
            "changed source that is still not executed",
            "# author-only manifest edit",
        );
        let changed_manifest = discovered(&paths);
        assert_eq!(
            state.status(only_package(&changed_manifest)),
            Ok(PluginTrustStatus::ReloadRequired)
        );
        assert_eq!(
            state.plugins[PLUGIN_ID].approved_permissions,
            vec![PluginPermission::ContextApplication]
        );
    }

    #[test]
    fn incompatible_missing_and_invalid_packages_fail_closed() {
        let (_temporary, paths, _store) = setup();
        let mut state = enabled_state(&paths);
        write_package(&paths.plugins, 99, &["context.application"], SOURCE_SENTINEL, "");
        let incompatible = discovered(&paths);
        let reconciliation = state.reconcile(&incompatible);
        assert!(reconciliation.changed);
        assert_eq!(reconciliation.unavailable_disabled, 1);
        assert_eq!(state.status(only_package(&incompatible)), Ok(PluginTrustStatus::Incompatible));

        write_package(&paths.plugins, 1, &["context.application"], SOURCE_SENTINEL, "");
        let compatible = discovered(&paths);
        state.record_successful_load(only_package(&compatible)).expect("enable again");
        fs::remove_dir_all(paths.plugins.join(PLUGIN_ID)).expect("remove package");
        let missing = discovered(&paths);
        let reconciliation = state.reconcile(&missing);
        assert!(reconciliation.changed);
        assert_eq!(reconciliation.statuses[PLUGIN_ID], PluginTrustStatus::Unavailable);

        write_package(&paths.plugins, 1, &["context.application"], SOURCE_SENTINEL, "");
        fs::write(paths.plugins.join(PLUGIN_ID).join("plugin.toml"), "invalid")
            .expect("invalid manifest");
        let invalid = discovered(&paths);
        assert_eq!(state.reconcile(&invalid).statuses[PLUGIN_ID], PluginTrustStatus::Invalid);
    }

    #[test]
    fn fault_is_persistable_and_disable_or_successful_reload_clears_it() {
        let (_temporary, paths, _store) = setup();
        let report = {
            write_package(&paths.plugins, 1, &[], SOURCE_SENTINEL, "");
            discovered(&paths)
        };
        let package = only_package(&report);
        let mut state = PluginTrustState::default();
        state.approve_permissions(package).expect("approve");
        state.record_successful_load(package).expect("load");
        state.mark_faulted(PLUGIN_ID).expect("fault");
        assert_eq!(state.status(package), Ok(PluginTrustStatus::Faulted));

        state.disable(PLUGIN_ID).expect("disable");
        assert_eq!(state.status(package), Ok(PluginTrustStatus::Disabled));
        state.record_successful_load(package).expect("enable after validated load");
        assert_eq!(state.status(package), Ok(PluginTrustStatus::Enabled));
    }

    #[test]
    fn atomic_save_preserves_backup_and_interruption_keeps_primary_active() {
        let (_temporary, paths, store) = setup();
        let initial = store.load_or_initialize().state;
        write_package(&paths.plugins, 1, &["notification"], SOURCE_SENTINEL, "");
        let report = discovered(&paths);
        let mut approved = initial.clone();
        approved.approve_permissions(only_package(&report)).expect("approval");
        let approved = store.save(&initial, &approved).expect("approval save");
        assert_eq!(read_plugin_state(&paths.plugins_state_backup).expect("backup"), initial);

        let mut enabled = approved.clone();
        enabled.record_successful_load(only_package(&report)).expect("load record");
        assert_eq!(
            store.save_internal(&approved, &enabled, SaveInterruption::AfterTemporaryFlush),
            Err(PluginStateStoreError::Interrupted)
        );
        assert_eq!(read_plugin_state(&paths.plugins_state).expect("active primary"), approved);
        assert!(paths.plugins_state_temporary.is_file());
    }

    #[test]
    fn malformed_primary_recovers_from_backup_without_overwriting_it() {
        let (_temporary, paths, store) = setup();
        let initial = store.load_or_initialize().state;
        write_package(&paths.plugins, 1, &[], SOURCE_SENTINEL, "");
        let report = discovered(&paths);
        let mut approved = initial.clone();
        approved.approve_permissions(only_package(&report)).expect("approval");
        let approved = store.save(&initial, &approved).expect("save approved");
        let mut enabled = approved.clone();
        enabled.record_successful_load(only_package(&report)).expect("load record");
        let _enabled = store.save(&approved, &enabled).expect("save enabled");
        let backup_bytes = fs::read(&paths.plugins_state_backup).expect("backup bytes");
        fs::write(&paths.plugins_state, b"damaged primary sentinel").expect("damage primary");

        let loaded = store.load_or_initialize();
        assert_eq!(loaded.source, PluginStateSource::BackupRecovery);
        assert_eq!(loaded.state, approved);
        assert_eq!(fs::read(&paths.plugins_state_backup).expect("unchanged backup"), backup_bytes);
        assert_eq!(
            fs::read(&paths.plugins_state).expect("unchanged broken primary"),
            b"damaged primary sentinel"
        );
    }

    #[test]
    fn corrupt_or_future_state_falls_back_disabled_without_rewriting_evidence() {
        let (_temporary, paths, store) = setup();
        fs::write(&paths.plugins_state, br#"{"schema_version":99,"revision":4,"plugins":{}}"#)
            .expect("future state");
        fs::write(&paths.plugins_state_backup, b"broken backup").expect("broken backup");

        let loaded = store.load_or_initialize();
        assert_eq!(loaded.source, PluginStateSource::InMemoryRecoveryDefault);
        assert_eq!(loaded.state, PluginTrustState::default());
        let warning = loaded.warning.expect("warning");
        assert_eq!(warning.primary_error, PluginStateFileErrorCode::UnsupportedSchema);
        assert_eq!(warning.backup_error, Some(PluginStateFileErrorCode::Parse));
        assert!(fs::read(&paths.plugins_state).expect("future preserved").starts_with(b"{"));
        assert_eq!(
            fs::read(&paths.plugins_state_backup).expect("backup preserved"),
            b"broken backup"
        );
    }

    #[test]
    fn oversized_and_impossible_enabled_state_are_rejected_before_use() {
        let (_temporary, paths, store) = setup();
        fs::write(&paths.plugins_state, vec![b' '; MAX_PLUGIN_STATE_BYTES + 1])
            .expect("oversized state");
        let oversized = store.load_or_initialize();
        let warning = oversized.warning.expect("oversized warning");
        assert_eq!(warning.primary_error, PluginStateFileErrorCode::TooLarge);
        assert_eq!(warning.backup_error, Some(PluginStateFileErrorCode::Missing));
        assert_eq!(oversized.state, PluginTrustState::default());

        let impossible = format!(
            r#"{{
              "schema_version": 1,
              "revision": 7,
              "plugins": {{
                "{PLUGIN_ID}": {{
                  "enabled": true,
                  "approved_permissions": [],
                  "last_loaded_manifest": null,
                  "faulted": false
                }}
              }}
            }}"#
        );
        fs::write(&paths.plugins_state, impossible).expect("impossible state");
        let impossible = store.load_or_initialize();
        assert_eq!(
            impossible.warning.expect("invalid warning").primary_error,
            PluginStateFileErrorCode::Invalid
        );
        assert_eq!(impossible.state, PluginTrustState::default());
    }

    #[test]
    fn serialized_state_contains_no_source_paths_parameters_or_credentials() {
        let (_temporary, paths, store) = setup();
        write_package(
            &paths.plugins,
            1,
            &["network.loopback"],
            "return { secret = 'private-source-sentinel' }",
            "",
        );
        let report = discovered(&paths);
        let initial = store.load_or_initialize().state;
        let mut enabled = initial.clone();
        enabled.approve_permissions(only_package(&report)).expect("approve");
        enabled.record_successful_load(only_package(&report)).expect("load record");
        store.save(&initial, &enabled).expect("save state");
        let text = fs::read_to_string(&paths.plugins_state).expect("state text");
        let private_plugin_path = paths.plugins.to_string_lossy().into_owned();

        assert!(text.contains("network.loopback"));
        assert!(text.contains(PLUGIN_ID));
        for private in [
            SOURCE_SENTINEL,
            "private-source-sentinel",
            "main.lua",
            "maximum_steps",
            "credential",
            &private_plugin_path,
        ] {
            assert!(!text.contains(private), "state leaked {private}");
        }
    }

    #[test]
    fn stale_save_is_rejected_without_replacing_newer_state() {
        let (_temporary, paths, store) = setup();
        let initial = store.load_or_initialize().state;
        write_package(&paths.plugins, 1, &[], SOURCE_SENTINEL, "");
        let report = discovered(&paths);
        let mut first = initial.clone();
        first.approve_permissions(only_package(&report)).expect("first approval");
        let saved = store.save(&initial, &first).expect("first save");
        let mut stale = initial.clone();
        stale.approve_permissions(only_package(&report)).expect("stale approval");

        assert_eq!(store.save(&initial, &stale), Err(PluginStateStoreError::StaleState));
        assert_eq!(read_plugin_state(&paths.plugins_state).expect("newer remains"), saved);
    }
}
