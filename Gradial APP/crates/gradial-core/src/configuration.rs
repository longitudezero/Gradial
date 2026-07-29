use std::path::PathBuf;

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::SliceCount;

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_GLOBAL_PROFILE_ID: Uuid =
    Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0001);

/// The action-free configuration foundation used by the profile milestone.
///
/// Menu action references and action definitions are deliberately introduced
/// with the executor slice. Keeping them out of this schema prevents the
/// persistence layer from accepting unvalidated placeholder actions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub schema_version: u32,
    pub config_revision: u64,
    pub global_profile: Profile,
    pub application_profiles: Vec<ApplicationProfile>,
}

impl Configuration {
    /// Validates the complete in-memory model before it can become active.
    ///
    /// # Errors
    ///
    /// Returns a field-specific [`ConfigurationError`] for unsupported
    /// schemas, duplicate IDs, invalid rules, or malformed menus.
    pub fn validate(&self) -> Result<(), ConfigurationError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigurationError::UnsupportedSchema {
                found: self.schema_version,
                supported: CONFIG_SCHEMA_VERSION,
            });
        }

        validate_profile(&self.global_profile)?;
        let mut profile_ids = vec![self.global_profile.id];
        for application_profile in &self.application_profiles {
            validate_profile(&application_profile.profile)?;
            validate_rule(application_profile)?;
            if profile_ids.contains(&application_profile.profile.id) {
                return Err(ConfigurationError::DuplicateProfileId {
                    profile_id: application_profile.profile.id,
                });
            }
            profile_ids.push(application_profile.profile.id);
        }
        Ok(())
    }

    #[must_use]
    pub fn profile(&self, profile_id: Uuid) -> Option<&Profile> {
        if self.global_profile.id == profile_id {
            return Some(&self.global_profile);
        }
        self.application_profiles
            .iter()
            .find(|candidate| candidate.profile.id == profile_id)
            .map(|candidate| &candidate.profile)
    }
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            config_revision: 0,
            global_profile: Profile {
                id: DEFAULT_GLOBAL_PROFILE_ID,
                name: "Global".to_owned(),
                menu: Menu::eight_slice_default(),
            },
            application_profiles: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    pub menu: Menu,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationProfile {
    pub profile: Profile,
    pub rule: ProfileRule,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileRule {
    pub executable_path: PathBuf,
    pub window_class: Option<String>,
    pub title_regex: Option<String>,
    pub priority: u8,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Menu {
    pub slice_count: u8,
    pub slices: Vec<MenuSlice>,
}

impl Menu {
    #[must_use]
    pub fn eight_slice_default() -> Self {
        Self {
            slice_count: 8,
            slices: ["N", "NE", "E", "SE", "S", "SW", "W", "NW"]
                .map(|label| MenuSlice { label: label.to_owned() })
                .into(),
        }
    }

    #[must_use]
    pub fn resolved_slice_count(&self) -> Option<SliceCount> {
        match self.slice_count {
            4 => Some(SliceCount::Four),
            8 => Some(SliceCount::Eight),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuSlice {
    pub label: String,
}

fn validate_profile(profile: &Profile) -> Result<(), ConfigurationError> {
    if profile.name.trim().is_empty() {
        return Err(ConfigurationError::EmptyProfileName { profile_id: profile.id });
    }
    let Some(slice_count) = profile.menu.resolved_slice_count() else {
        return Err(ConfigurationError::UnsupportedSliceCount {
            profile_id: profile.id,
            slice_count: profile.menu.slice_count,
        });
    };
    if profile.menu.slices.len() != slice_count.get() {
        return Err(ConfigurationError::IncorrectMenuLength {
            profile_id: profile.id,
            slice_count: profile.menu.slice_count,
            found: profile.menu.slices.len(),
        });
    }
    Ok(())
}

fn validate_rule(application_profile: &ApplicationProfile) -> Result<(), ConfigurationError> {
    let profile_id = application_profile.profile.id;
    if application_profile.rule.executable_path.as_os_str().is_empty() {
        return Err(ConfigurationError::EmptyExecutablePath { profile_id });
    }
    if application_profile.rule.priority > 100 {
        return Err(ConfigurationError::InvalidPriority {
            profile_id,
            priority: application_profile.rule.priority,
        });
    }
    if application_profile
        .rule
        .window_class
        .as_ref()
        .is_some_and(|window_class| window_class.trim().is_empty())
    {
        return Err(ConfigurationError::EmptyWindowClass { profile_id });
    }
    if let Some(pattern) = &application_profile.rule.title_regex {
        if pattern.is_empty() {
            return Err(ConfigurationError::EmptyTitleRegex { profile_id });
        }
        Regex::new(pattern)
            .map_err(|source| ConfigurationError::InvalidTitleRegex { profile_id, source })?;
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ConfigurationError {
    #[error("unsupported configuration schema {found}; this build supports schema {supported}")]
    UnsupportedSchema { found: u32, supported: u32 },

    #[error("profile {profile_id} has an empty name")]
    EmptyProfileName { profile_id: Uuid },

    #[error("profile {profile_id} uses unsupported slice count {slice_count}")]
    UnsupportedSliceCount { profile_id: Uuid, slice_count: u8 },

    #[error(
        "profile {profile_id} declares {slice_count} slices but contains {found} slice definitions"
    )]
    IncorrectMenuLength { profile_id: Uuid, slice_count: u8, found: usize },

    #[error("profile UUID {profile_id} appears more than once")]
    DuplicateProfileId { profile_id: Uuid },

    #[error("application profile {profile_id} has an empty executable path")]
    EmptyExecutablePath { profile_id: Uuid },

    #[error("application profile {profile_id} has priority {priority}; the maximum is 100")]
    InvalidPriority { profile_id: Uuid, priority: u8 },

    #[error("application profile {profile_id} has an empty window-class predicate")]
    EmptyWindowClass { profile_id: Uuid },

    #[error("application profile {profile_id} has an empty title-regex predicate")]
    EmptyTitleRegex { profile_id: Uuid },

    #[error("application profile {profile_id} has an invalid title regex")]
    InvalidTitleRegex {
        profile_id: Uuid,
        #[source]
        source: regex::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::{
        ApplicationProfile, Configuration, ConfigurationError, Menu, Profile, ProfileRule,
    };

    fn application_profile(id: u128) -> ApplicationProfile {
        ApplicationProfile {
            profile: Profile {
                id: Uuid::from_u128(id),
                name: format!("Profile {id}"),
                menu: Menu::eight_slice_default(),
            },
            rule: ProfileRule {
                executable_path: PathBuf::from(r"C:\Apps\Example.exe"),
                window_class: None,
                title_regex: None,
                priority: 50,
            },
        }
    }

    #[test]
    fn default_configuration_is_valid_and_action_free() {
        let configuration = Configuration::default();
        configuration.validate().expect("default configuration");
        assert_eq!(configuration.global_profile.menu.slices.len(), 8);
        assert!(configuration.application_profiles.is_empty());
    }

    #[test]
    fn duplicate_profile_ids_are_rejected() {
        let mut configuration = Configuration::default();
        let mut profile = application_profile(2);
        profile.profile.id = configuration.global_profile.id;
        configuration.application_profiles.push(profile);

        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::DuplicateProfileId { .. })
        ));
    }

    #[test]
    fn invalid_regex_is_rejected_before_activation() {
        let mut configuration = Configuration::default();
        let mut profile = application_profile(2);
        profile.rule.title_regex = Some("(".to_owned());
        configuration.application_profiles.push(profile);

        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::InvalidTitleRegex { .. })
        ));
    }

    #[test]
    fn menu_length_must_match_its_slice_count() {
        let mut configuration = Configuration::default();
        configuration.global_profile.menu.slice_count = 4;

        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::IncorrectMenuLength { slice_count: 4, found: 8, .. })
        ));
    }

    #[test]
    fn priorities_above_one_hundred_are_rejected() {
        let mut configuration = Configuration::default();
        let mut profile = application_profile(2);
        profile.rule.priority = 101;
        configuration.application_profiles.push(profile);

        assert!(matches!(
            configuration.validate(),
            Err(ConfigurationError::InvalidPriority { priority: 101, .. })
        ));
    }
}
