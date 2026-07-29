use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use gradial_core::{AppPaths, CONFIG_SCHEMA_VERSION, Configuration, LEGACY_CONFIG_SCHEMA_VERSION};
use windows::{
    Win32::Storage::FileSystem::{
        MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_WRITE_THROUGH, ReplaceFileW,
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

    #[error("configuration file {path} cannot migrate from schema one: {reason}")]
    InvalidMigration { path: PathBuf, reason: &'static str },
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
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|source| ConfigurationFileError::Parse { path: path.to_path_buf(), source })?;
    let schema_version = value.get("schema_version").and_then(serde_json::Value::as_u64);
    let migrating_schema_one = schema_version == Some(u64::from(LEGACY_CONFIG_SCHEMA_VERSION));
    if let Some(schema_version) = schema_version
        && schema_version != u64::from(CONFIG_SCHEMA_VERSION)
        && !migrating_schema_one
    {
        return Err(ConfigurationFileError::UnsupportedSchema {
            path: path.to_path_buf(),
            found: u32::try_from(schema_version).unwrap_or(u32::MAX),
            supported: CONFIG_SCHEMA_VERSION,
        });
    }
    if migrating_schema_one && schema_one_contains_action_fields(&value) {
        return Err(ConfigurationFileError::InvalidMigration {
            path: path.to_path_buf(),
            reason: "schema-one files cannot contain actions or action references",
        });
    }
    let mut configuration: Configuration = serde_json::from_value(value)
        .map_err(|source| ConfigurationFileError::Parse { path: path.to_path_buf(), source })?;
    let migration = migrating_schema_one.then_some(ConfigurationMigration {
        from_schema: LEGACY_CONFIG_SCHEMA_VERSION,
        to_schema: CONFIG_SCHEMA_VERSION,
    });
    if migrating_schema_one {
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
        AppPaths, CONFIG_SCHEMA_VERSION, Configuration, LEGACY_CONFIG_SCHEMA_VERSION,
    };

    use super::{
        ConfigStore, ConfigurationMigration, ConfigurationSource, SaveInterruption,
        read_configuration,
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

    #[test]
    fn first_load_atomically_initializes_schema_two() {
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
                .contains("cannot migrate from schema one")
        );
    }
}
