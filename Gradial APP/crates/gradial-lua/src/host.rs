use std::sync::Arc;

use gradial_core::{PluginLoopbackRequest, PluginNotificationText, PluginShortcutChord};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LuaCallbackPhase {
    Begin,
    Update,
    Commit,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LuaCapabilityRejection {
    InactiveSession,
    PermissionDenied,
    AllowlistDenied,
    RateLimited,
    Busy,
    InvalidRequest,
    InvalidNotification,
    InvalidLoopback,
    Cancelled,
}

impl LuaCapabilityRejection {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InactiveSession => "inactive_session",
            Self::PermissionDenied => "permission_denied",
            Self::AllowlistDenied => "allowlist_denied",
            Self::RateLimited => "rate_limited",
            Self::Busy => "busy",
            Self::InvalidRequest => "invalid_shortcut",
            Self::InvalidNotification => "invalid_notification",
            Self::InvalidLoopback => "invalid_loopback",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LuaCapabilityStatus {
    Accepted,
    Rejected(LuaCapabilityRejection),
}

/// Nonblocking service boundary used by protected Lua host functions.
pub trait LuaCapabilityHost: Send + Sync {
    fn request_shortcut(
        &self,
        session_id: Uuid,
        phase: LuaCallbackPhase,
        shortcut: PluginShortcutChord,
    ) -> LuaCapabilityStatus;

    fn request_notification(
        &self,
        _session_id: Uuid,
        _phase: LuaCallbackPhase,
        _text: PluginNotificationText,
    ) -> LuaCapabilityStatus {
        LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InactiveSession)
    }

    fn request_loopback(
        &self,
        _session_id: Uuid,
        _phase: LuaCallbackPhase,
        _request: PluginLoopbackRequest,
    ) -> LuaCapabilityStatus {
        LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InactiveSession)
    }
}

pub(crate) fn deny_all_capability_host() -> Arc<dyn LuaCapabilityHost> {
    Arc::new(DenyAllCapabilityHost)
}

struct DenyAllCapabilityHost;

impl LuaCapabilityHost for DenyAllCapabilityHost {
    fn request_shortcut(
        &self,
        _session_id: Uuid,
        _phase: LuaCallbackPhase,
        _shortcut: PluginShortcutChord,
    ) -> LuaCapabilityStatus {
        LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InactiveSession)
    }
}
