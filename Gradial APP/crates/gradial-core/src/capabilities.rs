//! Runtime-neutral payloads accepted by Gradial capability brokers.

use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{ShortcutKey, ShortcutModifier};

pub const MAX_PLUGIN_SHORTCUT_CHORD_BYTES: usize = 128;
pub const MAX_PLUGIN_NOTIFICATION_TEXT_BYTES: usize = 128;
pub const MAX_PLUGIN_LOOPBACK_PATH_BYTES: usize = 2_048;
pub const MAX_PLUGIN_LOOPBACK_BODY_BYTES: usize = 64 * 1_024;

/// One exact, canonical, cleartext HTTP loopback origin. It always includes an
/// explicit nonzero port and can be converted directly to a socket address
/// without DNS or proxy discovery.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PluginLoopbackOrigin {
    text: String,
    socket_addr: SocketAddr,
}

impl PluginLoopbackOrigin {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn socket_addr(&self) -> SocketAddr {
        self.socket_addr
    }
}

impl fmt::Debug for PluginLoopbackOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("PluginLoopbackOrigin").field(&self.text).finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PluginLoopbackOriginParseError {
    #[error("plugin_loopback_origin_invalid")]
    Invalid,
}

impl FromStr for PluginLoopbackOrigin {
    type Err = PluginLoopbackOriginParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let authority =
            value.strip_prefix("http://").ok_or(PluginLoopbackOriginParseError::Invalid)?;
        let socket_addr = if let Some(port) = authority.strip_prefix("[::1]:") {
            let port = parse_canonical_port(port)?;
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port)
        } else if let Some(port) = authority.strip_prefix("127.0.0.1:") {
            let port = parse_canonical_port(port)?;
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
        } else {
            return Err(PluginLoopbackOriginParseError::Invalid);
        };
        let canonical = match socket_addr {
            SocketAddr::V4(address) => format!("http://127.0.0.1:{}", address.port()),
            SocketAddr::V6(address) => format!("http://[::1]:{}", address.port()),
        };
        if value != canonical {
            return Err(PluginLoopbackOriginParseError::Invalid);
        }
        Ok(Self { text: canonical, socket_addr })
    }
}

fn parse_canonical_port(value: &str) -> Result<u16, PluginLoopbackOriginParseError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(PluginLoopbackOriginParseError::Invalid);
    }
    let port = value.parse::<u16>().map_err(|_| PluginLoopbackOriginParseError::Invalid)?;
    if port == 0 || value != port.to_string() {
        return Err(PluginLoopbackOriginParseError::Invalid);
    }
    Ok(port)
}

impl Serialize for PluginLoopbackOrigin {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PluginLoopbackOrigin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?.parse().map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum PluginLoopbackMethod {
    #[serde(rename = "GET")]
    Get,
    #[serde(rename = "POST")]
    Post,
}

impl PluginLoopbackMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

impl FromStr for PluginLoopbackMethod {
    type Err = PluginLoopbackRequestParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "GET" => Ok(Self::Get),
            "POST" => Ok(Self::Post),
            _ => Err(PluginLoopbackRequestParseError::Method),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginLoopbackGrant {
    pub origin: PluginLoopbackOrigin,
    pub methods: Vec<PluginLoopbackMethod>,
}

impl PluginLoopbackGrant {
    #[must_use]
    pub fn permits(&self, request: &PluginLoopbackRequest) -> bool {
        self.origin == request.origin && self.methods.contains(&request.method)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginLoopbackContentType {
    Binary,
    Json,
    Form,
    Text,
}

impl PluginLoopbackContentType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Binary => "application/octet-stream",
            Self::Json => "application/json",
            Self::Form => "application/x-www-form-urlencoded",
            Self::Text => "text/plain; charset=utf-8",
        }
    }
}

impl FromStr for PluginLoopbackContentType {
    type Err = PluginLoopbackRequestParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "application/octet-stream" => Ok(Self::Binary),
            "application/json" => Ok(Self::Json),
            "application/x-www-form-urlencoded" => Ok(Self::Form),
            "text/plain; charset=utf-8" => Ok(Self::Text),
            _ => Err(PluginLoopbackRequestParseError::ContentType),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct PluginLoopbackRequest {
    method: PluginLoopbackMethod,
    origin: PluginLoopbackOrigin,
    path: String,
    body: Vec<u8>,
    content_type: PluginLoopbackContentType,
}

impl PluginLoopbackRequest {
    /// Validates a complete request before it reaches a capability broker.
    ///
    /// # Errors
    ///
    /// Returns a bounded parse error for an unsafe request target, an
    /// oversized body, or a GET request carrying a body.
    pub fn new(
        method: PluginLoopbackMethod,
        origin: PluginLoopbackOrigin,
        path: impl Into<String>,
        body: Vec<u8>,
        content_type: PluginLoopbackContentType,
    ) -> Result<Self, PluginLoopbackRequestParseError> {
        let path = path.into();
        if path.is_empty()
            || path.len() > MAX_PLUGIN_LOOPBACK_PATH_BYTES
            || !path.is_ascii()
            || !path.starts_with('/')
            || path.starts_with("//")
            || path.bytes().any(|byte| byte.is_ascii_control() || byte == b' ' || byte == b'\\')
            || path.contains('#')
        {
            return Err(PluginLoopbackRequestParseError::Path);
        }
        if body.len() > MAX_PLUGIN_LOOPBACK_BODY_BYTES {
            return Err(PluginLoopbackRequestParseError::BodyTooLarge);
        }
        if method == PluginLoopbackMethod::Get && !body.is_empty() {
            return Err(PluginLoopbackRequestParseError::GetBody);
        }
        Ok(Self { method, origin, path, body, content_type })
    }

    #[must_use]
    pub const fn method(&self) -> PluginLoopbackMethod {
        self.method
    }

    #[must_use]
    pub const fn origin(&self) -> &PluginLoopbackOrigin {
        &self.origin
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    #[must_use]
    pub const fn content_type(&self) -> PluginLoopbackContentType {
        self.content_type
    }
}

impl fmt::Debug for PluginLoopbackRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginLoopbackRequest")
            .field("method", &self.method)
            .field("origin", &self.origin)
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PluginLoopbackRequestParseError {
    #[error("plugin_loopback_method_invalid")]
    Method,
    #[error("plugin_loopback_path_invalid")]
    Path,
    #[error("plugin_loopback_body_too_large")]
    BodyTooLarge,
    #[error("plugin_loopback_get_body_invalid")]
    GetBody,
    #[error("plugin_loopback_content_type_invalid")]
    ContentType,
}

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

/// One bounded single-line notification body. Gradial treats it only as
/// literal text and rejects URL-shaped content before it reaches a broker.
#[derive(Clone, Eq, PartialEq)]
pub struct PluginNotificationText(String);

impl PluginNotificationText {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PluginNotificationText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("PluginNotificationText").finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PluginNotificationParseError {
    #[error("plugin_notification_empty")]
    Empty,
    #[error("plugin_notification_too_long")]
    TooLong,
    #[error("plugin_notification_control_character")]
    ControlCharacter,
    #[error("plugin_notification_url_disallowed")]
    UrlDisallowed,
}

impl FromStr for PluginNotificationText {
    type Err = PluginNotificationParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.trim().is_empty() {
            return Err(PluginNotificationParseError::Empty);
        }
        if value.len() > MAX_PLUGIN_NOTIFICATION_TEXT_BYTES {
            return Err(PluginNotificationParseError::TooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(PluginNotificationParseError::ControlCharacter);
        }
        if notification_contains_url(value) {
            return Err(PluginNotificationParseError::UrlDisallowed);
        }
        Ok(Self(value.to_owned()))
    }
}

fn notification_contains_url(value: &str) -> bool {
    let lowercase = value.to_ascii_lowercase();
    if ["http:", "https:", "ftp:", "file:", "mailto:", "www.", "://", ":\\", "\\\\"]
        .iter()
        .any(|marker| lowercase.contains(marker))
        || lowercase.contains('@')
        || lowercase.contains("::")
    {
        return true;
    }
    lowercase.split_whitespace().any(|token| {
        let candidate = token.trim_matches(|character: char| {
            matches!(
                character,
                ',' | ';' | '!' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\''
            )
        });
        let host =
            candidate.split(['/', '?', '#']).next().unwrap_or_default().trim_end_matches('.');
        if host.eq_ignore_ascii_case("localhost") || host.starts_with("localhost:") {
            return true;
        }
        let host = host
            .rsplit_once(':')
            .filter(|(_, port)| !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()))
            .map_or(host, |(host, _)| host);
        if host.parse::<IpAddr>().is_ok() {
            return true;
        }
        let mut labels = host.split('.');
        let Some(first) = labels.next() else {
            return false;
        };
        let remaining = labels.collect::<Vec<_>>();
        let Some(last) = remaining.last() else {
            return false;
        };
        domain_label_is_valid(first)
            && remaining.iter().all(|label| domain_label_is_valid(label))
            && last.len() >= 2
            && last.bytes().all(|byte| byte.is_ascii_alphabetic())
    })
}

fn domain_label_is_valid(label: &str) -> bool {
    !label.is_empty()
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
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

    #[test]
    fn notification_text_is_bounded_literal_and_redacted() {
        let text: PluginNotificationText = "Export complete: 50% processed.".parse().expect("text");
        assert_eq!(text.as_str(), "Export complete: 50% processed.");
        assert!(!format!("{text:?}").contains("Export"));
        assert!("✓ Complete".parse::<PluginNotificationText>().is_ok());
    }

    #[test]
    fn notification_text_rejects_empty_control_and_oversized_values() {
        assert_eq!(
            "   ".parse::<PluginNotificationText>(),
            Err(PluginNotificationParseError::Empty)
        );
        assert_eq!(
            "line one\nline two".parse::<PluginNotificationText>(),
            Err(PluginNotificationParseError::ControlCharacter)
        );
        assert_eq!(
            "x".repeat(MAX_PLUGIN_NOTIFICATION_TEXT_BYTES + 1).parse::<PluginNotificationText>(),
            Err(PluginNotificationParseError::TooLong)
        );
    }

    #[test]
    fn notification_text_rejects_url_shaped_content_case_insensitively() {
        for value in [
            "Open HTTPS://example.test",
            "Visit www.example.test",
            "Send MAILTO:user@example.test",
            "custom://destination",
            "Visit example.test/path",
            "Contact user@example.test",
            "Connect to 127.0.0.1",
            "Connect to localhost:8080",
            r"Open C:\private\file.txt",
        ] {
            assert_eq!(
                value.parse::<PluginNotificationText>(),
                Err(PluginNotificationParseError::UrlDisallowed)
            );
        }
    }

    #[test]
    fn loopback_origins_are_canonical_literal_socket_addresses() {
        let ipv4: PluginLoopbackOrigin = "http://127.0.0.1:8080".parse().expect("IPv4");
        assert_eq!(ipv4.as_str(), "http://127.0.0.1:8080");
        assert_eq!(ipv4.socket_addr(), "127.0.0.1:8080".parse().expect("socket"));
        let ipv6: PluginLoopbackOrigin = "http://[::1]:65535".parse().expect("IPv6");
        assert_eq!(ipv6.socket_addr(), "[::1]:65535".parse().expect("socket"));

        for rejected in [
            "https://127.0.0.1:8080",
            "http://localhost:8080",
            "http://127.0.0.2:8080",
            "http://0.0.0.0:8080",
            "http://127.0.0.1",
            "http://127.0.0.1:0",
            "http://127.0.0.1:08080",
            "http://127.0.0.1:8080/path",
            "http://user@127.0.0.1:8080",
        ] {
            assert!(rejected.parse::<PluginLoopbackOrigin>().is_err(), "accepted {rejected}");
        }
    }

    #[test]
    fn loopback_request_is_bounded_and_debug_redacts_path_and_body() {
        let origin = "http://127.0.0.1:8080".parse().expect("origin");
        let request = PluginLoopbackRequest::new(
            PluginLoopbackMethod::Post,
            origin,
            "/private?token=secret",
            b"private-body".to_vec(),
            PluginLoopbackContentType::Json,
        )
        .expect("request");
        assert_eq!(request.method(), PluginLoopbackMethod::Post);
        assert_eq!(request.path(), "/private?token=secret");
        assert_eq!(request.body(), b"private-body");
        let debug = format!("{request:?}");
        assert!(!debug.contains("token"));
        assert!(!debug.contains("private-body"));
    }

    #[test]
    fn loopback_request_rejects_unsafe_targets_and_payloads() {
        let origin = || "http://127.0.0.1:8080".parse().expect("origin");
        for path in ["", "relative", "//other/path", "/space here", "/line\nbreak", "/a\\b", "/a#b"]
        {
            assert_eq!(
                PluginLoopbackRequest::new(
                    PluginLoopbackMethod::Get,
                    origin(),
                    path,
                    Vec::new(),
                    PluginLoopbackContentType::Binary,
                ),
                Err(PluginLoopbackRequestParseError::Path)
            );
        }
        assert_eq!(
            PluginLoopbackRequest::new(
                PluginLoopbackMethod::Get,
                origin(),
                "/",
                vec![1],
                PluginLoopbackContentType::Binary,
            ),
            Err(PluginLoopbackRequestParseError::GetBody)
        );
        assert_eq!(
            PluginLoopbackRequest::new(
                PluginLoopbackMethod::Post,
                origin(),
                "/",
                vec![0; MAX_PLUGIN_LOOPBACK_BODY_BYTES + 1],
                PluginLoopbackContentType::Binary,
            ),
            Err(PluginLoopbackRequestParseError::BodyTooLarge)
        );
    }
}
