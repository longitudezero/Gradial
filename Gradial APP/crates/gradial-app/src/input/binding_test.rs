use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};

use gradial_core::{ActivationBinding, ActivationBindingId, PhysicalModifiers, ScreenPoint};

const INACTIVE: u8 = 0;
const RESULT_NONE: u8 = 0;
const RESULT_MISMATCH: u8 = 1;
const RESULT_GESTURE: u8 = 2;
const M4_MASK: u8 = 1 << 0;
const M5_MASK: u8 = 1 << 1;

static EXPECTED_CHORD: AtomicU8 = AtomicU8::new(INACTIVE);
static ACTIVE_BUTTON: AtomicU8 = AtomicU8::new(INACTIVE);
static SUPPRESSED_RELEASES: AtomicU8 = AtomicU8::new(0);
static OBSERVED_CHORD: AtomicU8 = AtomicU8::new(INACTIVE);
static RESULT: AtomicU8 = AtomicU8::new(RESULT_NONE);
static ORIGIN_X: AtomicI32 = AtomicI32::new(0);
static ORIGIN_Y: AtomicI32 = AtomicI32::new(0);
static LAST_X: AtomicI32 = AtomicI32::new(0);
static LAST_Y: AtomicI32 = AtomicI32::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActivationBindingTestResult {
    pub(crate) actual: ActivationBinding,
    pub(crate) matched: bool,
    pub(crate) origin: ScreenPoint,
    pub(crate) release: ScreenPoint,
}

pub(crate) fn begin(binding: ActivationBinding) {
    RESULT.store(RESULT_NONE, Ordering::Release);
    OBSERVED_CHORD.store(INACTIVE, Ordering::Release);
    EXPECTED_CHORD.store(encode_chord(binding), Ordering::Release);
}

pub(crate) fn cancel() {
    EXPECTED_CHORD.store(INACTIVE, Ordering::Release);
    ACTIVE_BUTTON.store(INACTIVE, Ordering::Release);
    RESULT.store(RESULT_NONE, Ordering::Release);
    OBSERVED_CHORD.store(INACTIVE, Ordering::Release);
}

pub(crate) fn is_gesture_active() -> bool {
    ACTIVE_BUTTON.load(Ordering::Acquire) != INACTIVE
}

pub(crate) fn take_result() -> Option<ActivationBindingTestResult> {
    let result = RESULT.swap(RESULT_NONE, Ordering::AcqRel);
    if result == RESULT_NONE {
        return None;
    }
    let actual = decode_chord(OBSERVED_CHORD.load(Ordering::Acquire))?;
    Some(ActivationBindingTestResult {
        actual,
        matched: result == RESULT_GESTURE,
        origin: ScreenPoint::new(
            ORIGIN_X.load(Ordering::Acquire),
            ORIGIN_Y.load(Ordering::Acquire),
        ),
        release: ScreenPoint::new(LAST_X.load(Ordering::Acquire), LAST_Y.load(Ordering::Acquire)),
    })
}

pub(super) fn button_down(
    button: ActivationBindingId,
    modifiers: PhysicalModifiers,
    point: ScreenPoint,
) -> bool {
    let mask = button_mask(button);
    if SUPPRESSED_RELEASES.load(Ordering::Acquire) & mask != 0 {
        return true;
    }

    let expected = EXPECTED_CHORD.swap(INACTIVE, Ordering::AcqRel);
    if expected == INACTIVE {
        return false;
    }

    let actual = ActivationBinding { button, modifiers };
    let actual_chord = encode_chord(actual);
    OBSERVED_CHORD.store(actual_chord, Ordering::Release);
    ORIGIN_X.store(point.x, Ordering::Release);
    ORIGIN_Y.store(point.y, Ordering::Release);
    LAST_X.store(point.x, Ordering::Release);
    LAST_Y.store(point.y, Ordering::Release);
    SUPPRESSED_RELEASES.fetch_or(mask, Ordering::AcqRel);

    if actual_chord == expected {
        ACTIVE_BUTTON.store(button_code(button), Ordering::Release);
    } else {
        RESULT.store(RESULT_MISMATCH, Ordering::Release);
    }
    true
}

pub(super) fn pointer_moved(point: ScreenPoint) {
    if ACTIVE_BUTTON.load(Ordering::Acquire) != INACTIVE {
        LAST_X.store(point.x, Ordering::Release);
        LAST_Y.store(point.y, Ordering::Release);
    }
}

pub(super) fn button_up(button: ActivationBindingId, point: ScreenPoint) -> bool {
    let button_code = button_code(button);
    let mask = button_mask(button);
    let active =
        ACTIVE_BUTTON.compare_exchange(button_code, INACTIVE, Ordering::AcqRel, Ordering::Acquire);
    if active.is_ok() {
        LAST_X.store(point.x, Ordering::Release);
        LAST_Y.store(point.y, Ordering::Release);
        SUPPRESSED_RELEASES.fetch_and(!mask, Ordering::AcqRel);
        RESULT.store(RESULT_GESTURE, Ordering::Release);
        return true;
    }

    SUPPRESSED_RELEASES.fetch_and(!mask, Ordering::AcqRel) & mask != 0
}

const fn encode_chord(binding: ActivationBinding) -> u8 {
    button_code(binding.button)
        | ((binding.modifiers.control as u8) << 2)
        | ((binding.modifiers.alt as u8) << 3)
        | ((binding.modifiers.shift as u8) << 4)
}

const fn decode_chord(chord: u8) -> Option<ActivationBinding> {
    let button = match chord & 0b11 {
        1 => ActivationBindingId::MouseButton4,
        2 => ActivationBindingId::MouseButton5,
        _ => return None,
    };
    Some(ActivationBinding {
        button,
        modifiers: PhysicalModifiers {
            control: chord & (1 << 2) != 0,
            alt: chord & (1 << 3) != 0,
            shift: chord & (1 << 4) != 0,
        },
    })
}

const fn button_code(button: ActivationBindingId) -> u8 {
    match button {
        ActivationBindingId::MouseButton4 => 1,
        ActivationBindingId::MouseButton5 => 2,
    }
}

const fn button_mask(button: ActivationBindingId) -> u8 {
    match button {
        ActivationBindingId::MouseButton4 => M4_MASK,
        ActivationBindingId::MouseButton5 => M5_MASK,
    }
}
