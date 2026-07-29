use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{Configuration, ConfigurationError};

pub const DEFAULT_ACTION_STEP_TIMEOUT_MS: u32 = 5_000;
pub const MAX_ACTION_SEQUENCE_DURATION_MS: u32 = 30_000;
pub const MAX_ACTION_SEQUENCE_STEPS: usize = 32;
pub const MAX_ACTION_SEQUENCE_DEPTH: u8 = 4;
pub const MAX_TYPED_TEXT_BYTES: usize = 8 * 1_024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionDefinition {
    pub id: Uuid,
    pub name: String,
    pub kind: ActionKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionKind {
    SendShortcut {
        modifiers: Vec<ShortcutModifier>,
        key: ShortcutKey,
    },
    TypeText {
        text: String,
    },
    OpenUrl {
        url: String,
    },
    OpenPath {
        path: PathBuf,
    },
    RunExecutable {
        executable: PathBuf,
        arguments: Vec<String>,
        working_directory: Option<PathBuf>,
    },
    Window {
        command: WindowCommand,
    },
    Delay {
        milliseconds: u32,
    },
    Sequence {
        actions: Vec<Uuid>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutModifier {
    LeftControl,
    RightControl,
    LeftAlt,
    RightAlt,
    LeftShift,
    RightShift,
    LeftWindows,
    RightWindows,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortcutKey {
    pub scan_code: u16,
    pub extended: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowCommand {
    Close,
    Minimize,
    Maximize,
    Restore,
}

pub(crate) fn validate_actions(configuration: &Configuration) -> Result<(), ConfigurationError> {
    let mut definitions = BTreeMap::new();
    for definition in &configuration.actions {
        if definitions.insert(definition.id, definition).is_some() {
            return Err(ConfigurationError::DuplicateActionId { action_id: definition.id });
        }
        validate_action_fields(definition)?;
    }

    validate_menu_references(configuration, &definitions)?;
    for definition in &configuration.actions {
        let mut stack = Vec::new();
        let metrics = analyze_action(definition, 0, &mut stack, &definitions)?;
        if metrics.steps > MAX_ACTION_SEQUENCE_STEPS {
            return Err(ConfigurationError::SequenceStepLimitExceeded {
                action_id: definition.id,
                steps: metrics.steps,
                maximum: MAX_ACTION_SEQUENCE_STEPS,
            });
        }
        if metrics.delay_ms > MAX_ACTION_SEQUENCE_DURATION_MS {
            return Err(ConfigurationError::SequenceDelayLimitExceeded {
                action_id: definition.id,
                milliseconds: metrics.delay_ms,
                maximum: MAX_ACTION_SEQUENCE_DURATION_MS,
            });
        }
    }
    Ok(())
}

fn validate_action_fields(definition: &ActionDefinition) -> Result<(), ConfigurationError> {
    if definition.name.trim().is_empty() {
        return Err(ConfigurationError::EmptyActionName { action_id: definition.id });
    }
    match &definition.kind {
        ActionKind::SendShortcut { modifiers, key } => {
            if key.scan_code == 0 {
                return Err(ConfigurationError::InvalidShortcutScanCode {
                    action_id: definition.id,
                });
            }
            for (index, modifier) in modifiers.iter().enumerate() {
                if modifiers[..index].contains(modifier) {
                    return Err(ConfigurationError::DuplicateShortcutModifier {
                        action_id: definition.id,
                        modifier: *modifier,
                    });
                }
            }
        }
        ActionKind::TypeText { text } => {
            if text.len() > MAX_TYPED_TEXT_BYTES {
                return Err(ConfigurationError::TypedTextTooLong {
                    action_id: definition.id,
                    bytes: text.len(),
                    maximum: MAX_TYPED_TEXT_BYTES,
                });
            }
        }
        ActionKind::OpenUrl { url } => {
            if !is_allowed_url(url) {
                return Err(ConfigurationError::InvalidActionUrl { action_id: definition.id });
            }
        }
        ActionKind::OpenPath { path } => {
            validate_absolute_path(definition.id, "path", path)?;
        }
        ActionKind::RunExecutable { executable, arguments, working_directory } => {
            validate_absolute_path(definition.id, "executable", executable)?;
            if let Some(working_directory) = working_directory {
                validate_absolute_path(definition.id, "working_directory", working_directory)?;
            }
            if let Some((index, _)) =
                arguments.iter().enumerate().find(|(_, argument)| argument.contains('\0'))
            {
                return Err(ConfigurationError::ExecutableArgumentContainsNul {
                    action_id: definition.id,
                    index,
                });
            }
        }
        ActionKind::Window { .. } => {}
        ActionKind::Delay { milliseconds } => {
            if !(10..=10_000).contains(milliseconds) {
                return Err(ConfigurationError::InvalidDelay {
                    action_id: definition.id,
                    milliseconds: *milliseconds,
                });
            }
        }
        ActionKind::Sequence { actions } => {
            if actions.is_empty() || actions.len() > MAX_ACTION_SEQUENCE_STEPS {
                return Err(ConfigurationError::InvalidSequenceLength {
                    action_id: definition.id,
                    found: actions.len(),
                    maximum: MAX_ACTION_SEQUENCE_STEPS,
                });
            }
        }
    }
    Ok(())
}

fn validate_absolute_path(
    action_id: Uuid,
    field: &'static str,
    path: &Path,
) -> Result<(), ConfigurationError> {
    if !path.is_absolute() {
        return Err(ConfigurationError::ActionPathNotAbsolute { action_id, field });
    }
    if path.as_os_str().to_string_lossy().contains('\0') {
        return Err(ConfigurationError::ActionPathContainsNul { action_id, field });
    }
    Ok(())
}

fn is_allowed_url(value: &str) -> bool {
    let Some((scheme, remainder)) = value.split_once("://") else {
        return false;
    };
    let authority = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https")
        && !authority.is_empty()
        && !authority.starts_with(':')
        && !authority.ends_with(':')
        && !remainder.chars().any(char::is_whitespace)
        && !value.chars().any(|character| character.is_control() || character == '\\')
}

fn validate_menu_references(
    configuration: &Configuration,
    definitions: &BTreeMap<Uuid, &ActionDefinition>,
) -> Result<(), ConfigurationError> {
    validate_profile_menu_references(
        configuration.global_profile.id,
        &configuration.global_profile.menu.slices,
        definitions,
    )?;
    for application_profile in &configuration.application_profiles {
        validate_profile_menu_references(
            application_profile.profile.id,
            &application_profile.profile.menu.slices,
            definitions,
        )?;
    }
    Ok(())
}

fn validate_profile_menu_references(
    profile_id: Uuid,
    slices: &[crate::MenuSlice],
    definitions: &BTreeMap<Uuid, &ActionDefinition>,
) -> Result<(), ConfigurationError> {
    for (slice_index, slice) in slices.iter().enumerate() {
        if let Some(action_id) = slice.action_id
            && !definitions.contains_key(&action_id)
        {
            return Err(ConfigurationError::MissingMenuActionReference {
                profile_id,
                slice_index,
                action_id,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
struct ActionMetrics {
    steps: usize,
    delay_ms: u32,
}

fn analyze_action(
    definition: &ActionDefinition,
    sequence_depth: u8,
    stack: &mut Vec<Uuid>,
    definitions: &BTreeMap<Uuid, &ActionDefinition>,
) -> Result<ActionMetrics, ConfigurationError> {
    let ActionKind::Sequence { actions } = &definition.kind else {
        let delay_ms = match &definition.kind {
            ActionKind::Delay { milliseconds } => *milliseconds,
            _ => 0,
        };
        return Ok(ActionMetrics { steps: 1, delay_ms });
    };

    let next_depth = sequence_depth.saturating_add(1);
    if next_depth > MAX_ACTION_SEQUENCE_DEPTH {
        return Err(ConfigurationError::SequenceDepthLimitExceeded {
            action_id: definition.id,
            depth: next_depth,
            maximum: MAX_ACTION_SEQUENCE_DEPTH,
        });
    }
    if stack.contains(&definition.id) {
        return Err(ConfigurationError::SequenceCycle { action_id: definition.id });
    }

    stack.push(definition.id);
    let result = (|| {
        let mut metrics = ActionMetrics::default();
        for (index, child_id) in actions.iter().enumerate() {
            let Some(child) = definitions.get(child_id) else {
                return Err(ConfigurationError::MissingSequenceActionReference {
                    sequence_id: definition.id,
                    index,
                    action_id: *child_id,
                });
            };
            let child_metrics = analyze_action(child, next_depth, stack, definitions)?;
            metrics.steps = metrics.steps.saturating_add(child_metrics.steps);
            metrics.delay_ms = metrics.delay_ms.saturating_add(child_metrics.delay_ms);
            if metrics.steps > MAX_ACTION_SEQUENCE_STEPS {
                return Err(ConfigurationError::SequenceStepLimitExceeded {
                    action_id: definition.id,
                    steps: metrics.steps,
                    maximum: MAX_ACTION_SEQUENCE_STEPS,
                });
            }
            if metrics.delay_ms > MAX_ACTION_SEQUENCE_DURATION_MS {
                return Err(ConfigurationError::SequenceDelayLimitExceeded {
                    action_id: definition.id,
                    milliseconds: metrics.delay_ms,
                    maximum: MAX_ACTION_SEQUENCE_DURATION_MS,
                });
            }
        }
        Ok(metrics)
    })();
    stack.pop();
    result
}

/// Validated action definitions retained for bounded release-time compilation.
#[derive(Clone, Debug)]
pub struct ActionPlanCompiler {
    config_revision: u64,
    definitions: BTreeMap<Uuid, ActionDefinition>,
}

impl ActionPlanCompiler {
    /// Builds a compiler only from a completely valid active configuration.
    ///
    /// # Errors
    ///
    /// Returns the first field-specific configuration error.
    pub fn from_configuration(configuration: &Configuration) -> Result<Self, ConfigurationError> {
        configuration.validate()?;
        Ok(Self {
            config_revision: configuration.config_revision,
            definitions: configuration
                .actions
                .iter()
                .cloned()
                .map(|definition| (definition.id, definition))
                .collect(),
        })
    }

    /// Compiles one selected action into a flattened immutable plan.
    ///
    /// # Errors
    ///
    /// Returns an error when the action is absent, the invocation revision is
    /// stale, or a defensive step bound is exceeded.
    pub fn compile_plan(
        &self,
        action_id: Uuid,
        invocation_config_revision: u64,
    ) -> Result<ExecutionPlan, ActionPlanError> {
        if invocation_config_revision != self.config_revision {
            return Err(ActionPlanError::ConfigRevisionMismatch {
                invocation: invocation_config_revision,
                active: self.config_revision,
            });
        }
        let Some(definition) = self.definitions.get(&action_id) else {
            return Err(ActionPlanError::ActionNotFound { action_id });
        };
        let mut steps = Vec::new();
        self.flatten(definition, &mut steps)?;
        Ok(ExecutionPlan {
            root_action_id: action_id,
            config_revision: invocation_config_revision,
            steps: steps.into(),
            per_step_timeout_ms: DEFAULT_ACTION_STEP_TIMEOUT_MS,
            maximum_duration_ms: MAX_ACTION_SEQUENCE_DURATION_MS,
        })
    }

    fn flatten(
        &self,
        definition: &ActionDefinition,
        steps: &mut Vec<ExecutionStep>,
    ) -> Result<(), ActionPlanError> {
        if let ActionKind::Sequence { actions } = &definition.kind {
            for child_id in actions {
                let Some(child) = self.definitions.get(child_id) else {
                    return Err(ActionPlanError::ActionNotFound { action_id: *child_id });
                };
                self.flatten(child, steps)?;
            }
            return Ok(());
        }
        if steps.len() >= MAX_ACTION_SEQUENCE_STEPS {
            return Err(ActionPlanError::StepLimitExceeded { maximum: MAX_ACTION_SEQUENCE_STEPS });
        }
        steps.push(ExecutionStep {
            source_action_id: definition.id,
            operation: ExecutionOperation::from(&definition.kind),
        });
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionPlan {
    root_action_id: Uuid,
    config_revision: u64,
    steps: Arc<[ExecutionStep]>,
    per_step_timeout_ms: u32,
    maximum_duration_ms: u32,
}

impl ExecutionPlan {
    #[must_use]
    pub const fn root_action_id(&self) -> Uuid {
        self.root_action_id
    }

    #[must_use]
    pub const fn config_revision(&self) -> u64 {
        self.config_revision
    }

    #[must_use]
    pub fn steps(&self) -> &[ExecutionStep] {
        &self.steps
    }

    #[must_use]
    pub const fn per_step_timeout_ms(&self) -> u32 {
        self.per_step_timeout_ms
    }

    #[must_use]
    pub const fn maximum_duration_ms(&self) -> u32 {
        self.maximum_duration_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionStep {
    source_action_id: Uuid,
    operation: ExecutionOperation,
}

impl ExecutionStep {
    #[must_use]
    pub const fn source_action_id(&self) -> Uuid {
        self.source_action_id
    }

    #[must_use]
    pub const fn operation(&self) -> &ExecutionOperation {
        &self.operation
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionOperation {
    SendShortcut {
        modifiers: Arc<[ShortcutModifier]>,
        key: ShortcutKey,
    },
    TypeText {
        text: Arc<str>,
    },
    OpenUrl {
        url: Arc<str>,
    },
    OpenPath {
        path: PathBuf,
    },
    RunExecutable {
        executable: PathBuf,
        arguments: Arc<[String]>,
        working_directory: Option<PathBuf>,
    },
    Window {
        command: WindowCommand,
    },
    Delay {
        milliseconds: u32,
    },
}

impl ExecutionOperation {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::SendShortcut { .. } => "send_shortcut",
            Self::TypeText { .. } => "type_text",
            Self::OpenUrl { .. } => "open_url",
            Self::OpenPath { .. } => "open_path",
            Self::RunExecutable { .. } => "run_executable",
            Self::Window { .. } => "window",
            Self::Delay { .. } => "delay",
        }
    }
}

impl From<&ActionKind> for ExecutionOperation {
    fn from(kind: &ActionKind) -> Self {
        match kind {
            ActionKind::SendShortcut { modifiers, key } => {
                Self::SendShortcut { modifiers: modifiers.clone().into(), key: *key }
            }
            ActionKind::TypeText { text } => Self::TypeText { text: Arc::from(text.as_str()) },
            ActionKind::OpenUrl { url } => Self::OpenUrl { url: Arc::from(url.as_str()) },
            ActionKind::OpenPath { path } => Self::OpenPath { path: path.clone() },
            ActionKind::RunExecutable { executable, arguments, working_directory } => {
                Self::RunExecutable {
                    executable: executable.clone(),
                    arguments: arguments.clone().into(),
                    working_directory: working_directory.clone(),
                }
            }
            ActionKind::Window { command } => Self::Window { command: *command },
            ActionKind::Delay { milliseconds } => Self::Delay { milliseconds: *milliseconds },
            ActionKind::Sequence { .. } => {
                unreachable!("sequence nodes are flattened before operation compilation")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ActionPlanError {
    #[error("action {action_id} is not present in the active configuration")]
    ActionNotFound { action_id: Uuid },

    #[error(
        "invocation uses configuration revision {invocation}; active action catalog uses {active}"
    )]
    ConfigRevisionMismatch { invocation: u64, active: u64 },

    #[error("compiled action plan exceeds the {maximum}-step limit")]
    StepLimitExceeded { maximum: usize },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::{
        ActionDefinition, ActionKind, ActionPlanCompiler, ExecutionOperation,
        MAX_ACTION_SEQUENCE_DEPTH, MAX_ACTION_SEQUENCE_DURATION_MS, MAX_ACTION_SEQUENCE_STEPS,
        MAX_TYPED_TEXT_BYTES, ShortcutKey, ShortcutModifier,
    };
    use crate::{Configuration, ConfigurationError, MenuSlice};

    fn action(id: u128, kind: ActionKind) -> ActionDefinition {
        ActionDefinition { id: Uuid::from_u128(id), name: format!("Action {id}"), kind }
    }

    #[test]
    fn every_leaf_action_compiles_without_executing() {
        let actions = vec![
            action(
                1,
                ActionKind::SendShortcut {
                    modifiers: vec![ShortcutModifier::LeftControl],
                    key: ShortcutKey { scan_code: 0x1f, extended: false },
                },
            ),
            action(2, ActionKind::TypeText { text: "private text".to_owned() }),
            action(3, ActionKind::OpenUrl { url: "https://example.com".to_owned() }),
            action(4, ActionKind::OpenPath { path: PathBuf::from(r"C:\Example") }),
            action(
                5,
                ActionKind::RunExecutable {
                    executable: PathBuf::from(r"C:\Example\example.exe"),
                    arguments: vec!["private argument".to_owned()],
                    working_directory: Some(PathBuf::from(r"C:\Example")),
                },
            ),
            action(6, ActionKind::Window { command: super::WindowCommand::Restore }),
            action(7, ActionKind::Delay { milliseconds: 10 }),
        ];
        let configuration =
            Configuration { config_revision: 42, actions, ..Configuration::default() };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");

        for id in 1..=7 {
            let plan = compiler.compile_plan(Uuid::from_u128(id), 42).expect("leaf plan");
            assert_eq!(plan.steps().len(), 1);
            assert_eq!(plan.config_revision(), 42);
        }
    }

    #[test]
    fn nested_sequences_flatten_in_declared_order() {
        let configuration = Configuration {
            config_revision: 7,
            actions: vec![
                action(1, ActionKind::Delay { milliseconds: 10 }),
                action(2, ActionKind::Delay { milliseconds: 20 }),
                action(3, ActionKind::Sequence { actions: vec![Uuid::from_u128(1)] }),
                action(
                    4,
                    ActionKind::Sequence { actions: vec![Uuid::from_u128(3), Uuid::from_u128(2)] },
                ),
            ],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        let plan = compiler.compile_plan(Uuid::from_u128(4), 7).expect("sequence plan");

        assert_eq!(plan.steps().len(), 2);
        assert!(matches!(
            plan.steps()[0].operation(),
            ExecutionOperation::Delay { milliseconds: 10 }
        ));
        assert!(matches!(
            plan.steps()[1].operation(),
            ExecutionOperation::Delay { milliseconds: 20 }
        ));
    }

    #[test]
    fn cycles_are_rejected_before_compilation() {
        let configuration = Configuration {
            actions: vec![
                action(1, ActionKind::Sequence { actions: vec![Uuid::from_u128(2)] }),
                action(2, ActionKind::Sequence { actions: vec![Uuid::from_u128(1)] }),
            ],
            ..Configuration::default()
        };

        assert!(matches!(configuration.validate(), Err(ConfigurationError::SequenceCycle { .. })));

        let configuration = Configuration {
            actions: vec![action(1, ActionKind::Sequence { actions: vec![Uuid::from_u128(1)] })],
            ..Configuration::default()
        };
        assert!(matches!(configuration.validate(), Err(ConfigurationError::SequenceCycle { .. })));
    }

    #[test]
    fn compiler_rejects_a_different_invocation_revision() {
        let configuration = Configuration {
            config_revision: 4,
            actions: vec![action(1, ActionKind::Delay { milliseconds: 10 })],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");

        assert!(matches!(
            compiler.compile_plan(Uuid::from_u128(1), 3),
            Err(super::ActionPlanError::ConfigRevisionMismatch { invocation: 3, active: 4 })
        ));
    }

    #[test]
    fn leaf_field_validation_is_strict_and_field_specific() {
        let cases = [
            (
                ActionKind::SendShortcut {
                    modifiers: Vec::new(),
                    key: ShortcutKey { scan_code: 0, extended: false },
                },
                "scan code",
            ),
            (
                ActionKind::SendShortcut {
                    modifiers: vec![ShortcutModifier::LeftControl, ShortcutModifier::LeftControl],
                    key: ShortcutKey { scan_code: 1, extended: false },
                },
                "repeats modifier",
            ),
            (ActionKind::TypeText { text: "é".repeat((MAX_TYPED_TEXT_BYTES / 2) + 1) }, "maximum"),
            (ActionKind::OpenUrl { url: "file:///C:/private.txt".to_owned() }, "http or https"),
            (ActionKind::OpenPath { path: PathBuf::from("relative") }, "absolute path"),
            (
                ActionKind::RunExecutable {
                    executable: PathBuf::from(r"C:\Example\example.exe"),
                    arguments: vec!["bad\0argument".to_owned()],
                    working_directory: None,
                },
                "argument 0",
            ),
            (ActionKind::Delay { milliseconds: 9 }, "expected 10 through 10000"),
        ];

        for (kind, expected_message) in cases {
            let configuration =
                Configuration { actions: vec![action(1, kind)], ..Configuration::default() };
            let error = configuration.validate().expect_err("invalid action");
            assert!(
                error.to_string().contains(expected_message),
                "expected {expected_message:?} in {error}"
            );
        }
    }

    #[test]
    fn catalog_identity_and_sequence_shape_are_validated() {
        let duplicate = action(1, ActionKind::Delay { milliseconds: 10 });
        let configuration = Configuration {
            actions: vec![duplicate.clone(), duplicate],
            ..Configuration::default()
        };
        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::DuplicateActionId { .. })
        ));

        let configuration = Configuration {
            actions: vec![action(1, ActionKind::Sequence { actions: Vec::new() })],
            ..Configuration::default()
        };
        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::InvalidSequenceLength { found: 0, .. })
        ));

        let configuration = Configuration {
            actions: vec![action(
                1,
                ActionKind::Sequence {
                    actions: vec![Uuid::from_u128(2); MAX_ACTION_SEQUENCE_STEPS + 1],
                },
            )],
            ..Configuration::default()
        };
        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::InvalidSequenceLength { found, .. })
                if found == MAX_ACTION_SEQUENCE_STEPS + 1
        ));
    }

    #[test]
    fn urls_require_http_or_https_and_a_nonempty_authority() {
        for url in ["https://example.com", "HTTP://localhost:8080/path?query=yes", "http://[::1]/"]
        {
            let configuration = Configuration {
                actions: vec![action(1, ActionKind::OpenUrl { url: url.to_owned() })],
                ..Configuration::default()
            };
            configuration.validate().expect("valid URL");
        }

        for url in ["https://", "http://?query", "https://:443/path", r"https:\\example.com"] {
            let configuration = Configuration {
                actions: vec![action(1, ActionKind::OpenUrl { url: url.to_owned() })],
                ..Configuration::default()
            };
            assert!(matches!(
                configuration.validate(),
                Err(ConfigurationError::InvalidActionUrl { .. })
            ));
        }
    }

    #[test]
    fn missing_menu_and_sequence_references_are_rejected() {
        let missing = Uuid::from_u128(99);
        let mut configuration = Configuration::default();
        configuration.global_profile.menu.slices[2] =
            MenuSlice { label: "Missing".to_owned(), action_id: Some(missing) };
        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::MissingMenuActionReference {
                slice_index: 2,
                action_id,
                ..
            }) if action_id == missing
        ));

        let configuration = Configuration {
            actions: vec![action(1, ActionKind::Sequence { actions: vec![missing] })],
            ..Configuration::default()
        };
        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::MissingSequenceActionReference {
                action_id,
                ..
            }) if action_id == missing
        ));
    }

    #[test]
    fn nested_sequence_depth_is_bounded() {
        let mut actions = vec![action(1, ActionKind::Delay { milliseconds: 10 })];
        for id in 2..=u128::from(MAX_ACTION_SEQUENCE_DEPTH) + 2 {
            actions
                .push(action(id, ActionKind::Sequence { actions: vec![Uuid::from_u128(id - 1)] }));
        }
        let configuration = Configuration { actions, ..Configuration::default() };

        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::SequenceDepthLimitExceeded {
                maximum: MAX_ACTION_SEQUENCE_DEPTH,
                ..
            })
        ));
    }

    #[test]
    fn expanded_step_count_and_delay_duration_are_bounded() {
        let mut actions = Vec::new();
        for id in 1..=MAX_ACTION_SEQUENCE_STEPS + 1 {
            actions.push(action(
                u128::try_from(id).expect("small ID"),
                ActionKind::Delay { milliseconds: 10 },
            ));
        }
        let first_group = u128::try_from(MAX_ACTION_SEQUENCE_STEPS + 2).expect("small ID");
        let second_group = first_group + 1;
        let root = second_group + 1;
        actions.push(action(
            first_group,
            ActionKind::Sequence { actions: (1..=17).map(Uuid::from_u128).collect() },
        ));
        actions.push(action(
            second_group,
            ActionKind::Sequence { actions: (18..=33).map(Uuid::from_u128).collect() },
        ));
        actions.push(action(
            root,
            ActionKind::Sequence {
                actions: vec![Uuid::from_u128(first_group), Uuid::from_u128(second_group)],
            },
        ));
        let configuration = Configuration { actions, ..Configuration::default() };
        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::SequenceStepLimitExceeded {
                maximum: MAX_ACTION_SEQUENCE_STEPS,
                ..
            })
        ));

        let configuration = Configuration {
            actions: vec![
                action(1, ActionKind::Delay { milliseconds: 10_000 }),
                action(2, ActionKind::Delay { milliseconds: 10_000 }),
                action(3, ActionKind::Delay { milliseconds: 10_000 }),
                action(4, ActionKind::Delay { milliseconds: 10 }),
                action(5, ActionKind::Sequence { actions: (1..=4).map(Uuid::from_u128).collect() }),
            ],
            ..Configuration::default()
        };
        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::SequenceDelayLimitExceeded {
                maximum: MAX_ACTION_SEQUENCE_DURATION_MS,
                ..
            })
        ));
    }

    #[test]
    fn compiled_plans_do_not_borrow_mutable_configuration_data() {
        let mut configuration = Configuration {
            config_revision: 12,
            actions: vec![action(
                1,
                ActionKind::TypeText { text: "original private text".to_owned() },
            )],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        configuration.actions[0].kind = ActionKind::TypeText { text: "replacement".to_owned() };

        let plan = compiler.compile_plan(Uuid::from_u128(1), 12).expect("plan");
        assert!(matches!(
            plan.steps()[0].operation(),
            ExecutionOperation::TypeText { text } if text.as_ref() == "original private text"
        ));
    }

    #[test]
    fn action_payloads_reject_unknown_fields() {
        let configuration = Configuration {
            actions: vec![action(1, ActionKind::Delay { milliseconds: 10 })],
            ..Configuration::default()
        };
        let mut value = serde_json::to_value(configuration).expect("configuration value");
        value["actions"][0]["kind"]["unexpected"] = serde_json::Value::Bool(true);

        assert!(serde_json::from_value::<Configuration>(value).is_err());
    }
}
