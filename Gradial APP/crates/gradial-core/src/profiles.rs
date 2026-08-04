use std::{cmp::Ordering, path::Path, sync::Arc};

use regex::Regex;
use uuid::Uuid;

use crate::{Configuration, ConfigurationError, Profile, SliceCount};

#[derive(Debug)]
pub struct ProfileResolver {
    global_profile_id: Uuid,
    global_menu: Arc<ResolvedMenu>,
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
                    menu: compile_resolved_menu(&application_profile.profile)?,
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
        Ok(Self {
            global_profile_id: configuration.global_profile.id,
            global_menu: compile_resolved_menu(&configuration.global_profile)?,
            application_profiles,
        })
    }

    #[must_use]
    pub fn resolve(&self, target: ProfileTarget<'_>) -> ProfileResolution {
        let Some(executable_path) = target.executable_path else {
            return ProfileResolution::global(
                self.global_profile_id,
                Arc::clone(&self.global_menu),
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
                Arc::clone(&self.global_menu),
                ResolutionDecision::NoApplicationProfileMatched,
            );
        };
        let decision = decision_for_winner(&matching_profiles);
        ProfileResolution {
            profile_id: winner.profile_id,
            menu: Arc::clone(&winner.menu),
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

fn compile_resolved_menu(profile: &Profile) -> Result<Arc<ResolvedMenu>, ConfigurationError> {
    let slice_count =
        profile.menu.resolved_slice_count().ok_or(ConfigurationError::UnsupportedSliceCount {
            profile_id: profile.id,
            slice_count: profile.menu.slice_count,
        })?;
    ResolvedMenu::new_with_continuous_actions(
        slice_count,
        profile.menu.slices.iter().map(|slice| slice.label.clone()).collect(),
        profile.menu.slices.iter().map(|slice| slice.action_id).collect(),
        profile.menu.slices.iter().map(|slice| slice.continuous_action_id).collect(),
        profile.menu.slices.iter().map(|slice| slice.icon).collect(),
    )
    .map(Arc::new)
    .ok_or(ConfigurationError::IncorrectMenuLength {
        profile_id: profile.id,
        slice_count: profile.menu.slice_count,
        found: profile.menu.slices.len(),
    })
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
    pub menu: Arc<ResolvedMenu>,
    pub used_global_profile: bool,
    pub decision: ResolutionDecision,
    pub matching_profiles: Vec<ProfileMatch>,
}

impl ProfileResolution {
    fn global(profile_id: Uuid, menu: Arc<ResolvedMenu>, decision: ResolutionDecision) -> Self {
        Self {
            profile_id,
            menu,
            used_global_profile: true,
            decision,
            matching_profiles: Vec::new(),
        }
    }
}

/// A complete immutable menu selected for one invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedMenu {
    slice_count: SliceCount,
    labels: Arc<[String]>,
    action_ids: Arc<[Option<Uuid>]>,
    continuous_action_ids: Arc<[Option<Uuid>]>,
    icons: Arc<[Option<crate::IconReference>]>,
}

impl ResolvedMenu {
    /// Creates a menu only when every supported slice has exactly one label
    /// and one optional action reference.
    #[must_use]
    pub fn new(
        slice_count: SliceCount,
        labels: Arc<[String]>,
        action_ids: Arc<[Option<Uuid>]>,
    ) -> Option<Self> {
        let icons = vec![None; slice_count.get()].into();
        Self::new_with_icons(slice_count, labels, action_ids, icons)
    }

    /// Creates a complete menu including one optional icon reference per slice.
    #[must_use]
    pub fn new_with_icons(
        slice_count: SliceCount,
        labels: Arc<[String]>,
        action_ids: Arc<[Option<Uuid>]>,
        icons: Arc<[Option<crate::IconReference>]>,
    ) -> Option<Self> {
        let continuous_action_ids = vec![None; slice_count.get()].into();
        Self::new_with_continuous_actions(
            slice_count,
            labels,
            action_ids,
            continuous_action_ids,
            icons,
        )
    }

    /// Creates a complete menu including separate discrete and continuous
    /// assignments plus one optional icon reference per slice.
    #[must_use]
    pub fn new_with_continuous_actions(
        slice_count: SliceCount,
        labels: Arc<[String]>,
        action_ids: Arc<[Option<Uuid>]>,
        continuous_action_ids: Arc<[Option<Uuid>]>,
        icons: Arc<[Option<crate::IconReference>]>,
    ) -> Option<Self> {
        (labels.len() == slice_count.get()
            && action_ids.len() == slice_count.get()
            && continuous_action_ids.len() == slice_count.get()
            && icons.len() == slice_count.get())
        .then_some(Self { slice_count, labels, action_ids, continuous_action_ids, icons })
    }

    #[must_use]
    pub const fn slice_count(&self) -> SliceCount {
        self.slice_count
    }

    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    #[must_use]
    pub fn action_id(&self, slice_index: usize) -> Option<Uuid> {
        self.action_ids.get(slice_index).copied().flatten()
    }

    #[must_use]
    pub fn continuous_action_id(&self, slice_index: usize) -> Option<Uuid> {
        self.continuous_action_ids.get(slice_index).copied().flatten()
    }

    #[must_use]
    pub fn icon(&self, slice_index: usize) -> Option<crate::IconReference> {
        self.icons.get(slice_index).copied().flatten()
    }

    /// Creates the direction-labelled menu used by debug interaction checks.
    #[must_use]
    pub fn directional(slice_count: SliceCount) -> Self {
        let labels: &[&str] = match slice_count {
            SliceCount::Four => &["N", "E", "S", "W"],
            SliceCount::Eight => &["N", "NE", "E", "SE", "S", "SW", "W", "NW"],
        };
        Self {
            slice_count,
            labels: labels.iter().map(|label| (*label).to_owned()).collect(),
            action_ids: vec![None; slice_count.get()].into(),
            continuous_action_ids: vec![None; slice_count.get()].into(),
            icons: vec![None; slice_count.get()].into(),
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
    menu: Arc<ResolvedMenu>,
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
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use uuid::Uuid;

    use super::{ProfileResolver, ProfileTarget, ResolutionDecision};
    use crate::{
        ActionDefinition, ActionKind, ApplicationProfile, Configuration, IconFormat, IconReference,
        Menu, MenuSlice, Profile, ProfileRule, SliceCount,
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

    #[test]
    fn resolved_profile_freezes_its_complete_custom_menu() {
        let mut application_profile = profile(2, 10, None, None);
        application_profile.profile.menu = Menu {
            slice_count: 4,
            slices: ["Build", "Search", "Tests", "Terminal"]
                .map(|label| MenuSlice {
                    label: label.to_owned(),
                    action_id: None,
                    continuous_action_id: None,
                    icon: None,
                })
                .into(),
        };
        let mut configuration = Configuration {
            application_profiles: vec![application_profile],
            ..Configuration::default()
        };
        let icon = IconReference { asset_id: Uuid::from_u128(88), format: IconFormat::Svg };
        let continuous_action_id = Uuid::from_u128(89);
        configuration.application_profiles[0].profile.menu.slices[0].icon = Some(icon);
        configuration.application_profiles[0].profile.menu.slices[0].continuous_action_id =
            Some(continuous_action_id);
        configuration.actions.push(ActionDefinition {
            id: continuous_action_id,
            name: "Measure".to_owned(),
            kind: ActionKind::PluginContinuous {
                plugin_id: "org.gradial.meter".to_owned(),
                plugin_action_id: "measure".to_owned(),
                parameters: BTreeMap::new(),
            },
        });
        let resolver = ProfileResolver::compile(&configuration).expect("resolver");
        configuration.application_profiles[0].profile.menu.slices[0].label = "Changed".to_owned();
        configuration.application_profiles[0].profile.menu.slices[0].icon = None;
        configuration.application_profiles[0].profile.menu.slices[0].continuous_action_id = None;

        let first =
            resolver.resolve(target(Some(Path::new(r"C:\Apps\Editor.exe")), "", "Document"));
        let second =
            resolver.resolve(target(Some(Path::new(r"C:\Apps\Editor.exe")), "", "Document"));

        assert_eq!(first.menu.slice_count(), SliceCount::Four);
        assert_eq!(first.menu.labels(), ["Build", "Search", "Tests", "Terminal"]);
        assert_eq!(first.menu.icon(0), Some(icon));
        assert_eq!(first.menu.continuous_action_id(0), Some(continuous_action_id));
        assert!(std::sync::Arc::ptr_eq(&first.menu, &second.menu));
    }

    #[test]
    fn resolved_menu_rejects_incomplete_label_sets() {
        assert!(super::ResolvedMenu::new(SliceCount::Four, Arc::from([]), Arc::from([])).is_none());
        assert!(
            super::ResolvedMenu::new(
                SliceCount::Eight,
                Arc::from(["only one".to_owned()]),
                vec![None; 8].into(),
            )
            .is_none()
        );
    }

    #[test]
    fn resolved_menu_freezes_action_references_with_labels() {
        let action_id = Uuid::from_u128(77);
        let menu = super::ResolvedMenu::new(
            SliceCount::Four,
            ["One", "Two", "Three", "Four"].map(str::to_owned).into(),
            vec![Some(action_id), None, None, None].into(),
        )
        .expect("complete menu");

        assert_eq!(menu.action_id(0), Some(action_id));
        assert_eq!(menu.action_id(1), None);
        assert_eq!(menu.action_id(4), None);
    }
}
