//! Runtime-neutral payloads accepted by Gradial capability brokers.

use std::{fmt, str::FromStr};

use thiserror::Error;

use crate::{ShortcutKey, ShortcutModifier};

pub const MAX_PLUGIN_SHORTCUT_CHORD_BYTES: usize = 128;

/// One complete keyboard chord. It can never represent a held key or an
/// arbitrary input event sequence.
#[derive(Clone, Eq, PartialEq)]
pub struct PluginShortcutChord {
    modifiers: Vec<ShortcutModifier>,
    key: ShortcutKey,
}

impl PluginShortcutChord {
    #[must_use]
    pub fn modifiers(&self) -> &[ShortcutModifier] {
        &self.modifiers
    }

    #[must_use]
    pub const fn key(&self) -> ShortcutKey {
        self.key
    }
}

impl fmt::Debug for PluginShortcutChord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginShortcutChord")
            .field("modifier_count", &self.modifiers.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PluginShortcutParseError {
    #[error("plugin_shortcut_too_long")]
    TooLong,
    #[error("plugin_shortcut_empty")]
    Empty,
    #[error("plugin_shortcut_token_invalid")]
    InvalidToken,
    #[error("plugin_shortcut_modifier_duplicate")]
    DuplicateModifier,
    #[error("plugin_shortcut_primary_key_duplicate")]
    MultiplePrimaryKeys,
    #[error("plugin_shortcut_primary_key_missing")]
    MissingPrimaryKey,
}

impl FromStr for PluginShortcutChord {
    type Err = PluginShortcutParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            return Err(PluginShortcutParseError::Empty);
        }
        if value.len() > MAX_PLUGIN_SHORTCUT_CHORD_BYTES {
            return Err(PluginShortcutParseError::TooLong);
        }

        let mut modifiers = Vec::new();
        let mut key = None;
        for token in value.split('+') {
            if token.is_empty() {
                return Err(PluginShortcutParseError::InvalidToken);
            }
            if let Some(modifier) = modifier_for_name(token) {
                if key.is_some() {
                    return Err(PluginShortcutParseError::MultiplePrimaryKeys);
                }
                if modifiers.contains(&modifier) {
                    return Err(PluginShortcutParseError::DuplicateModifier);
                }
                modifiers.push(modifier);
                continue;
            }
            let Some(primary) = primary_key_for_name(token) else {
                return Err(PluginShortcutParseError::InvalidToken);
            };
            if key.replace(primary).is_some() {
                return Err(PluginShortcutParseError::MultiplePrimaryKeys);
            }
        }
        let key = key.ok_or(PluginShortcutParseError::MissingPrimaryKey)?;
        Ok(Self { modifiers, key })
    }
}

const fn modifier_for_name(value: &str) -> Option<ShortcutModifier> {
    match value.as_bytes() {
        b"left_control" => Some(ShortcutModifier::LeftControl),
        b"right_control" => Some(ShortcutModifier::RightControl),
        b"left_alt" => Some(ShortcutModifier::LeftAlt),
        b"right_alt" => Some(ShortcutModifier::RightAlt),
        b"left_shift" => Some(ShortcutModifier::LeftShift),
        b"right_shift" => Some(ShortcutModifier::RightShift),
        b"left_windows" => Some(ShortcutModifier::LeftWindows),
        b"right_windows" => Some(ShortcutModifier::RightWindows),
        _ => None,
    }
}

const fn primary_key_for_name(value: &str) -> Option<ShortcutKey> {
    let (scan_code, extended) = match value.as_bytes() {
        b"escape" => (0x01, false),
        b"1" => (0x02, false),
        b"2" => (0x03, false),
        b"3" => (0x04, false),
        b"4" => (0x05, false),
        b"5" => (0x06, false),
        b"6" => (0x07, false),
        b"7" => (0x08, false),
        b"8" => (0x09, false),
        b"9" => (0x0A, false),
        b"0" => (0x0B, false),
        b"minus" => (0x0C, false),
        b"equals" => (0x0D, false),
        b"backspace" => (0x0E, false),
        b"tab" => (0x0F, false),
        b"q" => (0x10, false),
        b"w" => (0x11, false),
        b"e" => (0x12, false),
        b"r" => (0x13, false),
        b"t" => (0x14, false),
        b"y" => (0x15, false),
        b"u" => (0x16, false),
        b"i" => (0x17, false),
        b"o" => (0x18, false),
        b"p" => (0x19, false),
        b"open_bracket" => (0x1A, false),
        b"close_bracket" => (0x1B, false),
        b"enter" => (0x1C, false),
        b"a" => (0x1E, false),
        b"s" => (0x1F, false),
        b"d" => (0x20, false),
        b"f" => (0x21, false),
        b"g" => (0x22, false),
        b"h" => (0x23, false),
        b"j" => (0x24, false),
        b"k" => (0x25, false),
        b"l" => (0x26, false),
        b"semicolon" => (0x27, false),
        b"quote" => (0x28, false),
        b"backtick" => (0x29, false),
        b"backslash" => (0x2B, false),
        b"z" => (0x2C, false),
        b"x" => (0x2D, false),
        b"c" => (0x2E, false),
        b"v" => (0x2F, false),
        b"b" => (0x30, false),
        b"n" => (0x31, false),
        b"m" => (0x32, false),
        b"comma" => (0x33, false),
        b"period" => (0x34, false),
        b"slash" => (0x35, false),
        b"space" => (0x39, false),
        b"f1" => (0x3B, false),
        b"f2" => (0x3C, false),
        b"f3" => (0x3D, false),
        b"f4" => (0x3E, false),
        b"f5" => (0x3F, false),
        b"f6" => (0x40, false),
        b"f7" => (0x41, false),
        b"f8" => (0x42, false),
        b"f9" => (0x43, false),
        b"f10" => (0x44, false),
        b"f11" => (0x57, false),
        b"f12" => (0x58, false),
        b"f13" => (0x64, false),
        b"f14" => (0x65, false),
        b"f15" => (0x66, false),
        b"f16" => (0x67, false),
        b"f17" => (0x68, false),
        b"f18" => (0x69, false),
        b"f19" => (0x6A, false),
        b"f20" => (0x6B, false),
        b"f21" => (0x6C, false),
        b"f22" => (0x6D, false),
        b"f23" => (0x6E, false),
        b"f24" => (0x76, false),
        b"insert" => (0x52, true),
        b"delete" => (0x53, true),
        b"home" => (0x47, true),
        b"end" => (0x4F, true),
        b"page_up" => (0x49, true),
        b"page_down" => (0x51, true),
        b"up" => (0x48, true),
        b"down" => (0x50, true),
        b"left" => (0x4B, true),
        b"right" => (0x4D, true),
        b"browser_back" => (0x6A, true),
        b"international_backslash" => (0x56, false),
        b"numpad_plus" => (0x4E, false),
        _ => return None,
    };
    Some(ShortcutKey { scan_code, extended })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_chords_preserve_side_specific_modifiers_and_extended_keys() {
        let chord: PluginShortcutChord =
            "left_control+right_shift+right".parse().expect("shortcut");
        assert_eq!(
            chord.modifiers(),
            &[ShortcutModifier::LeftControl, ShortcutModifier::RightShift]
        );
        assert_eq!(chord.key(), ShortcutKey { scan_code: 0x4D, extended: true });
        assert!(!format!("{chord:?}").contains("right"));
    }

    #[test]
    fn every_supported_key_family_has_a_stable_name() {
        for (name, expected) in [
            ("a", ShortcutKey { scan_code: 0x1E, extended: false }),
            ("1", ShortcutKey { scan_code: 0x02, extended: false }),
            ("f24", ShortcutKey { scan_code: 0x76, extended: false }),
            ("page_down", ShortcutKey { scan_code: 0x51, extended: true }),
            ("browser_back", ShortcutKey { scan_code: 0x6A, extended: true }),
            ("numpad_plus", ShortcutKey { scan_code: 0x4E, extended: false }),
        ] {
            let chord: PluginShortcutChord = name.parse().expect("supported key");
            assert_eq!(chord.key(), expected);
        }
    }

    #[test]
    fn malformed_or_ambiguous_chords_fail_closed() {
        assert_eq!("".parse::<PluginShortcutChord>(), Err(PluginShortcutParseError::Empty));
        assert_eq!(
            "left_control+left_control+a".parse::<PluginShortcutChord>(),
            Err(PluginShortcutParseError::DuplicateModifier)
        );
        assert_eq!(
            "left_control".parse::<PluginShortcutChord>(),
            Err(PluginShortcutParseError::MissingPrimaryKey)
        );
        for value in ["Control+a", "left_control +a", "a+b", "a+left_shift", "a+"] {
            assert!(value.parse::<PluginShortcutChord>().is_err(), "accepted {value}");
        }
        assert_eq!(
            "a".repeat(MAX_PLUGIN_SHORTCUT_CHORD_BYTES + 1).parse::<PluginShortcutChord>(),
            Err(PluginShortcutParseError::TooLong)
        );
    }
}
