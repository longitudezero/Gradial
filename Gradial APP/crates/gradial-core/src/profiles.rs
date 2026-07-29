use std::{cmp::Ordering, path::Path};

use regex::Regex;
use uuid::Uuid;

use crate::{Configuration, ConfigurationError, SliceCount};

#[derive(Debug)]
pub struct ProfileResolver {
    global_profile_id: Uuid,
    global_slice_count: SliceCount,
    application_profiles: Vec<CompiledApplicationProfile>,
}

impl ProfileResolver {
    /// Validates and compiles all profile rules once, when configuration loads.
    ///
    /// # Errors
    ///
    /// Returns the first field-specific configuration error. Invocation-time
    /// resolution is then allocation-bounded and cannot fail on a regex.
    pub fn compile(configuration: &Configuration) -> Result<Self, ConfigurationError> {
        configuration.validate()?;
        let application_profiles = configuration
            .application_profiles
            .iter()
            .map(|application_profile| {
                let profile_id = application_profile.profile.id;
                let slice_count = application_profile.profile.menu.resolved_slice_count().ok_or(
                    ConfigurationError::UnsupportedSliceCount {
                        profile_id,
                        slice_count: application_profile.profile.menu.slice_count,
                    },
                )?;
                let title_regex = application_profile
                    .rule
                    .title_regex
                    .as_ref()
                    .map(|pattern| {
                        Regex::new(pattern).map_err(|source| {
                            ConfigurationError::InvalidTitleRegex { profile_id, source }
                        })
                    })
                    .transpose()?;
                Ok(CompiledApplicationProfile {
                    profile_id,
                    slice_count,
                    executable_path: normalize_windows_path(
                        &application_profile.rule.executable_path,
                    ),
                    window_class: application_profile
                        .rule
                        .window_class
                        .as_deref()
                        .map(normalize_case_insensitive),
                    title_regex,
                    priority: application_profile.rule.priority,
                })
            })
            .collect::<Result<Vec<_>, ConfigurationError>>()?;
        let global_slice_count = configuration.global_profile.menu.resolved_slice_count().ok_or(
            ConfigurationError::UnsupportedSliceCount {
                profile_id: configuration.global_profile.id,
                slice_count: configuration.global_profile.menu.slice_count,
            },
        )?;
        Ok(Self {
            global_profile_id: configuration.global_profile.id,
            global_slice_count,
            application_profiles,
        })
    }

    #[must_use]
    pub fn resolve(&self, target: ProfileTarget<'_>) -> ProfileResolution {
        let Some(executable_path) = target.executable_path else {
            return ProfileResolution::global(
                self.global_profile_id,
                self.global_slice_count,
                ResolutionDecision::ExecutablePathUnavailable,
            );
        };
        let executable_path = normalize_windows_path(executable_path);
        let window_class = normalize_case_insensitive(target.window_class);
        let mut matching_profiles: Vec<_> = self
            .application_profiles
            .iter()
            .filter(|candidate| {
                candidate.executable_path == executable_path
                    && candidate
                        .window_class
                        .as_ref()
                        .is_none_or(|expected| expected == &window_class)
                    && candidate
                        .title_regex
                        .as_ref()
                        .is_none_or(|pattern| pattern.is_match(target.window_title))
            })
            .collect();

        matching_profiles.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| right.specificity().cmp(&left.specificity()))
                .then_with(|| left.profile_id.cmp(&right.profile_id))
        });

        let Some(winner) = matching_profiles.first() else {
            return ProfileResolution::global(
                self.global_profile_id,
                self.global_slice_count,
                ResolutionDecision::NoApplicationProfileMatched,
            );
        };
        let decision = decision_for_winner(&matching_profiles);
        ProfileResolution {
            profile_id: winner.profile_id,
            slice_count: winner.slice_count,
            used_global_profile: false,
            decision,
            matching_profiles: matching_profiles
                .iter()
                .map(|candidate| ProfileMatch {
                    profile_id: candidate.profile_id,
                    priority: candidate.priority,
                    specificity: candidate.specificity(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProfileTarget<'a> {
    pub executable_path: Option<&'a Path>,
    pub window_class: &'a str,
    pub window_title: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileResolution {
    pub profile_id: Uuid,
    pub slice_count: SliceCount,
    pub used_global_profile: bool,
    pub decision: ResolutionDecision,
    pub matching_profiles: Vec<ProfileMatch>,
}

impl ProfileResolution {
    fn global(profile_id: Uuid, slice_count: SliceCount, decision: ResolutionDecision) -> Self {
        Self {
            profile_id,
            slice_count,
            used_global_profile: true,
            decision,
            matching_profiles: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileMatch {
    pub profile_id: Uuid,
    pub priority: u8,
    pub specificity: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionDecision {
    ExecutablePathUnavailable,
    NoApplicationProfileMatched,
    OnlyApplicationProfileMatched,
    HighestPriority,
    HighestSpecificity,
    LexicographicallySmallestUuid,
}

impl ResolutionDecision {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExecutablePathUnavailable => "executable_path_unavailable",
            Self::NoApplicationProfileMatched => "no_application_profile_matched",
            Self::OnlyApplicationProfileMatched => "only_application_profile_matched",
            Self::HighestPriority => "highest_priority",
            Self::HighestSpecificity => "highest_specificity",
            Self::LexicographicallySmallestUuid => "lexicographically_smallest_uuid",
        }
    }
}

#[derive(Debug)]
struct CompiledApplicationProfile {
    profile_id: Uuid,
    slice_count: SliceCount,
    executable_path: String,
    window_class: Option<String>,
    title_regex: Option<Regex>,
    priority: u8,
}

impl CompiledApplicationProfile {
    fn specificity(&self) -> u8 {
        1 + u8::from(self.window_class.is_some()) + u8::from(self.title_regex.is_some())
    }
}

fn decision_for_winner(matching_profiles: &[&CompiledApplicationProfile]) -> ResolutionDecision {
    let [winner, runner_up, ..] = matching_profiles else {
        return ResolutionDecision::OnlyApplicationProfileMatched;
    };
    match winner.priority.cmp(&runner_up.priority) {
        Ordering::Greater => ResolutionDecision::HighestPriority,
        Ordering::Less => unreachable!("matching profiles are sorted by descending priority"),
        Ordering::Equal => match winner.specificity().cmp(&runner_up.specificity()) {
            Ordering::Greater => ResolutionDecision::HighestSpecificity,
            Ordering::Less => unreachable!("matching profiles are sorted by specificity"),
            Ordering::Equal => ResolutionDecision::LexicographicallySmallestUuid,
        },
    }
}

fn normalize_windows_path(path: &Path) -> String {
    let normalized_separators = path.as_os_str().to_string_lossy().replace('/', "\\");
    let without_trailing_separator = if normalized_separators.len() > 3 {
        normalized_separators.trim_end_matches('\\')
    } else {
        &normalized_separators
    };
    normalize_case_insensitive(without_trailing_separator)
}

fn normalize_case_insensitive(value: &str) -> String {
    value.to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use uuid::Uuid;

    use super::{ProfileResolver, ProfileTarget, ResolutionDecision};
    use crate::{
        ApplicationProfile, Configuration, Menu, Profile, ProfileRule,
        configuration::DEFAULT_GLOBAL_PROFILE_ID,
    };

    fn profile(
        id: u128,
        priority: u8,
        window_class: Option<&str>,
        title_regex: Option<&str>,
    ) -> ApplicationProfile {
        ApplicationProfile {
            profile: Profile {
                id: Uuid::from_u128(id),
                name: format!("Profile {id}"),
                menu: Menu::eight_slice_default(),
            },
            rule: ProfileRule {
                executable_path: PathBuf::from(r"C:\Apps\Editor.exe"),
                window_class: window_class.map(str::to_owned),
                title_regex: title_regex.map(str::to_owned),
                priority,
            },
        }
    }

    fn target<'a>(path: Option<&'a Path>, class: &'a str, title: &'a str) -> ProfileTarget<'a> {
        ProfileTarget { executable_path: path, window_class: class, window_title: title }
    }

    #[test]
    fn missing_executable_path_uses_only_the_global_profile() {
        let mut configuration = Configuration::default();
        configuration.application_profiles.push(profile(2, 100, None, None));
        let resolver = ProfileResolver::compile(&configuration).expect("resolver");

        let resolution = resolver.resolve(target(None, "", ""));
        assert_eq!(resolution.profile_id, DEFAULT_GLOBAL_PROFILE_ID);
        assert!(resolution.used_global_profile);
        assert_eq!(resolution.decision, ResolutionDecision::ExecutablePathUnavailable);
        assert!(resolution.matching_profiles.is_empty());
    }

    #[test]
    fn paths_and_classes_match_case_insensitively_with_normalized_separators() {
        let mut configuration = Configuration::default();
        configuration.application_profiles.push(profile(2, 10, Some("EditorWindow"), None));
        let resolver = ProfileResolver::compile(&configuration).expect("resolver");

        let resolution = resolver.resolve(target(
            Some(Path::new(r"c:/apps/EDITOR.exe")),
            "editorwindow",
            "Document",
        ));
        assert_eq!(resolution.profile_id, Uuid::from_u128(2));
        assert_eq!(resolution.decision, ResolutionDecision::OnlyApplicationProfileMatched);
    }

    #[test]
    fn highest_priority_wins_before_specificity() {
        let configuration = Configuration {
            application_profiles: vec![
                profile(2, 90, None, None),
                profile(3, 80, Some("EditorWindow"), Some("Document")),
            ],
            ..Configuration::default()
        };
        let resolver = ProfileResolver::compile(&configuration).expect("resolver");

        let resolution = resolver.resolve(target(
            Some(Path::new(r"C:\Apps\Editor.exe")),
            "EditorWindow",
            "Document",
        ));
        assert_eq!(resolution.profile_id, Uuid::from_u128(2));
        assert_eq!(resolution.decision, ResolutionDecision::HighestPriority);
    }

    #[test]
    fn specificity_breaks_equal_priority() {
        let configuration = Configuration {
            application_profiles: vec![
                profile(2, 90, None, None),
                profile(3, 90, Some("EditorWindow"), Some("^Document$")),
            ],
            ..Configuration::default()
        };
        let resolver = ProfileResolver::compile(&configuration).expect("resolver");

        let resolution = resolver.resolve(target(
            Some(Path::new(r"C:\Apps\Editor.exe")),
            "EditorWindow",
            "Document",
        ));
        assert_eq!(resolution.profile_id, Uuid::from_u128(3));
        assert_eq!(resolution.decision, ResolutionDecision::HighestSpecificity);
    }

    #[test]
    fn smallest_uuid_breaks_a_complete_tie() {
        let configuration = Configuration {
            application_profiles: vec![profile(9, 90, None, None), profile(2, 90, None, None)],
            ..Configuration::default()
        };
        let resolver = ProfileResolver::compile(&configuration).expect("resolver");

        let resolution = resolver.resolve(target(Some(Path::new(r"C:\Apps\Editor.exe")), "", ""));
        assert_eq!(resolution.profile_id, Uuid::from_u128(2));
        assert_eq!(resolution.decision, ResolutionDecision::LexicographicallySmallestUuid);
        assert_eq!(resolution.matching_profiles.len(), 2);
    }

    #[test]
    fn title_regex_is_opt_in_and_case_sensitive_by_default() {
        let mut configuration = Configuration::default();
        configuration.application_profiles.push(profile(2, 10, None, Some("^Document")));
        let resolver = ProfileResolver::compile(&configuration).expect("resolver");

        assert_eq!(
            resolver
                .resolve(target(Some(Path::new(r"C:\Apps\Editor.exe")), "", "document one",))
                .profile_id,
            DEFAULT_GLOBAL_PROFILE_ID
        );
        assert_eq!(
            resolver
                .resolve(target(Some(Path::new(r"C:\Apps\Editor.exe")), "", "Document one",))
                .profile_id,
            Uuid::from_u128(2)
        );
    }
}
