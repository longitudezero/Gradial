//! Bounded Lua 5.4 hosting for Gradial plugins.
//!
//! Filesystem discovery belongs to the Windows application. This crate accepts
//! an owned, validated source image and never reads plugin files itself.

mod sandbox;
mod source_image;

pub use sandbox::{
    DEFAULT_LUA_LOAD_INSTRUCTION_LIMIT, DEFAULT_LUA_LOAD_TIMEOUT, LUA_HOOK_INTERVAL,
    LUA_MEMORY_LIMIT_BYTES, LoadedLuaPlugin, LuaSandboxError, LuaSandboxLimits,
};
pub use source_image::{
    MAX_PLUGIN_DIRECTORY_DEPTH, MAX_PLUGIN_PACKAGE_ENTRIES, MAX_PLUGIN_ROOT_ENTRIES,
    MAX_PLUGIN_SOURCE_FILE_BYTES, MAX_PLUGIN_SOURCE_FILES, MAX_PLUGIN_SOURCE_SET_BYTES,
    PluginSourceImage, PluginSourceImageError,
};
