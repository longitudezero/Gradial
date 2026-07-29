use std::collections::BTreeSet;

use eframe::egui::{self, Event, Key, Modifiers};
use gradial_core::{ShortcutKey, ShortcutModifier};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecordedShortcut {
    pub(crate) modifiers: Vec<ShortcutModifier>,
    pub(crate) key: ShortcutKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CaptureOutcome {
    Captured(RecordedShortcut),
    Cancelled,
}

#[derive(Debug, Default)]
pub(crate) struct ShortcutRecorder {
    recording: bool,
    skip_events_once: bool,
    held_modifiers: BTreeSet<ShortcutModifier>,
    error: Option<&'static str>,
}

impl ShortcutRecorder {
    pub(crate) fn begin(&mut self) {
        self.recording = true;
        self.skip_events_once = true;
        self.held_modifiers.clear();
        self.error = None;
    }

    pub(crate) fn cancel(&mut self) {
        self.recording = false;
        self.skip_events_once = false;
        self.held_modifiers.clear();
        self.error = None;
    }

    pub(crate) const fn is_recording(&self) -> bool {
        self.recording
    }

    pub(crate) const fn error(&self) -> Option<&'static str> {
        self.error
    }

    pub(crate) fn update(&mut self, context: &egui::Context) -> Option<CaptureOutcome> {
        let events = context.input(|input| input.events.clone());
        self.process_events(&events)
    }

    fn process_events(&mut self, events: &[Event]) -> Option<CaptureOutcome> {
        if !self.recording {
            return None;
        }
        if self.skip_events_once {
            self.skip_events_once = false;
            return None;
        }

        for event in events {
            let Event::Key { key, physical_key, pressed, repeat, modifiers } = event else {
                continue;
            };
            let physical_key = physical_key.unwrap_or(*key);
            if let Some(modifier) = modifier_from_key(physical_key) {
                if *pressed {
                    self.held_modifiers.insert(modifier);
                } else {
                    self.held_modifiers.remove(&modifier);
                }
                continue;
            }
            if !pressed || *repeat {
                continue;
            }
            if *key == Key::Escape {
                self.cancel();
                return Some(CaptureOutcome::Cancelled);
            }
            let Some(key) = shortcut_key_from_egui(physical_key) else {
                self.error = Some(
                    "That key cannot be recorded. Try a letter, number, navigation key, or F1–F24.",
                );
                continue;
            };

            add_fallback_modifiers(&mut self.held_modifiers, *modifiers);
            let recorded =
                RecordedShortcut { modifiers: self.held_modifiers.iter().copied().collect(), key };
            self.cancel();
            return Some(CaptureOutcome::Captured(recorded));
        }
        None
    }
}

fn add_fallback_modifiers(held_modifiers: &mut BTreeSet<ShortcutModifier>, modifiers: Modifiers) {
    if modifiers.ctrl
        && !held_modifiers.contains(&ShortcutModifier::LeftControl)
        && !held_modifiers.contains(&ShortcutModifier::RightControl)
    {
        held_modifiers.insert(ShortcutModifier::LeftControl);
    }
    if modifiers.alt
        && !held_modifiers.contains(&ShortcutModifier::LeftAlt)
        && !held_modifiers.contains(&ShortcutModifier::RightAlt)
    {
        held_modifiers.insert(ShortcutModifier::LeftAlt);
    }
    if modifiers.shift
        && !held_modifiers.contains(&ShortcutModifier::LeftShift)
        && !held_modifiers.contains(&ShortcutModifier::RightShift)
    {
        held_modifiers.insert(ShortcutModifier::LeftShift);
    }
}

const fn modifier_from_key(key: Key) -> Option<ShortcutModifier> {
    match key {
        Key::ControlLeft => Some(ShortcutModifier::LeftControl),
        Key::ControlRight => Some(ShortcutModifier::RightControl),
        Key::AltLeft => Some(ShortcutModifier::LeftAlt),
        Key::AltRight => Some(ShortcutModifier::RightAlt),
        Key::ShiftLeft => Some(ShortcutModifier::LeftShift),
        Key::ShiftRight => Some(ShortcutModifier::RightShift),
        Key::SuperLeft => Some(ShortcutModifier::LeftWindows),
        Key::SuperRight => Some(ShortcutModifier::RightWindows),
        _ => None,
    }
}

const fn shortcut_key_from_egui(key: Key) -> Option<ShortcutKey> {
    let (scan_code, extended) = match key {
        Key::Escape => (0x01, false),
        Key::Num1 => (0x02, false),
        Key::Num2 => (0x03, false),
        Key::Num3 => (0x04, false),
        Key::Num4 => (0x05, false),
        Key::Num5 => (0x06, false),
        Key::Num6 => (0x07, false),
        Key::Num7 => (0x08, false),
        Key::Num8 => (0x09, false),
        Key::Num9 => (0x0A, false),
        Key::Num0 => (0x0B, false),
        Key::Minus => (0x0C, false),
        Key::Equals => (0x0D, false),
        Key::Backspace => (0x0E, false),
        Key::Tab => (0x0F, false),
        Key::Q => (0x10, false),
        Key::W => (0x11, false),
        Key::E => (0x12, false),
        Key::R => (0x13, false),
        Key::T => (0x14, false),
        Key::Y => (0x15, false),
        Key::U => (0x16, false),
        Key::I => (0x17, false),
        Key::O => (0x18, false),
        Key::P => (0x19, false),
        Key::OpenBracket => (0x1A, false),
        Key::CloseBracket => (0x1B, false),
        Key::Enter => (0x1C, false),
        Key::A => (0x1E, false),
        Key::S => (0x1F, false),
        Key::D => (0x20, false),
        Key::F => (0x21, false),
        Key::G => (0x22, false),
        Key::H => (0x23, false),
        Key::J => (0x24, false),
        Key::K => (0x25, false),
        Key::L => (0x26, false),
        Key::Semicolon => (0x27, false),
        Key::Quote => (0x28, false),
        Key::Backtick => (0x29, false),
        Key::Backslash => (0x2B, false),
        Key::Z => (0x2C, false),
        Key::X => (0x2D, false),
        Key::C => (0x2E, false),
        Key::V => (0x2F, false),
        Key::B => (0x30, false),
        Key::N => (0x31, false),
        Key::M => (0x32, false),
        Key::Comma => (0x33, false),
        Key::Period => (0x34, false),
        Key::Slash => (0x35, false),
        Key::Space => (0x39, false),
        Key::F1 => (0x3B, false),
        Key::F2 => (0x3C, false),
        Key::F3 => (0x3D, false),
        Key::F4 => (0x3E, false),
        Key::F5 => (0x3F, false),
        Key::F6 => (0x40, false),
        Key::F7 => (0x41, false),
        Key::F8 => (0x42, false),
        Key::F9 => (0x43, false),
        Key::F10 => (0x44, false),
        Key::F11 => (0x57, false),
        Key::F12 => (0x58, false),
        Key::F13 => (0x64, false),
        Key::F14 => (0x65, false),
        Key::F15 => (0x66, false),
        Key::F16 => (0x67, false),
        Key::F17 => (0x68, false),
        Key::F18 => (0x69, false),
        Key::F19 => (0x6A, false),
        Key::F20 => (0x6B, false),
        Key::F21 => (0x6C, false),
        Key::F22 => (0x6D, false),
        Key::F23 => (0x6E, false),
        Key::F24 => (0x76, false),
        Key::Insert => (0x52, true),
        Key::Delete => (0x53, true),
        Key::Home => (0x47, true),
        Key::End => (0x4F, true),
        Key::PageUp => (0x49, true),
        Key::PageDown => (0x51, true),
        Key::ArrowUp => (0x48, true),
        Key::ArrowDown => (0x50, true),
        Key::ArrowLeft => (0x4B, true),
        Key::ArrowRight => (0x4D, true),
        Key::BrowserBack => (0x6A, true),
        Key::IntlBackslash => (0x56, false),
        Key::Plus => (0x4E, false),
        _ => return None,
    };
    Some(ShortcutKey { scan_code, extended })
}

pub(crate) fn shortcut_display(modifiers: &[ShortcutModifier], key: ShortcutKey) -> String {
    let mut parts =
        modifiers.iter().map(|modifier| modifier_name(*modifier).to_owned()).collect::<Vec<_>>();
    parts.push(shortcut_key_name(key));
    parts.join(" + ")
}

const fn modifier_name(modifier: ShortcutModifier) -> &'static str {
    match modifier {
        ShortcutModifier::LeftControl => "Left Ctrl",
        ShortcutModifier::RightControl => "Right Ctrl",
        ShortcutModifier::LeftAlt => "Left Alt",
        ShortcutModifier::RightAlt => "Right Alt",
        ShortcutModifier::LeftShift => "Left Shift",
        ShortcutModifier::RightShift => "Right Shift",
        ShortcutModifier::LeftWindows => "Left Win",
        ShortcutModifier::RightWindows => "Right Win",
    }
}

fn shortcut_key_name(key: ShortcutKey) -> String {
    let name = match (key.scan_code, key.extended) {
        (0x01, false) => "Escape",
        (0x02, false) => "1",
        (0x03, false) => "2",
        (0x04, false) => "3",
        (0x05, false) => "4",
        (0x06, false) => "5",
        (0x07, false) => "6",
        (0x08, false) => "7",
        (0x09, false) => "8",
        (0x0A, false) => "9",
        (0x0B, false) => "0",
        (0x0C, false) => "Minus",
        (0x0D, false) => "Equals",
        (0x0E, false) => "Backspace",
        (0x0F, false) => "Tab",
        (0x10, false) => "Q",
        (0x11, false) => "W",
        (0x12, false) => "E",
        (0x13, false) => "R",
        (0x14, false) => "T",
        (0x15, false) => "Y",
        (0x16, false) => "U",
        (0x17, false) => "I",
        (0x18, false) => "O",
        (0x19, false) => "P",
        (0x1A, false) => "Open bracket",
        (0x1B, false) => "Close bracket",
        (0x1C, false) => "Enter",
        (0x1E, false) => "A",
        (0x1F, false) => "S",
        (0x20, false) => "D",
        (0x21, false) => "F",
        (0x22, false) => "G",
        (0x23, false) => "H",
        (0x24, false) => "J",
        (0x25, false) => "K",
        (0x26, false) => "L",
        (0x27, false) => "Semicolon",
        (0x28, false) => "Quote",
        (0x29, false) => "Backtick",
        (0x2B, false) => "Backslash",
        (0x2C, false) => "Z",
        (0x2D, false) => "X",
        (0x2E, false) => "C",
        (0x2F, false) => "V",
        (0x30, false) => "B",
        (0x31, false) => "N",
        (0x32, false) => "M",
        (0x33, false) => "Comma",
        (0x34, false) => "Period",
        (0x35, false) => "Slash",
        (0x39, false) => "Space",
        (0x3B, false) => "F1",
        (0x3C, false) => "F2",
        (0x3D, false) => "F3",
        (0x3E, false) => "F4",
        (0x3F, false) => "F5",
        (0x40, false) => "F6",
        (0x41, false) => "F7",
        (0x42, false) => "F8",
        (0x43, false) => "F9",
        (0x44, false) => "F10",
        (0x57, false) => "F11",
        (0x58, false) => "F12",
        (0x64, false) => "F13",
        (0x65, false) => "F14",
        (0x66, false) => "F15",
        (0x67, false) => "F16",
        (0x68, false) => "F17",
        (0x69, false) => "F18",
        (0x6A, false) => "F19",
        (0x6B, false) => "F20",
        (0x6C, false) => "F21",
        (0x6D, false) => "F22",
        (0x6E, false) => "F23",
        (0x76, false) => "F24",
        (0x52, true) => "Insert",
        (0x53, true) => "Delete",
        (0x47, true) => "Home",
        (0x4F, true) => "End",
        (0x49, true) => "Page Up",
        (0x51, true) => "Page Down",
        (0x48, true) => "Up",
        (0x50, true) => "Down",
        (0x4B, true) => "Left",
        (0x4D, true) => "Right",
        (0x6A, true) => "Browser Back",
        (0x56, false) => "International backslash",
        (0x4E, false) => "Numpad plus",
        _ => {
            return if key.extended {
                format!("Extended scan 0x{:02X}", key.scan_code)
            } else {
                format!("Scan 0x{:02X}", key.scan_code)
            };
        }
    };
    name.to_owned()
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Event, Key, Modifiers};
    use gradial_core::{ShortcutKey, ShortcutModifier};

    use super::{CaptureOutcome, RecordedShortcut, ShortcutRecorder, shortcut_display};

    fn key_event(key: Key, physical_key: Key, pressed: bool, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: Some(physical_key), pressed, repeat: false, modifiers }
    }

    #[test]
    fn recorder_captures_side_specific_modifier_and_f14_scan_code() {
        let mut recorder = ShortcutRecorder::default();
        recorder.begin();
        assert_eq!(recorder.process_events(&[]), None);
        let events = [
            key_event(Key::ControlLeft, Key::ControlLeft, true, Modifiers::CTRL),
            key_event(Key::F14, Key::F14, true, Modifiers::CTRL),
        ];

        assert_eq!(
            recorder.process_events(&events),
            Some(CaptureOutcome::Captured(RecordedShortcut {
                modifiers: vec![ShortcutModifier::LeftControl],
                key: ShortcutKey { scan_code: 0x65, extended: false },
            }))
        );
        assert!(!recorder.is_recording());
    }

    #[test]
    fn recorder_uses_a_left_side_fallback_for_a_modifier_held_before_recording() {
        let mut recorder = ShortcutRecorder::default();
        recorder.begin();
        assert_eq!(recorder.process_events(&[]), None);

        assert_eq!(
            recorder.process_events(&[key_event(Key::A, Key::A, true, Modifiers::CTRL)]),
            Some(CaptureOutcome::Captured(RecordedShortcut {
                modifiers: vec![ShortcutModifier::LeftControl],
                key: ShortcutKey { scan_code: 0x1E, extended: false },
            }))
        );
    }

    #[test]
    fn unsupported_key_keeps_the_recorder_active_with_a_safe_error() {
        let mut recorder = ShortcutRecorder::default();
        recorder.begin();
        assert_eq!(recorder.process_events(&[]), None);

        assert_eq!(
            recorder.process_events(&[key_event(Key::F25, Key::F25, true, Modifiers::NONE)]),
            None
        );
        assert!(recorder.is_recording());
        assert!(recorder.error().is_some());
    }

    #[test]
    fn escape_cancels_recording_without_producing_a_shortcut() {
        let mut recorder = ShortcutRecorder::default();
        recorder.begin();
        assert_eq!(recorder.process_events(&[]), None);

        assert_eq!(
            recorder.process_events(&[key_event(Key::Escape, Key::Escape, true, Modifiers::NONE)]),
            Some(CaptureOutcome::Cancelled)
        );
        assert!(!recorder.is_recording());
    }

    #[test]
    fn shortcut_display_keeps_modifier_side_and_key_name() {
        assert_eq!(
            shortcut_display(
                &[ShortcutModifier::RightAlt, ShortcutModifier::LeftShift],
                ShortcutKey { scan_code: 0x64, extended: false }
            ),
            "Right Alt + Left Shift + F13"
        );
    }
}
