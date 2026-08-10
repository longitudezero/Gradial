use std::sync::Arc;

use gradial_core::PluginShortcutChord;
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
    RateLimited,
    Busy,
    InvalidRequest,
    Cancelled,
}

impl LuaCapabilityRejection {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InactiveSession => "inactive_session",
            Self::PermissionDenied => "permission_denied",
            Self::RateLimited => "rate_limited",
            Self::Busy => "busy",
            Self::InvalidRequest => "invalid_shortcut",
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
