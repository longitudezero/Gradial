//! Declarative plugin package metadata.
//!
//! Parsing a manifest never loads or executes plugin code. Filesystem discovery
//! and package-boundary checks live in the Windows application crate.

use std::{collections::HashSet, path::Path};

use serde::Deserialize;
use thiserror::Error;

use crate::{MAX_CONTINUOUS_IDENTIFIER_BYTES, MAX_CONTINUOUS_UPDATE_HZ, MIN_CONTINUOUS_UPDATE_HZ};

pub const PLUGIN_MANIFEST_VERSION: u16 = 1;
pub const PLUGIN_API_VERSION: u16 = 1;
pub const MAX_PLUGIN_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_PLUGIN_ACTIONS: usize = 64;
pub const MAX_PLUGIN_PARAMETERS_PER_ACTION: usize = 32;
const MAX_DISPLAY_NAME_BYTES: usize = 128;
const MAX_DESCRIPTION_BYTES: usize = 1_024;
const MAX_PARAMETER_STRING_BYTES: usize = 4_096;
const MAX_CHOICE_VALUES: usize = 64;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub manifest_version: u16,
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u16,
    pub entry: String,
    #[serde(default)]
    pub permissions: Vec<PluginPermission>,
    #[serde(default)]
    pub actions: Vec<PluginManifestAction>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifestAction {
    pub id: String,
    pub name: String,
    pub kind: PluginActionKind,
    #[serde(default)]
    pub description: String,
    pub update_hz: u16,
    #[serde(default)]
    pub parameters: Vec<PluginParameterDefinition>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum PluginActionKind {
    Continuous,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
pub enum PluginPermission {
    #[serde(rename = "context.application")]
    ContextApplication,
    #[serde(rename = "context.window_title")]
    ContextWindowTitle,
    #[serde(rename = "input.shortcut")]
    InputShortcut,
    #[serde(rename = "notification")]
    Notification,
    #[serde(rename = "network.loopback")]
    NetworkLoopback,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginParameterDefinition {
    Boolean {
        key: String,
        label: String,
        #[serde(default)]
        required: bool,
        default: Option<bool>,
    },
    Integer {
        key: String,
        label: String,
        #[serde(default)]
        required: bool,
        default: Option<i64>,
        minimum: i64,
        maximum: i64,
    },
    Number {
        key: String,
        label: String,
        #[serde(default)]
        required: bool,
        default: Option<f64>,
        minimum: f64,
        maximum: f64,
    },
    String {
        key: String,
        label: String,
        #[serde(default)]
        required: bool,
        default: Option<String>,
        #[serde(default)]
        minimum_length: u32,
        maximum_length: u32,
    },
    Choice {
        key: String,
        label: String,
        #[serde(default)]
        required: bool,
        default: Option<String>,
        choices: Vec<String>,
    },
}

impl PluginParameterDefinition {
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Boolean { key, .. }
            | Self::Integer { key, .. }
            | Self::Number { key, .. }
            | Self::String { key, .. }
            | Self::Choice { key, .. } => key,
        }
    }

    fn validate(&self) -> Result<(), PluginManifestError> {
        let (key, label) = match self {
            Self::Boolean { key, label, .. }
            | Self::Integer { key, label, .. }
            | Self::Number { key, label, .. }
            | Self::String { key, label, .. }
            | Self::Choice { key, label, .. } => (key, label),
        };
        validate_identifier(key).map_err(|()| PluginManifestError::InvalidParameterKey)?;
        validate_display_name(label).map_err(|()| PluginManifestError::InvalidParameterLabel)?;

        match self {
            Self::Boolean { .. } => {}
            Self::Integer { default, minimum, maximum, .. } => {
                if minimum > maximum
                    || default.is_some_and(|value| value < *minimum || value > *maximum)
                {
                    return Err(PluginManifestError::InvalidParameterRange);
                }
            }
            Self::Number { default, minimum, maximum, .. } => {
                if !minimum.is_finite()
                    || !maximum.is_finite()
                    || minimum > maximum
                    || default.is_some_and(|value| {
                        !value.is_finite() || value < *minimum || value > *maximum
                    })
                {
                    return Err(PluginManifestError::InvalidParameterRange);
                }
            }
            Self::String { default, minimum_length, maximum_length, .. } => {
                let maximum_length = usize::try_from(*maximum_length).unwrap_or(usize::MAX);
                let minimum_length = usize::try_from(*minimum_length).unwrap_or(usize::MAX);
                if minimum_length > maximum_length
                    || maximum_length > MAX_PARAMETER_STRING_BYTES
                    || default.as_ref().is_some_and(|value| {
                        let length = value.chars().count();
                        length < minimum_length || length > maximum_length
                    })
                {
                    return Err(PluginManifestError::InvalidParameterRange);
                }
            }
            Self::Choice { default, choices, .. } => {
                if choices.is_empty() || choices.len() > MAX_CHOICE_VALUES {
                    return Err(PluginManifestError::InvalidParameterChoices);
                }
                let mut unique = HashSet::with_capacity(choices.len());
                if choices.iter().any(|choice| {
                    choice.is_empty()
                        || choice.len() > MAX_PARAMETER_STRING_BYTES
                        || !unique.insert(choice)
                }) || default.as_ref().is_some_and(|value| !unique.contains(value))
                {
                    return Err(PluginManifestError::InvalidParameterChoices);
                }
            }
        }
        Ok(())
    }
}

/// Parses and validates a bounded UTF-8 `plugin.toml` document.
///
/// API compatibility is deliberately not rejected here so discovery can show
/// an otherwise valid package as incompatible instead of corrupt.
///
/// # Errors
///
/// Returns a stable [`PluginManifestError`] when the document exceeds its
/// bound, is not valid UTF-8/TOML, or violates the closed manifest contract.
pub fn parse_plugin_manifest(bytes: &[u8]) -> Result<PluginManifest, PluginManifestError> {
    if bytes.len() > MAX_PLUGIN_MANIFEST_BYTES {
        return Err(PluginManifestError::TooLarge);
    }
    let source = std::str::from_utf8(bytes).map_err(|_| PluginManifestError::NotUtf8)?;
    let manifest: PluginManifest = toml::from_str(source).map_err(|_| PluginManifestError::Toml)?;
    manifest.validate()?;
    Ok(manifest)
}

impl PluginManifest {
    fn validate(&self) -> Result<(), PluginManifestError> {
        if self.manifest_version != PLUGIN_MANIFEST_VERSION {
            return Err(PluginManifestError::UnsupportedManifestVersion);
        }
        validate_identifier(&self.id).map_err(|()| PluginManifestError::InvalidPluginId)?;
        validate_display_name(&self.name).map_err(|()| PluginManifestError::InvalidPluginName)?;
        semver::Version::parse(&self.version).map_err(|_| PluginManifestError::InvalidVersion)?;
        validate_entry(&self.entry)?;
        if self.actions.len() > MAX_PLUGIN_ACTIONS {
            return Err(PluginManifestError::TooManyActions);
        }

        let mut permissions = HashSet::with_capacity(self.permissions.len());
        if self.permissions.iter().any(|permission| !permissions.insert(*permission)) {
            return Err(PluginManifestError::DuplicatePermission);
        }

        let mut action_ids = HashSet::with_capacity(self.actions.len());
        for action in &self.actions {
            if !action_ids.insert(action.id.as_str()) {
                return Err(PluginManifestError::DuplicateActionId);
            }
            action.validate()?;
        }
        Ok(())
    }
}

impl PluginManifestAction {
    fn validate(&self) -> Result<(), PluginManifestError> {
        validate_identifier(&self.id).map_err(|()| PluginManifestError::InvalidActionId)?;
        validate_display_name(&self.name).map_err(|()| PluginManifestError::InvalidActionName)?;
        if self.description.len() > MAX_DESCRIPTION_BYTES {
            return Err(PluginManifestError::DescriptionTooLong);
        }
        if !(MIN_CONTINUOUS_UPDATE_HZ..=MAX_CONTINUOUS_UPDATE_HZ).contains(&self.update_hz) {
            return Err(PluginManifestError::InvalidUpdateRate);
        }
        if self.parameters.len() > MAX_PLUGIN_PARAMETERS_PER_ACTION {
            return Err(PluginManifestError::TooManyParameters);
        }
        let mut keys = HashSet::with_capacity(self.parameters.len());
        for parameter in &self.parameters {
            if !keys.insert(parameter.key()) {
                return Err(PluginManifestError::DuplicateParameterKey);
            }
            parameter.validate()?;
        }
        Ok(())
    }
}

fn validate_identifier(value: &str) -> Result<(), ()> {
    if value.is_empty()
        || value.len() > MAX_CONTINUOUS_IDENTIFIER_BYTES
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(());
    }
    Ok(())
}

fn validate_display_name(value: &str) -> Result<(), ()> {
    if value.trim().is_empty()
        || value.len() > MAX_DISPLAY_NAME_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(());
    }
    Ok(())
}

fn validate_entry(entry: &str) -> Result<(), PluginManifestError> {
    if entry.is_empty()
        || entry.starts_with(['/', '\\'])
        || entry.contains(':')
        || Path::new(entry).extension().is_none_or(|extension| extension != "lua")
        || entry
            .split(['/', '\\'])
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(PluginManifestError::InvalidEntry);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PluginManifestError {
    #[error("manifest_too_large")]
    TooLarge,
    #[error("manifest_not_utf8")]
    NotUtf8,
    #[error("manifest_toml_invalid")]
    Toml,
    #[error("manifest_version_unsupported")]
    UnsupportedManifestVersion,
    #[error("plugin_id_invalid")]
    InvalidPluginId,
    #[error("plugin_name_invalid")]
    InvalidPluginName,
    #[error("plugin_version_invalid")]
    InvalidVersion,
    #[error("plugin_entry_invalid")]
    InvalidEntry,
    #[error("plugin_permission_duplicate")]
    DuplicatePermission,
    #[error("plugin_actions_too_many")]
    TooManyActions,
    #[error("plugin_action_id_duplicate")]
    DuplicateActionId,
    #[error("plugin_action_id_invalid")]
    InvalidActionId,
    #[error("plugin_action_name_invalid")]
    InvalidActionName,
    #[error("plugin_action_description_too_long")]
    DescriptionTooLong,
    #[error("plugin_action_update_rate_invalid")]
    InvalidUpdateRate,
    #[error("plugin_parameters_too_many")]
    TooManyParameters,
    #[error("plugin_parameter_key_duplicate")]
    DuplicateParameterKey,
    #[error("plugin_parameter_key_invalid")]
    InvalidParameterKey,
    #[error("plugin_parameter_label_invalid")]
    InvalidParameterLabel,
    #[error("plugin_parameter_range_invalid")]
    InvalidParameterRange,
    #[error("plugin_parameter_choices_invalid")]
    InvalidParameterChoices,
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
manifest_version = 1
id = "example.media-controls"
name = "Example media controls"
version = "0.1.0"
api_version = 1
entry = "main.lua"
permissions = ["context.application", "input.shortcut"]

[[actions]]
id = "seek-forward"
name = "Seek forward"
kind = "continuous"
description = "Move forward through the active player"
update_hz = 30

[[actions.parameters]]
key = "maximum_steps"
label = "Maximum steps"
type = "integer"
default = 12
minimum = 1
maximum = 120
"#;

    #[test]
    fn parses_plan_example_without_executing_source() {
        let manifest = parse_plugin_manifest(VALID.as_bytes()).expect("valid manifest");
        assert_eq!(manifest.id, "example.media-controls");
        assert_eq!(manifest.actions[0].parameters[0].key(), "maximum_steps");
    }

    #[test]
    fn accepts_future_api_for_discovery_to_classify() {
        let source = VALID.replace("api_version = 1", "api_version = 99");
        let manifest = parse_plugin_manifest(source.as_bytes()).expect("valid future manifest");
        assert_eq!(manifest.api_version, 99);
    }

    #[test]
    fn rejects_unknown_fields_and_permissions() {
        let unknown_field =
            VALID.replace("entry = \"main.lua\"", "entry = \"main.lua\"\nsneaky = true");
        assert_eq!(parse_plugin_manifest(unknown_field.as_bytes()), Err(PluginManifestError::Toml));

        let unknown_permission = VALID.replace("input.shortcut", "filesystem.full");
        assert_eq!(
            parse_plugin_manifest(unknown_permission.as_bytes()),
            Err(PluginManifestError::Toml)
        );
    }

    #[test]
    fn rejects_unsafe_entry_paths() {
        for entry in [r"..\\main.lua", r"C:\\main.lua", "lib//main.lua", "main.luac"] {
            let source = VALID.replace("main.lua", entry);
            assert_eq!(
                parse_plugin_manifest(source.as_bytes()),
                Err(PluginManifestError::InvalidEntry)
            );
        }
    }

    #[test]
    fn rejects_duplicate_action_and_parameter_identifiers() {
        let duplicate_action =
            format!("{VALID}{}", &VALID[VALID.find("[[actions]]").expect("action")..]);
        assert_eq!(
            parse_plugin_manifest(duplicate_action.as_bytes()),
            Err(PluginManifestError::DuplicateActionId)
        );

        let duplicate_parameter = format!(
            "{VALID}\n[[actions.parameters]]\nkey = \"maximum_steps\"\nlabel = \"Again\"\ntype = \"boolean\"\n"
        );
        assert_eq!(
            parse_plugin_manifest(duplicate_parameter.as_bytes()),
            Err(PluginManifestError::DuplicateParameterKey)
        );
    }

    #[test]
    fn rejects_defaults_outside_parameter_constraints() {
        let invalid = VALID.replace("default = 12", "default = 121");
        assert_eq!(
            parse_plugin_manifest(invalid.as_bytes()),
            Err(PluginManifestError::InvalidParameterRange)
        );
    }

    #[test]
    fn manifest_size_is_bounded_before_parsing() {
        let oversized = vec![b' '; MAX_PLUGIN_MANIFEST_BYTES + 1];
        assert_eq!(parse_plugin_manifest(&oversized), Err(PluginManifestError::TooLarge));
    }
}
