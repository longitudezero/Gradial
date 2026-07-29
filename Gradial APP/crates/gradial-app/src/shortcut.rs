use std::{
    mem::size_of,
    sync::atomic::{AtomicBool, Ordering},
};

use gradial_core::{
    ActionErrorCode, ActionResult, InvocationContext, ShortcutKey, ShortcutModifier,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput, VIRTUAL_KEY, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};

use crate::{
    action_target::{self, TargetValidationFailure},
    input::GRADIAL_INJECTED_MARKER,
};

pub(crate) fn execute(
    target: &InvocationContext,
    modifiers: &[ShortcutModifier],
    key: ShortcutKey,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
) -> ActionResult {
    execute_with_backend(
        &mut WindowsShortcutBackend,
        target,
        modifiers,
        key,
        cancellation_requested,
        shutdown_requested,
    )
}

trait ShortcutBackend {
    fn validate_target(
        &mut self,
        target: &InvocationContext,
    ) -> Result<(), TargetValidationFailure>;
    fn modifier_is_down(&self, modifier: ShortcutModifier) -> bool;
    fn send(&mut self, event: SyntheticKeyEvent) -> Result<(), ()>;
}

fn execute_with_backend(
    backend: &mut impl ShortcutBackend,
    target: &InvocationContext,
    modifiers: &[ShortcutModifier],
    key: ShortcutKey,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
) -> ActionResult {
    if cancelled(cancellation_requested, shutdown_requested) {
        return ActionResult::Cancelled;
    }
    if let Err(error) = backend.validate_target(target) {
        return target_failure(error);
    }
    if cancelled(cancellation_requested, shutdown_requested) {
        return ActionResult::Cancelled;
    }

    let modifiers_to_press: Vec<_> = modifiers
        .iter()
        .copied()
        .filter(|modifier| !backend.modifier_is_down(*modifier))
        .map(modifier_key)
        .collect();
    let mut pressed = PressedKeyGuard::new(backend, target.config_revision);
    for modifier in modifiers_to_press {
        if cancelled(cancellation_requested, shutdown_requested) {
            return ActionResult::Cancelled;
        }
        if pressed.press(modifier).is_err() {
            return injection_failure();
        }
    }

    if cancelled(cancellation_requested, shutdown_requested) {
        return ActionResult::Cancelled;
    }
    let primary_key = SyntheticKey { scan_code: key.scan_code, extended: key.extended };
    if pressed.press(primary_key).is_err() {
        return injection_failure();
    }
    if cancelled(cancellation_requested, shutdown_requested) {
        return ActionResult::Cancelled;
    }
    if pressed.release_last(primary_key).is_err() {
        return injection_failure();
    }
    if cancelled(cancellation_requested, shutdown_requested) {
        return ActionResult::Cancelled;
    }
    if pressed.finish().is_err() {
        return injection_failure();
    }

    ActionResult::Success
}

fn cancelled(cancellation_requested: &AtomicBool, shutdown_requested: &AtomicBool) -> bool {
    cancellation_requested.load(Ordering::Acquire) || shutdown_requested.load(Ordering::Acquire)
}

fn target_failure(error: TargetValidationFailure) -> ActionResult {
    let (code, safe_message) = match error {
        TargetValidationFailure::StaleTarget => (
            ActionErrorCode::StaleTarget,
            "the application selected at invocation is no longer the active target",
        ),
        TargetValidationFailure::ElevatedTarget => (
            ActionErrorCode::ElevatedTarget,
            "Gradial cannot send input to an administrator-elevated application",
        ),
        TargetValidationFailure::InspectionFailed => (
            ActionErrorCode::InputInjectionFailed,
            "Gradial could not verify the target before sending input",
        ),
    };
    ActionResult::Failure { code, safe_message: safe_message.to_owned() }
}

fn injection_failure() -> ActionResult {
    ActionResult::Failure {
        code: ActionErrorCode::InputInjectionFailed,
        safe_message: "Windows did not accept the complete keyboard shortcut".to_owned(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SyntheticKey {
    scan_code: u16,
    extended: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SyntheticKeyEvent {
    key: SyntheticKey,
    key_up: bool,
    marker: usize,
}

impl SyntheticKeyEvent {
    const fn down(key: SyntheticKey) -> Self {
        Self { key, key_up: false, marker: GRADIAL_INJECTED_MARKER }
    }

    const fn up(key: SyntheticKey) -> Self {
        Self { key, key_up: true, marker: GRADIAL_INJECTED_MARKER }
    }
}

struct PressedKeyGuard<'a, B: ShortcutBackend> {
    backend: &'a mut B,
    pressed: Vec<SyntheticKey>,
    config_revision: u64,
}

impl<'a, B: ShortcutBackend> PressedKeyGuard<'a, B> {
    fn new(backend: &'a mut B, config_revision: u64) -> Self {
        Self { backend, pressed: Vec::new(), config_revision }
    }

    fn press(&mut self, key: SyntheticKey) -> Result<(), ()> {
        self.backend.send(SyntheticKeyEvent::down(key))?;
        self.pressed.push(key);
        Ok(())
    }

    fn release_last(&mut self, key: SyntheticKey) -> Result<(), ()> {
        if self.pressed.last() != Some(&key) {
            return Err(());
        }
        self.backend.send(SyntheticKeyEvent::up(key))?;
        self.pressed.pop();
        Ok(())
    }

    fn finish(mut self) -> Result<(), ()> {
        while let Some(key) = self.pressed.last().copied() {
            self.backend.send(SyntheticKeyEvent::up(key))?;
            self.pressed.pop();
        }
        Ok(())
    }
}

impl<B: ShortcutBackend> Drop for PressedKeyGuard<'_, B> {
    fn drop(&mut self) {
        let mut cleanup_failures = 0_usize;
        while let Some(key) = self.pressed.pop() {
            if self.backend.send(SyntheticKeyEvent::up(key)).is_err() {
                cleanup_failures = cleanup_failures.saturating_add(1);
            }
        }
        if cleanup_failures != 0 {
            log::error!(
                target: "actions",
                "component=actions event_code=shortcut_cleanup_failed result_code=failure \
                 error_code=input_injection_failed config_revision={} \
                 failed_keyup_count={cleanup_failures}",
                self.config_revision
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct WindowsShortcutBackend;

impl ShortcutBackend for WindowsShortcutBackend {
    fn validate_target(
        &mut self,
        target: &InvocationContext,
    ) -> Result<(), TargetValidationFailure> {
        action_target::validate(target)
    }

    fn modifier_is_down(&self, modifier: ShortcutModifier) -> bool {
        let virtual_key = modifier_virtual_key(modifier);
        // SAFETY: GetAsyncKeyState has no memory-safety preconditions.
        unsafe { GetAsyncKeyState(i32::from(virtual_key.0)) < 0 }
    }

    fn send(&mut self, event: SyntheticKeyEvent) -> Result<(), ()> {
        let mut flags = KEYEVENTF_SCANCODE;
        if event.key.extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if event.key_up {
            flags |= KEYEVENTF_KEYUP;
        }
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: event.key.scan_code,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: event.marker,
                },
            },
        };
        let input_size = i32::try_from(size_of::<INPUT>()).map_err(|_| ())?;
        // SAFETY: the input slice is fully initialized and live for the
        // duration of this synchronous call.
        if unsafe { SendInput(&[input], input_size) } == 1 { Ok(()) } else { Err(()) }
    }
}

const fn modifier_virtual_key(modifier: ShortcutModifier) -> VIRTUAL_KEY {
    match modifier {
        ShortcutModifier::LeftControl => VK_LCONTROL,
        ShortcutModifier::RightControl => VK_RCONTROL,
        ShortcutModifier::LeftAlt => VK_LMENU,
        ShortcutModifier::RightAlt => VK_RMENU,
        ShortcutModifier::LeftShift => VK_LSHIFT,
        ShortcutModifier::RightShift => VK_RSHIFT,
        ShortcutModifier::LeftWindows => VK_LWIN,
        ShortcutModifier::RightWindows => VK_RWIN,
    }
}

const fn modifier_key(modifier: ShortcutModifier) -> SyntheticKey {
    match modifier {
        ShortcutModifier::LeftControl => SyntheticKey { scan_code: 0x1D, extended: false },
        ShortcutModifier::RightControl => SyntheticKey { scan_code: 0x1D, extended: true },
        ShortcutModifier::LeftAlt => SyntheticKey { scan_code: 0x38, extended: false },
        ShortcutModifier::RightAlt => SyntheticKey { scan_code: 0x38, extended: true },
        ShortcutModifier::LeftShift => SyntheticKey { scan_code: 0x2A, extended: false },
        ShortcutModifier::RightShift => SyntheticKey { scan_code: 0x36, extended: false },
        ShortcutModifier::LeftWindows => SyntheticKey { scan_code: 0x5B, extended: true },
        ShortcutModifier::RightWindows => SyntheticKey { scan_code: 0x5C, extended: true },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use gradial_core::{
        ActionErrorCode, ActionResult, InvocationContext, ShortcutKey, ShortcutModifier,
    };

    use super::{
        GRADIAL_INJECTED_MARKER, ShortcutBackend, SyntheticKey, SyntheticKeyEvent,
        TargetValidationFailure, execute_with_backend, modifier_key,
    };

    #[derive(Debug)]
    struct FakeBackend {
        validation: Result<(), TargetValidationFailure>,
        held_modifier: Option<ShortcutModifier>,
        events: Vec<SyntheticKeyEvent>,
        down: Vec<SyntheticKey>,
        send_attempt: usize,
        fail_once_at: Option<usize>,
        cancel_after_successful_send: Option<usize>,
        cancellation: Arc<AtomicBool>,
    }

    impl FakeBackend {
        fn new(cancellation: Arc<AtomicBool>) -> Self {
            Self {
                validation: Ok(()),
                held_modifier: None,
                events: Vec::new(),
                down: Vec::new(),
                send_attempt: 0,
                fail_once_at: None,
                cancel_after_successful_send: None,
                cancellation,
            }
        }
    }

    impl ShortcutBackend for FakeBackend {
        fn validate_target(
            &mut self,
            _target: &InvocationContext,
        ) -> Result<(), TargetValidationFailure> {
            self.validation
        }

        fn modifier_is_down(&self, modifier: ShortcutModifier) -> bool {
            self.held_modifier == Some(modifier)
        }

        fn send(&mut self, event: SyntheticKeyEvent) -> Result<(), ()> {
            let attempt = self.send_attempt;
            self.send_attempt = self.send_attempt.saturating_add(1);
            if self.fail_once_at == Some(attempt) {
                self.fail_once_at = None;
                return Err(());
            }
            if event.key_up {
                if let Some(index) = self.down.iter().rposition(|key| *key == event.key) {
                    self.down.remove(index);
                }
            } else {
                self.down.push(event.key);
            }
            self.events.push(event);
            if self.cancel_after_successful_send == Some(self.events.len()) {
                self.cancellation.store(true, Ordering::Release);
            }
            Ok(())
        }
    }

    fn execute(backend: &mut FakeBackend, cancellation: &AtomicBool) -> ActionResult {
        execute_with_backend(
            backend,
            &InvocationContext::default(),
            &[ShortcutModifier::LeftControl],
            ShortcutKey { scan_code: 0x64, extended: false },
            cancellation,
            &AtomicBool::new(false),
        )
    }

    #[test]
    fn shortcut_uses_scan_codes_marker_and_reverse_cleanup() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut backend = FakeBackend::new(Arc::clone(&cancellation));

        assert_eq!(execute(&mut backend, &cancellation), ActionResult::Success);
        assert!(backend.down.is_empty());
        assert_eq!(
            backend.events,
            vec![
                SyntheticKeyEvent::down(SyntheticKey { scan_code: 0x1D, extended: false }),
                SyntheticKeyEvent::down(SyntheticKey { scan_code: 0x64, extended: false }),
                SyntheticKeyEvent::up(SyntheticKey { scan_code: 0x64, extended: false }),
                SyntheticKeyEvent::up(SyntheticKey { scan_code: 0x1D, extended: false }),
            ]
        );
        assert!(backend.events.iter().all(|event| event.marker == GRADIAL_INJECTED_MARKER));
    }

    #[test]
    fn a_physically_held_modifier_is_never_synthesized_or_released() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut backend = FakeBackend::new(Arc::clone(&cancellation));
        backend.held_modifier = Some(ShortcutModifier::LeftControl);

        assert_eq!(execute(&mut backend, &cancellation), ActionResult::Success);
        assert_eq!(backend.events.len(), 2);
        assert!(backend.events.iter().all(|event| event.key.scan_code == 0x64));
        assert!(backend.down.is_empty());
    }

    #[test]
    fn every_left_right_modifier_has_the_required_scan_identity() {
        let cases = [
            (ShortcutModifier::LeftControl, 0x1D, false),
            (ShortcutModifier::RightControl, 0x1D, true),
            (ShortcutModifier::LeftAlt, 0x38, false),
            (ShortcutModifier::RightAlt, 0x38, true),
            (ShortcutModifier::LeftShift, 0x2A, false),
            (ShortcutModifier::RightShift, 0x36, false),
            (ShortcutModifier::LeftWindows, 0x5B, true),
            (ShortcutModifier::RightWindows, 0x5C, true),
        ];
        for (modifier, scan_code, extended) in cases {
            assert_eq!(modifier_key(modifier), SyntheticKey { scan_code, extended });
        }
    }

    #[test]
    fn configured_extended_flag_is_applied_to_the_primary_key() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut backend = FakeBackend::new(Arc::clone(&cancellation));

        assert_eq!(
            execute_with_backend(
                &mut backend,
                &InvocationContext::default(),
                &[],
                ShortcutKey { scan_code: 0x48, extended: true },
                &cancellation,
                &AtomicBool::new(false),
            ),
            ActionResult::Success
        );
        assert_eq!(backend.events.len(), 2);
        assert!(backend.events.iter().all(|event| event.key.extended));
    }

    #[test]
    fn every_injection_failure_runs_best_effort_keyup_cleanup() {
        for failure_index in 0..4 {
            let cancellation = Arc::new(AtomicBool::new(false));
            let mut backend = FakeBackend::new(Arc::clone(&cancellation));
            backend.fail_once_at = Some(failure_index);

            assert!(matches!(
                execute(&mut backend, &cancellation),
                ActionResult::Failure { code: ActionErrorCode::InputInjectionFailed, .. }
            ));
            assert!(backend.down.is_empty(), "failure index {failure_index} left a key down");
        }
    }

    #[test]
    fn cancellation_between_primary_keydown_and_keyup_cleans_every_key() {
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut backend = FakeBackend::new(Arc::clone(&cancellation));
        backend.cancel_after_successful_send = Some(2);

        assert_eq!(execute(&mut backend, &cancellation), ActionResult::Cancelled);
        assert!(backend.down.is_empty());
        assert!(backend.events[2].key_up);
        assert!(backend.events[3].key_up);
    }

    #[test]
    fn target_validation_failure_prevents_all_input() {
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
                execute(&mut backend, &cancellation),
                ActionResult::Failure { code, .. } if code == expected_code
            ));
            assert!(backend.events.is_empty());
        }
    }
}
