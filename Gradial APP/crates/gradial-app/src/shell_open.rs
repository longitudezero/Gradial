use std::{
    ffi::OsStr,
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
        System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
        },
        UI::{
            Shell::{
                SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_UNICODE, SHELLEXECUTEINFOW,
                ShellExecuteExW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
    core::{PCWSTR, w},
};

pub(crate) fn open_url(
    url: &str,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: OpenDeadlines,
    log_context: OpenLogContext,
) -> ActionResult {
    execute_with_backend(
        &mut WindowsShellBackend,
        OpenRequest::Url(url),
        cancellation_requested,
        shutdown_requested,
        deadlines,
        log_context,
    )
}

pub(crate) fn open_path(
    path: &Path,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: OpenDeadlines,
    log_context: OpenLogContext,
) -> ActionResult {
    execute_with_backend(
        &mut WindowsShellBackend,
        OpenRequest::Path(path),
        cancellation_requested,
        shutdown_requested,
        deadlines,
        log_context,
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OpenLogContext {
    config_revision: u64,
    invocation_id: Uuid,
    action_id: Uuid,
}

impl OpenLogContext {
    pub(crate) const fn new(config_revision: u64, invocation_id: Uuid, action_id: Uuid) -> Self {
        Self { config_revision, invocation_id, action_id }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OpenDeadlines {
    step: Instant,
    action: Instant,
}

impl OpenDeadlines {
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

#[derive(Clone, Copy, Debug)]
enum OpenRequest<'a> {
    Url(&'a str),
    Path(&'a Path),
}

impl<'a> OpenRequest<'a> {
    const fn kind(self) -> &'static str {
        match self {
            Self::Url(_) => "url",
            Self::Path(_) => "path",
        }
    }

    fn target(self) -> &'a OsStr {
        match self {
            Self::Url(url) => OsStr::new(url),
            Self::Path(path) => path.as_os_str(),
        }
    }
}

trait ShellBackend {
    fn path_exists(&mut self, path: &Path) -> Result<bool, u32>;
    fn open(&mut self, target: &OsStr) -> Result<(), u32>;
    fn now(&self) -> Instant;
}

fn execute_with_backend(
    backend: &mut impl ShellBackend,
    request: OpenRequest<'_>,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: OpenDeadlines,
    log_context: OpenLogContext,
) -> ActionResult {
    log::info!(
        target: "actions",
        "component=actions event_code=shell_open_step_started result_code=ok \
         config_revision={} invocation_id={} action_id={} request_kind={}",
        log_context.config_revision,
        log_context.invocation_id,
        log_context.action_id,
        request.kind(),
    );
    if let Some(result) =
        checkpoint(backend.now(), cancellation_requested, shutdown_requested, deadlines)
    {
        return result;
    }

    if let OpenRequest::Path(path) = request {
        if !path.is_absolute() {
            return launch_failure();
        }
        match backend.path_exists(path) {
            Ok(true) => {}
            Ok(false) => {
                return ActionResult::Failure {
                    code: ActionErrorCode::PathNotFound,
                    safe_message: "the configured file or folder no longer exists".to_owned(),
                };
            }
            Err(native_error_code) => {
                log_native_failure(
                    log_context,
                    request.kind(),
                    "path_inspection",
                    native_error_code,
                );
                return launch_failure();
            }
        }
    }

    if let Some(result) =
        checkpoint(backend.now(), cancellation_requested, shutdown_requested, deadlines)
    {
        return result;
    }

    if let Err(native_error_code) = backend.open(request.target()) {
        log_native_failure(log_context, request.kind(), "shell_rejected", native_error_code);
        return launch_failure();
    }

    ActionResult::Success
}

fn checkpoint(
    now: Instant,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: OpenDeadlines,
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

fn launch_failure() -> ActionResult {
    ActionResult::Failure {
        code: ActionErrorCode::ProcessLaunchFailed,
        safe_message: "Windows did not accept the open request".to_owned(),
    }
}

fn log_native_failure(
    context: OpenLogContext,
    request_kind: &str,
    phase: &str,
    native_error_code: u32,
) {
    log::error!(
        target: "actions",
        "component=actions event_code=shell_open_failed result_code=failure \
         error_code=process_launch_failed config_revision={} invocation_id={} action_id={} \
         request_kind={request_kind} phase={phase} native_error_code={native_error_code}",
        context.config_revision,
        context.invocation_id,
        context.action_id,
    );
}

#[derive(Clone, Copy, Debug)]
struct WindowsShellBackend;

impl ShellBackend for WindowsShellBackend {
    fn path_exists(&mut self, path: &Path) -> Result<bool, u32> {
        path.try_exists().map_err(|error| {
            error.raw_os_error().and_then(|code| u32::try_from(code).ok()).unwrap_or(0)
        })
    }

    fn open(&mut self, target: &OsStr) -> Result<(), u32> {
        let _com_apartment = ComApartment::initialize()?;
        let target_wide: Vec<_> = target.encode_wide().chain(std::iter::once(0)).collect();
        if target_wide.len() == 1 || target_wide[..target_wide.len() - 1].contains(&0) {
            return Err(0);
        }
        let mut execute = SHELLEXECUTEINFOW {
            cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).map_err(|_| 0_u32)?,
            fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI | SEE_MASK_UNICODE,
            lpVerb: w!("open"),
            lpFile: PCWSTR(target_wide.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        // SAFETY: every pointer in `execute` is either null or references the
        // live, null-terminated target buffer for this synchronous call.
        match unsafe { ShellExecuteExW(&raw mut execute) } {
            Ok(()) => Ok(()),
            Err(error) => {
                let legacy_code = u32::try_from(execute.hInstApp.0.addr()).unwrap_or(0);
                if (1..=32).contains(&legacy_code) {
                    Err(legacy_code)
                } else {
                    Err(error.code().0.cast_unsigned())
                }
            }
        }
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Debug)]
struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, u32> {
        // SAFETY: this initializes COM for the current action-worker thread
        // and every successful call is balanced by this guard's Drop.
        let result =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if result.is_ok() { Ok(Self) } else { Err(result.0.cast_unsigned()) }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: this guard exists only after successful CoInitializeEx on
        // this same thread, so one matching uninitialization is required.
        unsafe {
            CoUninitialize();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::{OsStr, OsString},
        path::{Path, PathBuf},
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    use gradial_core::{ActionErrorCode, ActionResult};

    use super::{OpenDeadlines, OpenLogContext, OpenRequest, ShellBackend, execute_with_backend};

    #[derive(Debug)]
    struct FakeBackend {
        path_exists: Result<bool, u32>,
        inspected_paths: Vec<PathBuf>,
        open_result: Result<(), u32>,
        opened_targets: Vec<OsString>,
        now: Instant,
    }

    impl Default for FakeBackend {
        fn default() -> Self {
            Self {
                path_exists: Ok(true),
                inspected_paths: Vec::new(),
                open_result: Ok(()),
                opened_targets: Vec::new(),
                now: Instant::now(),
            }
        }
    }

    impl ShellBackend for FakeBackend {
        fn path_exists(&mut self, path: &Path) -> Result<bool, u32> {
            self.inspected_paths.push(path.to_owned());
            self.path_exists
        }

        fn open(&mut self, target: &OsStr) -> Result<(), u32> {
            self.opened_targets.push(target.to_owned());
            self.open_result
        }

        fn now(&self) -> Instant {
            self.now
        }
    }

    fn execute(backend: &mut FakeBackend, request: OpenRequest<'_>) -> ActionResult {
        let deadlines = OpenDeadlines {
            step: backend.now + Duration::from_secs(5),
            action: backend.now + Duration::from_secs(30),
        };
        execute_with_backend(
            backend,
            request,
            &AtomicBool::new(false),
            &AtomicBool::new(false),
            deadlines,
            OpenLogContext::new(1, uuid::Uuid::nil(), uuid::Uuid::nil()),
        )
    }

    #[test]
    fn url_is_passed_to_the_shell_literally_without_path_expansion() {
        let mut backend = FakeBackend::default();
        let url = "https://example.test/%PATH%?literal=$env:PATH&value=one";

        assert_eq!(execute(&mut backend, OpenRequest::Url(url)), ActionResult::Success);
        assert!(backend.inspected_paths.is_empty());
        assert_eq!(backend.opened_targets, [OsString::from(url)]);
    }

    #[test]
    fn existing_absolute_file_or_folder_is_passed_to_the_shell_unchanged() {
        let mut backend = FakeBackend::default();
        let path = Path::new(r"C:\private\%PATH% & literal.txt");

        assert_eq!(execute(&mut backend, OpenRequest::Path(path)), ActionResult::Success);
        assert_eq!(backend.inspected_paths, [path.to_owned()]);
        assert_eq!(backend.opened_targets, [path.as_os_str().to_owned()]);
    }

    #[test]
    fn missing_path_stops_before_the_shell_with_stable_code() {
        let mut backend = FakeBackend { path_exists: Ok(false), ..FakeBackend::default() };
        let result = execute(&mut backend, OpenRequest::Path(Path::new(r"C:\missing.txt")));

        assert!(matches!(
            result,
            ActionResult::Failure { code: ActionErrorCode::PathNotFound, .. }
        ));
        assert!(backend.opened_targets.is_empty());
    }

    #[test]
    fn shell_rejection_is_redacted_and_has_stable_code() {
        let mut backend = FakeBackend { open_result: Err(31), ..FakeBackend::default() };
        let private_url = "https://example.test/private-token";
        let result = execute(&mut backend, OpenRequest::Url(private_url));

        assert!(matches!(
            &result,
            ActionResult::Failure {
                code: ActionErrorCode::ProcessLaunchFailed,
                safe_message,
            } if !safe_message.contains(private_url)
        ));
    }

    #[test]
    fn cancellation_and_deadlines_stop_before_shell_dispatch() {
        let cancelled = AtomicBool::new(true);
        let mut backend = FakeBackend::default();
        let deadlines = OpenDeadlines {
            step: backend.now + Duration::from_secs(5),
            action: backend.now + Duration::from_secs(30),
        };
        assert_eq!(
            execute_with_backend(
                &mut backend,
                OpenRequest::Url("https://example.test"),
                &cancelled,
                &AtomicBool::new(false),
                deadlines,
                OpenLogContext::new(1, uuid::Uuid::nil(), uuid::Uuid::nil()),
            ),
            ActionResult::Cancelled
        );
        assert!(backend.opened_targets.is_empty());

        let mut timed_out = FakeBackend::default();
        let expired =
            OpenDeadlines { step: timed_out.now, action: timed_out.now + Duration::from_secs(30) };
        assert!(matches!(
            execute_with_backend(
                &mut timed_out,
                OpenRequest::Url("https://example.test"),
                &AtomicBool::new(false),
                &AtomicBool::new(false),
                expired,
                OpenLogContext::new(1, uuid::Uuid::nil(), uuid::Uuid::nil()),
            ),
            ActionResult::Failure { code: ActionErrorCode::StepTimeout, .. }
        ));
        assert!(timed_out.opened_targets.is_empty());
    }
}
