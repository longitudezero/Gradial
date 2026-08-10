use std::{collections::HashMap, fmt, sync::Arc, time::Duration};

use gradial_core::{
    ContinuousBeginRequest, ContinuousCancelReason, ContinuousCancelRequest,
    ContinuousCommitRequest, ContinuousSample, ContinuousUpdateRequest, PermittedContext,
    PluginParameters, PluginValue,
};
use mlua::{Function, Lua, MultiValue, RegistryKey, Table, Value};
use uuid::Uuid;

use crate::{
    LUA_HOOK_INTERVAL, LoadedLuaPlugin, LuaSandboxError, LuaSandboxLimits, PluginSourceImage,
    sandbox::{classify_mlua_error, execute_with_limits},
};

pub const LUA_BEGIN_TIMEOUT: Duration = Duration::from_millis(100);
pub const LUA_UPDATE_TIMEOUT: Duration = Duration::from_millis(16);
pub const LUA_COMMIT_TIMEOUT: Duration = Duration::from_millis(100);
pub const LUA_CANCEL_TIMEOUT: Duration = Duration::from_millis(50);
pub const DEFAULT_LUA_BEGIN_INSTRUCTION_LIMIT: u64 = 1_000_000;
pub const DEFAULT_LUA_UPDATE_INSTRUCTION_LIMIT: u64 = 160_000;
pub const DEFAULT_LUA_COMMIT_INSTRUCTION_LIMIT: u64 = 1_000_000;
pub const DEFAULT_LUA_CANCEL_INSTRUCTION_LIMIT: u64 = 500_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LuaCallbackBudget {
    pub timeout: Duration,
    pub instructions: u64,
}

impl LuaCallbackBudget {
    /// Creates a nonzero callback budget. Phase-specific ceilings are checked
    /// by [`LuaCallbackLimits::new`].
    ///
    /// # Errors
    ///
    /// Returns [`LuaSandboxError::InvalidLimits`] when the timeout is zero or
    /// the instruction budget cannot reach the first execution hook.
    pub fn new(timeout: Duration, instructions: u64) -> Result<Self, LuaSandboxError> {
        if timeout.is_zero() || instructions < u64::from(LUA_HOOK_INTERVAL) {
            return Err(LuaSandboxError::InvalidLimits);
        }
        Ok(Self { timeout, instructions })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LuaCallbackLimits {
    pub begin: LuaCallbackBudget,
    pub update: LuaCallbackBudget,
    pub commit: LuaCallbackBudget,
    pub cancel: LuaCallbackBudget,
}

impl LuaCallbackLimits {
    /// Builds callback limits without permitting a caller to raise Gradial's
    /// v0.3 phase ceilings.
    ///
    /// # Errors
    ///
    /// Returns [`LuaSandboxError::InvalidLimits`] for a zero/too-small budget
    /// or when any timeout or instruction count exceeds its phase ceiling.
    pub fn new(
        begin: LuaCallbackBudget,
        update: LuaCallbackBudget,
        commit: LuaCallbackBudget,
        cancel: LuaCallbackBudget,
    ) -> Result<Self, LuaSandboxError> {
        let limits = Self { begin, update, commit, cancel };
        limits.validate()?;
        Ok(limits)
    }

    fn validate(self) -> Result<(), LuaSandboxError> {
        for (budget, max_timeout, max_instructions) in [
            (self.begin, LUA_BEGIN_TIMEOUT, DEFAULT_LUA_BEGIN_INSTRUCTION_LIMIT),
            (self.update, LUA_UPDATE_TIMEOUT, DEFAULT_LUA_UPDATE_INSTRUCTION_LIMIT),
            (self.commit, LUA_COMMIT_TIMEOUT, DEFAULT_LUA_COMMIT_INSTRUCTION_LIMIT),
            (self.cancel, LUA_CANCEL_TIMEOUT, DEFAULT_LUA_CANCEL_INSTRUCTION_LIMIT),
        ] {
            LuaCallbackBudget::new(budget.timeout, budget.instructions)?;
            if budget.timeout > max_timeout || budget.instructions > max_instructions {
                return Err(LuaSandboxError::InvalidLimits);
            }
        }
        Ok(())
    }
}

impl Default for LuaCallbackLimits {
    fn default() -> Self {
        Self {
            begin: LuaCallbackBudget {
                timeout: LUA_BEGIN_TIMEOUT,
                instructions: DEFAULT_LUA_BEGIN_INSTRUCTION_LIMIT,
            },
            update: LuaCallbackBudget {
                timeout: LUA_UPDATE_TIMEOUT,
                instructions: DEFAULT_LUA_UPDATE_INSTRUCTION_LIMIT,
            },
            commit: LuaCallbackBudget {
                timeout: LUA_COMMIT_TIMEOUT,
                instructions: DEFAULT_LUA_COMMIT_INSTRUCTION_LIMIT,
            },
            cancel: LuaCallbackBudget {
                timeout: LUA_CANCEL_TIMEOUT,
                instructions: DEFAULT_LUA_CANCEL_INSTRUCTION_LIMIT,
            },
        }
    }
}

struct CallbackSet {
    begin: RegistryKey,
    update: RegistryKey,
    commit: RegistryKey,
    cancel: RegistryKey,
}

/// Strict, inert adapter for one plugin's four continuous lifecycle callbacks.
///
/// Session tables never leave this adapter and are routed only by the opaque
/// session ID supplied by `gradial-core`. Any plugin execution/shape failure
/// faults this adapter and drops every retained session.
pub struct LuaLifecycleAdapter {
    plugin: LoadedLuaPlugin,
    callbacks: CallbackSet,
    sessions: HashMap<Uuid, RegistryKey>,
    limits: LuaCallbackLimits,
    faulted: bool,
}

impl LuaLifecycleAdapter {
    /// Loads and validates all four required callbacks with default budgets.
    ///
    /// # Errors
    ///
    /// Returns a sandbox/load error or a stable missing/invalid callback code.
    pub fn load(
        image: PluginSourceImage,
        sandbox_limits: LuaSandboxLimits,
    ) -> Result<Self, LuaSandboxError> {
        Self::load_with_callback_limits(image, sandbox_limits, LuaCallbackLimits::default())
    }

    /// Loads and validates all four required callbacks with bounded custom
    /// budgets, primarily for deterministic host tests.
    ///
    /// # Errors
    ///
    /// Returns [`LuaSandboxError::InvalidLimits`] if a phase ceiling is raised,
    /// or a stable load/callback-shape error.
    pub fn load_with_callback_limits(
        image: PluginSourceImage,
        sandbox_limits: LuaSandboxLimits,
        callback_limits: LuaCallbackLimits,
    ) -> Result<Self, LuaSandboxError> {
        callback_limits.validate()?;
        let plugin = LoadedLuaPlugin::load(image, sandbox_limits)?;
        let entry: Table = plugin.lua.registry_value(&plugin.entry).map_err(classify_mlua_error)?;
        let callbacks = CallbackSet {
            begin: required_callback(&plugin.lua, &entry, "begin")?,
            update: required_callback(&plugin.lua, &entry, "update")?,
            commit: required_callback(&plugin.lua, &entry, "commit")?,
            cancel: required_callback(&plugin.lua, &entry, "cancel")?,
        };
        drop(entry);
        Ok(Self {
            plugin,
            callbacks,
            sessions: HashMap::new(),
            limits: callback_limits,
            faulted: false,
        })
    }

    /// Calls `begin` and retains its one required table as private session
    /// state.
    ///
    /// # Errors
    ///
    /// Returns a stable state, resource, runtime, or return-shape error.
    pub fn begin(&mut self, request: &ContinuousBeginRequest) -> Result<(), LuaSandboxError> {
        self.ensure_healthy()?;
        let session_id = request.session_id();
        if self.sessions.contains_key(&session_id) {
            return Err(LuaSandboxError::SessionAlreadyActive);
        }

        let context_result = begin_context(&self.plugin.lua, request);
        let context = self.accept_host_value(context_result)?;
        let parameters_result = parameter_table(&self.plugin.lua, request.parameters());
        let parameters = self.accept_host_value(parameters_result)?;
        let initial_result = sample_table(&self.plugin.lua, request.initial());
        let initial = self.accept_host_value(initial_result)?;
        let function_result =
            self.plugin.lua.registry_value(&self.callbacks.begin).map_err(classify_mlua_error);
        let function: Function = self.accept_plugin_result(function_result)?;
        let budget = self.limits.begin;
        let result = execute_with_limits(
            &self.plugin.gate,
            &self.plugin.host_error,
            budget.timeout,
            budget.instructions,
            || function.call::<MultiValue>((context, parameters, initial)),
        );
        let returns = match result {
            Ok(returns) => returns,
            Err(error) => return self.fail(error),
        };
        let mut values = returns.into_iter();
        let Some(Value::Table(session)) = values.next() else {
            return self.fail(LuaSandboxError::BeginStateNotTable);
        };
        if values.next().is_some() {
            return self.fail(LuaSandboxError::CallbackReturnInvalid);
        }
        let key = match self.plugin.lua.create_registry_value(session) {
            Ok(key) => key,
            Err(error) => return self.fail(classify_mlua_error(error)),
        };
        self.sessions.insert(session_id, key);
        Ok(())
    }

    /// Calls `update` with the matching private session table and an immutable
    /// complete sample.
    ///
    /// # Errors
    ///
    /// Returns a stable state, resource, runtime, or return-shape error.
    pub fn update(&mut self, request: &ContinuousUpdateRequest) -> Result<(), LuaSandboxError> {
        self.ensure_healthy()?;
        let session = self.active_session(request.session_id())?;
        let sample_result = sample_table(&self.plugin.lua, request.sample());
        let sample = self.accept_host_value(sample_result)?;
        self.call_no_return(CallbackPhase::Update, session, Value::Table(sample))
    }

    /// Removes the matching session before making its one terminal `commit`
    /// call.
    ///
    /// # Errors
    ///
    /// Returns a stable state, resource, runtime, or return-shape error.
    pub fn commit(&mut self, request: &ContinuousCommitRequest) -> Result<(), LuaSandboxError> {
        self.ensure_healthy()?;
        let session = self.take_session(request.session_id())?;
        let sample_result = sample_table(&self.plugin.lua, request.final_sample());
        let sample = self.accept_host_value(sample_result)?;
        self.call_no_return(CallbackPhase::Commit, session, Value::Table(sample))
    }

    /// Removes the matching session before making its one best-effort
    /// terminal `cancel` call.
    ///
    /// # Errors
    ///
    /// Returns a stable state, resource, runtime, or return-shape error.
    pub fn cancel(&mut self, request: &ContinuousCancelRequest) -> Result<(), LuaSandboxError> {
        self.ensure_healthy()?;
        let session = self.take_session(request.session_id())?;
        let reason_result = self
            .plugin
            .lua
            .create_string(cancel_reason(request.reason()))
            .map_err(classify_mlua_error);
        let reason = self.accept_plugin_result(reason_result)?;
        self.call_no_return(CallbackPhase::Cancel, session, Value::String(reason))
    }

    #[must_use]
    pub const fn is_faulted(&self) -> bool {
        self.faulted
    }

    #[must_use]
    pub fn active_session_count(&self) -> usize {
        self.sessions.len()
    }

    #[must_use]
    pub fn used_memory(&self) -> usize {
        self.plugin.used_memory()
    }

    fn ensure_healthy(&self) -> Result<(), LuaSandboxError> {
        if self.faulted { Err(LuaSandboxError::PluginFaulted) } else { Ok(()) }
    }

    fn active_session(&mut self, session_id: Uuid) -> Result<Table, LuaSandboxError> {
        let result = {
            let key = self.sessions.get(&session_id).ok_or(LuaSandboxError::SessionNotActive)?;
            self.plugin.lua.registry_value(key).map_err(classify_mlua_error)
        };
        self.accept_plugin_result(result)
    }

    fn take_session(&mut self, session_id: Uuid) -> Result<Table, LuaSandboxError> {
        let key = self.sessions.remove(&session_id).ok_or(LuaSandboxError::SessionNotActive)?;
        let session_result = self.plugin.lua.registry_value(&key).map_err(classify_mlua_error);
        let session = self.accept_plugin_result(session_result)?;
        let remove_result = self.plugin.lua.remove_registry_value(key).map_err(classify_mlua_error);
        self.accept_plugin_result(remove_result)?;
        Ok(session)
    }

    fn call_no_return(
        &mut self,
        phase: CallbackPhase,
        session: Table,
        argument: Value,
    ) -> Result<(), LuaSandboxError> {
        let (callback, budget) = match phase {
            CallbackPhase::Update => (&self.callbacks.update, self.limits.update),
            CallbackPhase::Commit => (&self.callbacks.commit, self.limits.commit),
            CallbackPhase::Cancel => (&self.callbacks.cancel, self.limits.cancel),
        };
        let function_result = self.plugin.lua.registry_value(callback).map_err(classify_mlua_error);
        let function: Function = self.accept_plugin_result(function_result)?;
        let result = execute_with_limits(
            &self.plugin.gate,
            &self.plugin.host_error,
            budget.timeout,
            budget.instructions,
            || function.call::<MultiValue>((session, argument)),
        );
        match result {
            Ok(returns) if returns.is_empty() => Ok(()),
            Ok(_) => self.fail(LuaSandboxError::CallbackReturnInvalid),
            Err(error) => self.fail(error),
        }
    }

    fn fail<T>(&mut self, error: LuaSandboxError) -> Result<T, LuaSandboxError> {
        self.faulted = true;
        for (_, key) in self.sessions.drain() {
            let _ = self.plugin.lua.remove_registry_value(key);
        }
        Err(error)
    }

    fn accept_host_value<T>(
        &mut self,
        result: Result<T, LuaSandboxError>,
    ) -> Result<T, LuaSandboxError> {
        match result {
            Ok(value) => Ok(value),
            Err(LuaSandboxError::HostValueInvalid) => Err(LuaSandboxError::HostValueInvalid),
            Err(error) => self.fail(error),
        }
    }

    fn accept_plugin_result<T>(
        &mut self,
        result: Result<T, LuaSandboxError>,
    ) -> Result<T, LuaSandboxError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => self.fail(error),
        }
    }
}

impl fmt::Debug for LuaLifecycleAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LuaLifecycleAdapter")
            .field("active_sessions", &self.active_session_count())
            .field("faulted", &self.faulted)
            .field("used_memory", &self.used_memory())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
enum CallbackPhase {
    Update,
    Commit,
    Cancel,
}

fn required_callback(
    lua: &Lua,
    entry: &Table,
    name: &'static str,
) -> Result<RegistryKey, LuaSandboxError> {
    match entry.raw_get::<Value>(name).map_err(classify_mlua_error)? {
        Value::Nil => Err(LuaSandboxError::CallbackMissing { callback: name }),
        Value::Function(function) => {
            lua.create_registry_value(function).map_err(classify_mlua_error)
        }
        _ => Err(LuaSandboxError::CallbackNotFunction { callback: name }),
    }
}

fn begin_context(lua: &Lua, request: &ContinuousBeginRequest) -> Result<Table, LuaSandboxError> {
    let session_id = request.session_id().to_string();
    let api_version = i64::from(request.api_version());
    let action_id = request.action_id().as_str().to_owned();
    let permitted = request.context();
    let application = application_table(lua, permitted)?;
    let fields = vec![
        ("api_version".to_owned(), Value::Integer(api_version)),
        (
            "session_id".to_owned(),
            Value::String(lua.create_string(session_id).map_err(classify_mlua_error)?),
        ),
        (
            "action_id".to_owned(),
            Value::String(lua.create_string(action_id).map_err(classify_mlua_error)?),
        ),
        ("application".to_owned(), application.map_or(Value::Nil, Value::Table)),
        ("window_title".to_owned(), optional_string(lua, permitted.window_title())?),
    ];
    read_only_table(lua, fields).map_err(classify_mlua_error)
}

fn application_table(
    lua: &Lua,
    context: &PermittedContext,
) -> Result<Option<Table>, LuaSandboxError> {
    let Some(application) = context.application() else {
        return Ok(None);
    };
    let path = application.executable_path().to_str().ok_or(LuaSandboxError::HostValueInvalid)?;
    let fields = vec![
        (
            "executable_path".to_owned(),
            Value::String(lua.create_string(path).map_err(classify_mlua_error)?),
        ),
        (
            "window_class".to_owned(),
            Value::String(
                lua.create_string(application.window_class()).map_err(classify_mlua_error)?,
            ),
        ),
    ];
    read_only_table(lua, fields).map(Some).map_err(classify_mlua_error)
}

fn parameter_table(lua: &Lua, parameters: &PluginParameters) -> Result<Table, LuaSandboxError> {
    let fields = parameters
        .iter()
        .map(|(key, value)| {
            let value = match value {
                PluginValue::Boolean(value) => Value::Boolean(*value),
                PluginValue::Integer(value) => Value::Integer(*value),
                PluginValue::Number(value) => Value::Number(*value),
                PluginValue::String(value) | PluginValue::Choice(value) => {
                    Value::String(lua.create_string(value)?)
                }
            };
            Ok((key.clone(), value))
        })
        .collect::<Result<Vec<_>, mlua::Error>>()
        .map_err(classify_mlua_error)?;
    read_only_table(lua, fields).map_err(classify_mlua_error)
}

fn sample_table(lua: &Lua, sample: ContinuousSample) -> Result<Table, LuaSandboxError> {
    let sequence =
        i64::try_from(sample.sequence()).map_err(|_| LuaSandboxError::HostValueInvalid)?;
    let elapsed_ms =
        i64::try_from(sample.elapsed_ms()).map_err(|_| LuaSandboxError::HostValueInvalid)?;
    read_only_table(
        lua,
        vec![
            ("sequence".to_owned(), Value::Integer(sequence)),
            ("value".to_owned(), Value::Number(sample.value())),
            ("delta".to_owned(), Value::Number(sample.delta())),
            ("elapsed_ms".to_owned(), Value::Integer(elapsed_ms)),
        ],
    )
    .map_err(classify_mlua_error)
}

fn optional_string(lua: &Lua, value: Option<&str>) -> Result<Value, LuaSandboxError> {
    value.map_or(Ok(Value::Nil), |value| {
        lua.create_string(value).map(Value::String).map_err(classify_mlua_error)
    })
}

fn read_only_table(lua: &Lua, fields: Vec<(String, Value)>) -> mlua::Result<Table> {
    let backing = lua.create_table()?;
    let keys = fields.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>();
    for (key, value) in fields {
        backing.raw_set(key, value)?;
    }

    let proxy = lua.create_table()?;
    let metatable = lua.create_table()?;
    metatable.raw_set("__index", backing.clone())?;
    metatable.raw_set(
        "__newindex",
        lua.create_function(|_, (_table, _key, _value): (Table, Value, Value)| {
            Err::<(), _>(mlua::Error::runtime("gradial_callback_argument_read_only"))
        })?,
    )?;
    let field_count = keys.len();
    metatable.raw_set("__len", lua.create_function(move |_, _proxy: Table| Ok(field_count))?)?;

    let keys = Arc::new(keys);
    metatable.raw_set(
        "__pairs",
        lua.create_function(move |lua, _proxy: Table| {
            let iteration_keys = Arc::clone(&keys);
            let iteration_backing = backing.clone();
            let iterator =
                lua.create_function(move |lua, (_state, previous): (Value, Value)| {
                    let next_index = match previous {
                        Value::Nil => 0,
                        Value::String(previous) => iteration_keys
                            .iter()
                            .position(|key| previous.as_bytes().as_ref() == key.as_bytes())
                            .map_or(iteration_keys.len(), |index| index + 1),
                        _ => iteration_keys.len(),
                    };
                    let Some(key) = iteration_keys.get(next_index) else {
                        return Ok((Value::Nil, Value::Nil));
                    };
                    let value: Value = iteration_backing.raw_get(key.as_str())?;
                    Ok((Value::String(lua.create_string(key)?), value))
                })?;
            Ok((iterator, Value::Nil, Value::Nil))
        })?,
    )?;
    metatable.raw_set("__metatable", false)?;
    proxy.set_metatable(Some(metatable))?;
    Ok(proxy)
}

const fn cancel_reason(reason: ContinuousCancelReason) -> &'static str {
    match reason {
        ContinuousCancelReason::ReturnedToDiscrete => "returned_to_discrete",
        ContinuousCancelReason::DirectionChanged => "direction_changed",
        ContinuousCancelReason::UserRequested => "user_requested",
        ContinuousCancelReason::Escape => "escape",
        ContinuousCancelReason::OtherMouseButton => "other_mouse_button",
        ContinuousCancelReason::InputQueueOverflow => "input_queue_overflow",
        ContinuousCancelReason::HookUnresponsive => "hook_unresponsive",
        ContinuousCancelReason::InputReregistration => "input_reregistration",
        ContinuousCancelReason::DisplayTopologyChange => "display_topology_change",
        ContinuousCancelReason::TargetStale => "target_stale",
        ContinuousCancelReason::PluginFaulted => "plugin_faulted",
        ContinuousCancelReason::CapabilityUnavailable => "capability_unavailable",
        ContinuousCancelReason::ReloadRequested => "reload_requested",
        ContinuousCancelReason::PluginDisabled => "plugin_disabled",
        ContinuousCancelReason::ConfigurationChanged => "configuration_changed",
        ContinuousCancelReason::SessionLocked => "session_locked",
        ContinuousCancelReason::Shutdown => "shutdown",
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf, time::Duration};

    use gradial_core::{
        ContinuousActionSnapshot, ContinuousRequest, ContinuousSession,
        PermittedApplicationContext, PluginActionId, PluginId,
    };

    use super::*;

    fn image(source: &str) -> PluginSourceImage {
        PluginSourceImage::from_files(
            "main.lua",
            vec![(PathBuf::from("main.lua"), source.as_bytes().to_vec())],
        )
        .expect("source image")
    }

    fn source(begin: &str, update: &str, commit: &str, cancel: &str) -> String {
        format!(
            r"
            local plugin = {{}}
            function plugin.begin(context, parameters, initial) {begin} end
            function plugin.update(session, sample) {update} end
            function plugin.commit(session, sample) {commit} end
            function plugin.cancel(session, reason) {cancel} end
            return plugin
            "
        )
    }

    fn action() -> ContinuousActionSnapshot {
        let mut parameters = BTreeMap::new();
        parameters.insert("enabled".to_owned(), PluginValue::Boolean(true));
        parameters.insert("count".to_owned(), PluginValue::Integer(7));
        parameters.insert("amount".to_owned(), PluginValue::Number(0.5));
        parameters.insert("label".to_owned(), PluginValue::String("quiet".to_owned()));
        parameters.insert("mode".to_owned(), PluginValue::Choice("smooth".to_owned()));
        ContinuousActionSnapshot::new(
            PluginId::new("org.gradial.meter").expect("plugin ID"),
            PluginActionId::new("set-value").expect("action ID"),
            PermittedContext::new(
                Some(PermittedApplicationContext::new(
                    PathBuf::from(r"C:\Program Files\Example\example.exe"),
                    "ExampleWindow".to_owned(),
                )),
                Some("Frozen title".to_owned()),
            ),
            parameters,
            30,
        )
        .expect("action")
    }

    fn begin_request(session_id: Uuid) -> (ContinuousSession, ContinuousBeginRequest) {
        let (session, request) =
            ContinuousSession::begin(session_id, action(), 0.25).expect("begin request");
        let ContinuousRequest::Begin(request) = request else {
            panic!("expected begin request");
        };
        (session, request)
    }

    fn update_request(session: &mut ContinuousSession, value: f64) -> ContinuousUpdateRequest {
        if session.state() == gradial_core::ContinuousSessionState::BeginPending {
            assert!(session.accept_begin(Duration::ZERO).expect("accept begin").is_none());
        }
        session.observe(value, Duration::from_millis(40)).expect("observe");
        let request = session
            .take_due_update(Duration::from_millis(40))
            .expect("take update")
            .expect("due update");
        let ContinuousRequest::Update(request) = request else {
            panic!("expected update request");
        };
        request
    }

    fn commit_request(session: &mut ContinuousSession, value: f64) -> ContinuousCommitRequest {
        if session.state() == gradial_core::ContinuousSessionState::BeginPending {
            assert!(session.accept_begin(Duration::ZERO).expect("accept begin").is_none());
        }
        let request = session
            .commit(value, Duration::from_millis(60))
            .expect("commit")
            .expect("commit request");
        let ContinuousRequest::Commit(request) = request else {
            panic!("expected commit request");
        };
        request
    }

    fn cancel_request(
        session: &mut ContinuousSession,
        reason: ContinuousCancelReason,
    ) -> ContinuousCancelRequest {
        if session.state() == gradial_core::ContinuousSessionState::BeginPending {
            assert!(session.accept_begin(Duration::ZERO).expect("accept begin").is_none());
        }
        let request = session.cancel(reason).expect("cancel").expect("cancel request");
        let ContinuousRequest::Cancel(request) = request else {
            panic!("expected cancel request");
        };
        request
    }

    fn entry(adapter: &LuaLifecycleAdapter) -> Table {
        adapter.plugin.lua.registry_value(&adapter.plugin.entry).expect("entry table")
    }

    #[test]
    fn adapter_requires_all_four_functions() {
        let missing = LuaLifecycleAdapter::load(
            image("return { begin = function() return {} end }"),
            LuaSandboxLimits::default(),
        );
        assert_eq!(
            missing.expect_err("missing update"),
            LuaSandboxError::CallbackMissing { callback: "update" }
        );

        let wrong = LuaLifecycleAdapter::load(
            image(
                "return { begin = function() return {} end, update = true, commit = function() end, cancel = function() end }",
            ),
            LuaSandboxLimits::default(),
        );
        assert_eq!(
            wrong.expect_err("invalid update"),
            LuaSandboxError::CallbackNotFunction { callback: "update" }
        );
    }

    #[test]
    fn lifecycle_receives_read_only_complete_values_and_mutable_private_state() {
        let source = source(
            r#"
                assert(context.api_version == 1)
                assert(context.action_id == "set-value")
                assert(type(context.session_id) == "string")
                assert(context.application.executable_path == "C:\\Program Files\\Example\\example.exe")
                assert(context.application.window_class == "ExampleWindow")
                assert(context.window_title == "Frozen title")
                assert(parameters.enabled and parameters.count == 7 and parameters.amount == 0.5)
                assert(parameters.label == "quiet" and parameters.mode == "smooth")
                assert(initial.sequence == 0 and initial.value == 0.25)
                assert(not pcall(function() context.action_id = "changed" end))
                assert(not pcall(function() context.application.window_class = "changed" end))
                assert(not pcall(function() parameters.amount = 0.75 end))
                assert(not pcall(function() initial.value = 0.75 end))
                local count = 0
                for _ in pairs(parameters) do count = count + 1 end
                assert(count == 5)
                local _, exposed_state = pairs(parameters)
                assert(exposed_state == nil)
                return { updates = 0, identity = context.session_id }
            "#,
            r"
                assert(not pcall(function() sample.delta = 99 end))
                assert(sample.sequence == 1 and sample.value == 0.6 and sample.delta == 0.35)
                session.updates = session.updates + 1
                plugin.last_identity = session.identity
            ",
            r"
                assert(sample.sequence == 2 and sample.value == 0.8)
                assert(math.abs(sample.delta - 0.2) < 0.000000001)
                plugin.committed_updates = session.updates
            ",
            "error('cancel not expected')",
        );
        let mut adapter = LuaLifecycleAdapter::load(image(&source), LuaSandboxLimits::default())
            .expect("adapter");
        let session_id = Uuid::from_u128(1);
        let (mut session, begin) = begin_request(session_id);

        adapter.begin(&begin).expect("begin");
        adapter.update(&update_request(&mut session, 0.6)).expect("update");
        adapter.commit(&commit_request(&mut session, 0.8)).expect("commit");

        assert_eq!(adapter.active_session_count(), 0);
        assert!(!adapter.is_faulted());
        let entry = entry(&adapter);
        assert_eq!(entry.raw_get::<i64>("committed_updates").expect("updates"), 1);
        assert_eq!(
            entry.raw_get::<String>("last_identity").expect("identity"),
            session_id.to_string()
        );
    }

    #[test]
    fn absent_context_permissions_are_nil_in_lua() {
        let source = source(
            r"
                assert(context.application == nil)
                assert(context.window_title == nil)
                return {}
            ",
            "",
            "",
            "",
        );
        let mut adapter = LuaLifecycleAdapter::load(image(&source), LuaSandboxLimits::default())
            .expect("adapter");
        let snapshot = ContinuousActionSnapshot::new(
            PluginId::new("org.gradial.meter").expect("plugin ID"),
            PluginActionId::new("set-value").expect("action ID"),
            PermittedContext::default(),
            BTreeMap::new(),
            30,
        )
        .expect("action");
        let (_session, request) =
            ContinuousSession::begin(Uuid::from_u128(9), snapshot, 0.0).expect("begin request");
        let ContinuousRequest::Begin(request) = request else {
            panic!("expected begin request");
        };

        adapter.begin(&request).expect("begin without context permissions");
    }

    #[test]
    fn session_tables_are_isolated_and_terminal_calls_are_once_only() {
        let source = source(
            "return { identity = context.session_id, updates = 0 }",
            "session.updates = session.updates + 1",
            "plugin.committed = session.identity .. ':' .. session.updates",
            "plugin.cancelled = session.identity .. ':' .. reason",
        );
        let mut adapter = LuaLifecycleAdapter::load(image(&source), LuaSandboxLimits::default())
            .expect("adapter");
        let (mut first, first_begin) = begin_request(Uuid::from_u128(1));
        let (mut second, second_begin) = begin_request(Uuid::from_u128(2));
        adapter.begin(&first_begin).expect("first begin");
        adapter.begin(&second_begin).expect("second begin");
        adapter.update(&update_request(&mut first, 0.5)).expect("first update");
        adapter
            .cancel(&cancel_request(&mut second, ContinuousCancelReason::Escape))
            .expect("second cancel");
        adapter.commit(&commit_request(&mut first, 0.75)).expect("first commit");

        assert_eq!(adapter.active_session_count(), 0);
        let entry = entry(&adapter);
        assert_eq!(
            entry.raw_get::<String>("committed").expect("committed"),
            format!("{}:1", Uuid::from_u128(1))
        );
        assert_eq!(
            entry.raw_get::<String>("cancelled").expect("cancelled"),
            format!("{}:escape", Uuid::from_u128(2))
        );
        assert_eq!(
            adapter.commit(&commit_request_after_terminal(Uuid::from_u128(1))),
            Err(LuaSandboxError::SessionNotActive)
        );
        assert!(!adapter.is_faulted());
    }

    fn commit_request_after_terminal(session_id: Uuid) -> ContinuousCommitRequest {
        let (mut session, _) = begin_request(session_id);
        session.accept_begin(Duration::ZERO).expect("accept begin");
        commit_request(&mut session, 0.75)
    }

    #[test]
    fn duplicate_and_unknown_sessions_are_rejected_without_faulting_plugin() {
        let source = source("return {}", "", "", "");
        let mut adapter = LuaLifecycleAdapter::load(image(&source), LuaSandboxLimits::default())
            .expect("adapter");
        let (_, begin) = begin_request(Uuid::from_u128(1));
        let (mut unknown, _) = begin_request(Uuid::from_u128(2));
        unknown.accept_begin(Duration::ZERO).expect("accept unknown");

        adapter.begin(&begin).expect("begin");
        assert_eq!(adapter.begin(&begin), Err(LuaSandboxError::SessionAlreadyActive));
        assert_eq!(
            adapter.update(&update_request(&mut unknown, 0.5)),
            Err(LuaSandboxError::SessionNotActive)
        );
        assert!(!adapter.is_faulted());
        assert_eq!(adapter.active_session_count(), 1);
    }

    #[test]
    fn invalid_begin_or_callback_return_faults_and_clears_sessions() {
        let invalid_begin = source("return 42", "", "", "");
        let mut adapter =
            LuaLifecycleAdapter::load(image(&invalid_begin), LuaSandboxLimits::default())
                .expect("adapter");
        let (_, begin) = begin_request(Uuid::from_u128(1));
        assert_eq!(adapter.begin(&begin), Err(LuaSandboxError::BeginStateNotTable));
        assert!(adapter.is_faulted());
        assert_eq!(adapter.begin(&begin), Err(LuaSandboxError::PluginFaulted));

        let invalid_update = source("return {}", "return true", "", "");
        let mut adapter =
            LuaLifecycleAdapter::load(image(&invalid_update), LuaSandboxLimits::default())
                .expect("adapter");
        let (mut first, first_begin) = begin_request(Uuid::from_u128(2));
        let (_, second_begin) = begin_request(Uuid::from_u128(3));
        adapter.begin(&first_begin).expect("first begin");
        adapter.begin(&second_begin).expect("second begin");
        assert_eq!(
            adapter.update(&update_request(&mut first, 0.5)),
            Err(LuaSandboxError::CallbackReturnInvalid)
        );
        assert!(adapter.is_faulted());
        assert_eq!(adapter.active_session_count(), 0);
    }

    #[test]
    fn each_phase_enforces_its_own_instruction_budget() {
        for phase in ["begin", "update", "commit", "cancel"] {
            let body = "while true do end";
            let source = source(
                if phase == "begin" { body } else { "return {}" },
                if phase == "update" { body } else { "" },
                if phase == "commit" { body } else { "" },
                if phase == "cancel" { body } else { "" },
            );
            let tight =
                LuaCallbackBudget::new(Duration::from_millis(10), 10_000).expect("tight budget");
            let limits = LuaCallbackLimits::new(tight, tight, tight, tight).expect("limits");
            let mut adapter = LuaLifecycleAdapter::load_with_callback_limits(
                image(&source),
                LuaSandboxLimits::default(),
                limits,
            )
            .expect("adapter");
            let (mut session, begin) = begin_request(Uuid::from_u128(4));

            let result = if phase == "begin" {
                adapter.begin(&begin)
            } else {
                adapter.begin(&begin).expect("begin");
                match phase {
                    "update" => adapter.update(&update_request(&mut session, 0.5)),
                    "commit" => {
                        session.accept_begin(Duration::ZERO).expect("accept begin");
                        adapter.commit(&commit_request(&mut session, 0.5))
                    }
                    "cancel" => {
                        session.accept_begin(Duration::ZERO).expect("accept begin");
                        adapter.cancel(&cancel_request(
                            &mut session,
                            ContinuousCancelReason::UserRequested,
                        ))
                    }
                    _ => unreachable!(),
                }
            };

            assert_eq!(result, Err(LuaSandboxError::InstructionLimit), "phase {phase}");
            assert!(adapter.is_faulted());
            assert_eq!(adapter.active_session_count(), 0);
        }
    }

    #[test]
    fn callback_wall_deadline_remains_distinct_from_instruction_limit() {
        let source = source("return {}", "while true do end", "", "");
        let deadline =
            LuaCallbackBudget::new(Duration::from_nanos(1), DEFAULT_LUA_UPDATE_INSTRUCTION_LIMIT)
                .expect("deadline budget");
        let defaults = LuaCallbackLimits::default();
        let limits =
            LuaCallbackLimits::new(defaults.begin, deadline, defaults.commit, defaults.cancel)
                .expect("limits");
        let mut adapter = LuaLifecycleAdapter::load_with_callback_limits(
            image(&source),
            LuaSandboxLimits::default(),
            limits,
        )
        .expect("adapter");
        let (mut session, begin) = begin_request(Uuid::from_u128(5));
        adapter.begin(&begin).expect("begin");

        assert_eq!(
            adapter.update(&update_request(&mut session, 0.5)),
            Err(LuaSandboxError::DeadlineExceeded)
        );
        assert!(adapter.is_faulted());
    }

    #[test]
    fn phase_limits_cannot_be_raised() {
        let too_slow =
            LuaCallbackBudget::new(Duration::from_millis(17), 10_000).expect("generic budget");
        assert_eq!(
            LuaCallbackLimits::new(
                LuaCallbackLimits::default().begin,
                too_slow,
                LuaCallbackLimits::default().commit,
                LuaCallbackLimits::default().cancel,
            ),
            Err(LuaSandboxError::InvalidLimits)
        );
        let too_many =
            LuaCallbackBudget::new(LUA_UPDATE_TIMEOUT, DEFAULT_LUA_UPDATE_INSTRUCTION_LIMIT + 1)
                .expect("generic instruction budget");
        assert_eq!(
            LuaCallbackLimits::new(
                LuaCallbackLimits::default().begin,
                too_many,
                LuaCallbackLimits::default().commit,
                LuaCallbackLimits::default().cancel,
            ),
            Err(LuaSandboxError::InvalidLimits)
        );
    }

    #[test]
    fn adapter_is_sendable_and_debug_output_is_content_free() {
        fn assert_send<T: Send>() {}
        assert_send::<LuaLifecycleAdapter>();
        let source = source("return { secret = 'private' }", "", "", "");
        let adapter = LuaLifecycleAdapter::load(image(&source), LuaSandboxLimits::default())
            .expect("adapter");
        let debug = format!("{adapter:?}");
        assert!(!debug.contains("private"));
        assert!(debug.contains("active_sessions"));
    }
}
