use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const CONTINUOUS_API_VERSION: u16 = 1;
pub const DEFAULT_CONTINUOUS_UPDATE_HZ: u16 = 30;
pub const MIN_CONTINUOUS_UPDATE_HZ: u16 = 10;
pub const MAX_CONTINUOUS_UPDATE_HZ: u16 = 60;
pub const MAX_CONTINUOUS_IDENTIFIER_BYTES: usize = 96;
pub const MAX_PLUGIN_PARAMETERS_SERIALIZED_BYTES: usize = 16 * 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuousIdentifierKind {
    Plugin,
    Action,
}

impl ContinuousIdentifierKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Plugin => "plugin ID",
            Self::Action => "plugin action ID",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PluginId(String);

impl PluginId {
    /// Creates a validated manifest plugin identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ContinuousProtocolError::InvalidIdentifier`] when the value
    /// is empty, exceeds 96 bytes, or is outside the closed ASCII grammar.
    pub fn new(value: impl Into<String>) -> Result<Self, ContinuousProtocolError> {
        let value = value.into();
        validate_identifier(&value, ContinuousIdentifierKind::Plugin)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PluginActionId(String);

impl PluginActionId {
    /// Creates a validated manifest action identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ContinuousProtocolError::InvalidIdentifier`] when the value
    /// is empty, exceeds 96 bytes, or is outside the closed ASCII grammar.
    pub fn new(value: impl Into<String>) -> Result<Self, ContinuousProtocolError> {
        let value = value.into();
        validate_identifier(&value, ContinuousIdentifierKind::Action)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_identifier(
    value: &str,
    kind: ContinuousIdentifierKind,
) -> Result<(), ContinuousProtocolError> {
    if value.is_empty()
        || value.len() > MAX_CONTINUOUS_IDENTIFIER_BYTES
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
    {
        return Err(ContinuousProtocolError::InvalidIdentifier { kind });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermittedApplicationContext {
    executable_path: PathBuf,
    window_class: String,
}

impl PermittedApplicationContext {
    #[must_use]
    pub fn new(executable_path: PathBuf, window_class: String) -> Self {
        Self { executable_path, window_class }
    }

    #[must_use]
    pub fn executable_path(&self) -> &Path {
        &self.executable_path
    }

    #[must_use]
    pub fn window_class(&self) -> &str {
        &self.window_class
    }
}

/// A permission-filtered, owned context snapshot. It deliberately contains no
/// PID, window handle, process handle, screen coordinate, or mutable target.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PermittedContext {
    application: Option<PermittedApplicationContext>,
    window_title: Option<String>,
}

impl PermittedContext {
    #[must_use]
    pub fn new(
        application: Option<PermittedApplicationContext>,
        window_title: Option<String>,
    ) -> Self {
        Self { application, window_title }
    }

    #[must_use]
    pub const fn application(&self) -> Option<&PermittedApplicationContext> {
        self.application.as_ref()
    }

    #[must_use]
    pub fn window_title(&self) -> Option<&str> {
        self.window_title.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginValue {
    Boolean(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Choice(String),
}

pub type PluginParameters = BTreeMap<String, PluginValue>;

struct ParameterSizeWriter {
    bytes: usize,
}

impl Write for ParameterSizeWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let Some(next) = self.bytes.checked_add(buffer.len()) else {
            return Err(io::Error::other("plugin parameter size overflow"));
        };
        if next > MAX_PLUGIN_PARAMETERS_SERIALIZED_BYTES {
            return Err(io::Error::other("plugin parameters exceed the serialized size limit"));
        }
        self.bytes = next;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn plugin_parameters_fit_serialized_limit(parameters: &PluginParameters) -> bool {
    let mut writer = ParameterSizeWriter { bytes: 0 };
    serde_json::to_writer(&mut writer, parameters).is_ok()
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContinuousActionSnapshot {
    plugin_id: PluginId,
    action_id: PluginActionId,
    context: PermittedContext,
    parameters: PluginParameters,
    update_hz: u16,
}

impl ContinuousActionSnapshot {
    /// Freezes the action data that may cross the runtime-neutral boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when the update rate is outside 10–60 Hz or a numeric
    /// parameter is not finite. Manifest-specific parameter bounds are applied
    /// by the later configuration slice before this snapshot is constructed.
    pub fn new(
        plugin_id: PluginId,
        action_id: PluginActionId,
        context: PermittedContext,
        parameters: PluginParameters,
        update_hz: u16,
    ) -> Result<Self, ContinuousProtocolError> {
        if !(MIN_CONTINUOUS_UPDATE_HZ..=MAX_CONTINUOUS_UPDATE_HZ).contains(&update_hz) {
            return Err(ContinuousProtocolError::InvalidUpdateRate { update_hz });
        }
        if parameters
            .values()
            .any(|value| matches!(value, PluginValue::Number(number) if !number.is_finite()))
        {
            return Err(ContinuousProtocolError::InvalidParameterNumber);
        }
        if !plugin_parameters_fit_serialized_limit(&parameters) {
            return Err(ContinuousProtocolError::ParametersTooLarge {
                maximum: MAX_PLUGIN_PARAMETERS_SERIALIZED_BYTES,
            });
        }
        Ok(Self { plugin_id, action_id, context, parameters, update_hz })
    }

    #[must_use]
    pub const fn update_hz(&self) -> u16 {
        self.update_hz
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContinuousSample {
    sequence: u64,
    value: f64,
    delta: f64,
    elapsed_ms: u64,
}

impl ContinuousSample {
    fn initial(value: f64) -> Self {
        Self { sequence: 0, value, delta: 0.0, elapsed_ms: 0 }
    }

    #[must_use]
    pub const fn sequence(self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub const fn value(self) -> f64 {
        self.value
    }

    #[must_use]
    pub const fn delta(self) -> f64 {
        self.delta
    }

    #[must_use]
    pub const fn elapsed_ms(self) -> u64 {
        self.elapsed_ms
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContinuousBeginRequest {
    api_version: u16,
    session_id: Uuid,
    plugin_id: PluginId,
    action_id: PluginActionId,
    context: PermittedContext,
    parameters: PluginParameters,
    initial: ContinuousSample,
}

impl ContinuousBeginRequest {
    #[must_use]
    pub const fn api_version(&self) -> u16 {
        self.api_version
    }

    #[must_use]
    pub const fn session_id(&self) -> Uuid {
        self.session_id
    }

    #[must_use]
    pub const fn plugin_id(&self) -> &PluginId {
        &self.plugin_id
    }

    #[must_use]
    pub const fn action_id(&self) -> &PluginActionId {
        &self.action_id
    }

    #[must_use]
    pub const fn context(&self) -> &PermittedContext {
        &self.context
    }

    #[must_use]
    pub const fn parameters(&self) -> &PluginParameters {
        &self.parameters
    }

    #[must_use]
    pub const fn initial(&self) -> ContinuousSample {
        self.initial
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContinuousUpdateRequest {
    session_id: Uuid,
    sample: ContinuousSample,
}

impl ContinuousUpdateRequest {
    #[must_use]
    pub const fn session_id(self) -> Uuid {
        self.session_id
    }

    #[must_use]
    pub const fn sample(self) -> ContinuousSample {
        self.sample
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContinuousCommitRequest {
    session_id: Uuid,
    final_sample: ContinuousSample,
}

impl ContinuousCommitRequest {
    #[must_use]
    pub const fn session_id(self) -> Uuid {
        self.session_id
    }

    #[must_use]
    pub const fn final_sample(self) -> ContinuousSample {
        self.final_sample
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuousCancelReason {
    ReturnedToDiscrete,
    DirectionChanged,
    UserRequested,
    Escape,
    OtherMouseButton,
    InputQueueOverflow,
    HookUnresponsive,
    InputReregistration,
    DisplayTopologyChange,
    TargetStale,
    PluginFaulted,
    CapabilityUnavailable,
    ReloadRequested,
    PluginDisabled,
    ConfigurationChanged,
    SessionLocked,
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContinuousCancelRequest {
    session_id: Uuid,
    reason: ContinuousCancelReason,
}

impl ContinuousCancelRequest {
    /// Creates a standalone best-effort cancellation request for a Begin that
    /// may still be waiting in a runtime queue.
    ///
    /// # Errors
    ///
    /// Returns an error for the nil session identifier.
    pub fn new(
        session_id: Uuid,
        reason: ContinuousCancelReason,
    ) -> Result<Self, ContinuousProtocolError> {
        if session_id.is_nil() {
            return Err(ContinuousProtocolError::InvalidSessionId);
        }
        Ok(Self { session_id, reason })
    }

    #[must_use]
    pub const fn session_id(self) -> Uuid {
        self.session_id
    }

    #[must_use]
    pub const fn reason(self) -> ContinuousCancelReason {
        self.reason
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ContinuousRequest {
    Begin(ContinuousBeginRequest),
    Update(ContinuousUpdateRequest),
    Commit(ContinuousCommitRequest),
    Cancel(ContinuousCancelRequest),
}

impl ContinuousRequest {
    #[must_use]
    pub const fn session_id(&self) -> Uuid {
        match self {
            Self::Begin(request) => request.session_id(),
            Self::Update(request) => request.session_id(),
            Self::Commit(request) => request.session_id(),
            Self::Cancel(request) => request.session_id(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuousSessionState {
    BeginPending,
    Active,
    Terminated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuousSessionTerminal {
    BeginRejected,
    Committed,
    Cancelled(ContinuousCancelReason),
}

#[derive(Clone, Copy, Debug)]
struct PendingObservation {
    sequence: u64,
    value: f64,
    observed_at: Duration,
}

#[derive(Clone, Copy, Debug)]
enum PendingTerminal {
    Commit(PendingObservation),
    Cancel(ContinuousCancelReason),
}

#[derive(Debug)]
enum SessionPhase {
    BeginPending { terminal: Option<PendingTerminal> },
    Active,
    Terminated(ContinuousSessionTerminal),
}

/// Owns the protocol lifecycle for one outside-wedge segment.
///
/// Time is supplied by the host so tests and future runtime adapters do not
/// depend on a particular clock. At most one undelivered ordinary sample is
/// retained; terminal decisions are never coalesced away.
#[derive(Debug)]
pub struct ContinuousSession {
    session_id: Uuid,
    update_interval: Duration,
    phase: SessionPhase,
    next_sequence: u64,
    latest_value: f64,
    last_delivered_value: f64,
    pending_update: Option<PendingObservation>,
    last_observed_at: Option<Duration>,
    accepted_at: Option<Duration>,
    next_update_due: Option<Duration>,
}

impl ContinuousSession {
    /// Creates one segment and its only `Begin` request.
    ///
    /// # Errors
    ///
    /// Returns an error for a nil session ID or an initial scalar that is not
    /// finite and inclusive of zero through one.
    pub fn begin(
        session_id: Uuid,
        snapshot: ContinuousActionSnapshot,
        initial_value: f64,
    ) -> Result<(Self, ContinuousRequest), ContinuousProtocolError> {
        if session_id.is_nil() {
            return Err(ContinuousProtocolError::InvalidSessionId);
        }
        validate_sample_value(initial_value)?;

        let ContinuousActionSnapshot { plugin_id, action_id, context, parameters, update_hz } =
            snapshot;
        let initial = ContinuousSample::initial(initial_value);
        let request = ContinuousRequest::Begin(ContinuousBeginRequest {
            api_version: CONTINUOUS_API_VERSION,
            session_id,
            plugin_id,
            action_id,
            context,
            parameters,
            initial,
        });
        let update_interval =
            Duration::from_nanos(1_000_000_000_u64.div_ceil(u64::from(update_hz)));
        Ok((
            Self {
                session_id,
                update_interval,
                phase: SessionPhase::BeginPending { terminal: None },
                next_sequence: 1,
                latest_value: initial_value,
                last_delivered_value: initial_value,
                pending_update: None,
                last_observed_at: None,
                accepted_at: None,
                next_update_due: None,
            },
            request,
        ))
    }

    #[must_use]
    pub const fn session_id(&self) -> Uuid {
        self.session_id
    }

    #[must_use]
    pub const fn state(&self) -> ContinuousSessionState {
        match self.phase {
            SessionPhase::BeginPending { .. } => ContinuousSessionState::BeginPending,
            SessionPhase::Active => ContinuousSessionState::Active,
            SessionPhase::Terminated(_) => ContinuousSessionState::Terminated,
        }
    }

    #[must_use]
    pub const fn terminal(&self) -> Option<ContinuousSessionTerminal> {
        match self.phase {
            SessionPhase::Terminated(terminal) => Some(terminal),
            SessionPhase::BeginPending { .. } | SessionPhase::Active => None,
        }
    }

    #[must_use]
    pub const fn latest_value(&self) -> f64 {
        self.latest_value
    }

    #[must_use]
    pub const fn update_interval(&self) -> Duration {
        self.update_interval
    }

    /// Records that the backend accepted `Begin`.
    ///
    /// A terminal decision made while `Begin` was pending is returned
    /// immediately, preserving `Begin` then terminal ordering.
    ///
    /// # Errors
    ///
    /// Returns [`ContinuousProtocolError::InvalidTransition`] unless `Begin`
    /// is pending.
    pub fn accept_begin(
        &mut self,
        accepted_at: Duration,
    ) -> Result<Option<ContinuousRequest>, ContinuousProtocolError> {
        let SessionPhase::BeginPending { terminal } = &mut self.phase else {
            return Err(ContinuousProtocolError::InvalidTransition {
                operation: "accept_begin",
                state: self.state(),
            });
        };
        let terminal = terminal.take();
        self.accepted_at = Some(accepted_at);
        self.next_update_due = accepted_at.checked_add(self.update_interval);

        match terminal {
            None => {
                self.phase = SessionPhase::Active;
                Ok(None)
            }
            Some(PendingTerminal::Commit(observation)) => {
                let request = self.commit_request(observation, accepted_at);
                self.phase = SessionPhase::Terminated(ContinuousSessionTerminal::Committed);
                Ok(Some(request))
            }
            Some(PendingTerminal::Cancel(reason)) => {
                self.phase = SessionPhase::Terminated(ContinuousSessionTerminal::Cancelled(reason));
                Ok(Some(self.cancel_request(reason)))
            }
        }
    }

    /// Records a rejected `Begin` without generating a terminal callback.
    ///
    /// # Errors
    ///
    /// Returns [`ContinuousProtocolError::InvalidTransition`] unless `Begin`
    /// is pending.
    pub fn reject_begin(&mut self) -> Result<(), ContinuousProtocolError> {
        if !matches!(self.phase, SessionPhase::BeginPending { .. }) {
            return Err(ContinuousProtocolError::InvalidTransition {
                operation: "reject_begin",
                state: self.state(),
            });
        }
        self.pending_update = None;
        self.phase = SessionPhase::Terminated(ContinuousSessionTerminal::BeginRejected);
        Ok(())
    }

    /// Publishes complete current state into the single latest-value slot.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid scalar, an exhausted sequence, or a
    /// session whose terminal decision has already been made.
    pub fn observe(
        &mut self,
        value: f64,
        observed_at: Duration,
    ) -> Result<(), ContinuousProtocolError> {
        self.observe_coalesced(value, observed_at, 1)
    }

    /// Publishes the latest value after one or more upstream observations.
    ///
    /// `observation_count` includes the retained value. Replaced upstream
    /// values advance the sequence so the backend can detect the gap without
    /// requiring an event queue.
    ///
    /// # Errors
    ///
    /// Returns an error for a zero count, invalid scalar, exhausted sequence,
    /// backward timestamp, or a session whose terminal decision already
    /// exists.
    pub fn observe_coalesced(
        &mut self,
        value: f64,
        observed_at: Duration,
        observation_count: u64,
    ) -> Result<(), ContinuousProtocolError> {
        validate_sample_value(value)?;
        if observation_count == 0 {
            return Err(ContinuousProtocolError::InvalidObservationCount);
        }
        match self.phase {
            SessionPhase::BeginPending { terminal: None } | SessionPhase::Active => {}
            SessionPhase::BeginPending { terminal: Some(_) } | SessionPhase::Terminated(_) => {
                return Err(ContinuousProtocolError::InvalidTransition {
                    operation: "observe",
                    state: self.state(),
                });
            }
        }
        self.validate_observed_at(observed_at)?;
        let sequence = self.allocate_sequences(observation_count)?;
        self.latest_value = value;
        self.pending_update = Some(PendingObservation { sequence, value, observed_at });
        self.last_observed_at = Some(observed_at);
        Ok(())
    }

    /// Takes the coalesced ordinary update when the configured cadence is due.
    ///
    /// # Errors
    ///
    /// Returns [`ContinuousProtocolError::ClockUnavailable`] only if internal
    /// accepted-session clock state is inconsistent.
    pub fn take_due_update(
        &mut self,
        now: Duration,
    ) -> Result<Option<ContinuousRequest>, ContinuousProtocolError> {
        if !matches!(self.phase, SessionPhase::Active) {
            return Ok(None);
        }
        let Some(due_at) = self.next_update_due else {
            return Err(ContinuousProtocolError::ClockUnavailable);
        };
        if now < due_at {
            return Ok(None);
        }
        let Some(observation) = self.pending_update.take() else {
            return Ok(None);
        };
        let Some(accepted_at) = self.accepted_at else {
            return Err(ContinuousProtocolError::ClockUnavailable);
        };
        let sample = self.sample(observation, accepted_at);
        self.last_delivered_value = observation.value;
        self.next_update_due = now.checked_add(self.update_interval);
        Ok(Some(ContinuousRequest::Update(ContinuousUpdateRequest {
            session_id: self.session_id,
            sample,
        })))
    }

    /// Makes an exact final scalar the session's one commit decision.
    ///
    /// When `Begin` is pending, the decision is retained and this returns
    /// `None`; accepting `Begin` then yields the commit request.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid scalar, an exhausted sequence, or a
    /// session that already has a terminal decision.
    pub fn commit(
        &mut self,
        final_value: f64,
        observed_at: Duration,
    ) -> Result<Option<ContinuousRequest>, ContinuousProtocolError> {
        validate_sample_value(final_value)?;
        if matches!(self.phase, SessionPhase::Terminated(_))
            || matches!(self.phase, SessionPhase::BeginPending { terminal: Some(_) })
        {
            return Err(ContinuousProtocolError::InvalidTransition {
                operation: "commit",
                state: self.state(),
            });
        }
        self.validate_observed_at(observed_at)?;
        let sequence = self.allocate_sequence()?;
        let observation = PendingObservation { sequence, value: final_value, observed_at };
        self.latest_value = final_value;
        self.pending_update = None;
        self.last_observed_at = Some(observed_at);

        match &mut self.phase {
            SessionPhase::BeginPending { terminal } => {
                *terminal = Some(PendingTerminal::Commit(observation));
                Ok(None)
            }
            SessionPhase::Active => {
                let Some(accepted_at) = self.accepted_at else {
                    return Err(ContinuousProtocolError::ClockUnavailable);
                };
                let request = self.commit_request(observation, accepted_at);
                self.phase = SessionPhase::Terminated(ContinuousSessionTerminal::Committed);
                Ok(Some(request))
            }
            SessionPhase::Terminated(_) => Err(ContinuousProtocolError::InvalidTransition {
                operation: "commit",
                state: ContinuousSessionState::Terminated,
            }),
        }
    }

    /// Makes cancellation the session's one terminal decision.
    ///
    /// When `Begin` is pending, the decision is retained and this returns
    /// `None`; accepting `Begin` then yields the cancel request.
    ///
    /// # Errors
    ///
    /// Returns [`ContinuousProtocolError::InvalidTransition`] when the
    /// session already has a terminal decision.
    pub fn cancel(
        &mut self,
        reason: ContinuousCancelReason,
    ) -> Result<Option<ContinuousRequest>, ContinuousProtocolError> {
        self.pending_update = None;
        match &mut self.phase {
            SessionPhase::BeginPending { terminal: terminal @ None } => {
                *terminal = Some(PendingTerminal::Cancel(reason));
                Ok(None)
            }
            SessionPhase::Active => {
                self.phase = SessionPhase::Terminated(ContinuousSessionTerminal::Cancelled(reason));
                Ok(Some(self.cancel_request(reason)))
            }
            SessionPhase::BeginPending { terminal: Some(_) } | SessionPhase::Terminated(_) => {
                Err(ContinuousProtocolError::InvalidTransition {
                    operation: "cancel",
                    state: self.state(),
                })
            }
        }
    }

    fn allocate_sequence(&mut self) -> Result<u64, ContinuousProtocolError> {
        self.allocate_sequences(1)
    }

    fn allocate_sequences(&mut self, count: u64) -> Result<u64, ContinuousProtocolError> {
        let sequence = self
            .next_sequence
            .checked_add(count.saturating_sub(1))
            .ok_or(ContinuousProtocolError::SequenceExhausted)?;
        self.next_sequence =
            sequence.checked_add(1).ok_or(ContinuousProtocolError::SequenceExhausted)?;
        Ok(sequence)
    }

    fn validate_observed_at(&self, observed_at: Duration) -> Result<(), ContinuousProtocolError> {
        if self.last_observed_at.is_some_and(|previous| observed_at < previous)
            || matches!(self.phase, SessionPhase::Active)
                && self.accepted_at.is_some_and(|accepted_at| observed_at < accepted_at)
        {
            return Err(ContinuousProtocolError::NonMonotonicTimestamp);
        }
        Ok(())
    }

    fn sample(&self, observation: PendingObservation, accepted_at: Duration) -> ContinuousSample {
        ContinuousSample {
            sequence: observation.sequence,
            value: observation.value,
            delta: observation.value - self.last_delivered_value,
            elapsed_ms: elapsed_milliseconds(observation.observed_at, accepted_at),
        }
    }

    fn commit_request(
        &mut self,
        observation: PendingObservation,
        accepted_at: Duration,
    ) -> ContinuousRequest {
        let final_sample = self.sample(observation, accepted_at);
        self.last_delivered_value = observation.value;
        ContinuousRequest::Commit(ContinuousCommitRequest {
            session_id: self.session_id,
            final_sample,
        })
    }

    const fn cancel_request(&self, reason: ContinuousCancelReason) -> ContinuousRequest {
        ContinuousRequest::Cancel(ContinuousCancelRequest { session_id: self.session_id, reason })
    }
}

fn validate_sample_value(value: f64) -> Result<(), ContinuousProtocolError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(ContinuousProtocolError::InvalidSampleValue);
    }
    Ok(())
}

fn elapsed_milliseconds(observed_at: Duration, accepted_at: Duration) -> u64 {
    let milliseconds = observed_at.saturating_sub(accepted_at).as_millis();
    u64::try_from(milliseconds).unwrap_or(u64::MAX)
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ContinuousProtocolError {
    #[error("{kind} is empty, too long, or contains unsupported characters", kind = kind.label())]
    InvalidIdentifier { kind: ContinuousIdentifierKind },
    #[error("continuous session ID must not be nil")]
    InvalidSessionId,
    #[error("continuous update rate {update_hz} Hz is outside the supported range")]
    InvalidUpdateRate { update_hz: u16 },
    #[error("continuous parameters contain a non-finite number")]
    InvalidParameterNumber,
    #[error("continuous parameters exceed the {maximum}-byte serialized maximum")]
    ParametersTooLarge { maximum: usize },
    #[error("continuous sample must be finite and between zero and one")]
    InvalidSampleValue,
    #[error("continuous observation count must be at least one")]
    InvalidObservationCount,
    #[error("continuous sequence is exhausted")]
    SequenceExhausted,
    #[error("continuous observation timestamp moved backward")]
    NonMonotonicTimestamp,
    #[error("continuous session clock state is unavailable")]
    ClockUnavailable,
    #[error("cannot {operation} while continuous session is {state:?}")]
    InvalidTransition { operation: &'static str, state: ContinuousSessionState },
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf, time::Duration};

    use uuid::Uuid;

    use super::{
        CONTINUOUS_API_VERSION, ContinuousActionSnapshot, ContinuousCancelReason,
        ContinuousCancelRequest, ContinuousIdentifierKind, ContinuousProtocolError,
        ContinuousRequest, ContinuousSession, ContinuousSessionState, ContinuousSessionTerminal,
        MAX_PLUGIN_PARAMETERS_SERIALIZED_BYTES, PermittedApplicationContext, PermittedContext,
        PluginActionId, PluginId, PluginValue,
    };

    fn action(update_hz: u16) -> ContinuousActionSnapshot {
        let mut parameters = BTreeMap::new();
        parameters.insert("amount".to_owned(), PluginValue::Number(0.5));
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
            update_hz,
        )
        .expect("continuous action")
    }

    fn begin(session_id: Uuid, value: f64) -> (ContinuousSession, ContinuousRequest) {
        ContinuousSession::begin(session_id, action(30), value).expect("valid begin")
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < f64::EPSILON, "expected {expected}, got {actual}");
    }

    #[test]
    fn begin_owns_a_versioned_permission_filtered_snapshot() {
        let session_id = Uuid::from_u128(1);
        let (session, request) = begin(session_id, 0.25);
        assert_eq!(session.state(), ContinuousSessionState::BeginPending);
        assert_eq!(session.update_interval(), Duration::from_nanos(33_333_334));

        let ContinuousRequest::Begin(request) = request else {
            panic!("expected begin request");
        };
        assert_eq!(request.api_version(), CONTINUOUS_API_VERSION);
        assert_eq!(request.session_id(), session_id);
        assert_eq!(request.plugin_id().as_str(), "org.gradial.meter");
        assert_eq!(request.action_id().as_str(), "set-value");
        assert_eq!(request.initial().sequence(), 0);
        assert_close(request.initial().value(), 0.25);
        assert_close(request.initial().delta(), 0.0);
        assert_eq!(request.initial().elapsed_ms(), 0);
        let application = request.context().application().expect("application context");
        assert_eq!(application.window_class(), "ExampleWindow");
        assert_eq!(request.context().window_title(), Some("Frozen title"));
        assert_eq!(request.parameters().get("amount"), Some(&PluginValue::Number(0.5)));
    }

    #[test]
    fn standalone_cancel_requests_require_a_real_session_id() {
        assert_eq!(
            ContinuousCancelRequest::new(Uuid::nil(), ContinuousCancelReason::PluginFaulted),
            Err(ContinuousProtocolError::InvalidSessionId)
        );
        let request = ContinuousCancelRequest::new(
            Uuid::from_u128(77),
            ContinuousCancelReason::ReloadRequested,
        )
        .expect("standalone cancel");
        assert_eq!(request.session_id(), Uuid::from_u128(77));
        assert_eq!(request.reason(), ContinuousCancelReason::ReloadRequested);
    }

    #[test]
    fn identifiers_rates_parameters_and_samples_fail_closed() {
        for value in ["", "Uppercase", "contains space", "slash/value"] {
            assert_eq!(
                PluginId::new(value),
                Err(ContinuousProtocolError::InvalidIdentifier {
                    kind: ContinuousIdentifierKind::Plugin,
                })
            );
        }
        assert!(PluginId::new("a".repeat(96)).is_ok());
        assert!(PluginId::new("a".repeat(97)).is_err());
        assert!(PluginActionId::new("seek.backward_1").is_ok());

        for update_hz in [0, 9, 61, u16::MAX] {
            assert!(matches!(
                ContinuousActionSnapshot::new(
                    PluginId::new("valid.plugin").expect("plugin"),
                    PluginActionId::new("valid-action").expect("action"),
                    PermittedContext::default(),
                    BTreeMap::new(),
                    update_hz,
                ),
                Err(ContinuousProtocolError::InvalidUpdateRate { .. })
            ));
        }

        let mut parameters = BTreeMap::new();
        parameters.insert("bad".to_owned(), PluginValue::Number(f64::NAN));
        assert!(matches!(
            ContinuousActionSnapshot::new(
                PluginId::new("valid.plugin").expect("plugin"),
                PluginActionId::new("valid-action").expect("action"),
                PermittedContext::default(),
                parameters,
                30,
            ),
            Err(ContinuousProtocolError::InvalidParameterNumber)
        ));

        let mut parameters = BTreeMap::new();
        parameters.insert(
            "text".to_owned(),
            PluginValue::String("x".repeat(MAX_PLUGIN_PARAMETERS_SERIALIZED_BYTES)),
        );
        assert!(matches!(
            ContinuousActionSnapshot::new(
                PluginId::new("valid.plugin").expect("plugin"),
                PluginActionId::new("valid-action").expect("action"),
                PermittedContext::default(),
                parameters,
                30,
            ),
            Err(ContinuousProtocolError::ParametersTooLarge { .. })
        ));

        assert!(matches!(
            ContinuousSession::begin(Uuid::nil(), action(30), 0.5),
            Err(ContinuousProtocolError::InvalidSessionId)
        ));
        for value in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
            assert!(matches!(
                ContinuousSession::begin(Uuid::from_u128(2), action(30), value),
                Err(ContinuousProtocolError::InvalidSampleValue)
            ));
        }
    }

    #[test]
    fn ordinary_updates_coalesce_and_expose_sequence_gaps() {
        let session_id = Uuid::from_u128(3);
        let (mut session, _) = begin(session_id, 0.1);
        assert_eq!(session.accept_begin(Duration::from_millis(5)), Ok(None));
        session.observe(0.2, Duration::from_millis(10)).expect("sample one");
        session.observe(0.4, Duration::from_millis(20)).expect("sample two replaces one");
        assert_eq!(session.take_due_update(Duration::from_millis(30)), Ok(None));

        let Some(ContinuousRequest::Update(update)) =
            session.take_due_update(Duration::from_millis(40)).expect("due update")
        else {
            panic!("expected coalesced update");
        };
        assert_eq!(update.session_id(), session_id);
        assert_eq!(update.sample().sequence(), 2);
        assert_close(update.sample().value(), 0.4);
        assert!((update.sample().delta() - 0.3).abs() < f64::EPSILON);
        assert_eq!(update.sample().elapsed_ms(), 15);

        session.observe(0.8, Duration::from_millis(50)).expect("third sample");
        assert_eq!(session.take_due_update(Duration::from_millis(60)), Ok(None));
        let Some(ContinuousRequest::Update(update)) =
            session.take_due_update(Duration::from_millis(74)).expect("second due update")
        else {
            panic!("expected second update");
        };
        assert_eq!(update.sample().sequence(), 3);
        assert!((update.sample().delta() - 0.4).abs() < f64::EPSILON);

        session
            .observe_coalesced(0.9, Duration::from_millis(80), 3)
            .expect("three upstream observations");
        let Some(ContinuousRequest::Update(update)) =
            session.take_due_update(Duration::from_millis(108)).expect("coalesced upstream update")
        else {
            panic!("expected upstream-coalesced update");
        };
        assert_eq!(update.sample().sequence(), 6);
        assert_close(update.sample().value(), 0.9);
        assert_eq!(
            session.observe_coalesced(1.0, Duration::from_millis(110), 0),
            Err(ContinuousProtocolError::InvalidObservationCount)
        );
    }

    #[test]
    fn observation_time_cannot_precede_acceptance_or_move_backward() {
        let (mut session, _) = begin(Uuid::from_u128(9), 0.1);
        session.accept_begin(Duration::from_millis(10)).expect("accept");
        assert_eq!(
            session.observe(0.2, Duration::from_millis(9)),
            Err(ContinuousProtocolError::NonMonotonicTimestamp)
        );
        session.observe(0.3, Duration::from_millis(12)).expect("forward sample");
        assert_eq!(
            session.commit(0.4, Duration::from_millis(11)),
            Err(ContinuousProtocolError::NonMonotonicTimestamp)
        );
        assert_eq!(session.state(), ContinuousSessionState::Active);
    }

    #[test]
    fn commit_is_exactly_once_and_supersedes_an_ordinary_pending_update() {
        let session_id = Uuid::from_u128(4);
        let (mut session, _) = begin(session_id, 0.2);
        session.accept_begin(Duration::from_millis(10)).expect("accept");
        session.observe(0.5, Duration::from_millis(15)).expect("pending update");
        let Some(ContinuousRequest::Commit(commit)) =
            session.commit(0.65, Duration::from_millis(20)).expect("commit")
        else {
            panic!("expected commit request");
        };
        assert_eq!(commit.session_id(), session_id);
        assert_eq!(commit.final_sample().sequence(), 2);
        assert_close(commit.final_sample().value(), 0.65);
        assert!((commit.final_sample().delta() - 0.45).abs() < f64::EPSILON);
        assert_eq!(commit.final_sample().elapsed_ms(), 10);
        assert_eq!(session.terminal(), Some(ContinuousSessionTerminal::Committed));
        assert_eq!(session.take_due_update(Duration::from_secs(1)), Ok(None));
        assert!(session.commit(0.7, Duration::from_secs(1)).is_err());
        assert!(session.cancel(ContinuousCancelReason::Shutdown).is_err());
        assert!(session.observe(0.7, Duration::from_secs(1)).is_err());
    }

    #[test]
    fn terminal_decision_waits_for_pending_begin_and_rejection_has_no_callback() {
        let (mut committed, _) = begin(Uuid::from_u128(5), 0.1);
        committed.observe(0.4, Duration::from_millis(2)).expect("pending sample");
        assert_eq!(committed.commit(0.6, Duration::from_millis(3)), Ok(None));
        let Some(ContinuousRequest::Commit(commit)) =
            committed.accept_begin(Duration::from_millis(10)).expect("accept pending commit")
        else {
            panic!("accepted begin must be followed by commit");
        };
        assert_eq!(commit.final_sample().sequence(), 2);
        assert_eq!(commit.final_sample().elapsed_ms(), 0);

        let reason = ContinuousCancelReason::DirectionChanged;
        let (mut cancelled, _) = begin(Uuid::from_u128(6), 0.1);
        assert_eq!(cancelled.cancel(reason), Ok(None));
        let Some(ContinuousRequest::Cancel(cancel)) =
            cancelled.accept_begin(Duration::from_millis(10)).expect("accept pending cancel")
        else {
            panic!("accepted begin must be followed by cancel");
        };
        assert_eq!(cancel.reason(), reason);
        assert_eq!(cancelled.terminal(), Some(ContinuousSessionTerminal::Cancelled(reason)));

        let (mut rejected, _) = begin(Uuid::from_u128(7), 0.1);
        assert_eq!(rejected.cancel(reason), Ok(None));
        rejected.reject_begin().expect("reject begin");
        assert_eq!(rejected.terminal(), Some(ContinuousSessionTerminal::BeginRejected));
        assert_eq!(rejected.take_due_update(Duration::from_secs(1)), Ok(None));
    }

    #[test]
    fn active_cancel_drops_updates_and_terminates_once() {
        let session_id = Uuid::from_u128(8);
        let (mut session, _) = begin(session_id, 0.5);
        session.accept_begin(Duration::ZERO).expect("accept");
        session.observe(0.75, Duration::from_millis(10)).expect("pending update");
        let reason = ContinuousCancelReason::ReturnedToDiscrete;
        let Some(ContinuousRequest::Cancel(cancel)) = session.cancel(reason).expect("cancel")
        else {
            panic!("expected cancel request");
        };
        assert_eq!(cancel.session_id(), session_id);
        assert_eq!(cancel.reason(), reason);
        assert_eq!(session.take_due_update(Duration::from_secs(1)), Ok(None));
        assert!(session.cancel(reason).is_err());
    }

    #[test]
    fn ten_thousand_sessions_finish_without_retained_work() {
        for index in 1..=10_000_u128 {
            let (mut session, request) = begin(Uuid::from_u128(index), 0.0);
            assert_eq!(request.session_id(), Uuid::from_u128(index));
            session.accept_begin(Duration::ZERO).expect("accept");
            session.observe(0.25, Duration::from_millis(1)).expect("first");
            session.observe(0.75, Duration::from_millis(2)).expect("latest");
            let Some(ContinuousRequest::Commit(commit)) =
                session.commit(1.0, Duration::from_millis(3)).expect("commit")
            else {
                panic!("expected terminal request");
            };
            assert_eq!(commit.final_sample().sequence(), 3);
            assert_eq!(session.state(), ContinuousSessionState::Terminated);
        }
    }
}
