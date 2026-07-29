use std::{
    mem::size_of,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use gradial_core::{ActionErrorCode, ActionResult, InvocationContext};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput,
    VIRTUAL_KEY,
};

use crate::{
    action_target::{self, TargetValidationFailure},
    input::GRADIAL_INJECTED_MARKER,
};

pub(crate) fn execute(
    target: &InvocationContext,
    text: &str,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: TextDeadlines,
) -> ActionResult {
    execute_with_backend(
        &mut WindowsTextBackend,
        target,
        text,
        cancellation_requested,
        shutdown_requested,
        deadlines,
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextDeadlines {
    step: Instant,
    action: Instant,
}

impl TextDeadlines {
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

trait TextBackend {
    fn validate_target(
        &mut self,
        target: &InvocationContext,
    ) -> Result<(), TargetValidationFailure>;
    fn validate_freshness(
        &mut self,
        target: &InvocationContext,
    ) -> Result<(), TargetValidationFailure>;
    fn send(&mut self, event: UnicodeKeyEvent) -> Result<(), ()>;
    fn now(&self) -> Instant;
}

fn execute_with_backend(
    backend: &mut impl TextBackend,
    target: &InvocationContext,
    text: &str,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: TextDeadlines,
) -> ActionResult {
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

    for utf16_unit in text.encode_utf16() {
        if let Some(result) =
            checkpoint(backend.now(), cancellation_requested, shutdown_requested, deadlines)
        {
            return result;
        }
        if let Err(error) = backend.validate_freshness(target) {
            return target_failure(error);
        }
        if let Some(result) =
            checkpoint(backend.now(), cancellation_requested, shutdown_requested, deadlines)
        {
            return result;
        }

        let mut pending = PendingUnicodeGuard::new(backend, target.config_revision);
        if pending.press(utf16_unit).is_err() {
            return injection_failure();
        }
        if let Some(result) =
            pending.checkpoint(cancellation_requested, shutdown_requested, deadlines)
        {
            return result;
        }
        if let Err(error) = pending.validate_freshness(target) {
            return target_failure(error);
        }
        if let Some(result) =
            pending.checkpoint(cancellation_requested, shutdown_requested, deadlines)
        {
            return result;
        }
        if pending.release().is_err() {
            return injection_failure();
        }
    }

    ActionResult::Success
}

fn checkpoint(
    now: Instant,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: TextDeadlines,
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
            "the application selected at invocation is no longer the active target",
        ),
        TargetValidationFailure::ElevatedTarget => (
            ActionErrorCode::ElevatedTarget,
            "Gradial cannot send text to an administrator-elevated application",
        ),
        TargetValidationFailure::InspectionFailed => (
            ActionErrorCode::InputInjectionFailed,
            "Gradial could not verify the target before sending text",
        ),
    };
    ActionResult::Failure { code, safe_message: safe_message.to_owned() }
}

fn injection_failure() -> ActionResult {
    ActionResult::Failure {
        code: ActionErrorCode::InputInjectionFailed,
        safe_message: "Windows did not accept the complete Unicode text".to_owned(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UnicodeKeyEvent {
    utf16_unit: u16,
    key_up: bool,
    marker: usize,
}

impl UnicodeKeyEvent {
    const fn down(utf16_unit: u16) -> Self {
        Self { utf16_unit, key_up: false, marker: GRADIAL_INJECTED_MARKER }
    }

    const fn up(utf16_unit: u16) -> Self {
        Self { utf16_unit, key_up: true, marker: GRADIAL_INJECTED_MARKER }
    }
}

struct PendingUnicodeGuard<'a, B: TextBackend> {
    backend: &'a mut B,
    pending_unit: Option<u16>,
    config_revision: u64,
}

impl<'a, B: TextBackend> PendingUnicodeGuard<'a, B> {
    fn new(backend: &'a mut B, config_revision: u64) -> Self {
        Self { backend, pending_unit: None, config_revision }
    }

    fn press(&mut self, utf16_unit: u16) -> Result<(), ()> {
        self.backend.send(UnicodeKeyEvent::down(utf16_unit))?;
        self.pending_unit = Some(utf16_unit);
        Ok(())
    }

    fn validate_freshness(
        &mut self,
        target: &InvocationContext,
    ) -> Result<(), TargetValidationFailure> {
        self.backend.validate_freshness(target)
    }

    fn checkpoint(
        &self,
        cancellation_requested: &AtomicBool,
        shutdown_requested: &AtomicBool,
        deadlines: TextDeadlines,
    ) -> Option<ActionResult> {
        checkpoint(self.backend.now(), cancellation_requested, shutdown_requested, deadlines)
    }

    fn release(&mut self) -> Result<(), ()> {
        let Some(utf16_unit) = self.pending_unit else {
            return Err(());
        };
        self.backend.send(UnicodeKeyEvent::up(utf16_unit))?;
        self.pending_unit = None;
        Ok(())
    }
}

impl<B: TextBackend> Drop for PendingUnicodeGuard<'_, B> {
    fn drop(&mut self) {
        let Some(utf16_unit) = self.pending_unit.take() else {
            return;
        };
        if self.backend.send(UnicodeKeyEvent::up(utf16_unit)).is_err() {
            log::error!(
                target: "actions",
                "component=actions event_code=unicode_cleanup_failed result_code=failure \
                 error_code=input_injection_failed config_revision={} failed_keyup_count=1",
                self.config_revision
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct WindowsTextBackend;

impl TextBackend for WindowsTextBackend {
    fn validate_target(
        &mut self,
        target: &InvocationContext,
    ) -> Result<(), TargetValidationFailure> {
        action_target::validate(target)
    }

    fn validate_freshness(
        &mut self,
        target: &InvocationContext,
    ) -> Result<(), TargetValidationFailure> {
        action_target::validate_freshness(target)
    }

    fn send(&mut self, event: UnicodeKeyEvent) -> Result<(), ()> {
        let mut flags = KEYEVENTF_UNICODE;
        if event.key_up {
            flags |= KEYEVENTF_KEYUP;
        }
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: event.utf16_unit,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: event.marker,
                },
            },
        };
        let input_size = i32::try_from(size_of::<INPUT>()).map_err(|_| ())?;
        // SAFETY: the keyboard input is fully initialized and the slice is
        // live for this synchronous call.
        if unsafe { SendInput(&[input], input_size) } == 1 { Ok(()) } else { Err(()) }
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    use gradial_core::{ActionErrorCode, ActionResult, InvocationContext};

    use super::{
        GRADIAL_INJECTED_MARKER, TargetValidationFailure, TextBackend, TextDeadlines,
        UnicodeKeyEvent, execute_with_backend,
    };

    #[derive(Debug)]
    struct FakeBackend {
        validation: Result<(), TargetValidationFailure>,
        events: Vec<UnicodeKeyEvent>,
        pending: Vec<u16>,
        send_attempt: usize,
        fail_once_at: Option<usize>,
        cancel_after_successful_send: Option<usize>,
        stale_after_successful_send: Option<usize>,
        freshness_checks: usize,
        cancellation: Arc<AtomicBool>,
        now: Instant,
        advance_after_successful_send: Option<Duration>,
    }

    impl FakeBackend {
        fn new(cancellation: Arc<AtomicBool>) -> Self {
            Self {
                validation: Ok(()),
                events: Vec::new(),
                pending: Vec::new(),
                send_attempt: 0,
                fail_once_at: None,
                cancel_after_successful_send: None,
                stale_after_successful_send: None,
                freshness_checks: 0,
                cancellation,
                now: Instant::now(),
                advance_after_successful_send: None,
            }
        }
    }

    impl TextBackend for FakeBackend {
        fn validate_target(
            &mut self,
            _target: &InvocationContext,
        ) -> Result<(), TargetValidationFailure> {
            self.validation
        }

        fn validate_freshness(
            &mut self,
            _target: &InvocationContext,
        ) -> Result<(), TargetValidationFailure> {
            self.freshness_checks = self.freshness_checks.saturating_add(1);
            if self.stale_after_successful_send == Some(self.events.len()) {
                Err(TargetValidationFailure::StaleTarget)
            } else {
                Ok(())
            }
        }

        fn send(&mut self, event: UnicodeKeyEvent) -> Result<(), ()> {
            let attempt = self.send_attempt;
            self.send_attempt = self.send_attempt.saturating_add(1);
            if self.fail_once_at == Some(attempt) {
                self.fail_once_at = None;
                return Err(());
            }
            if event.key_up {
                if let Some(index) = self.pending.iter().rposition(|unit| *unit == event.utf16_unit)
                {
                    self.pending.remove(index);
                }
            } else {
                self.pending.push(event.utf16_unit);
            }
            self.events.push(event);
            if self.cancel_after_successful_send == Some(self.events.len()) {
                self.cancellation.store(true, Ordering::Release);
            }
            if let Some(duration) = self.advance_after_successful_send {
                self.now += duration;
            }
            Ok(())
        }

        fn now(&self) -> Instant {
            self.now
        }
    }

    fn execute(backend: &mut FakeBackend, text: &str, cancellation: &AtomicBool) -> ActionResult {
        let deadlines = TextDeadlines {
            step: backend.now + Duration::from_secs(5),
            action: backend.now + Duration::from_secs(30),
        };
        execute_with_backend(
            backend,
            &InvocationContext::default(),
            text,
            cancellation,
            &AtomicBool::new(false),
            deadlines,
        )
    }

    #[test]
    fn utf8_text_becomes_marked_utf16_events_including_surrogate_pairs() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut backend = FakeBackend::new(Arc::clone(&cancellation));

        assert_eq!(execute(&mut backend, "Aé🦀", &cancellation), ActionResult::Success);
        let expected_units: Vec<_> = "Aé🦀".encode_utf16().collect();
        assert_eq!(backend.events.len(), expected_units.len() * 2);
        for (events, expected_unit) in
            backend.events.chunks_exact(2).zip(expected_units.iter().copied())
        {
            assert_eq!(events[0], UnicodeKeyEvent::down(expected_unit));
            assert_eq!(events[1], UnicodeKeyEvent::up(expected_unit));
        }
        assert!(backend.events.iter().all(|event| event.marker == GRADIAL_INJECTED_MARKER));
        assert_eq!(backend.freshness_checks, expected_units.len() * 2);
        assert!(backend.pending.is_empty());
    }

    #[test]
    fn cancellation_between_unicode_keydown_and_keyup_cleans_the_unit() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut backend = FakeBackend::new(Arc::clone(&cancellation));
        backend.cancel_after_successful_send = Some(1);

        assert_eq!(execute(&mut backend, "private", &cancellation), ActionResult::Cancelled);
        assert_eq!(backend.events.len(), 2);
        assert!(backend.events[1].key_up);
        assert!(backend.pending.is_empty());
    }

    #[test]
    fn target_change_after_keydown_stops_text_and_cleans_the_unit() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut backend = FakeBackend::new(Arc::clone(&cancellation));
        backend.stale_after_successful_send = Some(1);

        assert!(matches!(
            execute(&mut backend, "private", &cancellation),
            ActionResult::Failure { code: ActionErrorCode::StaleTarget, .. }
        ));
        assert_eq!(backend.events.len(), 2);
        assert!(backend.events[1].key_up);
        assert!(backend.pending.is_empty());
    }

    #[test]
    fn step_timeout_after_keydown_stops_text_and_cleans_the_unit() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut backend = FakeBackend::new(Arc::clone(&cancellation));
        backend.advance_after_successful_send = Some(Duration::from_secs(6));

        assert!(matches!(
            execute(&mut backend, "private", &cancellation),
            ActionResult::Failure { code: ActionErrorCode::StepTimeout, .. }
        ));
        assert_eq!(backend.events.len(), 2);
        assert!(backend.events[1].key_up);
        assert!(backend.pending.is_empty());
    }

    #[test]
    fn every_injection_failure_runs_pending_unicode_cleanup() {
        let event_count = "A🦀".encode_utf16().count() * 2;
        for failure_index in 0..event_count {
            let cancellation = Arc::new(AtomicBool::new(false));
            let mut backend = FakeBackend::new(Arc::clone(&cancellation));
            backend.fail_once_at = Some(failure_index);

            assert!(matches!(
                execute(&mut backend, "A🦀", &cancellation),
                ActionResult::Failure { code: ActionErrorCode::InputInjectionFailed, .. }
            ));
            assert!(
                backend.pending.is_empty(),
                "failure index {failure_index} left a UTF-16 unit down"
            );
        }
    }

    #[test]
    fn full_target_validation_failure_prevents_all_text_events() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let scenarios = [
            (TargetValidationFailure::StaleTarget, ActionErrorCode::StaleTarget),
            (TargetValidationFailure::ElevatedTarget, ActionErrorCode::ElevatedTarget),
            (TargetValidationFailure::InspectionFailed, ActionErrorCode::InputInjectionFailed),
        ];
        for (validation, expected_code) in scenarios {
            let mut backend = FakeBackend::new(Arc::clone(&cancellation));
            backend.validation = Err(validation);
            assert!(matches!(
                execute(&mut backend, "must not be sent", &cancellation),
                ActionResult::Failure { code, .. } if code == expected_code
            ));
            assert!(backend.events.is_empty());
        }
    }
}
