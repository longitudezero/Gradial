use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use gradial_core::{ActionErrorCode, ActionResult, InvocationContext, WindowCommand};
use uuid::Uuid;
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{
        PostMessageW, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, WM_CLOSE, WM_SYSCOMMAND,
    },
};

use crate::action_target::{self, TargetValidationFailure};
use crate::focus;

pub(crate) fn execute(
    target: &InvocationContext,
    command: WindowCommand,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: WindowDeadlines,
    log_context: WindowLogContext,
) -> ActionResult {
    execute_with_backend(
        &mut WindowsWindowBackend,
        target,
        command,
        cancellation_requested,
        shutdown_requested,
        deadlines,
        log_context,
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowLogContext {
    config_revision: u64,
    invocation_id: Uuid,
    action_id: Uuid,
}

impl WindowLogContext {
    pub(crate) const fn new(config_revision: u64, invocation_id: Uuid, action_id: Uuid) -> Self {
        Self { config_revision, invocation_id, action_id }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowDeadlines {
    step: Instant,
    action: Instant,
}

impl WindowDeadlines {
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

trait WindowBackend {
    fn validate_target(
        &mut self,
        target: &InvocationContext,
    ) -> Result<(), TargetValidationFailure>;
    fn dispatch(&mut self, window: usize, command: WindowCommand) -> Result<(), u32>;
    fn now(&self) -> Instant;
}

#[allow(clippy::too_many_arguments)]
fn execute_with_backend(
    backend: &mut impl WindowBackend,
    target: &InvocationContext,
    command: WindowCommand,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: WindowDeadlines,
    log_context: WindowLogContext,
) -> ActionResult {
    log::info!(
        target: "actions",
        "component=actions event_code=window_command_step_started result_code=ok \
         config_revision={} invocation_id={} action_id={} command={}",
        log_context.config_revision,
        log_context.invocation_id,
        log_context.action_id,
        command.as_str(),
    );
    if let Some(result) =
        checkpoint(backend.now(), cancellation_requested, shutdown_requested, deadlines)
    {
        return result;
    }
    if let Err(error) = backend.validate_target(target) {
        return target_failure(error);
    }
    if let Some(result) =
        checkpoint(backend.now(), cancellation_requested, shutdown_requested, deadlines)
    {
        return result;
    }

    focus::allow_target_change(log_context.invocation_id);
    if let Err(native_error_code) = backend.dispatch(target.foreground_window, command) {
        focus::revoke_target_change(log_context.invocation_id);
        log::error!(
            target: "actions",
            "component=actions event_code=window_command_failed result_code=failure \
             error_code=window_command_failed config_revision={} invocation_id={} action_id={} \
             command={} phase=post_message native_error_code={native_error_code}",
            log_context.config_revision,
            log_context.invocation_id,
            log_context.action_id,
            command.as_str(),
        );
        return command_failure();
    }
    ActionResult::Success
}

fn checkpoint(
    now: Instant,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: WindowDeadlines,
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

fn target_failure(error: TargetValidationFailure) -> ActionResult {
    let (code, safe_message) = match error {
        TargetValidationFailure::StaleTarget => (
            ActionErrorCode::StaleTarget,
            "the window selected at invocation is no longer the active target",
        ),
        TargetValidationFailure::ElevatedTarget => (
            ActionErrorCode::ElevatedTarget,
            "Gradial cannot control an administrator-elevated window",
        ),
        TargetValidationFailure::InspectionFailed => {
            (ActionErrorCode::WindowCommandFailed, "Gradial could not verify the invocation window")
        }
    };
    ActionResult::Failure { code, safe_message: safe_message.to_owned() }
}

fn command_failure() -> ActionResult {
    ActionResult::Failure {
        code: ActionErrorCode::WindowCommandFailed,
        safe_message: "Windows did not accept the invocation-window command".to_owned(),
    }
}

#[derive(Clone, Copy, Debug)]
struct NativeMessage {
    message: u32,
    parameter: u32,
}

const fn native_message(command: WindowCommand) -> NativeMessage {
    match command {
        WindowCommand::Close => NativeMessage { message: WM_CLOSE, parameter: 0 },
        WindowCommand::Minimize => NativeMessage { message: WM_SYSCOMMAND, parameter: SC_MINIMIZE },
        WindowCommand::Maximize => NativeMessage { message: WM_SYSCOMMAND, parameter: SC_MAXIMIZE },
        WindowCommand::Restore => NativeMessage { message: WM_SYSCOMMAND, parameter: SC_RESTORE },
    }
}

#[derive(Clone, Copy, Debug)]
struct WindowsWindowBackend;

impl WindowBackend for WindowsWindowBackend {
    fn validate_target(
        &mut self,
        target: &InvocationContext,
    ) -> Result<(), TargetValidationFailure> {
        action_target::validate(target)
    }

    fn dispatch(&mut self, window: usize, command: WindowCommand) -> Result<(), u32> {
        let native = native_message(command);
        // SAFETY: the HWND is the exact handle captured at invocation and was
        // revalidated immediately before this asynchronous queue operation.
        unsafe {
            PostMessageW(
                Some(hwnd(window)),
                native.message,
                WPARAM(native.parameter as usize),
                LPARAM(0),
            )
        }
        .map_err(|error| error.code().0.cast_unsigned())
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

fn hwnd(address: usize) -> HWND {
    HWND(std::ptr::with_exposed_provenance_mut(address))
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };

    use gradial_core::{ActionErrorCode, ActionResult, InvocationContext, WindowCommand};
    use windows::Win32::UI::WindowsAndMessaging::{
        SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, WM_CLOSE, WM_SYSCOMMAND,
    };

    use super::{
        NativeMessage, TargetValidationFailure, WindowBackend, WindowDeadlines, WindowLogContext,
        execute_with_backend, native_message,
    };

    #[derive(Debug)]
    struct FakeBackend<'a> {
        validation: Result<(), TargetValidationFailure>,
        cancellation_after_validation: Option<&'a AtomicBool>,
        dispatch_result: Result<(), u32>,
        validations: usize,
        dispatches: Vec<(usize, WindowCommand)>,
        now: Instant,
    }

    impl Default for FakeBackend<'_> {
        fn default() -> Self {
            Self {
                validation: Ok(()),
                cancellation_after_validation: None,
                dispatch_result: Ok(()),
                validations: 0,
                dispatches: Vec::new(),
                now: Instant::now(),
            }
        }
    }

    impl WindowBackend for FakeBackend<'_> {
        fn validate_target(
            &mut self,
            _target: &InvocationContext,
        ) -> Result<(), TargetValidationFailure> {
            self.validations += 1;
            if let Some(cancellation) = self.cancellation_after_validation {
                cancellation.store(true, Ordering::Release);
            }
            self.validation
        }

        fn dispatch(&mut self, window: usize, command: WindowCommand) -> Result<(), u32> {
            self.dispatches.push((window, command));
            self.dispatch_result
        }

        fn now(&self) -> Instant {
            self.now
        }
    }

    fn target() -> InvocationContext {
        InvocationContext {
            foreground_window: 29,
            foreground_pid: 41,
            process_start_time: 73,
            ..InvocationContext::default()
        }
    }

    fn execute(
        backend: &mut FakeBackend<'_>,
        command: WindowCommand,
        cancellation_requested: &AtomicBool,
    ) -> ActionResult {
        let deadlines = WindowDeadlines {
            step: backend.now + Duration::from_secs(5),
            action: backend.now + Duration::from_secs(30),
        };
        execute_with_backend(
            backend,
            &target(),
            command,
            cancellation_requested,
            &AtomicBool::new(false),
            deadlines,
            WindowLogContext::new(1, uuid::Uuid::nil(), uuid::Uuid::nil()),
        )
    }

    #[test]
    fn every_command_targets_only_the_frozen_invocation_window_once() {
        for command in [
            WindowCommand::Close,
            WindowCommand::Minimize,
            WindowCommand::Maximize,
            WindowCommand::Restore,
        ] {
            let mut backend = FakeBackend::default();
            assert_eq!(
                execute(&mut backend, command, &AtomicBool::new(false)),
                ActionResult::Success
            );
            assert_eq!(backend.validations, 1);
            assert_eq!(backend.dispatches, [(29, command)]);
        }
    }

    #[test]
    fn commands_map_to_polite_asynchronous_window_messages() {
        let cases = [
            (WindowCommand::Close, NativeMessage { message: WM_CLOSE, parameter: 0 }),
            (
                WindowCommand::Minimize,
                NativeMessage { message: WM_SYSCOMMAND, parameter: SC_MINIMIZE },
            ),
            (
                WindowCommand::Maximize,
                NativeMessage { message: WM_SYSCOMMAND, parameter: SC_MAXIMIZE },
            ),
            (
                WindowCommand::Restore,
                NativeMessage { message: WM_SYSCOMMAND, parameter: SC_RESTORE },
            ),
        ];

        for (command, expected) in cases {
            let actual = native_message(command);
            assert_eq!((actual.message, actual.parameter), (expected.message, expected.parameter));
        }
    }

    #[test]
    fn target_validation_failures_use_stable_action_codes() {
        let cases = [
            (TargetValidationFailure::StaleTarget, ActionErrorCode::StaleTarget),
            (TargetValidationFailure::ElevatedTarget, ActionErrorCode::ElevatedTarget),
            (TargetValidationFailure::InspectionFailed, ActionErrorCode::WindowCommandFailed),
        ];
        for (validation, expected_code) in cases {
            let mut backend = FakeBackend { validation: Err(validation), ..FakeBackend::default() };
            assert!(matches!(
                execute(
                    &mut backend,
                    WindowCommand::Minimize,
                    &AtomicBool::new(false)
                ),
                ActionResult::Failure { code, .. } if code == expected_code
            ));
            assert!(backend.dispatches.is_empty());
        }
    }

    #[test]
    fn cancellation_before_or_after_validation_prevents_dispatch() {
        let already_cancelled = AtomicBool::new(true);
        let mut before = FakeBackend::default();
        assert_eq!(
            execute(&mut before, WindowCommand::Minimize, &already_cancelled),
            ActionResult::Cancelled
        );
        assert_eq!(before.validations, 0);
        assert!(before.dispatches.is_empty());

        let cancellation = AtomicBool::new(false);
        let mut after = FakeBackend {
            cancellation_after_validation: Some(&cancellation),
            ..FakeBackend::default()
        };
        assert_eq!(
            execute(&mut after, WindowCommand::Minimize, &cancellation),
            ActionResult::Cancelled
        );
        assert_eq!(after.validations, 1);
        assert!(after.dispatches.is_empty());
    }

    #[test]
    fn expired_deadline_and_native_rejection_fail_without_retry() {
        let mut timed_out = FakeBackend::default();
        let expired = WindowDeadlines {
            step: timed_out.now,
            action: timed_out.now + Duration::from_secs(30),
        };
        assert!(matches!(
            execute_with_backend(
                &mut timed_out,
                &target(),
                WindowCommand::Maximize,
                &AtomicBool::new(false),
                &AtomicBool::new(false),
                expired,
                WindowLogContext::new(1, uuid::Uuid::nil(), uuid::Uuid::nil()),
            ),
            ActionResult::Failure { code: ActionErrorCode::StepTimeout, .. }
        ));
        assert_eq!(timed_out.validations, 0);
        assert!(timed_out.dispatches.is_empty());

        let mut rejected = FakeBackend { dispatch_result: Err(5), ..FakeBackend::default() };
        assert!(matches!(
            execute(&mut rejected, WindowCommand::Close, &AtomicBool::new(false)),
            ActionResult::Failure { code: ActionErrorCode::WindowCommandFailed, .. }
        ));
        assert_eq!(rejected.dispatches, [(29, WindowCommand::Close)]);
    }
}
