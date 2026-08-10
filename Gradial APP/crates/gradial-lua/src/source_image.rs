use std::{
    collections::BTreeMap,
    fmt,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use thiserror::Error;

pub const MAX_PLUGIN_SOURCE_FILE_BYTES: usize = 256 * 1024;
pub const MAX_PLUGIN_SOURCE_SET_BYTES: usize = 1024 * 1024;
pub const MAX_PLUGIN_SOURCE_FILES: usize = 256;
pub const MAX_PLUGIN_ROOT_ENTRIES: usize = 1_024;
pub const MAX_PLUGIN_PACKAGE_ENTRIES: usize = 4_096;
pub const MAX_PLUGIN_DIRECTORY_DEPTH: usize = 32;
const MAX_MODULE_NAME_BYTES: usize = 192;

/// Immutable Lua text captured after package-boundary validation.
#[derive(Clone, Eq, PartialEq)]
pub struct PluginSourceImage {
    entry_path: String,
    sources: BTreeMap<String, Arc<str>>,
    total_bytes: usize,
}

impl PluginSourceImage {
    /// Constructs a source image without retaining access to its filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`PluginSourceImageError`] when a path is unsafe or duplicated,
    /// an entry is missing, source is not plain UTF-8 Lua text, or a source
    /// count/size bound is exceeded.
    pub fn from_files(
        entry: &str,
        files: Vec<(PathBuf, Vec<u8>)>,
    ) -> Result<Self, PluginSourceImageError> {
        let entry_path = normalize_manifest_path(entry)?;
        if files.len() > MAX_PLUGIN_SOURCE_FILES {
            return Err(PluginSourceImageError::TooManyFiles);
        }
        let mut sources = BTreeMap::new();
        let mut total_bytes = 0usize;
        for (path, bytes) in files {
            let path = normalize_relative_path(&path)?;
            if Path::new(&path).extension().is_none_or(|extension| extension != "lua") {
                return Err(PluginSourceImageError::NotLuaSource);
            }
            if bytes.len() > MAX_PLUGIN_SOURCE_FILE_BYTES {
                return Err(PluginSourceImageError::FileTooLarge);
            }
            if bytes.first() == Some(&0x1b) {
                return Err(PluginSourceImageError::BytecodeRejected);
            }
            let text =
                std::str::from_utf8(&bytes).map_err(|_| PluginSourceImageError::SourceNotUtf8)?;
            total_bytes = total_bytes
                .checked_add(bytes.len())
                .ok_or(PluginSourceImageError::SourceSetTooLarge)?;
            if total_bytes > MAX_PLUGIN_SOURCE_SET_BYTES {
                return Err(PluginSourceImageError::SourceSetTooLarge);
            }
            if sources.insert(path, Arc::<str>::from(text)).is_some() {
                return Err(PluginSourceImageError::DuplicatePath);
            }
        }
        if !sources.contains_key(&entry_path) {
            return Err(PluginSourceImageError::EntryNotFound);
        }
        Ok(Self { entry_path, sources, total_bytes })
    }

    #[must_use]
    pub fn entry_path(&self) -> &str {
        &self.entry_path
    }

    #[must_use]
    pub fn file_count(&self) -> usize {
        self.sources.len()
    }

    #[must_use]
    pub const fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub(crate) fn entry_source(&self) -> &str {
        self.sources.get(&self.entry_path).expect("entry existence is an image invariant")
    }

    pub(crate) fn module_source(
        &self,
        module_name: &str,
    ) -> Result<Option<(&str, &str)>, PluginSourceImageError> {
        let path = module_path(module_name)?;
        Ok(self.sources.get_key_value(&path).map(|(path, source)| (path.as_str(), source.as_ref())))
    }
}

impl fmt::Debug for PluginSourceImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginSourceImage")
            .field("file_count", &self.file_count())
            .field("total_bytes", &self.total_bytes)
            .finish_non_exhaustive()
    }
}

fn normalize_manifest_path(path: &str) -> Result<String, PluginSourceImageError> {
    if path.is_empty() || path.starts_with(['/', '\\']) || path.contains(':') {
        return Err(PluginSourceImageError::UnsafePath);
    }
    let components = path.split(['/', '\\']).collect::<Vec<_>>();
    if components.iter().any(|component| component.is_empty() || matches!(*component, "." | "..")) {
        return Err(PluginSourceImageError::UnsafePath);
    }
    let normalized = components.join("/");
    if Path::new(&normalized).extension().is_none_or(|extension| extension != "lua") {
        return Err(PluginSourceImageError::NotLuaSource);
    }
    Ok(normalized)
}

fn normalize_relative_path(path: &Path) -> Result<String, PluginSourceImageError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(PluginSourceImageError::UnsafePath);
    }
    let mut components = Vec::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            return Err(PluginSourceImageError::UnsafePath);
        };
        let component = component.to_str().ok_or(PluginSourceImageError::UnsafePath)?;
        if component.is_empty() || component.contains(':') {
            return Err(PluginSourceImageError::UnsafePath);
        }
        components.push(component);
    }
    if components.is_empty() {
        return Err(PluginSourceImageError::UnsafePath);
    }
    Ok(components.join("/"))
}

fn module_path(module_name: &str) -> Result<String, PluginSourceImageError> {
    if module_name.is_empty()
        || module_name.len() > MAX_MODULE_NAME_BYTES
        || module_name.split('.').any(|component| {
            component.is_empty()
                || !component.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-')
                })
        })
    {
        return Err(PluginSourceImageError::InvalidModuleName);
    }
    Ok(format!("{}.lua", module_name.replace('.', "/")))
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PluginSourceImageError {
    #[error("plugin_source_path_unsafe")]
    UnsafePath,
    #[error("plugin_source_not_lua")]
    NotLuaSource,
    #[error("plugin_source_path_duplicate")]
    DuplicatePath,
    #[error("plugin_entry_not_found")]
    EntryNotFound,
    #[error("plugin_source_files_too_many")]
    TooManyFiles,
    #[error("plugin_source_file_too_large")]
    FileTooLarge,
    #[error("plugin_source_set_too_large")]
    SourceSetTooLarge,
    #[error("plugin_source_not_utf8")]
    SourceNotUtf8,
    #[error("plugin_lua_bytecode_rejected")]
    BytecodeRejected,
    #[error("plugin_module_name_invalid")]
    InvalidModuleName,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_image_is_owned_normalized_and_redacted_in_debug() {
        let mut original = b"return { value = 42 }".to_vec();
        let image = PluginSourceImage::from_files(
            r"lib\entry.lua",
            vec![(PathBuf::from("lib").join("entry.lua"), original.clone())],
        )
        .expect("source image");
        original.fill(b'x');

        assert_eq!(image.entry_path(), "lib/entry.lua");
        assert_eq!(image.entry_source(), "return { value = 42 }");
        assert!(!format!("{image:?}").contains("value = 42"));
    }

    #[test]
    fn unsafe_duplicate_missing_and_bytecode_inputs_fail_closed() {
        assert_eq!(
            PluginSourceImage::from_files("../main.lua", vec![]),
            Err(PluginSourceImageError::UnsafePath)
        );
        assert_eq!(
            PluginSourceImage::from_files(
                "main.lua",
                vec![
                    (PathBuf::from("main.lua"), b"return {}".to_vec()),
                    (PathBuf::from("main.lua"), b"return {}".to_vec()),
                ],
            ),
            Err(PluginSourceImageError::DuplicatePath)
        );
        assert_eq!(
            PluginSourceImage::from_files(
                "main.lua",
                vec![(PathBuf::from("other.lua"), b"return {}".to_vec())],
            ),
            Err(PluginSourceImageError::EntryNotFound)
        );
        assert_eq!(
            PluginSourceImage::from_files(
                "main.lua",
                vec![(PathBuf::from("main.lua"), b"\x1bLua".to_vec())],
            ),
            Err(PluginSourceImageError::BytecodeRejected)
        );
    }

    #[test]
    fn module_names_map_only_to_rooted_lua_paths() {
        let image = PluginSourceImage::from_files(
            "main.lua",
            vec![
                (PathBuf::from("main.lua"), b"return {}".to_vec()),
                (PathBuf::from("lib").join("mapping.lua"), b"return 42".to_vec()),
            ],
        )
        .expect("source image");

        assert_eq!(
            image.module_source("lib.mapping").expect("module").map(|item| item.0),
            Some("lib/mapping.lua")
        );
        for invalid in ["../secret", ".secret", "secret.", "lib//secret", "UPPER"] {
            assert_eq!(
                image.module_source(invalid),
                Err(PluginSourceImageError::InvalidModuleName)
            );
        }
    }
}
