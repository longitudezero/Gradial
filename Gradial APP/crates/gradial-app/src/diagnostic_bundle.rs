use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    mem::size_of,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use flexi_logger::LoggerHandle;
use gradial_core::{ActionKind, AppPaths, Configuration, MonitorGeometry};
use serde::Serialize;
use uuid::Uuid;
use windows::{
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    Win32::System::SystemInformation::OSVERSIONINFOW,
    core::PCWSTR,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const BUNDLE_FORMAT_VERSION: u32 = 1;
const MAX_LOG_FILES: usize = 5;
const MAX_LOG_BYTES: u64 = 1_100_000;
const MANIFEST_ENTRY: &str = "manifest.json";
const PROCESS_PATHS_ENTRY: &str = "configured-process-paths.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticInputStatus {
    Responsive,
    Recovering,
    Faulted,
}

impl DiagnosticInputStatus {
    pub(crate) const fn user_text(self) -> &'static str {
        match self {
            Self::Responsive => "Hook thread responsive",
            Self::Recovering => "Hook thread recovering",
            Self::Faulted => "Hook thread faulted",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiagnosticLogPreview {
    pub(crate) name: String,
    pub(crate) size_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct DiagnosticDisplay {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
    pub(crate) work_left: i32,
    pub(crate) work_top: i32,
    pub(crate) work_right: i32,
    pub(crate) work_bottom: i32,
    pub(crate) dpi: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct DiagnosticWindowsVersion {
    pub(crate) major: u32,
    pub(crate) minor: u32,
    pub(crate) build: u32,
}

impl From<MonitorGeometry> for DiagnosticDisplay {
    fn from(monitor: MonitorGeometry) -> Self {
        Self {
            left: monitor.bounds.left,
            top: monitor.bounds.top,
            right: monitor.bounds.right,
            bottom: monitor.bounds.bottom,
            work_left: monitor.work_area.left,
            work_top: monitor.work_area.top,
            work_right: monitor.work_area.right,
            work_bottom: monitor.work_area.bottom,
            dpi: monitor.dpi,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct DiagnosticSnapshot {
    created_unix_seconds: u64,
    schema_version: u32,
    config_revision: u64,
    input_status: DiagnosticInputStatus,
    windows_version: Option<DiagnosticWindowsVersion>,
    displays: Vec<DiagnosticDisplay>,
    logs: Vec<SnapshotLog>,
    process_paths: Vec<PathBuf>,
}

impl std::fmt::Debug for DiagnosticSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DiagnosticSnapshot")
            .field("created_unix_seconds", &self.created_unix_seconds)
            .field("schema_version", &self.schema_version)
            .field("config_revision", &self.config_revision)
            .field("input_status", &self.input_status)
            .field("windows_version_available", &self.windows_version.is_some())
            .field("display_count", &self.displays.len())
            .field("log_count", &self.logs.len())
            .field("process_path_count", &self.process_paths.len())
            .finish()
    }
}

impl DiagnosticSnapshot {
    #[cfg(test)]
    pub(crate) fn test_fixture(process_path: PathBuf) -> Self {
        Self {
            created_unix_seconds: 0,
            schema_version: gradial_core::CONFIG_SCHEMA_VERSION,
            config_revision: 1,
            input_status: DiagnosticInputStatus::Responsive,
            windows_version: None,
            displays: Vec::new(),
            logs: Vec::new(),
            process_paths: vec![process_path],
        }
    }

    pub(crate) const fn config_revision(&self) -> u64 {
        self.config_revision
    }

    pub(crate) const fn input_status(&self) -> DiagnosticInputStatus {
        self.input_status
    }

    pub(crate) const fn windows_version(&self) -> Option<DiagnosticWindowsVersion> {
        self.windows_version
    }

    pub(crate) fn displays(&self) -> &[DiagnosticDisplay] {
        &self.displays
    }

    pub(crate) fn logs(&self) -> Vec<DiagnosticLogPreview> {
        self.logs
            .iter()
            .map(|log| DiagnosticLogPreview { name: log.name.clone(), size_bytes: log.bytes.len() })
            .collect()
    }

    pub(crate) fn process_paths(&self) -> &[PathBuf] {
        &self.process_paths
    }

    pub(crate) fn archive_entry_count(&self, include_process_paths: bool) -> usize {
        1_usize.saturating_add(self.logs.len()).saturating_add(usize::from(include_process_paths))
    }

    pub(crate) fn estimated_size_bytes(&self, include_process_paths: bool) -> usize {
        let logs = self.logs.iter().map(|log| log.bytes.len()).sum::<usize>();
        let paths = if include_process_paths {
            serialize_process_paths(&self.process_paths).map_or(0, |bytes| bytes.len())
        } else {
            0
        };
        logs.saturating_add(paths).saturating_add(2_048)
    }
}

#[derive(Clone, Eq, PartialEq)]
struct SnapshotLog {
    name: String,
    bytes: Vec<u8>,
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct DiagnosticBundleRequest {
    pub(crate) snapshot: Arc<DiagnosticSnapshot>,
    pub(crate) include_process_paths: bool,
}

impl std::fmt::Debug for DiagnosticBundleRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DiagnosticBundleRequest")
            .field("snapshot", &self.snapshot)
            .field("include_process_paths", &self.include_process_paths)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DiagnosticBundleSummary {
    pub(crate) logs: usize,
    pub(crate) process_paths: usize,
    pub(crate) archive_entries: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum DiagnosticBundleError {
    #[error("the diagnostic preview could not be prepared")]
    Preview,
    #[error("the selected diagnostic destination is invalid")]
    InvalidPath,
    #[error("the selected destination is owned by Gradial")]
    ProtectedDestination,
    #[error("the diagnostic bundle could not be written")]
    Write,
}

impl DiagnosticBundleError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Preview => "preview_failed",
            Self::InvalidPath => "invalid_path",
            Self::ProtectedDestination => "protected_destination",
            Self::Write => "write_failed",
        }
    }
}

#[derive(Clone)]
pub(crate) struct DiagnosticBundleService {
    paths: AppPaths,
    logger: LoggerHandle,
}

impl std::fmt::Debug for DiagnosticBundleService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DiagnosticBundleService(<private paths>)")
    }
}

impl DiagnosticBundleService {
    pub(crate) const fn new(paths: AppPaths, logger: LoggerHandle) -> Self {
        Self { paths, logger }
    }

    pub(crate) fn capture(
        &self,
        configuration: &Configuration,
        monitors: &[MonitorGeometry],
        input_status: DiagnosticInputStatus,
    ) -> Result<DiagnosticSnapshot, DiagnosticBundleError> {
        self.logger.flush();
        capture_snapshot(&self.paths.logs, configuration, monitors, input_status)
    }

    pub(crate) fn export(
        &self,
        destination: &Path,
        request: &DiagnosticBundleRequest,
    ) -> Result<DiagnosticBundleSummary, DiagnosticBundleError> {
        export_snapshot(&self.paths, destination, request)
    }
}

#[derive(Serialize)]
struct BundleManifest<'a> {
    bundle_format_version: u32,
    created_unix_seconds: u64,
    gradial: GradialManifest,
    system: SystemManifest,
    input: InputManifest,
    displays: &'a [DiagnosticDisplay],
    logs: Vec<ManifestLog>,
    privacy: PrivacyManifest,
}

#[derive(Serialize)]
struct GradialManifest {
    version: &'static str,
    schema_version: u32,
    config_revision: u64,
}

#[derive(Serialize)]
struct SystemManifest {
    operating_system: &'static str,
    architecture: &'static str,
    windows_version: Option<DiagnosticWindowsVersion>,
}

#[derive(Serialize)]
struct InputManifest {
    hook_thread_status: DiagnosticInputStatus,
    status_description: &'static str,
}

#[derive(Serialize)]
struct ManifestLog {
    archive_path: String,
    size_bytes: usize,
}

#[derive(Serialize)]
struct PrivacyManifest {
    logs_sanitized: bool,
    configured_process_paths_included: bool,
    configured_process_path_count: usize,
    always_excluded: [&'static str; 9],
}

#[derive(Serialize)]
struct ProcessPathDocument {
    notice: &'static str,
    paths: Vec<String>,
}

fn capture_snapshot(
    log_directory: &Path,
    configuration: &Configuration,
    monitors: &[MonitorGeometry],
    input_status: DiagnosticInputStatus,
) -> Result<DiagnosticSnapshot, DiagnosticBundleError> {
    let process_paths = configured_process_paths(configuration);
    let redactions = redaction_values(&process_paths);
    let mut logs = Vec::new();
    let entries = fs::read_dir(log_directory).map_err(|_| DiagnosticBundleError::Preview)?;
    for entry in entries {
        let entry = entry.map_err(|_| DiagnosticBundleError::Preview)?;
        let file_type = entry.file_type().map_err(|_| DiagnosticBundleError::Preview)?;
        if !file_type.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str().filter(|name| is_owned_log_name(name)) else {
            continue;
        };
        let metadata = entry.metadata().map_err(|_| DiagnosticBundleError::Preview)?;
        if metadata.len() > MAX_LOG_BYTES {
            return Err(DiagnosticBundleError::Preview);
        }
        let bytes = fs::read(entry.path()).map_err(|_| DiagnosticBundleError::Preview)?;
        let text = String::from_utf8(bytes).map_err(|_| DiagnosticBundleError::Preview)?;
        logs.push(SnapshotLog {
            name: name.to_owned(),
            bytes: sanitize_log(&text, &redactions).into_bytes(),
        });
    }
    logs.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    if logs.len() > MAX_LOG_FILES {
        return Err(DiagnosticBundleError::Preview);
    }

    Ok(DiagnosticSnapshot {
        created_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        schema_version: configuration.schema_version,
        config_revision: configuration.config_revision,
        input_status,
        windows_version: windows_version(),
        displays: monitors.iter().copied().map(DiagnosticDisplay::from).collect(),
        logs,
        process_paths,
    })
}

fn configured_process_paths(configuration: &Configuration) -> Vec<PathBuf> {
    let mut paths = configuration
        .application_profiles
        .iter()
        .map(|profile| profile.rule.executable_path.clone())
        .chain(configuration.actions.iter().filter_map(|action| match &action.kind {
            ActionKind::RunExecutable { executable, .. } => Some(executable.clone()),
            _ => None,
        }))
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();
    paths.sort_unstable_by_key(|path| path.as_os_str().to_string_lossy().to_ascii_lowercase());
    paths.dedup_by(|left, right| {
        left.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
    });
    paths
}

fn redaction_values(process_paths: &[PathBuf]) -> Vec<(String, &'static str)> {
    let mut redactions = process_paths
        .iter()
        .map(|path| (path.as_os_str().to_string_lossy().into_owned(), "<configured-process-path>"))
        .collect::<Vec<_>>();
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        redactions.push((
            PathBuf::from(local_app_data).as_os_str().to_string_lossy().into_owned(),
            "<local-app-data>",
        ));
    }
    if let Some(user_profile) = env::var_os("USERPROFILE") {
        redactions.push((
            PathBuf::from(user_profile).as_os_str().to_string_lossy().into_owned(),
            "<user-profile>",
        ));
    }
    redactions.retain(|(value, _)| !value.is_empty());
    redactions.sort_unstable_by_key(|redaction| std::cmp::Reverse(redaction.0.len()));
    redactions
}

fn sanitize_log(text: &str, redactions: &[(String, &'static str)]) -> String {
    redactions.iter().fold(text.to_owned(), |sanitized, (private, replacement)| {
        replace_ascii_case_insensitive(&sanitized, private, replacement)
    })
}

fn replace_ascii_case_insensitive(text: &str, private: &str, replacement: &str) -> String {
    if private.is_empty() || private.len() > text.len() || !private.is_ascii() {
        return text.replace(private, replacement);
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_private = private.to_ascii_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut start = 0;
    while let Some(relative) = lower_text[start..].find(&lower_private) {
        let index = start.saturating_add(relative);
        result.push_str(&text[start..index]);
        result.push_str(replacement);
        start = index.saturating_add(private.len());
    }
    result.push_str(&text[start..]);
    result
}

fn is_owned_log_name(name: &str) -> bool {
    if name == "gradial_rCURRENT.log" {
        return true;
    }
    name.strip_prefix("gradial_r")
        .and_then(|value| value.strip_suffix(".log"))
        .is_some_and(|number| number.len() == 5 && number.bytes().all(|byte| byte.is_ascii_digit()))
}

fn export_snapshot(
    paths: &AppPaths,
    destination: &Path,
    request: &DiagnosticBundleRequest,
) -> Result<DiagnosticBundleSummary, DiagnosticBundleError> {
    validate_destination(paths, destination)?;
    let parent = destination.parent().ok_or(DiagnosticBundleError::InvalidPath)?;
    let temporary =
        parent.join(format!(".gradial-diagnostics-{}.tmp", Uuid::new_v4().as_hyphenated()));
    let result = (|| {
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&temporary)
            .map_err(|_| DiagnosticBundleError::Write)?;
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let manifest = serialize_manifest(&request.snapshot, request.include_process_paths)?;
        archive.start_file(MANIFEST_ENTRY, options).map_err(|_| DiagnosticBundleError::Write)?;
        archive.write_all(&manifest).map_err(|_| DiagnosticBundleError::Write)?;
        for log in &request.snapshot.logs {
            archive
                .start_file(format!("logs/{}", log.name), options)
                .map_err(|_| DiagnosticBundleError::Write)?;
            archive.write_all(&log.bytes).map_err(|_| DiagnosticBundleError::Write)?;
        }
        if request.include_process_paths {
            archive
                .start_file(PROCESS_PATHS_ENTRY, options)
                .map_err(|_| DiagnosticBundleError::Write)?;
            archive
                .write_all(&serialize_process_paths(&request.snapshot.process_paths)?)
                .map_err(|_| DiagnosticBundleError::Write)?;
        }
        let file = archive.finish().map_err(|_| DiagnosticBundleError::Write)?;
        file.sync_all().map_err(|_| DiagnosticBundleError::Write)?;
        drop(file);
        atomic_replace(&temporary, destination)
    })();
    if result.is_err() {
        let _removed = fs::remove_file(&temporary);
    }
    result?;

    Ok(DiagnosticBundleSummary {
        logs: request.snapshot.logs.len(),
        process_paths: if request.include_process_paths {
            request.snapshot.process_paths.len()
        } else {
            0
        },
        archive_entries: request.snapshot.archive_entry_count(request.include_process_paths),
    })
}

fn serialize_manifest(
    snapshot: &DiagnosticSnapshot,
    include_process_paths: bool,
) -> Result<Vec<u8>, DiagnosticBundleError> {
    let logs = snapshot
        .logs
        .iter()
        .map(|log| ManifestLog {
            archive_path: format!("logs/{}", log.name),
            size_bytes: log.bytes.len(),
        })
        .collect();
    let manifest = BundleManifest {
        bundle_format_version: BUNDLE_FORMAT_VERSION,
        created_unix_seconds: snapshot.created_unix_seconds,
        gradial: GradialManifest {
            version: env!("CARGO_PKG_VERSION"),
            schema_version: snapshot.schema_version,
            config_revision: snapshot.config_revision,
        },
        system: SystemManifest {
            operating_system: env::consts::OS,
            architecture: env::consts::ARCH,
            windows_version: snapshot.windows_version,
        },
        input: InputManifest {
            hook_thread_status: snapshot.input_status,
            status_description: snapshot.input_status.user_text(),
        },
        displays: &snapshot.displays,
        logs,
        privacy: PrivacyManifest {
            logs_sanitized: true,
            configured_process_paths_included: include_process_paths,
            configured_process_path_count: if include_process_paths {
                snapshot.process_paths.len()
            } else {
                0
            },
            always_excluded: [
                "configuration",
                "typed_text",
                "executable_arguments",
                "working_directories",
                "file_and_folder_paths",
                "urls",
                "window_titles",
                "clipboard_data",
                "custom_icons",
            ],
        },
    };
    let mut bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|_| DiagnosticBundleError::Write)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn serialize_process_paths(paths: &[PathBuf]) -> Result<Vec<u8>, DiagnosticBundleError> {
    let document = ProcessPathDocument {
        notice: "Included only after explicit confirmation in Gradial.",
        paths: paths.iter().map(|path| path.as_os_str().to_string_lossy().into_owned()).collect(),
    };
    let mut bytes =
        serde_json::to_vec_pretty(&document).map_err(|_| DiagnosticBundleError::Write)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn windows_version() -> Option<DiagnosticWindowsVersion> {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: u32::try_from(size_of::<OSVERSIONINFOW>()).ok()?,
        ..OSVERSIONINFOW::default()
    };
    // SAFETY: the initialized structure remains writable for this synchronous
    // ntdll query and declares its exact byte size.
    let status = unsafe { RtlGetVersion(&raw mut version) };
    (status.0 >= 0).then_some(DiagnosticWindowsVersion {
        major: version.dwMajorVersion,
        minor: version.dwMinorVersion,
        build: version.dwBuildNumber,
    })
}

fn validate_destination(paths: &AppPaths, destination: &Path) -> Result<(), DiagnosticBundleError> {
    if !destination.is_absolute()
        || destination.file_name().is_none()
        || !destination
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        return Err(DiagnosticBundleError::InvalidPath);
    }
    let Some(parent) = destination.parent().filter(|parent| parent.is_dir()) else {
        return Err(DiagnosticBundleError::InvalidPath);
    };
    let normalized_destination = normalized_windows_path(destination);
    let normalized_root = normalized_windows_path(&paths.root);
    if windows_path_is_within(&normalized_destination, &normalized_root) {
        return Err(DiagnosticBundleError::ProtectedDestination);
    }
    if !parent.is_dir() {
        return Err(DiagnosticBundleError::InvalidPath);
    }
    Ok(())
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

fn windows_path_is_within(path: &Path, root: &Path) -> bool {
    let path = path.as_os_str().to_string_lossy().replace('/', "\\");
    let root =
        root.as_os_str().to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_owned();
    path.eq_ignore_ascii_case(&root)
        || path.get(..root.len()).is_some_and(|prefix| prefix.eq_ignore_ascii_case(&root))
            && path.as_bytes().get(root.len()) == Some(&b'\\')
}

fn atomic_replace(temporary: &Path, destination: &Path) -> Result<(), DiagnosticBundleError> {
    let temporary_wide = wide_path(temporary);
    let destination_wide = wide_path(destination);
    // SAFETY: both buffers are live, null-terminated paths in the same
    // directory. Windows replaces an existing file as one write-through move.
    unsafe {
        MoveFileExW(
            PCWSTR::from_raw(temporary_wide.as_ptr()),
            PCWSTR::from_raw(destination_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(|_| DiagnosticBundleError::Write)
    }
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Read,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use gradial_core::{
        ActionDefinition, ActionKind, AppPaths, ApplicationProfile, Configuration, MonitorGeometry,
        ProfileRule, ScreenRect,
    };
    use uuid::Uuid;
    use zip::ZipArchive;

    use super::{
        DiagnosticBundleError, DiagnosticBundleRequest, DiagnosticInputStatus, PROCESS_PATHS_ENTRY,
        capture_snapshot, export_snapshot, is_owned_log_name, replace_ascii_case_insensitive,
    };

    fn configuration_with_process_paths() -> Configuration {
        let process_path = PathBuf::from(r"C:\Private\Notepad.exe");
        let mut configuration = Configuration { config_revision: 7, ..Configuration::default() };
        let mut application_menu = configuration.global_profile.clone();
        application_menu.id = Uuid::from_u128(701);
        application_menu.name = "Private app".to_owned();
        configuration.application_profiles.push(ApplicationProfile {
            rule: ProfileRule {
                executable_path: process_path.clone(),
                window_class: None,
                title_regex: None,
                priority: 50,
            },
            profile: application_menu,
        });
        configuration.actions.push(ActionDefinition {
            id: Uuid::from_u128(700),
            name: "Run".to_owned(),
            kind: ActionKind::RunExecutable {
                executable: process_path,
                arguments: vec!["private argument".to_owned()],
                working_directory: Some(PathBuf::from(r"C:\Private")),
            },
        });
        configuration
    }

    fn snapshot_fixture(root: &Path) -> super::DiagnosticSnapshot {
        let logs = root.join("logs");
        fs::create_dir_all(&logs).expect("logs");
        fs::write(
            logs.join("gradial_rCURRENT.log"),
            "component=test executable=C:\\Private\\Notepad.exe\n",
        )
        .expect("current log");
        fs::write(logs.join("gradial_r00000.log"), "component=test result_code=ok\n")
            .expect("archived log");
        fs::write(logs.join("notes.log"), "unknown").expect("unknown log");
        fs::create_dir(logs.join("gradial_r00001.log")).expect("log-shaped directory");
        capture_snapshot(
            &logs,
            &configuration_with_process_paths(),
            &[MonitorGeometry {
                bounds: ScreenRect::new(-1920, 0, 0, 1080),
                work_area: ScreenRect::new(-1920, 0, 0, 1040),
                dpi: 144,
            }],
            DiagnosticInputStatus::Responsive,
        )
        .expect("snapshot")
    }

    #[test]
    fn owned_log_names_are_exact_and_closed() {
        assert!(is_owned_log_name("gradial_rCURRENT.log"));
        assert!(is_owned_log_name("gradial_r00042.log"));
        assert!(!is_owned_log_name("gradial.log"));
        assert!(!is_owned_log_name("gradial_r42.log"));
        assert!(!is_owned_log_name("GRADIAL_r00042.log"));
        assert!(!is_owned_log_name("../gradial_r00042.log"));
    }

    #[test]
    fn ascii_redaction_is_case_insensitive_without_touching_other_text() {
        assert_eq!(
            replace_ascii_case_insensitive(
                r"before c:\PRIVATE\notepad.exe after",
                r"C:\Private\Notepad.exe",
                "<path>",
            ),
            "before <path> after"
        );
        assert_eq!(replace_ascii_case_insensitive("unchanged", "missing", "<path>"), "unchanged");
    }

    #[test]
    fn preview_freezes_only_owned_logs_and_deduplicates_private_process_paths() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let snapshot = snapshot_fixture(temporary.path());

        assert_eq!(snapshot.logs.len(), 2);
        assert_eq!(snapshot.process_paths, [PathBuf::from(r"C:\Private\Notepad.exe")]);
        assert_eq!(snapshot.displays[0].dpi, 144);
        assert_eq!(snapshot.input_status, DiagnosticInputStatus::Responsive);
        assert!(snapshot.windows_version.is_some_and(|version| version.build > 0));
        let current = snapshot
            .logs
            .iter()
            .find(|log| log.name == "gradial_rCURRENT.log")
            .expect("current log");
        let current_text = String::from_utf8(current.bytes.clone()).expect("UTF-8");
        assert!(!current_text.contains("Notepad.exe"));
        assert!(current_text.contains("<configured-process-path>"));
    }

    #[test]
    fn default_export_is_atomic_and_omits_process_paths() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let paths = AppPaths::from_local_app_data(temporary.path().join("local"));
        paths.ensure_directories().expect("app directories");
        let snapshot = snapshot_fixture(&paths.root);
        let destination = temporary.path().join("diagnostics.zip");
        fs::write(&destination, b"previous").expect("existing destination");

        let summary = export_snapshot(
            &paths,
            &destination,
            &DiagnosticBundleRequest { snapshot: Arc::new(snapshot), include_process_paths: false },
        )
        .expect("export");

        assert_eq!(summary.logs, 2);
        assert_eq!(summary.process_paths, 0);
        let file = fs::File::open(&destination).expect("bundle");
        let mut archive = ZipArchive::new(file).expect("ZIP");
        assert_eq!(archive.len(), 3);
        assert!(archive.by_name(PROCESS_PATHS_ENTRY).is_err());
        let mut manifest = String::new();
        archive
            .by_name("manifest.json")
            .expect("manifest")
            .read_to_string(&mut manifest)
            .expect("manifest text");
        assert!(manifest.contains("\"configured_process_paths_included\": false"));
        assert!(!manifest.contains("Notepad.exe"));
        assert!(fs::read_dir(temporary.path()).expect("temporary root").all(|entry| {
            !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".gradial-diagnostics-")
        }));
    }

    #[test]
    fn explicit_export_includes_only_the_deduplicated_process_path_document() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let paths = AppPaths::from_local_app_data(temporary.path().join("local"));
        paths.ensure_directories().expect("app directories");
        let snapshot = snapshot_fixture(&paths.root);
        let destination = temporary.path().join("diagnostics.zip");

        let summary = export_snapshot(
            &paths,
            &destination,
            &DiagnosticBundleRequest { snapshot: Arc::new(snapshot), include_process_paths: true },
        )
        .expect("export");

        assert_eq!(summary.process_paths, 1);
        let file = fs::File::open(&destination).expect("bundle");
        let mut archive = ZipArchive::new(file).expect("ZIP");
        let mut process_paths = String::new();
        archive
            .by_name(PROCESS_PATHS_ENTRY)
            .expect("process paths")
            .read_to_string(&mut process_paths)
            .expect("process path text");
        assert_eq!(process_paths.matches("Notepad.exe").count(), 1);
        assert!(!process_paths.contains("private argument"));
        assert!(!process_paths.contains("\"C:\\\\Private\""));
    }

    #[test]
    fn invalid_and_gradial_owned_destinations_never_replace_files() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let paths = AppPaths::from_local_app_data(temporary.path().join("local"));
        paths.ensure_directories().expect("app directories");
        let request = DiagnosticBundleRequest {
            snapshot: Arc::new(snapshot_fixture(&paths.root)),
            include_process_paths: false,
        };
        let protected = paths.root.join("diagnostics.zip");
        fs::write(&protected, b"protected").expect("protected fixture");

        assert_eq!(
            export_snapshot(&paths, &protected, &request),
            Err(DiagnosticBundleError::ProtectedDestination)
        );
        assert_eq!(fs::read(&protected).expect("protected bytes"), b"protected");
        assert_eq!(
            export_snapshot(&paths, Path::new("relative.zip"), &request),
            Err(DiagnosticBundleError::InvalidPath)
        );
        assert_eq!(
            export_snapshot(&paths, &temporary.path().join("diagnostics.json"), &request),
            Err(DiagnosticBundleError::InvalidPath)
        );
    }

    #[test]
    fn request_debug_output_never_contains_process_paths() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let request = DiagnosticBundleRequest {
            snapshot: Arc::new(snapshot_fixture(temporary.path())),
            include_process_paths: true,
        };

        let debug = format!("{request:?}");

        assert!(debug.contains("process_path_count"));
        assert!(!debug.contains("Notepad.exe"));
        assert!(!debug.contains(r"C:\Private"));
    }
}
