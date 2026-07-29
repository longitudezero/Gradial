use std::{
    fs,
    mem::size_of,
    os::windows::ffi::OsStrExt,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use gradial_core::{ActionErrorCode, ActionResult};
use uuid::Uuid;
use windows::{
    Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            CreateProcessW, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTUPINFOW,
        },
    },
    core::{PCWSTR, PWSTR},
};

// CreateProcessW limits its mutable command-line buffer to 32,767 UTF-16
// code units including the terminating null.
const MAX_COMMAND_LINE_UTF16_UNITS: usize = 32_767;

pub(crate) fn execute(
    executable: &Path,
    arguments: &[String],
    working_directory: Option<&Path>,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: LaunchDeadlines,
    log_context: LaunchLogContext,
) -> ActionResult {
    execute_with_backend(
        &mut WindowsProcessBackend,
        executable,
        arguments,
        working_directory,
        cancellation_requested,
        shutdown_requested,
        deadlines,
        log_context,
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LaunchLogContext {
    config_revision: u64,
    invocation_id: Uuid,
    action_id: Uuid,
}

impl LaunchLogContext {
    pub(crate) const fn new(config_revision: u64, invocation_id: Uuid, action_id: Uuid) -> Self {
        Self { config_revision, invocation_id, action_id }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LaunchDeadlines {
    step: Instant,
    action: Instant,
}

impl LaunchDeadlines {
    pub(crate) fn new(
        step_started: Instant,
        per_step_timeout: Duration,
        action_started: Instant,
        maximum_action_duration: Duration,
    ) -> Self {
        Self {
            step: step_started + per_step_timeout,
            action: action_started + maximum_action_duration,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LaunchSuccess {
    cleanup_error_code: Option<u32>,
}

trait ProcessBackend {
    fn path_kind(&mut self, path: &Path) -> Result<Option<PathKind>, u32>;
    fn launch(
        &mut self,
        executable: &Path,
        command_line: &[u16],
        working_directory: Option<&Path>,
    ) -> Result<LaunchSuccess, u32>;
    fn now(&self) -> Instant;
}

#[allow(clippy::too_many_arguments)]
fn execute_with_backend(
    backend: &mut impl ProcessBackend,
    executable: &Path,
    arguments: &[String],
    working_directory: Option<&Path>,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: LaunchDeadlines,
    log_context: LaunchLogContext,
) -> ActionResult {
    log::info!(
        target: "actions",
        "component=actions event_code=process_launch_step_started result_code=ok \
         config_revision={} invocation_id={} action_id={} argument_count={} \
         working_directory_configured={}",
        log_context.config_revision,
        log_context.invocation_id,
        log_context.action_id,
        arguments.len(),
        working_directory.is_some(),
    );
    if let Some(result) =
        checkpoint(backend.now(), cancellation_requested, shutdown_requested, deadlines)
    {
        return result;
    }

    if !is_allowed_executable(executable) {
        return launch_failure();
    }
    match backend.path_kind(executable) {
        Ok(Some(PathKind::File)) => {}
        Ok(None) => return path_not_found("the configured executable no longer exists"),
        Ok(Some(PathKind::Directory)) => return launch_failure(),
        Err(native_error_code) => {
            log_native_failure(log_context, "executable_inspection", native_error_code);
            return launch_failure();
        }
    }
    if let Some(result) =
        checkpoint(backend.now(), cancellation_requested, shutdown_requested, deadlines)
    {
        return result;
    }

    if let Some(directory) = working_directory {
        if !directory.is_absolute() {
            return launch_failure();
        }
        match backend.path_kind(directory) {
            Ok(Some(PathKind::Directory)) => {}
            Ok(None) => {
                return path_not_found("the configured working directory no longer exists");
            }
            Ok(Some(PathKind::File)) => return launch_failure(),
            Err(native_error_code) => {
                log_native_failure(log_context, "working_directory_inspection", native_error_code);
                return launch_failure();
            }
        }
    }

    let Ok(command_line) = build_command_line(executable, arguments) else {
        return launch_failure();
    };
    if let Some(result) =
        checkpoint(backend.now(), cancellation_requested, shutdown_requested, deadlines)
    {
        return result;
    }

    match backend.launch(executable, &command_line, working_directory) {
        Ok(success) => {
            if let Some(native_error_code) = success.cleanup_error_code {
                log::warn!(
                    target: "actions",
                    "component=actions event_code=process_handle_cleanup_failed \
                     result_code=failure error_code=handle_cleanup_failed \
                     config_revision={} invocation_id={} action_id={} \
                     native_error_code={native_error_code}",
                    log_context.config_revision,
                    log_context.invocation_id,
                    log_context.action_id,
                );
            }
            ActionResult::Success
        }
        Err(native_error_code) => {
            log_native_failure(log_context, "create_process", native_error_code);
            launch_failure()
        }
    }
}

fn checkpoint(
    now: Instant,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: LaunchDeadlines,
) -> Option<ActionResult> {
    if cancellation_requested.load(Ordering::Acquire) || shutdown_requested.load(Ordering::Acquire)
    {
        return Some(ActionResult::Cancelled);
    }
    if now >= deadlines.action {
        return Some(ActionResult::Failure {
            code: ActionErrorCode::ActionTimeout,
            safe_message: "the action exceeded its maximum duration".to_owned(),
        });
    }
    if now >= deadlines.step {
        return Some(ActionResult::Failure {
            code: ActionErrorCode::StepTimeout,
            safe_message: "an action step timed out".to_owned(),
        });
    }
    None
}

fn is_allowed_executable(path: &Path) -> bool {
    path.is_absolute()
        && path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        && !path.as_os_str().encode_wide().any(|unit| unit == 0)
}

fn build_command_line(executable: &Path, arguments: &[String]) -> Result<Vec<u16>, ()> {
    let executable: Vec<_> = executable.as_os_str().encode_wide().collect();
    if executable.is_empty() || executable.contains(&0) {
        return Err(());
    }

    let mut command_line = Vec::new();
    append_quoted_argument(&mut command_line, &executable);
    for argument in arguments {
        let argument: Vec<_> = argument.encode_utf16().collect();
        if argument.contains(&0) {
            return Err(());
        }
        command_line.push(u16::from(b' '));
        append_quoted_argument(&mut command_line, &argument);
    }
    command_line.push(0);
    if command_line.len() > MAX_COMMAND_LINE_UTF16_UNITS {
        return Err(());
    }
    Ok(command_line)
}

fn append_quoted_argument(command_line: &mut Vec<u16>, argument: &[u16]) {
    const BACKSLASH: u16 = b'\\' as u16;
    const QUOTE: u16 = b'"' as u16;

    command_line.push(QUOTE);
    let mut backslashes = 0;
    for &unit in argument {
        if unit == BACKSLASH {
            backslashes += 1;
        } else if unit == QUOTE {
            command_line.extend(std::iter::repeat_n(BACKSLASH, (backslashes * 2) + 1));
            command_line.push(QUOTE);
            backslashes = 0;
        } else {
            command_line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
            command_line.push(unit);
            backslashes = 0;
        }
    }
    command_line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
    command_line.push(QUOTE);
}

fn path_not_found(message: &str) -> ActionResult {
    ActionResult::Failure { code: ActionErrorCode::PathNotFound, safe_message: message.to_owned() }
}

fn launch_failure() -> ActionResult {
    ActionResult::Failure {
        code: ActionErrorCode::ProcessLaunchFailed,
        safe_message: "Windows did not create the configured executable".to_owned(),
    }
}

fn log_native_failure(context: LaunchLogContext, phase: &str, native_error_code: u32) {
    log::error!(
        target: "actions",
        "component=actions event_code=process_launch_failed result_code=failure \
         error_code=process_launch_failed config_revision={} invocation_id={} action_id={} \
         phase={phase} native_error_code={native_error_code}",
        context.config_revision,
        context.invocation_id,
        context.action_id,
    );
}

#[derive(Clone, Copy, Debug)]
struct WindowsProcessBackend;

impl ProcessBackend for WindowsProcessBackend {
    fn path_kind(&mut self, path: &Path) -> Result<Option<PathKind>, u32> {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => Ok(Some(PathKind::File)),
            Ok(metadata) if metadata.is_dir() => Ok(Some(PathKind::Directory)),
            Ok(_) => Err(0),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(native_error_code(&error)),
        }
    }

    fn launch(
        &mut self,
        executable: &Path,
        command_line: &[u16],
        working_directory: Option<&Path>,
    ) -> Result<LaunchSuccess, u32> {
        let executable: Vec<_> =
            executable.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let working_directory: Option<Vec<_>> = working_directory.map(|directory| {
            directory.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
        });
        if executable.len() == 1
            || executable[..executable.len() - 1].contains(&0)
            || working_directory
                .as_ref()
                .is_some_and(|directory| directory[..directory.len() - 1].contains(&0))
        {
            return Err(0);
        }
        let current_directory = working_directory
            .as_ref()
            .map_or_else(PCWSTR::null, |directory| PCWSTR(directory.as_ptr()));
        let mut mutable_command_line = command_line.to_vec();
        let startup = STARTUPINFOW {
            cb: u32::try_from(size_of::<STARTUPINFOW>()).map_err(|_| 0_u32)?,
            ..Default::default()
        };
        let mut process = PROCESS_INFORMATION::default();
        // SAFETY: all strings are live null-terminated UTF-16 buffers, the
        // command line is mutable as required, and both output handles are
        // closed immediately below. Null environment inherits this process's
        // environment; false prevents handle inheritance.
        unsafe {
            CreateProcessW(
                PCWSTR(executable.as_ptr()),
                Some(PWSTR(mutable_command_line.as_mut_ptr())),
                None,
                None,
                false,
                PROCESS_CREATION_FLAGS(0),
                None,
                current_directory,
                &raw const startup,
                &raw mut process,
            )
        }
        .map_err(|error| error.code().0.cast_unsigned())?;

        // Gradial intentionally does not wait for child exit. Closing these
        // kernel handles releases Gradial's ownership without affecting the
        // newly created process.
        let thread_close = unsafe { CloseHandle(process.hThread) };
        let process_close = unsafe { CloseHandle(process.hProcess) };
        let cleanup_error_code = thread_close
            .err()
            .or_else(|| process_close.err())
            .map(|error| error.code().0.cast_unsigned());
        Ok(LaunchSuccess { cleanup_error_code })
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

fn native_error_code(error: &std::io::Error) -> u32 {
    error.raw_os_error().and_then(|code| u32::try_from(code).ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::c_void,
        path::{Path, PathBuf},
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    use gradial_core::{ActionErrorCode, ActionResult};
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            UI::Shell::CommandLineToArgvW,
        },
        core::PCWSTR,
    };

    use super::{
        LaunchDeadlines, LaunchLogContext, LaunchSuccess, PathKind, ProcessBackend,
        build_command_line, execute_with_backend,
    };

    #[derive(Debug)]
    struct FakeBackend {
        executable_kind: Result<Option<PathKind>, u32>,
        directory_kind: Result<Option<PathKind>, u32>,
        inspected_paths: Vec<PathBuf>,
        launch_result: Result<LaunchSuccess, u32>,
        launches: Vec<(PathBuf, Vec<u16>, Option<PathBuf>)>,
        now: Instant,
    }

    impl Default for FakeBackend {
        fn default() -> Self {
            Self {
                executable_kind: Ok(Some(PathKind::File)),
                directory_kind: Ok(Some(PathKind::Directory)),
                inspected_paths: Vec::new(),
                launch_result: Ok(LaunchSuccess::default()),
                launches: Vec::new(),
                now: Instant::now(),
            }
        }
    }

    impl ProcessBackend for FakeBackend {
        fn path_kind(&mut self, path: &Path) -> Result<Option<PathKind>, u32> {
            self.inspected_paths.push(path.to_owned());
            if self.inspected_paths.len() == 1 { self.executable_kind } else { self.directory_kind }
        }

        fn launch(
            &mut self,
            executable: &Path,
            command_line: &[u16],
            working_directory: Option<&Path>,
        ) -> Result<LaunchSuccess, u32> {
            self.launches.push((
                executable.to_owned(),
                command_line.to_vec(),
                working_directory.map(Path::to_owned),
            ));
            self.launch_result
        }

        fn now(&self) -> Instant {
            self.now
        }
    }

    fn execute(
        backend: &mut FakeBackend,
        executable: &Path,
        arguments: &[String],
        working_directory: Option<&Path>,
    ) -> ActionResult {
        let deadlines = LaunchDeadlines {
            step: backend.now + Duration::from_secs(5),
            action: backend.now + Duration::from_secs(30),
        };
        execute_with_backend(
            backend,
            executable,
            arguments,
            working_directory,
            &AtomicBool::new(false),
            &AtomicBool::new(false),
            deadlines,
            LaunchLogContext::new(1, uuid::Uuid::nil(), uuid::Uuid::nil()),
        )
    }

    #[test]
    fn windows_quoting_round_trips_every_argument_boundary() {
        let executable = Path::new(r"C:\Program Files\Gradial\receiver.exe");
        let arguments = vec![
            String::new(),
            "plain".to_owned(),
            "two words".to_owned(),
            "quote\"value".to_owned(),
            "trailing\\".to_owned(),
            "slashes\\\\before\"quote".to_owned(),
            "%PATH%".to_owned(),
            "$env:PATH".to_owned(),
            "Ω café 🦀".to_owned(),
        ];
        let command_line = build_command_line(executable, &arguments).expect("command line");
        let parsed = parse_windows_command_line(&command_line);
        let expected = std::iter::once(executable.to_string_lossy().into_owned())
            .chain(arguments)
            .collect::<Vec<_>>();

        assert_eq!(parsed, expected);
    }

    #[test]
    fn exact_executable_arguments_and_working_directory_reach_create_process() {
        let mut backend = FakeBackend::default();
        let executable = Path::new(r"C:\Private App\receiver.EXE");
        let working_directory = Path::new(r"C:\Private Work");
        let arguments = vec![String::new(), "literal %PATH% & value".to_owned()];

        assert_eq!(
            execute(&mut backend, executable, &arguments, Some(working_directory)),
            ActionResult::Success
        );
        assert_eq!(backend.inspected_paths, [executable.to_owned(), working_directory.to_owned()]);
        assert_eq!(backend.launches.len(), 1);
        assert_eq!(backend.launches[0].0, executable);
        assert_eq!(backend.launches[0].2.as_deref(), Some(working_directory));
        assert_eq!(
            parse_windows_command_line(&backend.launches[0].1),
            std::iter::once(executable.to_string_lossy().into_owned())
                .chain(arguments)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn missing_paths_and_wrong_path_kinds_stop_before_creation() {
        let executable = Path::new(r"C:\Private\receiver.exe");
        let directory = Path::new(r"C:\Private");
        let mut missing_executable =
            FakeBackend { executable_kind: Ok(None), ..FakeBackend::default() };
        assert!(matches!(
            execute(&mut missing_executable, executable, &[], None),
            ActionResult::Failure { code: ActionErrorCode::PathNotFound, .. }
        ));
        assert!(missing_executable.launches.is_empty());

        let mut missing_directory =
            FakeBackend { directory_kind: Ok(None), ..FakeBackend::default() };
        assert!(matches!(
            execute(&mut missing_directory, executable, &[], Some(directory)),
            ActionResult::Failure { code: ActionErrorCode::PathNotFound, .. }
        ));
        assert!(missing_directory.launches.is_empty());

        let mut directory_is_file =
            FakeBackend { directory_kind: Ok(Some(PathKind::File)), ..FakeBackend::default() };
        assert!(matches!(
            execute(&mut directory_is_file, executable, &[], Some(directory)),
            ActionResult::Failure { code: ActionErrorCode::ProcessLaunchFailed, .. }
        ));
        assert!(directory_is_file.launches.is_empty());
    }

    #[test]
    fn shell_files_and_oversized_command_lines_are_rejected() {
        let mut shell_file = FakeBackend::default();
        assert!(matches!(
            execute(&mut shell_file, Path::new(r"C:\Private\receiver.cmd"), &[], None),
            ActionResult::Failure { code: ActionErrorCode::ProcessLaunchFailed, .. }
        ));
        assert!(shell_file.inspected_paths.is_empty());
        assert!(shell_file.launches.is_empty());

        let mut oversized = FakeBackend::default();
        let arguments = vec!["x".repeat(32_767)];
        assert!(matches!(
            execute(&mut oversized, Path::new(r"C:\Private\receiver.exe"), &arguments, None),
            ActionResult::Failure { code: ActionErrorCode::ProcessLaunchFailed, .. }
        ));
        assert!(oversized.launches.is_empty());
    }

    #[test]
    fn create_process_failure_is_redacted_and_has_a_stable_code() {
        let mut backend = FakeBackend { launch_result: Err(5), ..FakeBackend::default() };
        let private_executable = Path::new(r"C:\Private Token\receiver.exe");
        let result =
            execute(&mut backend, private_executable, &["private argument".to_owned()], None);

        assert!(matches!(
            &result,
            ActionResult::Failure {
                code: ActionErrorCode::ProcessLaunchFailed,
                safe_message,
            } if !safe_message.contains("Private Token") && !safe_message.contains("private argument")
        ));
    }

    #[test]
    fn cancellation_and_deadlines_stop_before_process_creation() {
        let executable = Path::new(r"C:\Private\receiver.exe");
        let cancelled = AtomicBool::new(true);
        let mut backend = FakeBackend::default();
        let deadlines = LaunchDeadlines {
            step: backend.now + Duration::from_secs(5),
            action: backend.now + Duration::from_secs(30),
        };
        assert_eq!(
            execute_with_backend(
                &mut backend,
                executable,
                &[],
                None,
                &cancelled,
                &AtomicBool::new(false),
                deadlines,
                LaunchLogContext::new(1, uuid::Uuid::nil(), uuid::Uuid::nil()),
            ),
            ActionResult::Cancelled
        );
        assert!(backend.launches.is_empty());

        let mut timed_out = FakeBackend::default();
        let expired = LaunchDeadlines {
            step: timed_out.now,
            action: timed_out.now + Duration::from_secs(30),
        };
        assert!(matches!(
            execute_with_backend(
                &mut timed_out,
                executable,
                &[],
                None,
                &AtomicBool::new(false),
                &AtomicBool::new(false),
                expired,
                LaunchLogContext::new(1, uuid::Uuid::nil(), uuid::Uuid::nil()),
            ),
            ActionResult::Failure { code: ActionErrorCode::StepTimeout, .. }
        ));
        assert!(timed_out.launches.is_empty());
    }

    fn parse_windows_command_line(command_line: &[u16]) -> Vec<String> {
        let mut count = 0;
        // SAFETY: the builder always returns a live null-terminated buffer and
        // the returned LocalAlloc block is released below.
        let arguments =
            unsafe { CommandLineToArgvW(PCWSTR(command_line.as_ptr()), &raw mut count) };
        assert!(!arguments.is_null());
        let count = usize::try_from(count).expect("argument count");
        let pointers = unsafe { std::slice::from_raw_parts(arguments, count) };
        let parsed = pointers
            .iter()
            .map(|argument| unsafe { argument.to_string().expect("UTF-16 argument") })
            .collect();
        // SAFETY: CommandLineToArgvW returns a single LocalAlloc block.
        unsafe {
            LocalFree(Some(HLOCAL(arguments.cast::<c_void>())));
        }
        parsed
    }
}
