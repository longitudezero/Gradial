use std::{
    fs::{self, File, Metadata},
    io::{self, Read},
    path::{Path, PathBuf},
};

use gradial_core::{
    MAX_PLUGIN_MANIFEST_BYTES, PLUGIN_API_VERSION, PluginManifest, PluginManifestError,
    parse_plugin_manifest,
};
use gradial_lua::{
    MAX_PLUGIN_DIRECTORY_DEPTH, MAX_PLUGIN_PACKAGE_ENTRIES, MAX_PLUGIN_ROOT_ENTRIES,
    MAX_PLUGIN_SOURCE_FILE_BYTES, MAX_PLUGIN_SOURCE_FILES, MAX_PLUGIN_SOURCE_SET_BYTES,
    PluginSourceImage, PluginSourceImageError,
};
use thiserror::Error;

const MANIFEST_FILE_NAME: &str = "plugin.toml";
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PluginPackageStatus {
    Compatible { manifest: PluginManifest, sources: PluginSourceImage },
    IncompatibleApi { manifest: PluginManifest, sources: PluginSourceImage, supported: u16 },
    Invalid(PluginDiscoveryError),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DiscoveredPlugin {
    pub(crate) directory_name: String,
    pub(crate) package_root: PathBuf,
    pub(crate) status: PluginPackageStatus,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PluginDiscoveryReport {
    pub(crate) packages: Vec<DiscoveredPlugin>,
    pub(crate) root_error: Option<PluginDiscoveryError>,
}

impl PluginDiscoveryReport {
    fn counts(&self) -> (usize, usize, usize) {
        self.packages.iter().fold((0, 0, 0), |mut counts, package| {
            match package.status {
                PluginPackageStatus::Compatible { .. } => counts.0 += 1,
                PluginPackageStatus::IncompatibleApi { .. } => counts.1 += 1,
                PluginPackageStatus::Invalid(_) => counts.2 += 1,
            }
            counts
        })
    }
}

pub(crate) fn discover_plugins(root: &Path) -> PluginDiscoveryReport {
    let Ok(root_metadata) = fs::symlink_metadata(root) else {
        return root_failure(PluginDiscoveryError::Io);
    };
    if !root_metadata.is_dir() {
        return root_failure(PluginDiscoveryError::RootNotDirectory);
    }
    if is_reparse_point(&root_metadata) {
        return root_failure(PluginDiscoveryError::ReparsePoint);
    }
    let Ok(canonical_root) = fs::canonicalize(root) else {
        return root_failure(PluginDiscoveryError::Io);
    };
    let entries = match sorted_entries(
        root,
        MAX_PLUGIN_ROOT_ENTRIES,
        PluginDiscoveryError::TooManyRootEntries,
    ) {
        Ok(entries) => entries,
        Err(error) => return root_failure(error),
    };

    let mut packages = Vec::new();
    for entry in entries {
        let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
            packages.push(invalid_entry(&entry.path(), PluginDiscoveryError::Io));
            continue;
        };
        if is_reparse_point(&metadata) {
            packages.push(invalid_entry(&entry.path(), PluginDiscoveryError::ReparsePoint));
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }
        let directory_name_os = entry.file_name();
        let Some(directory_name) = directory_name_os.to_str().map(str::to_owned) else {
            packages.push(invalid_entry(&entry.path(), PluginDiscoveryError::UnsafePath));
            continue;
        };
        let package_root = entry.path();
        let status = inspect_package(&canonical_root, &package_root, &directory_name, &metadata);
        packages.push(DiscoveredPlugin { directory_name, package_root, status });
    }
    PluginDiscoveryReport { packages, root_error: None }
}

pub(crate) fn discover_and_log(root: &Path) {
    let report = discover_plugins(root);
    log_discovery_report(&report);
}

pub(crate) fn log_discovery_report(report: &PluginDiscoveryReport) {
    let (compatible, incompatible, invalid) = report.counts();
    if let Some(error) = report.root_error {
        log::error!(
            target: "plugins",
            "component=plugins event_code=plugin_discovery result_code=failure error_code={error}"
        );
        return;
    }
    log::info!(
        target: "plugins",
        "component=plugins event_code=plugin_discovery result_code=ok compatible={} \
         incompatible={} invalid={} total={}",
        compatible,
        incompatible,
        invalid,
        report.packages.len()
    );
    for package in &report.packages {
        match &package.status {
            PluginPackageStatus::Compatible { manifest, sources } => log::info!(
                target: "plugins",
                "component=plugins event_code=plugin_discovered result_code=compatible \
                 plugin_id={} plugin_version={} source_files={} source_bytes={}",
                manifest.id,
                manifest.version,
                sources.file_count(),
                sources.total_bytes()
            ),
            PluginPackageStatus::IncompatibleApi { manifest, sources, supported } => log::warn!(
                target: "plugins",
                "component=plugins event_code=plugin_discovered result_code=incompatible_api \
                 plugin_id={} plugin_api_version={} supported_api_version={} source_files={} \
                 source_bytes={}",
                manifest.id,
                manifest.api_version,
                supported,
                sources.file_count(),
                sources.total_bytes()
            ),
            PluginPackageStatus::Invalid(error) => log::warn!(
                target: "plugins",
                "component=plugins event_code=plugin_discovered result_code=invalid \
                 package_name={} error_code={error}",
                log_token(&package.directory_name)
            ),
        }
    }
}

fn inspect_package(
    canonical_root: &Path,
    package_root: &Path,
    directory_name: &str,
    metadata: &Metadata,
) -> PluginPackageStatus {
    match inspect_package_inner(canonical_root, package_root, directory_name, metadata) {
        Ok((manifest, sources)) if manifest.api_version == PLUGIN_API_VERSION => {
            PluginPackageStatus::Compatible { manifest, sources }
        }
        Ok((manifest, sources)) => PluginPackageStatus::IncompatibleApi {
            manifest,
            sources,
            supported: PLUGIN_API_VERSION,
        },
        Err(error) => PluginPackageStatus::Invalid(error),
    }
}

fn inspect_package_inner(
    canonical_root: &Path,
    package_root: &Path,
    directory_name: &str,
    metadata: &Metadata,
) -> Result<(PluginManifest, PluginSourceImage), PluginDiscoveryError> {
    if directory_name.contains(':') {
        return Err(PluginDiscoveryError::UnsafePath);
    }
    if is_reparse_point(metadata) {
        return Err(PluginDiscoveryError::ReparsePoint);
    }
    let canonical_package = fs::canonicalize(package_root).map_err(|_| PluginDiscoveryError::Io)?;
    if canonical_package.parent() != Some(canonical_root) {
        return Err(PluginDiscoveryError::OutsidePluginRoot);
    }

    let manifest_path = package_root.join(MANIFEST_FILE_NAME);
    validate_regular_package_file(&canonical_package, &manifest_path)?;
    let manifest_bytes =
        read_bounded(&manifest_path, MAX_PLUGIN_MANIFEST_BYTES).map_err(|error| {
            if error.kind() == io::ErrorKind::FileTooLarge {
                PluginDiscoveryError::Manifest(PluginManifestError::TooLarge)
            } else {
                PluginDiscoveryError::Io
            }
        })?;
    let manifest =
        parse_plugin_manifest(&manifest_bytes).map_err(PluginDiscoveryError::Manifest)?;
    if manifest.id != directory_name {
        return Err(PluginDiscoveryError::DirectoryIdMismatch);
    }

    let sources = inventory_sources(&canonical_package, package_root, &manifest.entry)?;
    Ok((manifest, sources))
}

fn inventory_sources(
    canonical_package: &Path,
    package_root: &Path,
    entry_path: &str,
) -> Result<PluginSourceImage, PluginDiscoveryError> {
    let mut pending = vec![(package_root.to_path_buf(), 0usize)];
    let mut files = Vec::new();
    let mut total_bytes = 0usize;
    let mut entries_seen = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        let remaining_entries = MAX_PLUGIN_PACKAGE_ENTRIES.saturating_sub(entries_seen);
        let entries = sorted_entries(
            &directory,
            remaining_entries,
            PluginDiscoveryError::TooManyPackageEntries,
        )?;
        entries_seen += entries.len();
        for entry in entries {
            let path = entry.path();
            let relative = path
                .strip_prefix(package_root)
                .map_err(|_| PluginDiscoveryError::OutsidePackage)?
                .to_path_buf();
            validate_relative_components(&relative)?;
            let metadata = fs::symlink_metadata(&path).map_err(|_| PluginDiscoveryError::Io)?;
            if is_reparse_point(&metadata) {
                return Err(PluginDiscoveryError::ReparsePoint);
            }
            let canonical = fs::canonicalize(&path).map_err(|_| PluginDiscoveryError::Io)?;
            if !canonical.starts_with(canonical_package) {
                return Err(PluginDiscoveryError::OutsidePackage);
            }
            if metadata.is_dir() {
                if depth >= MAX_PLUGIN_DIRECTORY_DEPTH {
                    return Err(PluginDiscoveryError::DirectoryTooDeep);
                }
                pending.push((path, depth + 1));
                continue;
            }
            if !metadata.is_file() {
                return Err(PluginDiscoveryError::UnsupportedFileType);
            }
            if relative == Path::new(MANIFEST_FILE_NAME) {
                continue;
            }
            let extension = path.extension().and_then(|value| value.to_str()).unwrap_or("");
            if is_native_or_bytecode_extension(extension) {
                return Err(PluginDiscoveryError::NativeOrBytecodeModule);
            }
            if extension != "lua" {
                continue;
            }
            if files.len() >= MAX_PLUGIN_SOURCE_FILES {
                return Err(PluginDiscoveryError::TooManySourceFiles);
            }
            let declared_length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
            if declared_length > MAX_PLUGIN_SOURCE_FILE_BYTES {
                return Err(PluginDiscoveryError::SourceFileTooLarge);
            }
            let bytes = read_bounded(&path, MAX_PLUGIN_SOURCE_FILE_BYTES).map_err(|error| {
                if error.kind() == io::ErrorKind::FileTooLarge {
                    PluginDiscoveryError::SourceFileTooLarge
                } else {
                    PluginDiscoveryError::Io
                }
            })?;
            total_bytes = total_bytes
                .checked_add(bytes.len())
                .ok_or(PluginDiscoveryError::SourceSetTooLarge)?;
            if total_bytes > MAX_PLUGIN_SOURCE_SET_BYTES {
                return Err(PluginDiscoveryError::SourceSetTooLarge);
            }
            files.push((relative, bytes));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    PluginSourceImage::from_files(entry_path, files).map_err(map_source_image_error)
}

fn validate_regular_package_file(
    canonical_package: &Path,
    path: &Path,
) -> Result<(), PluginDiscoveryError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| PluginDiscoveryError::MissingManifest)?;
    if is_reparse_point(&metadata) {
        return Err(PluginDiscoveryError::ReparsePoint);
    }
    if !metadata.is_file() {
        return Err(PluginDiscoveryError::UnsupportedFileType);
    }
    let canonical = fs::canonicalize(path).map_err(|_| PluginDiscoveryError::Io)?;
    if !canonical.starts_with(canonical_package) {
        return Err(PluginDiscoveryError::OutsidePackage);
    }
    Ok(())
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let limit_with_marker = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::with_capacity(limit.min(16 * 1024));
    file.take(limit_with_marker).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "bounded plugin file exceeded its limit",
        ));
    }
    Ok(bytes)
}

fn sorted_entries(
    directory: &Path,
    maximum: usize,
    overflow_error: PluginDiscoveryError,
) -> Result<Vec<fs::DirEntry>, PluginDiscoveryError> {
    let read_directory = fs::read_dir(directory).map_err(|_| PluginDiscoveryError::Io)?;
    let mut entries = Vec::with_capacity(maximum.min(64));
    for entry in read_directory {
        if entries.len() >= maximum {
            return Err(overflow_error);
        }
        entries.push(entry.map_err(|_| PluginDiscoveryError::Io)?);
    }
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn validate_relative_components(relative: &Path) -> Result<(), PluginDiscoveryError> {
    if relative.components().any(|component| {
        component.as_os_str().to_str().is_none_or(|value| value.contains(':') || value.is_empty())
    }) {
        return Err(PluginDiscoveryError::UnsafePath);
    }
    Ok(())
}

const fn map_source_image_error(error: PluginSourceImageError) -> PluginDiscoveryError {
    match error {
        PluginSourceImageError::UnsafePath
        | PluginSourceImageError::NotLuaSource
        | PluginSourceImageError::DuplicatePath
        | PluginSourceImageError::InvalidModuleName => PluginDiscoveryError::UnsafePath,
        PluginSourceImageError::EntryNotFound => PluginDiscoveryError::EntryNotSource,
        PluginSourceImageError::TooManyFiles => PluginDiscoveryError::TooManySourceFiles,
        PluginSourceImageError::FileTooLarge => PluginDiscoveryError::SourceFileTooLarge,
        PluginSourceImageError::SourceSetTooLarge => PluginDiscoveryError::SourceSetTooLarge,
        PluginSourceImageError::SourceNotUtf8 => PluginDiscoveryError::SourceNotUtf8,
        PluginSourceImageError::BytecodeRejected => PluginDiscoveryError::NativeOrBytecodeModule,
    }
}

fn is_native_or_bytecode_extension(extension: &str) -> bool {
    matches!(extension.to_ascii_lowercase().as_str(), "dll" | "exe" | "luac" | "so" | "dylib")
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(metadata: &Metadata) -> bool {
    metadata.is_symlink()
}

fn invalid_entry(path: &Path, error: PluginDiscoveryError) -> DiscoveredPlugin {
    DiscoveredPlugin {
        directory_name: path
            .file_name()
            .map_or_else(|| "<unknown>".to_owned(), |name| name.to_string_lossy().into_owned()),
        package_root: path.to_path_buf(),
        status: PluginPackageStatus::Invalid(error),
    }
}

fn root_failure(error: PluginDiscoveryError) -> PluginDiscoveryReport {
    PluginDiscoveryReport { packages: Vec::new(), root_error: Some(error) }
}

fn log_token(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(96)
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum PluginDiscoveryError {
    #[error("plugin_io_error")]
    Io,
    #[error("plugin_root_not_directory")]
    RootNotDirectory,
    #[error("plugin_path_unsafe")]
    UnsafePath,
    #[error("plugin_reparse_point_rejected")]
    ReparsePoint,
    #[error("plugin_outside_root")]
    OutsidePluginRoot,
    #[error("plugin_outside_package")]
    OutsidePackage,
    #[error("plugin_manifest_missing")]
    MissingManifest,
    #[error("plugin_{0}")]
    Manifest(PluginManifestError),
    #[error("plugin_directory_id_mismatch")]
    DirectoryIdMismatch,
    #[error("plugin_entry_not_source")]
    EntryNotSource,
    #[error("plugin_file_type_unsupported")]
    UnsupportedFileType,
    #[error("plugin_native_or_bytecode_rejected")]
    NativeOrBytecodeModule,
    #[error("plugin_source_files_too_many")]
    TooManySourceFiles,
    #[error("plugin_root_entries_too_many")]
    TooManyRootEntries,
    #[error("plugin_package_entries_too_many")]
    TooManyPackageEntries,
    #[error("plugin_directory_too_deep")]
    DirectoryTooDeep,
    #[error("plugin_source_file_too_large")]
    SourceFileTooLarge,
    #[error("plugin_source_set_too_large")]
    SourceSetTooLarge,
    #[error("plugin_source_not_utf8")]
    SourceNotUtf8,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gradial_lua::{LoadedLuaPlugin, LuaSandboxLimits};

    use super::*;

    fn manifest(id: &str, api_version: u16) -> String {
        format!(
            r#"manifest_version = 1
id = "{id}"
name = "Fixture"
version = "0.1.0"
api_version = {api_version}
entry = "main.lua"
permissions = ["context.application"]

[[actions]]
id = "adjust"
name = "Adjust"
kind = "continuous"
update_hz = 30
"#
        )
    }

    fn package(root: &Path, id: &str, api_version: u16) -> PathBuf {
        let package = root.join(id);
        fs::create_dir_all(&package).expect("package directory");
        fs::write(package.join(MANIFEST_FILE_NAME), manifest(id, api_version)).expect("manifest");
        fs::write(package.join("main.lua"), "return {}\n").expect("source");
        package
    }

    #[test]
    fn one_invalid_package_does_not_hide_healthy_packages() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("plugins");
        fs::create_dir(&root).expect("plugin root");
        package(&root, "zebra.good", PLUGIN_API_VERSION);
        let invalid = package(&root, "alpha.bad", PLUGIN_API_VERSION);
        fs::write(invalid.join(MANIFEST_FILE_NAME), "not = [valid").expect("invalid manifest");

        let report = discover_plugins(&root);
        assert_eq!(report.packages.len(), 2);
        assert_eq!(report.packages[0].directory_name, "alpha.bad");
        assert!(matches!(
            report.packages[0].status,
            PluginPackageStatus::Invalid(PluginDiscoveryError::Manifest(PluginManifestError::Toml))
        ));
        assert!(matches!(report.packages[1].status, PluginPackageStatus::Compatible { .. }));
    }

    #[test]
    fn future_api_is_valid_but_incompatible() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("plugins");
        fs::create_dir(&root).expect("plugin root");
        package(&root, "future.plugin", PLUGIN_API_VERSION + 1);

        let report = discover_plugins(&root);
        assert!(matches!(
            report.packages[0].status,
            PluginPackageStatus::IncompatibleApi { supported: PLUGIN_API_VERSION, .. }
        ));
    }

    #[test]
    fn directory_name_must_exactly_match_manifest_id() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("plugins");
        fs::create_dir(&root).expect("plugin root");
        let package = package(&root, "folder.id", PLUGIN_API_VERSION);
        fs::write(package.join(MANIFEST_FILE_NAME), manifest("different.id", PLUGIN_API_VERSION))
            .expect("changed manifest");

        let report = discover_plugins(&root);
        assert!(matches!(
            report.packages[0].status,
            PluginPackageStatus::Invalid(PluginDiscoveryError::DirectoryIdMismatch)
        ));
    }

    #[test]
    fn entry_must_be_an_inventoried_utf8_lua_file() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("plugins");
        fs::create_dir(&root).expect("plugin root");
        let package = package(&root, "binary.source", PLUGIN_API_VERSION);
        fs::write(package.join("main.lua"), [0xff, 0xfe]).expect("non UTF-8 source");

        let report = discover_plugins(&root);
        assert!(matches!(
            report.packages[0].status,
            PluginPackageStatus::Invalid(PluginDiscoveryError::SourceNotUtf8)
        ));
    }

    #[test]
    fn discovery_freezes_source_and_rejects_bytecode_markers() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("plugins");
        fs::create_dir(&root).expect("plugin root");
        let frozen_package = package(&root, "frozen.source", PLUGIN_API_VERSION);
        let bytecode_package = package(&root, "bytecode.source", PLUGIN_API_VERSION);
        fs::write(bytecode_package.join("main.lua"), b"\x1bLua").expect("bytecode marker");

        let report = discover_plugins(&root);
        let frozen_image = report
            .packages
            .iter()
            .find_map(|package| match &package.status {
                PluginPackageStatus::Compatible { manifest, sources }
                    if manifest.id == "frozen.source" =>
                {
                    Some(sources.clone())
                }
                _ => None,
            })
            .expect("frozen compatible source");
        assert!(report.packages.iter().any(|package| matches!(
            package.status,
            PluginPackageStatus::Invalid(PluginDiscoveryError::NativeOrBytecodeModule)
        )));

        fs::write(frozen_package.join("main.lua"), "this is no longer valid Lua")
            .expect("replace package source after discovery");
        LoadedLuaPlugin::load(frozen_image, LuaSandboxLimits::default())
            .expect("frozen source remains independently loadable");
    }

    #[test]
    fn native_modules_make_only_the_containing_package_invalid() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("plugins");
        fs::create_dir(&root).expect("plugin root");
        let native_package = package(&root, "native.bad", PLUGIN_API_VERSION);
        fs::write(native_package.join("extension.dll"), []).expect("native marker");
        package(&root, "plain.good", PLUGIN_API_VERSION);

        let report = discover_plugins(&root);
        assert!(report.packages.iter().any(|package| matches!(
            package.status,
            PluginPackageStatus::Invalid(PluginDiscoveryError::NativeOrBytecodeModule)
        )));
        assert!(
            report
                .packages
                .iter()
                .any(|package| matches!(package.status, PluginPackageStatus::Compatible { .. }))
        );
    }

    #[test]
    fn source_file_and_source_set_limits_are_enforced() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("plugins");
        fs::create_dir(&root).expect("plugin root");
        let large_file_package = package(&root, "large.file", PLUGIN_API_VERSION);
        fs::write(
            large_file_package.join("main.lua"),
            vec![b' '; MAX_PLUGIN_SOURCE_FILE_BYTES + 1],
        )
        .expect("large source");

        let large_set_package = package(&root, "large.set", PLUGIN_API_VERSION);
        for index in 0..4 {
            fs::write(
                large_set_package.join(format!("part-{index}.lua")),
                vec![b' '; MAX_PLUGIN_SOURCE_FILE_BYTES],
            )
            .expect("source set member");
        }

        let report = discover_plugins(&root);
        assert!(matches!(
            report.packages[0].status,
            PluginPackageStatus::Invalid(PluginDiscoveryError::SourceFileTooLarge)
        ));
        assert!(matches!(
            report.packages[1].status,
            PluginPackageStatus::Invalid(PluginDiscoveryError::SourceSetTooLarge)
        ));
    }

    #[test]
    fn source_count_and_directory_depth_are_enforced() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("plugins");
        fs::create_dir(&root).expect("plugin root");
        let many_sources = package(&root, "many.sources", PLUGIN_API_VERSION);
        for index in 0..MAX_PLUGIN_SOURCE_FILES {
            fs::write(many_sources.join(format!("empty-{index:03}.lua")), []).expect("source");
        }

        let deep_package = package(&root, "deep.package", PLUGIN_API_VERSION);
        let mut directory = deep_package;
        for index in 0..=MAX_PLUGIN_DIRECTORY_DEPTH {
            directory = directory.join(format!("level-{index:02}"));
            fs::create_dir(&directory).expect("nested directory");
        }

        let report = discover_plugins(&root);
        assert!(report.packages.iter().any(|package| matches!(
            package.status,
            PluginPackageStatus::Invalid(PluginDiscoveryError::TooManySourceFiles)
        )));
        assert!(report.packages.iter().any(|package| matches!(
            package.status,
            PluginPackageStatus::Invalid(PluginDiscoveryError::DirectoryTooDeep)
        )));
    }

    #[test]
    fn plugin_root_entry_count_is_bounded() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("plugins");
        fs::create_dir(&root).expect("plugin root");
        for index in 0..=MAX_PLUGIN_ROOT_ENTRIES {
            fs::write(root.join(format!("inert-{index:04}.txt")), []).expect("root entry");
        }

        let report = discover_plugins(&root);
        assert_eq!(report.root_error, Some(PluginDiscoveryError::TooManyRootEntries));
        assert!(report.packages.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn top_level_directory_symlink_is_rejected_when_supported() {
        use std::os::windows::fs::symlink_dir;

        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path().join("plugins");
        fs::create_dir(&root).expect("plugin root");
        let target = temporary.path().join("target");
        fs::create_dir(&target).expect("target directory");
        let link = root.join("linked.plugin");
        if symlink_dir(&target, &link).is_err() {
            return;
        }

        let report = discover_plugins(&root);
        assert!(matches!(
            report.packages[0].status,
            PluginPackageStatus::Invalid(PluginDiscoveryError::ReparsePoint)
        ));
    }

    #[test]
    fn absent_or_replaced_root_is_reported_without_panicking() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let absent = discover_plugins(&temporary.path().join("absent"));
        assert_eq!(absent.root_error, Some(PluginDiscoveryError::Io));

        let file = temporary.path().join("file");
        fs::write(&file, []).expect("root file");
        let replaced = discover_plugins(&file);
        assert_eq!(replaced.root_error, Some(PluginDiscoveryError::RootNotDirectory));
    }
}
