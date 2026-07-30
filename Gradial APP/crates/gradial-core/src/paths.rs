use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

const APP_DIRECTORY_NAME: &str = "Gradial";

/// All persistent paths owned by Gradial.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub config_backup: PathBuf,
    pub config_temporary: PathBuf,
    pub onboarding: PathBuf,
    pub onboarding_temporary: PathBuf,
    pub icons: PathBuf,
    pub logs: PathBuf,
}

impl AppPaths {
    /// Resolves paths from the current Windows user's `LOCALAPPDATA`.
    ///
    /// # Errors
    ///
    /// Returns [`AppPathsError::MissingLocalAppData`] when Windows does not
    /// provide `LOCALAPPDATA` for the current process.
    pub fn discover() -> Result<Self, AppPathsError> {
        let local_app_data =
            env::var_os("LOCALAPPDATA").ok_or(AppPathsError::MissingLocalAppData)?;
        Ok(Self::from_local_app_data(local_app_data))
    }

    /// Builds the stable directory layout under a supplied local-app-data root.
    #[must_use]
    pub fn from_local_app_data(local_app_data: impl Into<OsString>) -> Self {
        let root = PathBuf::from(local_app_data.into()).join(APP_DIRECTORY_NAME);
        Self {
            config: root.join("config.json"),
            config_backup: root.join("config.json.bak"),
            config_temporary: root.join("config.json.tmp"),
            onboarding: root.join("onboarding-v1.json"),
            onboarding_temporary: root.join("onboarding-v1.json.tmp"),
            icons: root.join("icons"),
            logs: root.join("logs"),
            root,
        }
    }

    /// Creates the directories needed by the foundation without creating a
    /// configuration file.
    ///
    /// # Errors
    ///
    /// Returns [`AppPathsError::CreateDirectory`] with the failing path when
    /// Windows cannot create one of the directories.
    pub fn ensure_directories(&self) -> Result<(), AppPathsError> {
        create_directory(&self.root)?;
        create_directory(&self.icons)?;
        create_directory(&self.logs)?;
        Ok(())
    }
}

fn create_directory(path: &Path) -> Result<(), AppPathsError> {
    fs::create_dir_all(path)
        .map_err(|source| AppPathsError::CreateDirectory { path: path.to_path_buf(), source })
}

#[derive(Debug, Error)]
pub enum AppPathsError {
    #[error("LOCALAPPDATA is not available for the current Windows user")]
    MissingLocalAppData,

    #[error("failed to create application directory {path}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::AppPaths;

    #[test]
    fn layout_matches_the_v02_plan() {
        let paths = AppPaths::from_local_app_data(r"C:\Users\Test\AppData\Local");

        assert_eq!(paths.root, std::path::PathBuf::from(r"C:\Users\Test\AppData\Local\Gradial"));
        assert_eq!(paths.config, paths.root.join("config.json"));
        assert_eq!(paths.config_backup, paths.root.join("config.json.bak"));
        assert_eq!(paths.config_temporary, paths.root.join("config.json.tmp"));
        assert_eq!(paths.onboarding, paths.root.join("onboarding-v1.json"));
        assert_eq!(paths.onboarding_temporary, paths.root.join("onboarding-v1.json.tmp"));
        assert_eq!(paths.icons, paths.root.join("icons"));
        assert_eq!(paths.logs, paths.root.join("logs"));
    }

    #[test]
    fn directory_creation_is_idempotent() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let paths = AppPaths::from_local_app_data(temp.path());

        paths.ensure_directories().expect("first creation");
        paths.ensure_directories().expect("second creation");

        assert!(paths.root.is_dir());
        assert!(paths.icons.is_dir());
        assert!(paths.logs.is_dir());
        assert!(!paths.config.exists());
    }
}
