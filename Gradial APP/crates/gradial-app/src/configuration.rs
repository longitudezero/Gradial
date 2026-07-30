use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use gradial_core::{
    ACTION_CONFIG_SCHEMA_VERSION, ACTIVATION_CONFIG_SCHEMA_VERSION, ActionPlanCompiler, AppPaths,
    Appearance, CONFIG_SCHEMA_VERSION, Configuration, LEGACY_CONFIG_SCHEMA_VERSION,
    ProfileResolver,
};
use uuid::Uuid;
use windows::{
    Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_WRITE_THROUGH,
        ReplaceFileW,
    },
    core::PCWSTR,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigurationSource {
    Primary,
    InitializedDefault,
    BackupRecovery,
    InMemoryRecoveryDefault,
}

impl ConfigurationSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::InitializedDefault => "initialized_default",
            Self::BackupRecovery => "backup_recovery",
            Self::InMemoryRecoveryDefault => "in_memory_recovery_default",
        }
    }
}

#[derive(Debug)]
pub(crate) struct LoadedConfiguration {
    pub(crate) configuration: Configuration,
    pub(crate) source: ConfigurationSource,
    pub(crate) migration: Option<ConfigurationMigration>,
    pub(crate) warning: Option<ConfigurationWarning>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConfigurationMigration {
    pub(crate) from_schema: u32,
    pub(crate) to_schema: u32,
}

#[derive(Debug)]
pub(crate) struct ConfigurationWarning {
    pub(crate) primary_error: String,
    pub(crate) backup_error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImportedConfiguration {
    pub(crate) configuration: Configuration,
    pub(crate) migration: Option<ConfigurationMigration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum ConfigurationTransferError {
    #[error("the selected path is not available")]
    InvalidPath,
    #[error("the selected file does not exist")]
    Missing,
    #[error("the selected file could not be read")]
    Read,
    #[error("the selected file is not valid Gradial settings")]
    Parse,
    #[error("the selected file uses an unsupported settings version")]
    UnsupportedSchema,
    #[error("the selected settings are invalid")]
    Invalid,
    #[error("the selected destination is reserved by Gradial")]
    ProtectedDestination,
    #[error("the settings export could not be written")]
    Write,
}

impl ConfigurationTransferError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::InvalidPath => "invalid_path",
            Self::Missing => "missing",
            Self::Read => "read_failed",
            Self::Parse => "parse_failed",
            Self::UnsupportedSchema => "unsupported_schema",
            Self::Invalid => "config_invalid",
            Self::ProtectedDestination => "protected_destination",
            Self::Write => "write_failed",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ConfigStore {
    paths: AppPaths,
}

impl ConfigStore {
    pub(crate) const fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub(crate) fn load_or_initialize(&self) -> Result<LoadedConfiguration> {
        match read_configuration(&self.paths.config) {
            Ok(read) => Ok(LoadedConfiguration {
                configuration: read.configuration,
                source: ConfigurationSource::Primary,
                migration: read.migration,
                warning: None,
            }),
            Err(primary_error @ ConfigurationFileError::Missing { .. }) => {
                match read_configuration(&self.paths.config_backup) {
                    Ok(read) => Ok(LoadedConfiguration {
                        configuration: read.configuration,
                        source: ConfigurationSource::BackupRecovery,
                        migration: read.migration,
                        warning: Some(ConfigurationWarning {
                            primary_error: primary_error.to_string(),
                            backup_error: None,
                        }),
                    }),
                    Err(ConfigurationFileError::Missing { .. }) => {
                        let default = Configuration::default();
                        let configuration = self
                            .save(&default, &default)
                            .context("could not initialize the default configuration")?;
                        Ok(LoadedConfiguration {
                            configuration,
                            source: ConfigurationSource::InitializedDefault,
                            migration: None,
                            warning: None,
                        })
                    }
                    Err(backup_error) => Self::recovery_default(&primary_error, &backup_error),
                }
            }
            Err(primary_error) => match read_configuration(&self.paths.config_backup) {
                Ok(read) => Ok(LoadedConfiguration {
                    configuration: read.configuration,
                    source: ConfigurationSource::BackupRecovery,
                    migration: read.migration,
                    warning: Some(ConfigurationWarning {
                        primary_error: primary_error.to_string(),
                        backup_error: None,
                    }),
                }),
                Err(backup_error) => Self::recovery_default(&primary_error, &backup_error),
            },
        }
    }

    pub(crate) fn save(
        &self,
        current: &Configuration,
        proposed: &Configuration,
    ) -> Result<Configuration> {
        self.save_internal(current, proposed, SaveInterruption::None)
    }

    pub(crate) fn import_draft(
        &self,
        current: &Configuration,
        path: &Path,
    ) -> Result<ImportedConfiguration, ConfigurationTransferError> {
        if !path.is_absolute() {
            return Err(ConfigurationTransferError::InvalidPath);
        }
        if windows_paths_equal(path, &self.paths.config_temporary) {
            return Err(ConfigurationTransferError::InvalidPath);
        }
        let read = read_configuration(path).map_err(ConfigurationTransferError::from)?;
        let mut configuration = read.configuration;
        configuration.config_revision = current.config_revision;
        validate_complete_configuration(&configuration)?;
        Ok(ImportedConfiguration { configuration, migration: read.migration })
    }

    pub(crate) fn export_draft(
        &self,
        current: &Configuration,
        path: &Path,
        configuration: &Configuration,
    ) -> Result<(), ConfigurationTransferError> {
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(ConfigurationTransferError::InvalidPath);
        }
        if self.is_protected_destination(path) {
            return Err(ConfigurationTransferError::ProtectedDestination);
        }
        if configuration.config_revision != current.config_revision {
            return Err(ConfigurationTransferError::Invalid);
        }
        validate_complete_configuration(configuration)?;
        let mut serialized = serde_json::to_vec_pretty(configuration)
            .map_err(|_| ConfigurationTransferError::Invalid)?;
        serialized.push(b'\n');
        write_export_atomically(path, &serialized)
    }

    fn is_protected_destination(&self, path: &Path) -> bool {
        [
            &self.paths.config,
            &self.paths.config_backup,
            &self.paths.config_temporary,
            &self.paths.onboarding,
            &self.paths.onboarding_temporary,
        ]
        .into_iter()
        .any(|protected| windows_paths_equal(path, protected))
    }

    fn save_internal(
        &self,
        current: &Configuration,
        proposed: &Configuration,
        interruption: SaveInterruption,
    ) -> Result<Configuration> {
        let mut next = proposed.clone();
        next.config_revision = current
            .config_revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("configuration revision is exhausted"))?;
        next.validate().context("proposed configuration is invalid")?;

        let mut serialized =
            serde_json::to_vec_pretty(&next).context("could not serialize configuration")?;
        serialized.push(b'\n');
        write_temporary_file(&self.paths.config_temporary, &serialized)?;
        if interruption == SaveInterruption::AfterTemporaryFlush {
            bail!("injected interruption after temporary configuration flush");
        }

        let preserve_existing_backup = self.paths.config.exists()
            && read_configuration(&self.paths.config)
                .map_or(true, |primary| primary.configuration != *current);
        replace_configuration(
            &self.paths.config,
            &self.paths.config_temporary,
            &self.paths.config_backup,
            preserve_existing_backup,
        )?;
        Ok(next)
    }

    fn recovery_default(
        primary_error: &ConfigurationFileError,
        backup_error: &ConfigurationFileError,
    ) -> Result<LoadedConfiguration> {
        let configuration = Configuration::default();
        configuration.validate().context("built-in configuration is invalid")?;
        Ok(LoadedConfiguration {
            configuration,
            source: ConfigurationSource::InMemoryRecoveryDefault,
            migration: None,
            warning: Some(ConfigurationWarning {
                primary_error: primary_error.to_string(),
                backup_error: Some(backup_error.to_string()),
            }),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SaveInterruption {
    None,
    AfterTemporaryFlush,
}

#[derive(Debug, thiserror::Error)]
enum ConfigurationFileError {
    #[error("configuration file {path} does not exist")]
    Missing { path: PathBuf },

    #[error("could not read configuration file {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not parse configuration file {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error(
        "configuration file {path} uses schema {found}; this build supports schema {supported}"
    )]
    UnsupportedSchema { path: PathBuf, found: u32, supported: u32 },

    #[error("configuration file {path} is invalid")]
    Invalid {
        path: PathBuf,
        #[source]
        source: gradial_core::ConfigurationError,
    },

    #[error("configuration file {path} cannot migrate from schema {from_schema}: {reason}")]
    InvalidMigration { path: PathBuf, from_schema: u32, reason: &'static str },
}

impl From<ConfigurationFileError> for ConfigurationTransferError {
    fn from(error: ConfigurationFileError) -> Self {
        match error {
            ConfigurationFileError::Missing { .. } => Self::Missing,
            ConfigurationFileError::Read { .. } => Self::Read,
            ConfigurationFileError::Parse { .. } => Self::Parse,
            ConfigurationFileError::UnsupportedSchema { .. } => Self::UnsupportedSchema,
            ConfigurationFileError::Invalid { .. }
            | ConfigurationFileError::InvalidMigration { .. } => Self::Invalid,
        }
    }
}

#[derive(Debug)]
struct ReadConfiguration {
    configuration: Configuration,
    migration: Option<ConfigurationMigration>,
}

fn read_configuration(path: &Path) -> Result<ReadConfiguration, ConfigurationFileError> {
    let bytes = fs::read(path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            ConfigurationFileError::Missing { path: path.to_path_buf() }
        } else {
            ConfigurationFileError::Read { path: path.to_path_buf(), source }
        }
    })?;
    let mut value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|source| ConfigurationFileError::Parse { path: path.to_path_buf(), source })?;
    let schema_version = value.get("schema_version").and_then(serde_json::Value::as_u64);
    let migration_from =
        schema_version.and_then(|schema| u32::try_from(schema).ok()).filter(|schema| {
            matches!(
                *schema,
                LEGACY_CONFIG_SCHEMA_VERSION
                    | ACTION_CONFIG_SCHEMA_VERSION
                    | ACTIVATION_CONFIG_SCHEMA_VERSION
            )
        });
    if let Some(schema_version) = schema_version
        && schema_version != u64::from(CONFIG_SCHEMA_VERSION)
        && migration_from.is_none()
    {
        return Err(ConfigurationFileError::UnsupportedSchema {
            path: path.to_path_buf(),
            found: u32::try_from(schema_version).unwrap_or(u32::MAX),
            supported: CONFIG_SCHEMA_VERSION,
        });
    }
    if migration_from == Some(LEGACY_CONFIG_SCHEMA_VERSION)
        && schema_one_contains_action_fields(&value)
    {
        return Err(ConfigurationFileError::InvalidMigration {
            path: path.to_path_buf(),
            from_schema: LEGACY_CONFIG_SCHEMA_VERSION,
            reason: "schema-one files cannot contain actions or action references",
        });
    }
    if let Some(from_schema) = migration_from {
        let root =
            value.as_object_mut().ok_or_else(|| ConfigurationFileError::InvalidMigration {
                path: path.to_path_buf(),
                from_schema,
                reason: "the root must be a JSON object",
            })?;
        if from_schema < ACTIVATION_CONFIG_SCHEMA_VERSION && root.contains_key("activation_binding")
        {
            return Err(ConfigurationFileError::InvalidMigration {
                path: path.to_path_buf(),
                from_schema,
                reason: "older schemas cannot contain an activation binding",
            });
        }
        if root.contains_key("appearance") {
            return Err(ConfigurationFileError::InvalidMigration {
                path: path.to_path_buf(),
                from_schema,
                reason: "older schemas cannot contain appearance settings",
            });
        }
        if from_schema < ACTIVATION_CONFIG_SCHEMA_VERSION {
            root.insert(
                "activation_binding".to_owned(),
                serde_json::to_value(gradial_core::ActivationBinding::default())
                    .expect("the built-in activation binding is serializable"),
            );
        }
        root.insert(
            "appearance".to_owned(),
            serde_json::to_value(Appearance::default())
                .expect("the built-in appearance is serializable"),
        );
    }
    let mut configuration: Configuration = serde_json::from_value(value)
        .map_err(|source| ConfigurationFileError::Parse { path: path.to_path_buf(), source })?;
    let migration = migration_from.map(|from_schema| ConfigurationMigration {
        from_schema,
        to_schema: CONFIG_SCHEMA_VERSION,
    });
    if migration.is_some() {
        configuration.schema_version = CONFIG_SCHEMA_VERSION;
    }
    configuration
        .validate()
        .map_err(|source| ConfigurationFileError::Invalid { path: path.to_path_buf(), source })?;
    Ok(ReadConfiguration { configuration, migration })
}

fn schema_one_contains_action_fields(value: &serde_json::Value) -> bool {
    if value.get("actions").is_some() {
        return true;
    }
    let global_has_action =
        value.get("global_profile").is_some_and(profile_contains_action_reference);
    let application_has_action = value
        .get("application_profiles")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|profiles| {
            profiles.iter().any(|application_profile| {
                application_profile.get("profile").is_some_and(profile_contains_action_reference)
            })
        });
    global_has_action || application_has_action
}

fn profile_contains_action_reference(profile: &serde_json::Value) -> bool {
    profile
        .get("menu")
        .and_then(|menu| menu.get("slices"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|slices| slices.iter().any(|slice| slice.get("action_id").is_some()))
}

fn write_temporary_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temporary =
        OpenOptions::new().create(true).truncate(true).write(true).open(path).with_context(
            || format!("could not create temporary configuration {}", path.display()),
        )?;
    temporary
        .write_all(bytes)
        .with_context(|| format!("could not write temporary configuration {}", path.display()))?;
    temporary
        .sync_all()
        .with_context(|| format!("could not flush temporary configuration {}", path.display()))
}

fn validate_complete_configuration(
    configuration: &Configuration,
) -> Result<(), ConfigurationTransferError> {
    configuration.validate().map_err(|_| ConfigurationTransferError::Invalid)?;
    ProfileResolver::compile(configuration).map_err(|_| ConfigurationTransferError::Invalid)?;
    ActionPlanCompiler::from_configuration(configuration)
        .map_err(|_| ConfigurationTransferError::Invalid)?;
    Ok(())
}

fn write_export_atomically(
    destination: &Path,
    bytes: &[u8],
) -> Result<(), ConfigurationTransferError> {
    let Some(parent) = destination.parent().filter(|parent| parent.is_dir()) else {
        return Err(ConfigurationTransferError::InvalidPath);
    };
    let temporary = parent.join(format!(".gradial-export-{}.tmp", Uuid::new_v4().as_hyphenated()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|_| ConfigurationTransferError::Write)?;
        file.write_all(bytes).map_err(|_| ConfigurationTransferError::Write)?;
        file.sync_all().map_err(|_| ConfigurationTransferError::Write)?;
        drop(file);

        let temporary_wide = wide_path(&temporary);
        let destination_wide = wide_path(destination);
        // SAFETY: both buffers are live, null-terminated paths in the same
        // directory. Windows replaces an existing file as one move.
        unsafe {
            MoveFileExW(
                PCWSTR::from_raw(temporary_wide.as_ptr()),
                PCWSTR::from_raw(destination_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .map_err(|_| ConfigurationTransferError::Write)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _removed = fs::remove_file(&temporary);
    }
    result
}

fn windows_paths_equal(left: &Path, right: &Path) -> bool {
    let left = normalized_windows_path(left);
    let right = normalized_windows_path(right);
    left.as_os_str()
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(right.as_os_str().to_string_lossy().trim_end_matches(['\\', '/']))
}

fn normalized_windows_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }
    let Some(parent) = path.parent() else {
        return path.to_path_buf();
    };
    let Ok(canonical_parent) = fs::canonicalize(parent) else {
        return path.to_path_buf();
    };
    path.file_name().map_or(canonical_parent.clone(), |name| canonical_parent.join(name))
}

fn replace_configuration(
    primary: &Path,
    temporary: &Path,
    backup: &Path,
    preserve_existing_backup: bool,
) -> Result<()> {
    let primary_wide = wide_path(primary);
    let temporary_wide = wide_path(temporary);
    if primary.exists() {
        if preserve_existing_backup {
            // SAFETY: both buffers are live, null-terminated same-volume paths.
            // Omitting a backup keeps the already validated recovery backup
            // instead of replacing it with the broken primary.
            unsafe {
                ReplaceFileW(
                    PCWSTR::from_raw(primary_wide.as_ptr()),
                    PCWSTR::from_raw(temporary_wide.as_ptr()),
                    PCWSTR::null(),
                    REPLACEFILE_WRITE_THROUGH,
                    None,
                    None,
                )
                .context("Windows could not atomically repair config.json")?;
            }
        } else {
            if backup.exists() {
                fs::remove_file(backup)
                    .with_context(|| format!("could not remove old backup {}", backup.display()))?;
            }
            let backup_wide = wide_path(backup);
            // SAFETY: all three buffers are live, null-terminated paths on the
            // same volume. ReplaceFileW atomically exchanges the primary and
            // preserves the previous valid primary at the backup path.
            unsafe {
                ReplaceFileW(
                    PCWSTR::from_raw(primary_wide.as_ptr()),
                    PCWSTR::from_raw(temporary_wide.as_ptr()),
                    PCWSTR::from_raw(backup_wide.as_ptr()),
                    REPLACEFILE_WRITE_THROUGH,
                    None,
                    None,
                )
                .context("Windows could not atomically replace config.json")?;
            }
        }
    } else {
        // SAFETY: both buffers are live, null-terminated same-volume paths.
        // No destination exists during first-run initialization.
        unsafe {
            MoveFileExW(
                PCWSTR::from_raw(temporary_wide.as_ptr()),
                PCWSTR::from_raw(primary_wide.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
            .context("Windows could not install the initial config.json")?;
        }
    }
    Ok(())
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gradial_core::{
        ACTION_CONFIG_SCHEMA_VERSION, ACTIVATION_CONFIG_SCHEMA_VERSION, ActionDefinition,
        ActionKind, ActivationBinding, AppPaths, Appearance, CONFIG_SCHEMA_VERSION, Configuration,
        LEGACY_CONFIG_SCHEMA_VERSION,
    };
    use uuid::Uuid;

    use super::{
        ConfigStore, ConfigurationMigration, ConfigurationSource, ConfigurationTransferError,
        SaveInterruption, read_configuration,
    };

    fn store() -> (tempfile::TempDir, AppPaths, ConfigStore) {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let paths = AppPaths::from_local_app_data(temporary_directory.path());
        paths.ensure_directories().expect("configuration directories");
        let store = ConfigStore::new(paths.clone());
        (temporary_directory, paths, store)
    }

    fn active_configuration(path: &std::path::Path) -> Configuration {
        read_configuration(path).expect("configuration").configuration
    }

    fn schema_one_bytes() -> Vec<u8> {
        let mut value =
            serde_json::to_value(Configuration::default()).expect("configuration value");
        let root = value.as_object_mut().expect("configuration object");
        root.insert(
            "schema_version".to_owned(),
            serde_json::Value::from(LEGACY_CONFIG_SCHEMA_VERSION),
        );
        root.remove("activation_binding");
        root.remove("appearance");
        root.remove("actions");
        let slices = root
            .get_mut("global_profile")
            .and_then(|profile| profile.get_mut("menu"))
            .and_then(|menu| menu.get_mut("slices"))
            .and_then(serde_json::Value::as_array_mut)
            .expect("global menu slices");
        for slice in slices {
            slice.as_object_mut().expect("slice object").remove("action_id");
        }
        serde_json::to_vec_pretty(&value).expect("legacy bytes")
    }

    fn schema_two_bytes() -> Vec<u8> {
        let mut value =
            serde_json::to_value(Configuration::default()).expect("configuration value");
        let root = value.as_object_mut().expect("configuration object");
        root.insert(
            "schema_version".to_owned(),
            serde_json::Value::from(ACTION_CONFIG_SCHEMA_VERSION),
        );
        root.remove("activation_binding");
        root.remove("appearance");
        serde_json::to_vec_pretty(&value).expect("schema-two bytes")
    }

    fn schema_three_bytes() -> Vec<u8> {
        let mut value =
            serde_json::to_value(Configuration::default()).expect("configuration value");
        let root = value.as_object_mut().expect("configuration object");
        root.insert(
            "schema_version".to_owned(),
            serde_json::Value::from(ACTIVATION_CONFIG_SCHEMA_VERSION),
        );
        root.remove("appearance");
        serde_json::to_vec_pretty(&value).expect("schema-three bytes")
    }

    #[test]
    fn first_load_atomically_initializes_schema_four() {
        let (_temporary_directory, paths, store) = store();
        let loaded = store.load_or_initialize().expect("initialize configuration");

        assert_eq!(loaded.source, ConfigurationSource::InitializedDefault);
        assert_eq!(loaded.configuration.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(loaded.configuration.config_revision, 1);
        assert_eq!(active_configuration(&paths.config), loaded.configuration);
        assert!(!paths.config_temporary.exists());
    }

    #[test]
    fn successful_replacement_preserves_the_previous_valid_file_as_backup() {
        let (_temporary_directory, paths, store) = store();
        let first = store.load_or_initialize().expect("first configuration").configuration;
        let mut proposed = first.clone();
        proposed.global_profile.name = "Changed".to_owned();

        let second = store.save(&first, &proposed).expect("replacement");

        assert_eq!(second.config_revision, 2);
        assert_eq!(active_configuration(&paths.config), second);
        assert_eq!(active_configuration(&paths.config_backup), first);
    }

    #[test]
    fn an_interruption_before_replace_leaves_the_primary_active() {
        let (_temporary_directory, paths, store) = store();
        let first = store.load_or_initialize().expect("first configuration").configuration;
        let mut proposed = first.clone();
        proposed.global_profile.name = "Interrupted".to_owned();

        assert!(
            store.save_internal(&first, &proposed, SaveInterruption::AfterTemporaryFlush).is_err()
        );
        assert_eq!(active_configuration(&paths.config), first);
        assert!(paths.config_temporary.exists());
    }

    #[test]
    fn broken_primary_recovers_from_backup_without_overwriting_the_broken_file() {
        let (_temporary_directory, paths, store) = store();
        let first = store.load_or_initialize().expect("first configuration").configuration;
        let mut proposed = first.clone();
        proposed.global_profile.name = "Second".to_owned();
        store.save(&first, &proposed).expect("second configuration");
        fs::write(&paths.config, b"{ broken").expect("corrupt primary");

        let loaded = store.load_or_initialize().expect("backup recovery");

        assert_eq!(loaded.source, ConfigurationSource::BackupRecovery);
        assert_eq!(loaded.configuration, first);
        assert!(loaded.warning.is_some());
        assert_eq!(fs::read(&paths.config).expect("broken primary retained"), b"{ broken");
    }

    #[test]
    fn explicit_save_after_backup_recovery_repairs_primary_and_keeps_valid_backup() {
        let (_temporary_directory, paths, store) = store();
        let first = store.load_or_initialize().expect("first configuration").configuration;
        let mut second_proposal = first.clone();
        second_proposal.global_profile.name = "Second".to_owned();
        store.save(&first, &second_proposal).expect("second configuration");
        fs::write(&paths.config, b"{ broken").expect("corrupt primary");
        let recovered = store.load_or_initialize().expect("backup recovery").configuration;
        let mut repaired_proposal = recovered.clone();
        repaired_proposal.global_profile.name = "Repaired".to_owned();

        let repaired = store.save(&recovered, &repaired_proposal).expect("explicit repair");

        assert_eq!(repaired.config_revision, 2);
        assert_eq!(active_configuration(&paths.config), repaired);
        assert_eq!(active_configuration(&paths.config_backup), recovered);
    }

    #[test]
    fn newer_schema_and_missing_backup_use_default_without_touching_primary() {
        let (_temporary_directory, paths, store) = store();
        let newer = format!(
            "{{\"schema_version\":{},\"config_revision\":9,\
             \"global_profile\":{{}},\"application_profiles\":[]}}",
            CONFIG_SCHEMA_VERSION + 1
        );
        fs::write(&paths.config, newer.as_bytes()).expect("newer configuration");

        let loaded = store.load_or_initialize().expect("safe recovery default");

        assert_eq!(loaded.source, ConfigurationSource::InMemoryRecoveryDefault);
        assert_eq!(loaded.configuration, Configuration::default());
        assert!(
            loaded
                .warning
                .as_ref()
                .is_some_and(|warning| warning.primary_error.contains("uses schema"))
        );
        assert_eq!(fs::read_to_string(&paths.config).expect("primary retained"), newer);
    }

    #[test]
    fn invalid_proposal_never_changes_disk_or_revision() {
        let (_temporary_directory, paths, store) = store();
        let first = store.load_or_initialize().expect("first configuration").configuration;
        let mut invalid = first.clone();
        invalid.global_profile.menu.slice_count = 12;

        assert!(store.save(&first, &invalid).is_err());
        assert_eq!(first.config_revision, 1);
        assert_eq!(active_configuration(&paths.config), first);
    }

    #[test]
    fn schema_one_migrates_only_in_memory_and_keeps_original_bytes() {
        let (_temporary_directory, paths, store) = store();
        let legacy = schema_one_bytes();
        fs::write(&paths.config, &legacy).expect("legacy configuration");

        let loaded = store.load_or_initialize().expect("migrated configuration");

        assert_eq!(loaded.source, ConfigurationSource::Primary);
        assert_eq!(
            loaded.migration,
            Some(ConfigurationMigration {
                from_schema: LEGACY_CONFIG_SCHEMA_VERSION,
                to_schema: CONFIG_SCHEMA_VERSION,
            })
        );
        assert_eq!(loaded.configuration.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(loaded.configuration.activation_binding, ActivationBinding::default());
        assert_eq!(loaded.configuration.appearance, Appearance::default());
        assert!(loaded.configuration.actions.is_empty());
        assert!(
            loaded
                .configuration
                .global_profile
                .menu
                .slices
                .iter()
                .all(|slice| slice.action_id.is_none())
        );
        assert_eq!(fs::read(&paths.config).expect("legacy retained"), legacy);
    }

    #[test]
    fn schema_two_gains_binding_and_appearance_defaults_only_in_memory() {
        let (_temporary_directory, paths, store) = store();
        let schema_two = schema_two_bytes();
        fs::write(&paths.config, &schema_two).expect("schema-two configuration");

        let loaded = store.load_or_initialize().expect("migrated configuration");

        assert_eq!(loaded.source, ConfigurationSource::Primary);
        assert_eq!(
            loaded.migration,
            Some(ConfigurationMigration {
                from_schema: ACTION_CONFIG_SCHEMA_VERSION,
                to_schema: CONFIG_SCHEMA_VERSION,
            })
        );
        assert_eq!(loaded.configuration.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(loaded.configuration.activation_binding, ActivationBinding::default());
        assert_eq!(loaded.configuration.appearance, Appearance::default());
        assert_eq!(fs::read(&paths.config).expect("schema two retained"), schema_two);
    }

    #[test]
    fn schema_three_gains_only_the_default_appearance_in_memory() {
        let (_temporary_directory, paths, store) = store();
        let schema_three = schema_three_bytes();
        fs::write(&paths.config, &schema_three).expect("schema-three configuration");

        let loaded = store.load_or_initialize().expect("migrated configuration");

        assert_eq!(loaded.source, ConfigurationSource::Primary);
        assert_eq!(
            loaded.migration,
            Some(ConfigurationMigration {
                from_schema: ACTIVATION_CONFIG_SCHEMA_VERSION,
                to_schema: CONFIG_SCHEMA_VERSION,
            })
        );
        assert_eq!(loaded.configuration.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(loaded.configuration.activation_binding, ActivationBinding::default());
        assert_eq!(loaded.configuration.appearance, Appearance::default());
        assert_eq!(fs::read(&paths.config).expect("schema three retained"), schema_three);
    }

    #[test]
    fn schema_one_cannot_smuggle_schema_two_action_fields() {
        let (_temporary_directory, paths, store) = store();
        let mut value: serde_json::Value =
            serde_json::from_slice(&schema_one_bytes()).expect("legacy value");
        value["actions"] = serde_json::Value::Array(Vec::new());
        fs::write(&paths.config, serde_json::to_vec_pretty(&value).expect("legacy bytes"))
            .expect("legacy configuration");

        let loaded = store.load_or_initialize().expect("recovery configuration");

        assert_eq!(loaded.source, ConfigurationSource::InMemoryRecoveryDefault);
        assert!(loaded.migration.is_none());
        assert!(
            loaded
                .warning
                .expect("migration warning")
                .primary_error
                .contains("cannot migrate from schema 1")
        );
    }

    #[test]
    fn older_schemas_cannot_smuggle_an_activation_binding() {
        let (_temporary_directory, paths, store) = store();
        let mut value: serde_json::Value =
            serde_json::from_slice(&schema_two_bytes()).expect("schema-two value");
        value["activation_binding"] =
            serde_json::to_value(ActivationBinding::default()).expect("binding");
        fs::write(&paths.config, serde_json::to_vec_pretty(&value).expect("schema-two bytes"))
            .expect("schema-two configuration");

        let loaded = store.load_or_initialize().expect("recovery configuration");

        assert_eq!(loaded.source, ConfigurationSource::InMemoryRecoveryDefault);
        assert!(loaded.migration.is_none());
        assert!(
            loaded
                .warning
                .expect("migration warning")
                .primary_error
                .contains("older schemas cannot contain an activation binding")
        );
    }

    #[test]
    fn older_schemas_cannot_smuggle_appearance_settings() {
        let (_temporary_directory, paths, store) = store();
        let mut value: serde_json::Value =
            serde_json::from_slice(&schema_three_bytes()).expect("schema-three value");
        value["appearance"] = serde_json::to_value(Appearance::default()).expect("appearance");
        fs::write(&paths.config, serde_json::to_vec_pretty(&value).expect("schema-three bytes"))
            .expect("schema-three configuration");

        let loaded = store.load_or_initialize().expect("recovery configuration");

        assert_eq!(loaded.source, ConfigurationSource::InMemoryRecoveryDefault);
        assert!(loaded.migration.is_none());
        assert!(
            loaded
                .warning
                .expect("migration warning")
                .primary_error
                .contains("older schemas cannot contain appearance settings")
        );
    }

    #[test]
    fn import_migrates_privately_resets_revision_and_preserves_literal_paths() {
        let (_temporary_directory, paths, store) = store();
        let current = store.load_or_initialize().expect("current configuration").configuration;
        let mut imported = Configuration { config_revision: 999, ..Configuration::default() };
        imported.global_profile.name = "Imported draft".to_owned();
        imported.actions.push(ActionDefinition {
            id: Uuid::from_u128(701),
            name: "Literal application".to_owned(),
            kind: ActionKind::RunExecutable {
                executable: std::path::PathBuf::from(r"C:\Private Folder\literal.exe"),
                arguments: vec![r"--value=C:\Keep\This Exactly".to_owned()],
                working_directory: Some(std::path::PathBuf::from(r"C:\Private Folder")),
            },
        });
        let mut value = serde_json::to_value(&imported).expect("configuration value");
        let root = value.as_object_mut().expect("configuration object");
        root.insert(
            "schema_version".to_owned(),
            serde_json::Value::from(ACTION_CONFIG_SCHEMA_VERSION),
        );
        root.remove("activation_binding");
        root.remove("appearance");
        let bytes = serde_json::to_vec_pretty(&value).expect("import bytes");
        let source = paths.root.join("private-import.json");
        fs::write(&source, &bytes).expect("import source");

        let result = store.import_draft(&current, &source).expect("imported draft");

        assert_eq!(result.configuration.config_revision, current.config_revision);
        assert_eq!(result.configuration.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(
            result.migration,
            Some(ConfigurationMigration {
                from_schema: ACTION_CONFIG_SCHEMA_VERSION,
                to_schema: CONFIG_SCHEMA_VERSION,
            })
        );
        let ActionKind::RunExecutable { executable, arguments, working_directory } =
            &result.configuration.actions[0].kind
        else {
            panic!("application action");
        };
        assert_eq!(executable, &std::path::PathBuf::from(r"C:\Private Folder\literal.exe"));
        assert_eq!(arguments, &[r"--value=C:\Keep\This Exactly"]);
        assert_eq!(
            working_directory.as_ref(),
            Some(&std::path::PathBuf::from(r"C:\Private Folder"))
        );
        assert_eq!(fs::read(&source).expect("source retained"), bytes);
        assert_eq!(
            read_configuration(&paths.config).expect("active config").configuration,
            current
        );
    }

    #[test]
    fn import_rejects_newer_and_invalid_files_with_fixed_categories() {
        let (_temporary_directory, paths, store) = store();
        let current = Configuration::default();
        let newer = paths.root.join("newer.json");
        fs::write(&newer, format!("{{\"schema_version\":{}}}", CONFIG_SCHEMA_VERSION + 1))
            .expect("newer file");
        let invalid = paths.root.join("invalid.json");
        fs::write(&invalid, b"{ private broken data").expect("invalid file");
        let invalid_model = paths.root.join("invalid-model.json");
        let mut invalid_configuration = Configuration::default();
        invalid_configuration.global_profile.menu.slices[0].label = " ".to_owned();
        fs::write(
            &invalid_model,
            serde_json::to_vec_pretty(&invalid_configuration).expect("invalid model bytes"),
        )
        .expect("invalid model");

        assert_eq!(
            store.import_draft(&current, &newer),
            Err(ConfigurationTransferError::UnsupportedSchema)
        );
        assert_eq!(store.import_draft(&current, &invalid), Err(ConfigurationTransferError::Parse));
        assert_eq!(
            store.import_draft(&current, &invalid_model),
            Err(ConfigurationTransferError::Invalid)
        );
    }

    #[test]
    fn export_writes_a_complete_atomic_snapshot_without_temporary_debris() {
        let (_temporary_directory, paths, store) = store();
        let mut draft = Configuration::default();
        draft.global_profile.menu.slices[0].label = "Unsaved exported label".to_owned();
        let destination = paths.root.join("exports").join("draft.json");
        fs::create_dir_all(destination.parent().expect("export parent")).expect("export parent");

        store.export_draft(&draft, &destination, &draft).expect("export");

        assert_eq!(read_configuration(&destination).expect("exported config").configuration, draft);
        let debris = fs::read_dir(destination.parent().expect("export parent"))
            .expect("export directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".gradial-export-"))
            .count();
        assert_eq!(debris, 0);
    }

    #[test]
    fn invalid_exports_do_not_touch_an_existing_destination() {
        let (_temporary_directory, paths, store) = store();
        let destination = paths.root.join("existing.json");
        fs::write(&destination, b"keep this").expect("existing destination");
        let mut invalid = Configuration::default();
        invalid.global_profile.menu.slice_count = 12;

        assert_eq!(
            store.export_draft(&Configuration::default(), &destination, &invalid),
            Err(ConfigurationTransferError::Invalid)
        );
        assert_eq!(fs::read(&destination).expect("existing retained"), b"keep this");
    }

    #[test]
    fn export_cannot_replace_any_gradial_owned_state_path() {
        let (_temporary_directory, paths, store) = store();
        let draft = Configuration::default();

        for protected in [
            &paths.config,
            &paths.config_backup,
            &paths.config_temporary,
            &paths.onboarding,
            &paths.onboarding_temporary,
        ] {
            assert_eq!(
                store.export_draft(&draft, protected, &draft),
                Err(ConfigurationTransferError::ProtectedDestination)
            );
        }
        let differently_cased =
            std::path::PathBuf::from(paths.config.as_os_str().to_string_lossy().to_uppercase());
        assert_eq!(
            store.export_draft(&draft, &differently_cased, &draft),
            Err(ConfigurationTransferError::ProtectedDestination)
        );
        let parent_alias = paths.icons.join("..").join("config.json");
        assert_eq!(
            store.export_draft(&draft, &parent_alias, &draft),
            Err(ConfigurationTransferError::ProtectedDestination)
        );
    }
}
