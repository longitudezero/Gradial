//! Bounded Lua 5.4 hosting for Gradial plugins.
//!
//! Filesystem discovery belongs to the Windows application. This crate accepts
//! an owned, validated source image and never reads plugin files itself.

mod host;
mod lifecycle;
mod sandbox;
mod source_image;

pub use host::{LuaCallbackPhase, LuaCapabilityHost, LuaCapabilityRejection, LuaCapabilityStatus};
pub use lifecycle::{
    DEFAULT_LUA_BEGIN_INSTRUCTION_LIMIT, DEFAULT_LUA_CANCEL_INSTRUCTION_LIMIT,
    DEFAULT_LUA_COMMIT_INSTRUCTION_LIMIT, DEFAULT_LUA_UPDATE_INSTRUCTION_LIMIT, LUA_BEGIN_TIMEOUT,
    LUA_CANCEL_TIMEOUT, LUA_COMMIT_TIMEOUT, LUA_UPDATE_TIMEOUT, LuaCallbackBudget,
    LuaCallbackLimits, LuaLifecycleAdapter,
};
pub use sandbox::{
    DEFAULT_LUA_LOAD_INSTRUCTION_LIMIT, DEFAULT_LUA_LOAD_TIMEOUT, LUA_HOOK_INTERVAL,
    LUA_MEMORY_LIMIT_BYTES, LoadedLuaPlugin, LuaSandboxError, LuaSandboxLimits,
};
pub use source_image::{
    MAX_PLUGIN_DIRECTORY_DEPTH, MAX_PLUGIN_PACKAGE_ENTRIES, MAX_PLUGIN_ROOT_ENTRIES,
    MAX_PLUGIN_SOURCE_FILE_BYTES, MAX_PLUGIN_SOURCE_FILES, MAX_PLUGIN_SOURCE_SET_BYTES,
    PluginSourceImage, PluginSourceImageError,
};
