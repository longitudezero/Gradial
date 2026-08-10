use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use mlua::{
    Error as MluaError, Function, HookTriggers, Lua, LuaOptions, LuaString, RegistryKey, StdLib,
    Table, Value, VmState, chunk::ChunkMode,
};
use thiserror::Error;

use crate::{
    LuaCallbackPhase, LuaCapabilityHost, LuaCapabilityRejection, LuaCapabilityStatus,
    PluginSourceImage, PluginSourceImageError, host::deny_all_capability_host,
};

pub const LUA_MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_LUA_LOAD_TIMEOUT: Duration = Duration::from_millis(100);
pub const DEFAULT_LUA_LOAD_INSTRUCTION_LIMIT: u64 = 1_000_000;
pub const LUA_HOOK_INTERVAL: u32 = 1_000;
const MIN_LUA_MEMORY_LIMIT_BYTES: usize = 1024 * 1024;
const MAX_LUA_EXECUTION_TIMEOUT: Duration = Duration::from_secs(1);
const READ_ONLY_HOST_ERROR: &str = "gradial_host_table_read_only";
const EXECUTION_LIMIT_ERROR: &str = "gradial_execution_limit";
const MODULE_LOAD_ERROR: &str = "gradial_module_load_rejected";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LuaSandboxLimits {
    pub memory_bytes: usize,
    pub load_timeout: Duration,
    pub load_instructions: u64,
}

impl LuaSandboxLimits {
    /// Builds limits that cannot exceed Gradial's v0.3 memory/time ceilings.
    ///
    /// # Errors
    ///
    /// Returns [`LuaSandboxError::InvalidLimits`] for a memory limit outside
    /// 1–16 MiB, a zero or greater-than-one-second timeout, or an instruction
    /// budget below one hook interval.
    pub fn new(
        memory_bytes: usize,
        load_timeout: Duration,
        load_instructions: u64,
    ) -> Result<Self, LuaSandboxError> {
        if !(MIN_LUA_MEMORY_LIMIT_BYTES..=LUA_MEMORY_LIMIT_BYTES).contains(&memory_bytes)
            || load_timeout.is_zero()
            || load_timeout > MAX_LUA_EXECUTION_TIMEOUT
            || load_instructions < u64::from(LUA_HOOK_INTERVAL)
        {
            return Err(LuaSandboxError::InvalidLimits);
        }
        Ok(Self { memory_bytes, load_timeout, load_instructions })
    }
}

impl Default for LuaSandboxLimits {
    fn default() -> Self {
        Self {
            memory_bytes: LUA_MEMORY_LIMIT_BYTES,
            load_timeout: DEFAULT_LUA_LOAD_TIMEOUT,
            load_instructions: DEFAULT_LUA_LOAD_INSTRUCTION_LIMIT,
        }
    }
}

/// A loaded entry module owned by one isolated Lua state.
///
/// This foundation exposes inspection only. [`crate::LuaLifecycleAdapter`]
/// owns callback execution and reuses this state's budget mechanism.
pub struct LoadedLuaPlugin {
    pub(crate) lua: Lua,
    pub(crate) entry: RegistryKey,
    pub(crate) gate: Arc<ExecutionGate>,
    pub(crate) host_error: Arc<Mutex<Option<LuaSandboxError>>>,
    pub(crate) callback_scope: Arc<Mutex<Option<LuaCallbackScope>>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LuaCallbackScope {
    pub(crate) session_id: uuid::Uuid,
    pub(crate) phase: LuaCallbackPhase,
}

impl LoadedLuaPlugin {
    /// Creates a fresh restricted Lua 5.4 state and executes only the frozen
    /// entry image and modules it explicitly requests through
    /// `gradial.require`.
    ///
    /// # Errors
    ///
    /// Returns a stable [`LuaSandboxError`] for initialization, resource,
    /// module-resolution, syntax, runtime, or entry-shape failure. Lua source
    /// text and native error strings are never retained in the error.
    pub fn load(
        image: PluginSourceImage,
        limits: LuaSandboxLimits,
    ) -> Result<Self, LuaSandboxError> {
        Self::load_with_capability_host(image, limits, deny_all_capability_host())
    }

    /// Loads a plugin with a nonblocking host capability boundary. Capability
    /// authorization remains the host application's responsibility.
    ///
    /// # Errors
    ///
    /// Returns the same bounded, redacted errors as [`Self::load`].
    pub fn load_with_capability_host(
        image: PluginSourceImage,
        limits: LuaSandboxLimits,
        capability_host: Arc<dyn LuaCapabilityHost>,
    ) -> Result<Self, LuaSandboxError> {
        validate_limits(limits)?;
        let lua = create_lua(limits.memory_bytes)?;
        let gate = Arc::new(ExecutionGate::default());
        let host_error = Arc::new(Mutex::new(None));
        let callback_scope = Arc::new(Mutex::new(None));
        install_execution_hook(&lua, Arc::clone(&gate))?;
        let image = Arc::new(image);
        install_environment(&lua, &image, &host_error, &callback_scope, capability_host)?;

        let entry_path = image.entry_path().to_owned();
        let entry_value = execute_with_limits(
            &gate,
            &host_error,
            limits.load_timeout,
            limits.load_instructions,
            || {
                lua.load(image.entry_source().as_bytes())
                    .set_name(format!("@plugin/{entry_path}"))
                    .set_mode(ChunkMode::Text)
                    .eval::<Value>()
            },
        )?;
        let Value::Table(entry_table) = entry_value else {
            return Err(LuaSandboxError::EntryNotTable);
        };
        let entry = lua.create_registry_value(entry_table).map_err(classify_mlua_error)?;
        Ok(Self { lua, entry, gate, host_error, callback_scope })
    }

    /// Checks a raw entry-table field without invoking Lua metamethods or
    /// plugin code.
    ///
    /// # Errors
    ///
    /// Returns [`LuaSandboxError::Runtime`] if the retained registry value can
    /// no longer be inspected.
    pub fn has_entry_function(&self, name: &str) -> Result<bool, LuaSandboxError> {
        let entry: Table = self.lua.registry_value(&self.entry).map_err(classify_mlua_error)?;
        let value: Value = entry.raw_get(name).map_err(classify_mlua_error)?;
        Ok(matches!(value, Value::Function(_)))
    }

    #[must_use]
    pub fn used_memory(&self) -> usize {
        self.lua.used_memory()
    }
}

impl fmt::Debug for LoadedLuaPlugin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoadedLuaPlugin")
            .field("used_memory", &self.used_memory())
            .finish_non_exhaustive()
    }
}

fn validate_limits(limits: LuaSandboxLimits) -> Result<(), LuaSandboxError> {
    LuaSandboxLimits::new(limits.memory_bytes, limits.load_timeout, limits.load_instructions)?;
    Ok(())
}

fn create_lua(memory_limit: usize) -> Result<Lua, LuaSandboxError> {
    let libraries = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8;
    let lua = Lua::new_with(libraries, LuaOptions::new().catch_rust_panics(false))
        .map_err(|_| LuaSandboxError::Initialization)?;
    lua.set_memory_limit(memory_limit).map_err(classify_setup_error)?;
    lua.set_warning_function(|_, _, _| Ok(()));
    Ok(lua)
}

fn install_execution_hook(lua: &Lua, gate: Arc<ExecutionGate>) -> Result<(), LuaSandboxError> {
    lua.set_global_hook(
        HookTriggers { every_nth_instruction: Some(LUA_HOOK_INTERVAL), ..HookTriggers::default() },
        move |_, _| {
            if let Some(reason) = gate.on_hook() {
                return Err(MluaError::runtime(match reason {
                    ExecutionStop::InstructionLimit | ExecutionStop::Deadline => {
                        EXECUTION_LIMIT_ERROR
                    }
                }));
            }
            Ok(VmState::Continue)
        },
    )
    .map_err(classify_setup_error)
}

fn install_environment(
    lua: &Lua,
    image: &Arc<PluginSourceImage>,
    host_error: &Arc<Mutex<Option<LuaSandboxError>>>,
    callback_scope: &Arc<Mutex<Option<LuaCallbackScope>>>,
    capability_host: Arc<dyn LuaCapabilityHost>,
) -> Result<(), LuaSandboxError> {
    let globals = lua.globals();
    for excluded in [
        "collectgarbage",
        "coroutine",
        "debug",
        "dofile",
        "io",
        "load",
        "loadfile",
        "os",
        "package",
        "print",
        "rawset",
        "require",
        "warn",
    ] {
        globals.raw_set(excluded, Value::Nil).map_err(classify_setup_error)?;
    }
    let string_library: Table = globals.get("string").map_err(classify_setup_error)?;
    string_library.raw_set("dump", Value::Nil).map_err(classify_setup_error)?;

    let cache = lua.create_table().map_err(classify_setup_error)?;
    let loading = lua.create_table().map_err(classify_setup_error)?;
    let require_image = Arc::clone(image);
    let require_error = Arc::clone(host_error);
    let require = lua
        .create_function(move |lua, module_name: LuaString| {
            let Ok(module_name) = module_name.to_str() else {
                record_host_error(&require_error, LuaSandboxError::ModuleNameInvalid);
                return Err(MluaError::runtime(MODULE_LOAD_ERROR));
            };
            let module_name = module_name.as_ref();
            let cached: Value = cache.raw_get(module_name)?;
            if !matches!(cached, Value::Nil) {
                return Ok(cached);
            }
            if loading.raw_get::<bool>(module_name).unwrap_or(false) {
                record_host_error(&require_error, LuaSandboxError::ModuleCycle);
                return Err(MluaError::runtime(MODULE_LOAD_ERROR));
            }
            let (path, source) = match require_image.module_source(module_name) {
                Ok(Some(module)) => module,
                Ok(None) => {
                    record_host_error(&require_error, LuaSandboxError::ModuleNotFound);
                    return Err(MluaError::runtime(MODULE_LOAD_ERROR));
                }
                Err(PluginSourceImageError::InvalidModuleName) => {
                    record_host_error(&require_error, LuaSandboxError::ModuleNameInvalid);
                    return Err(MluaError::runtime(MODULE_LOAD_ERROR));
                }
                Err(_) => {
                    record_host_error(&require_error, LuaSandboxError::Runtime);
                    return Err(MluaError::runtime(MODULE_LOAD_ERROR));
                }
            };
            loading.raw_set(module_name, true)?;
            let evaluated = lua
                .load(source.as_bytes())
                .set_name(format!("@plugin/{path}"))
                .set_mode(ChunkMode::Text)
                .eval::<Value>();
            loading.raw_set(module_name, false)?;
            let value = evaluated?;
            let value = if matches!(value, Value::Nil) { Value::Boolean(true) } else { value };
            cache.raw_set(module_name, value.clone())?;
            Ok(value)
        })
        .map_err(classify_setup_error)?;

    let host_exports = lua.create_table().map_err(classify_setup_error)?;
    host_exports.raw_set("require", require).map_err(classify_setup_error)?;
    let input = input_host_proxy(lua, callback_scope, Arc::clone(&capability_host))?;
    host_exports.raw_set("input", input).map_err(classify_setup_error)?;
    let notification = notification_host_function(lua, callback_scope, capability_host)?;
    host_exports.raw_set("notification", notification).map_err(classify_setup_error)?;
    let host_proxy = read_only_proxy(lua, host_exports)?;
    globals.raw_set("gradial", host_proxy).map_err(classify_setup_error)?;
    Ok(())
}

fn input_host_proxy(
    lua: &Lua,
    callback_scope: &Arc<Mutex<Option<LuaCallbackScope>>>,
    capability_host: Arc<dyn LuaCapabilityHost>,
) -> Result<Table, LuaSandboxError> {
    let shortcut_scope = Arc::clone(callback_scope);
    let shortcut = lua
        .create_function(move |_, chord: Value| {
            let parsed = match chord {
                Value::String(chord) => chord.to_str().ok().and_then(|value| value.parse().ok()),
                _ => None,
            };
            let status = parsed.map_or(
                LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InvalidRequest),
                |shortcut| {
                    let scope = *lock_unpoisoned(&shortcut_scope);
                    scope.map_or(
                        LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InactiveSession),
                        |scope| {
                            capability_host.request_shortcut(
                                scope.session_id,
                                scope.phase,
                                shortcut,
                            )
                        },
                    )
                },
            );
            Ok(match status {
                LuaCapabilityStatus::Accepted => (true, None),
                LuaCapabilityStatus::Rejected(reason) => (false, Some(reason.as_str())),
            })
        })
        .map_err(classify_setup_error)?;
    let input_exports = lua.create_table().map_err(classify_setup_error)?;
    input_exports.raw_set("shortcut", shortcut).map_err(classify_setup_error)?;
    read_only_proxy(lua, input_exports)
}

fn notification_host_function(
    lua: &Lua,
    callback_scope: &Arc<Mutex<Option<LuaCallbackScope>>>,
    capability_host: Arc<dyn LuaCapabilityHost>,
) -> Result<Function, LuaSandboxError> {
    let notification_scope = Arc::clone(callback_scope);
    lua.create_function(move |_, text: Value| {
        let parsed = match text {
            Value::String(text) => text.to_str().ok().and_then(|value| value.parse().ok()),
            _ => None,
        };
        let status = parsed.map_or(
            LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InvalidNotification),
            |text| {
                let scope = *lock_unpoisoned(&notification_scope);
                scope.map_or(
                    LuaCapabilityStatus::Rejected(LuaCapabilityRejection::InactiveSession),
                    |scope| {
                        capability_host.request_notification(scope.session_id, scope.phase, text)
                    },
                )
            },
        );
        Ok(match status {
            LuaCapabilityStatus::Accepted => (true, None),
            LuaCapabilityStatus::Rejected(reason) => (false, Some(reason.as_str())),
        })
    })
    .map_err(classify_setup_error)
}

fn read_only_proxy(lua: &Lua, exports: Table) -> Result<Table, LuaSandboxError> {
    let proxy = lua.create_table().map_err(classify_setup_error)?;
    let metatable = lua.create_table().map_err(classify_setup_error)?;
    metatable.raw_set("__index", exports).map_err(classify_setup_error)?;
    metatable
        .raw_set(
            "__newindex",
            lua.create_function(|_, (_table, _key, _value): (Table, Value, Value)| {
                Err::<(), _>(MluaError::runtime(READ_ONLY_HOST_ERROR))
            })
            .map_err(classify_setup_error)?,
        )
        .map_err(classify_setup_error)?;
    metatable.raw_set("__metatable", false).map_err(classify_setup_error)?;
    proxy.set_metatable(Some(metatable)).map_err(classify_setup_error)?;
    Ok(proxy)
}

pub(crate) fn execute_with_limits<T>(
    gate: &ExecutionGate,
    host_error: &Mutex<Option<LuaSandboxError>>,
    timeout: Duration,
    instructions: u64,
    operation: impl FnOnce() -> Result<T, MluaError>,
) -> Result<T, LuaSandboxError> {
    *lock_unpoisoned(host_error) = None;
    gate.begin(timeout, instructions);
    let result = operation();
    let stop = gate.finish();
    if let Some(stop) = stop {
        return Err(match stop {
            ExecutionStop::InstructionLimit => LuaSandboxError::InstructionLimit,
            ExecutionStop::Deadline => LuaSandboxError::DeadlineExceeded,
        });
    }
    if let Some(error) = *lock_unpoisoned(host_error) {
        return Err(error);
    }
    result.map_err(classify_mlua_error)
}

fn classify_setup_error(error: MluaError) -> LuaSandboxError {
    let classified = classify_mlua_error_ref(&error);
    drop(error);
    if classified == LuaSandboxError::MemoryLimit {
        classified
    } else {
        LuaSandboxError::Initialization
    }
}

pub(crate) fn classify_mlua_error(error: MluaError) -> LuaSandboxError {
    let classified = classify_mlua_error_ref(&error);
    drop(error);
    classified
}

fn classify_mlua_error_ref(error: &MluaError) -> LuaSandboxError {
    match error {
        MluaError::MemoryError(_) => LuaSandboxError::MemoryLimit,
        MluaError::SyntaxError { .. } => LuaSandboxError::Syntax,
        MluaError::CallbackError { cause, .. }
        | MluaError::BadArgument { cause, .. }
        | MluaError::WithContext { cause, .. } => classify_mlua_error_ref(cause),
        _ => LuaSandboxError::Runtime,
    }
}

fn record_host_error(slot: &Mutex<Option<LuaSandboxError>>, error: LuaSandboxError) {
    let mut slot = lock_unpoisoned(slot);
    if slot.is_none() {
        *slot = Some(error);
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecutionStop {
    InstructionLimit,
    Deadline,
}

#[derive(Debug)]
struct ActiveBudget {
    deadline: Instant,
    instructions_remaining: u64,
    stopped: Option<ExecutionStop>,
}

#[derive(Debug, Default)]
pub(crate) struct ExecutionGate {
    active: Mutex<Option<ActiveBudget>>,
}

impl ExecutionGate {
    fn begin(&self, timeout: Duration, instructions: u64) {
        let deadline = Instant::now().checked_add(timeout).unwrap_or_else(Instant::now);
        *lock_unpoisoned(&self.active) =
            Some(ActiveBudget { deadline, instructions_remaining: instructions, stopped: None });
    }

    fn on_hook(&self) -> Option<ExecutionStop> {
        let mut active = lock_unpoisoned(&self.active);
        let active = active.as_mut()?;
        if let Some(stopped) = active.stopped {
            return Some(stopped);
        }
        if Instant::now() >= active.deadline {
            active.stopped = Some(ExecutionStop::Deadline);
            return active.stopped;
        }
        let interval = u64::from(LUA_HOOK_INTERVAL);
        if active.instructions_remaining <= interval {
            active.stopped = Some(ExecutionStop::InstructionLimit);
            return active.stopped;
        }
        active.instructions_remaining -= interval;
        None
    }

    fn finish(&self) -> Option<ExecutionStop> {
        let mut active = lock_unpoisoned(&self.active);
        let active = active.take()?;
        active
            .stopped
            .or_else(|| (Instant::now() >= active.deadline).then_some(ExecutionStop::Deadline))
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LuaSandboxError {
    #[error("plugin_lua_limits_invalid")]
    InvalidLimits,
    #[error("plugin_lua_initialization_failed")]
    Initialization,
    #[error("plugin_lua_memory_limit")]
    MemoryLimit,
    #[error("plugin_lua_instruction_limit")]
    InstructionLimit,
    #[error("plugin_lua_deadline_exceeded")]
    DeadlineExceeded,
    #[error("plugin_module_name_invalid")]
    ModuleNameInvalid,
    #[error("plugin_module_not_found")]
    ModuleNotFound,
    #[error("plugin_module_cycle")]
    ModuleCycle,
    #[error("plugin_lua_syntax_error")]
    Syntax,
    #[error("plugin_lua_runtime_error")]
    Runtime,
    #[error("plugin_entry_not_table")]
    EntryNotTable,
    #[error("plugin_callback_missing:{callback}")]
    CallbackMissing { callback: &'static str },
    #[error("plugin_callback_not_function:{callback}")]
    CallbackNotFunction { callback: &'static str },
    #[error("plugin_begin_state_not_table")]
    BeginStateNotTable,
    #[error("plugin_callback_return_invalid")]
    CallbackReturnInvalid,
    #[error("plugin_session_already_active")]
    SessionAlreadyActive,
    #[error("plugin_session_not_active")]
    SessionNotActive,
    #[error("plugin_faulted")]
    PluginFaulted,
    #[error("plugin_host_value_invalid")]
    HostValueInvalid,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn image(entry: &str, files: &[(&str, &str)]) -> PluginSourceImage {
        PluginSourceImage::from_files(
            entry,
            files
                .iter()
                .map(|(path, source)| (PathBuf::from(path), source.as_bytes().to_vec()))
                .collect(),
        )
        .expect("source image")
    }

    fn entry_table(plugin: &LoadedLuaPlugin) -> Table {
        plugin.lua.registry_value(&plugin.entry).expect("entry table")
    }

    #[test]
    fn reviewed_environment_excludes_external_and_dynamic_code_surfaces() {
        let plugin = LoadedLuaPlugin::load(
            image(
                "main.lua",
                &[(
                    "main.lua",
                    r#"
                    assert(os == nil and io == nil and package == nil and debug == nil)
                    assert(require == nil and load == nil and loadfile == nil and dofile == nil)
                    assert(print == nil and warn == nil and collectgarbage == nil and rawset == nil)
                    assert(coroutine == nil and string.dump == nil)
                    assert(_VERSION == "Lua 5.4")
                    assert(type(math) == "table" and type(string) == "table")
                    assert(type(table) == "table" and type(utf8) == "table")
                    local changed = pcall(function() gradial.require = function() end end)
                    assert(not changed)
                    return { begin = function() end }
                    "#,
                )],
            ),
            LuaSandboxLimits::default(),
        )
        .expect("restricted plugin");

        assert!(plugin.has_entry_function("begin").expect("function check"));
        assert!(!plugin.has_entry_function("update").expect("missing function check"));
    }

    #[test]
    fn gradial_require_is_rooted_recursive_and_cached() {
        let plugin = LoadedLuaPlugin::load(
            image(
                "main.lua",
                &[
                    (
                        "main.lua",
                        r#"
                        local first = gradial.require("lib.mapping")
                        local second = gradial.require("lib.mapping")
                        assert(first == second)
                        return { answer = first.answer, loads = module_loads }
                        "#,
                    ),
                    (
                        "lib/mapping.lua",
                        r#"
                        module_loads = (module_loads or 0) + 1
                        local value = gradial.require("lib.value")
                        return { answer = value + 1 }
                        "#,
                    ),
                    ("lib/value.lua", "return 41"),
                ],
            ),
            LuaSandboxLimits::default(),
        )
        .expect("module plugin");
        let entry = entry_table(&plugin);

        assert_eq!(entry.raw_get::<i64>("answer").expect("answer"), 42);
        assert_eq!(entry.raw_get::<i64>("loads").expect("loads"), 1);
    }

    #[test]
    fn invalid_missing_and_cyclic_modules_have_stable_failures() {
        let invalid = LoadedLuaPlugin::load(
            image("main.lua", &[("main.lua", "gradial.require('../secret'); return {}")]),
            LuaSandboxLimits::default(),
        );
        assert_eq!(invalid.expect_err("invalid module"), LuaSandboxError::ModuleNameInvalid);

        let missing = LoadedLuaPlugin::load(
            image("main.lua", &[("main.lua", "gradial.require('missing'); return {}")]),
            LuaSandboxLimits::default(),
        );
        assert_eq!(missing.expect_err("missing module"), LuaSandboxError::ModuleNotFound);

        let cyclic = LoadedLuaPlugin::load(
            image(
                "main.lua",
                &[
                    ("main.lua", "gradial.require('a'); return {}"),
                    ("a.lua", "return gradial.require('b')"),
                    ("b.lua", "return gradial.require('a')"),
                ],
            ),
            LuaSandboxLimits::default(),
        );
        assert_eq!(cyclic.expect_err("cyclic module"), LuaSandboxError::ModuleCycle);
    }

    #[test]
    fn entry_and_module_syntax_errors_are_redacted() {
        let entry = LoadedLuaPlugin::load(
            image("main.lua", &[("main.lua", "local = secret-sentinel")]),
            LuaSandboxLimits::default(),
        );
        assert_eq!(entry.expect_err("entry syntax"), LuaSandboxError::Syntax);

        let module = LoadedLuaPlugin::load(
            image(
                "main.lua",
                &[
                    ("main.lua", "gradial.require('broken'); return {}"),
                    ("broken.lua", "local = private-source"),
                ],
            ),
            LuaSandboxLimits::default(),
        );
        let error = module.expect_err("module syntax");
        assert_eq!(error, LuaSandboxError::Syntax);
        assert!(!format!("{error:?}").contains("private-source"));
    }

    #[test]
    fn entry_must_return_a_table() {
        let result = LoadedLuaPlugin::load(
            image("main.lua", &[("main.lua", "return 42")]),
            LuaSandboxLimits::default(),
        );
        assert_eq!(result.expect_err("non-table entry"), LuaSandboxError::EntryNotTable);
    }

    #[test]
    fn memory_instruction_and_wall_limits_are_distinct() {
        let memory = LoadedLuaPlugin::load(
            image(
                "main.lua",
                &[(
                    "main.lua",
                    "local value = string.rep('x', 20 * 1024 * 1024); return { value = value }",
                )],
            ),
            LuaSandboxLimits::default(),
        );
        assert_eq!(memory.expect_err("memory limit"), LuaSandboxError::MemoryLimit);

        let instruction_limits =
            LuaSandboxLimits::new(LUA_MEMORY_LIMIT_BYTES, Duration::from_secs(1), 10_000)
                .expect("instruction limits");
        let instruction = LoadedLuaPlugin::load(
            image("main.lua", &[("main.lua", "while true do end")]),
            instruction_limits,
        );
        assert_eq!(instruction.expect_err("instruction limit"), LuaSandboxError::InstructionLimit);

        let deadline_limits =
            LuaSandboxLimits::new(LUA_MEMORY_LIMIT_BYTES, Duration::from_millis(1), u64::MAX)
                .expect("deadline limits");
        let deadline = LoadedLuaPlugin::load(
            image("main.lua", &[("main.lua", "while true do end")]),
            deadline_limits,
        );
        assert_eq!(deadline.expect_err("deadline"), LuaSandboxError::DeadlineExceeded);
    }

    #[test]
    fn caught_budget_error_still_fails_the_load() {
        let limits = LuaSandboxLimits::new(LUA_MEMORY_LIMIT_BYTES, Duration::from_secs(1), 10_000)
            .expect("limits");
        let result = LoadedLuaPlugin::load(
            image(
                "main.lua",
                &[("main.lua", "pcall(function() while true do end end); return {}")],
            ),
            limits,
        );
        assert_eq!(result.expect_err("caught hook error"), LuaSandboxError::InstructionLimit);
    }

    #[test]
    fn runtime_error_does_not_retain_plugin_message() {
        let error = LoadedLuaPlugin::load(
            image("main.lua", &[("main.lua", "error('private-sentinel')")]),
            LuaSandboxLimits::default(),
        )
        .expect_err("runtime error");

        assert_eq!(error, LuaSandboxError::Runtime);
        assert!(!format!("{error:?}").contains("private-sentinel"));
    }

    #[test]
    fn each_load_owns_a_fresh_sendable_state() {
        fn assert_send<T: Send>() {}
        assert_send::<LoadedLuaPlugin>();

        let source = image(
            "main.lua",
            &[(
                "main.lua",
                "state_counter = (state_counter or 0) + 1; return { value = state_counter }",
            )],
        );
        let first = LoadedLuaPlugin::load(source.clone(), LuaSandboxLimits::default())
            .expect("first state");
        let second =
            LoadedLuaPlugin::load(source, LuaSandboxLimits::default()).expect("second state");

        assert_eq!(entry_table(&first).raw_get::<i64>("value").expect("first value"), 1);
        assert_eq!(entry_table(&second).raw_get::<i64>("value").expect("second value"), 1);
    }

    #[test]
    fn invalid_limits_are_rejected_before_state_creation() {
        assert_eq!(
            LuaSandboxLimits::new(
                LUA_MEMORY_LIMIT_BYTES + 1,
                DEFAULT_LUA_LOAD_TIMEOUT,
                DEFAULT_LUA_LOAD_INSTRUCTION_LIMIT,
            ),
            Err(LuaSandboxError::InvalidLimits)
        );
        assert_eq!(
            LuaSandboxLimits::new(LUA_MEMORY_LIMIT_BYTES, Duration::ZERO, u64::MAX),
            Err(LuaSandboxError::InvalidLimits)
        );
        assert_eq!(
            LuaSandboxLimits::new(
                LUA_MEMORY_LIMIT_BYTES,
                DEFAULT_LUA_LOAD_TIMEOUT,
                u64::from(LUA_HOOK_INTERVAL) - 1,
            ),
            Err(LuaSandboxError::InvalidLimits)
        );
    }
}
