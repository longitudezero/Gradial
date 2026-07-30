use std::{
    collections::HashMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align, Color32, ComboBox, DragValue, Id, Layout, Modal, RichText, ScrollArea, Sense,
    TextEdit, UiBuilder, Vec2, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId,
};
use gradial_core::{
    AccentColor, ActionDefinition, ActionKind, ActionPlanCompiler, ActivationBinding,
    ActivationBindingId, Appearance, ApplicationProfile, Configuration, ConfigurationError,
    ExecutionOperation, IconReference, LabelTheme, MAX_ACTION_SEQUENCE_DEPTH,
    MAX_ACTION_SEQUENCE_DURATION_MS, MAX_ACTION_SEQUENCE_STEPS, MAX_TYPED_TEXT_BYTES, Menu,
    MenuSlice, PhysicalModifiers, Profile, ProfileResolver, ProfileRule, ProfileTarget,
    ResolutionDecision, ResolvedMenu, ShortcutKey, SizePreset, SliceCount, WindowCommand,
    slice_for_delta,
};
#[cfg(debug_assertions)]
use gradial_core::{AppPaths, ShortcutModifier};
use regex::Regex;
use uuid::Uuid;

use crate::{
    configuration::{ConfigurationMigration, ConfigurationTransferError, PortableIconSummary},
    diagnostic_bundle::{
        DiagnosticBundleError, DiagnosticBundleRequest, DiagnosticBundleSummary, DiagnosticSnapshot,
    },
    icons::{IconImportError, IconManager},
    input::{self, ActivationBindingTestResult},
    native_file_dialog::{PathDialogKind, PathDialogOutcome, choose_path},
    overlay::{AnimationMode, FoundationOverlayRenderer, OverlayRenderFrame, OverlayRenderer},
    platform::{self, ProfileTestTarget},
    shortcut_recorder::{CaptureOutcome, ShortcutRecorder, shortcut_display},
};

const SETTINGS_SIZE: [f32; 2] = [940.0, 680.0];
const SETTINGS_MIN_SIZE: [f32; 2] = [820.0, 580.0];
const PREVIEW_SIZE: f32 = 288.0;
const PROFILE_TEST_COUNTDOWN: Duration = Duration::from_secs(3);
const PREVIEW_INVOCATION_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0901);
#[cfg(debug_assertions)]
const DEBUG_SHORTCUT_ACTION_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0902);
#[cfg(debug_assertions)]
const DEBUG_TEXT_ACTION_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0903);
#[cfg(debug_assertions)]
const DEBUG_DUPLICATE_ACTION_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0904);
#[cfg(debug_assertions)]
const DEBUG_URL_ACTION_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0905);
#[cfg(debug_assertions)]
const DEBUG_PATH_ACTION_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0906);
#[cfg(debug_assertions)]
const DEBUG_EXECUTABLE_ACTION_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0907);
#[cfg(debug_assertions)]
const DEBUG_DELAY_ACTION_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0908);
#[cfg(debug_assertions)]
const DEBUG_WINDOW_ACTION_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0909);
#[cfg(debug_assertions)]
const DEBUG_SEQUENCE_ACTION_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0910);
#[cfg(debug_assertions)]
const DEBUG_NESTED_SEQUENCE_ACTION_ID: Uuid =
    Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0911);
#[cfg(debug_assertions)]
const DEBUG_APPLICATION_PROFILE_ID: Uuid =
    Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0920);
#[cfg(debug_assertions)]
const DEBUG_DUPLICATE_PROFILE_ID: Uuid = Uuid::from_u128(0x6772_6164_6961_4c00_8000_0000_0000_0921);
#[cfg(debug_assertions)]
const DEBUG_PROFILE_TEST_TITLE: &str = "Profile test title private 7F3C";
pub(crate) fn settings_viewport_id() -> ViewportId {
    ViewportId::from_hash_of("gradial-settings")
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SettingsDraft {
    original: Configuration,
    proposed: Configuration,
    eight_slice_stashes: HashMap<Uuid, Vec<MenuSlice>>,
}

impl SettingsDraft {
    fn new(configuration: &Configuration) -> Self {
        Self {
            original: configuration.clone(),
            proposed: configuration.clone(),
            eight_slice_stashes: HashMap::new(),
        }
    }

    fn is_dirty(&self) -> bool {
        self.proposed != self.original
    }

    fn discard(&mut self) {
        self.proposed = self.original.clone();
        self.eight_slice_stashes.clear();
    }

    fn accept_saved(&mut self, configuration: &Configuration) {
        self.original = configuration.clone();
        self.proposed = configuration.clone();
        self.eight_slice_stashes.clear();
    }

    fn replace_proposed(&mut self, mut configuration: Configuration) {
        configuration.config_revision = self.original.config_revision;
        self.proposed = configuration;
        self.eight_slice_stashes.clear();
    }

    fn set_slice_count(&mut self, slice_count: SliceCount) {
        self.set_profile_slice_count(self.proposed.global_profile.id, slice_count);
    }

    fn set_profile_slice_count(&mut self, profile_id: Uuid, slice_count: SliceCount) {
        let mut stash = self.eight_slice_stashes.remove(&profile_id);
        let Some(menu) = profile_menu_mut(&mut self.proposed, profile_id) else {
            return;
        };
        resize_menu(menu, slice_count, &mut stash);
        if let Some(stash) = stash {
            self.eight_slice_stashes.insert(profile_id, stash);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CloseOutcome {
    Closed,
    ConfirmationShown,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum EditorLifecycle {
    #[default]
    Closed,
    Editing,
    ConfirmingClose,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SettingsSection {
    #[default]
    Menu,
    Binding,
    Appearance,
    Profiles,
    Actions,
    Diagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NewActionTemplate {
    Shortcut,
    Text,
    Website,
    FileOrFolder,
    Application,
    Delay,
    Window,
    Sequence,
}

impl NewActionTemplate {
    const ALL: [Self; 8] = [
        Self::Shortcut,
        Self::Text,
        Self::Website,
        Self::FileOrFolder,
        Self::Application,
        Self::Delay,
        Self::Window,
        Self::Sequence,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Shortcut => "Keyboard shortcut",
            Self::Text => "Text",
            Self::Website => "Website",
            Self::FileOrFolder => "File or folder",
            Self::Application => "Application",
            Self::Delay => "Delay",
            Self::Window => "Window command",
            Self::Sequence => "Sequence",
        }
    }

    const fn preferred_name(self) -> &'static str {
        match self {
            Self::Shortcut => "Keyboard shortcut",
            Self::Text => "Type text",
            Self::Website => "Open website",
            Self::FileOrFolder => "Open file or folder",
            Self::Application => "Run application",
            Self::Delay => "Delay",
            Self::Window => "Window command",
            Self::Sequence => "Sequence",
        }
    }

    fn initial_kind(self, initial_sequence_child: Option<Uuid>) -> Option<ActionKind> {
        Some(match self {
            Self::Shortcut => ActionKind::SendShortcut {
                modifiers: Vec::new(),
                key: ShortcutKey { scan_code: 0x64, extended: false },
            },
            Self::Text => ActionKind::TypeText { text: String::new() },
            Self::Website => ActionKind::OpenUrl { url: "https://example.com".to_owned() },
            Self::FileOrFolder => ActionKind::OpenPath { path: default_browse_path() },
            Self::Application => ActionKind::RunExecutable {
                executable: default_executable_path(),
                arguments: Vec::new(),
                working_directory: None,
            },
            Self::Delay => ActionKind::Delay { milliseconds: 250 },
            Self::Window => ActionKind::Window { command: WindowCommand::Minimize },
            Self::Sequence => ActionKind::Sequence { actions: vec![initial_sequence_child?] },
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum SaveFeedback {
    #[default]
    None,
    Saved,
    Error(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TransferFeedback {
    Info(String),
    Error(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DiagnosticFeedback {
    Info(String),
    Error(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransferOperation {
    Import,
    Export,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IconEditorFeedback {
    profile_id: Uuid,
    slice_index: usize,
    message: String,
    error: bool,
}

impl IconEditorFeedback {
    fn info(profile_id: Uuid, slice_index: usize, message: impl Into<String>) -> Self {
        Self { profile_id, slice_index, message: message.into(), error: false }
    }

    fn error(profile_id: Uuid, slice_index: usize, message: impl Into<String>) -> Self {
        Self { profile_id, slice_index, message: message.into(), error: true }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingIconImport {
    path: PrivatePath,
    profile_id: Uuid,
    slice_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DraftValidationError {
    EmptyLabel { index: usize, slice_count: SliceCount },
    EmptyApplicationLabel { profile: usize, index: usize, slice_count: SliceCount },
    EmptyApplicationProfileName { profile: usize },
    EmptyApplicationProfilePath { profile: usize },
    InvalidApplicationProfilePriority { profile: usize, priority: u8 },
    EmptyApplicationWindowClass { profile: usize },
    EmptyApplicationTitleRegex { profile: usize },
    InvalidApplicationTitleRegex { profile: usize },
    EmptyActionName { index: usize },
    TypedTextTooLong { index: usize, bytes: usize },
    InvalidUrl { index: usize },
    InvalidPath { index: usize, field: &'static str },
    InvalidExecutable { index: usize },
    InvalidArgument { index: usize, argument: usize },
    InvalidDelay { index: usize, milliseconds: u32 },
    InvalidSequenceLength { index: usize, found: usize },
    SequenceCycle { index: usize },
    SequenceDepth { index: usize, depth: u8 },
    SequenceSteps { index: usize, steps: usize },
    SequenceDelay { index: usize, milliseconds: u32 },
    MissingSequenceStep { index: usize, step: usize },
    InvalidConfiguration,
}

impl DraftValidationError {
    #[allow(clippy::too_many_lines)]
    fn message(&self) -> String {
        match self {
            Self::EmptyLabel { index, slice_count } => {
                format!("Give {} a label before saving.", direction_name(*index, *slice_count))
            }
            Self::EmptyApplicationLabel { profile, index, slice_count } => {
                format!(
                    "Give {} in application profile {} a label before saving.",
                    direction_name(*index, *slice_count),
                    profile + 1
                )
            }
            Self::EmptyApplicationProfileName { profile } => {
                format!("Give application profile {} a name before saving.", profile + 1)
            }
            Self::EmptyApplicationProfilePath { profile } => {
                format!("Choose an application for profile {} before saving.", profile + 1)
            }
            Self::InvalidApplicationProfilePriority { profile, priority } => {
                format!(
                    "Set application profile {} to a priority from 0 through 100; it currently uses {priority}.",
                    profile + 1
                )
            }
            Self::EmptyApplicationWindowClass { profile } => {
                format!(
                    "Enter a window class or turn off that match for application profile {}.",
                    profile + 1
                )
            }
            Self::EmptyApplicationTitleRegex { profile } => {
                format!(
                    "Enter a title pattern or turn off that match for application profile {}.",
                    profile + 1
                )
            }
            Self::InvalidApplicationTitleRegex { profile } => {
                format!(
                    "Fix the title pattern in application profile {} before saving.",
                    profile + 1
                )
            }
            Self::EmptyActionName { index } => {
                format!("Give action {} a name before saving.", index + 1)
            }
            Self::TypedTextTooLong { index, bytes } => {
                format!(
                    "Action {} contains {bytes} bytes of text; the limit is {MAX_TYPED_TEXT_BYTES}.",
                    index + 1
                )
            }
            Self::InvalidUrl { index } => {
                format!("Enter a complete http:// or https:// address for action {}.", index + 1)
            }
            Self::InvalidPath { index, field } => {
                format!(
                    "Choose or enter an absolute {} for action {}.",
                    action_field_name(field),
                    index + 1
                )
            }
            Self::InvalidExecutable { index } => {
                format!("Choose an .exe application for action {}.", index + 1)
            }
            Self::InvalidArgument { index, argument } => {
                format!(
                    "Remove the unsupported null character from argument {} of action {}.",
                    argument + 1,
                    index + 1
                )
            }
            Self::InvalidDelay { index, milliseconds } => {
                format!(
                    "Set action {} to a delay from 10 through 10000 ms; it currently uses {milliseconds} ms.",
                    index + 1
                )
            }
            Self::InvalidSequenceLength { index, found } => {
                format!(
                    "Give sequence action {} from 1 through {MAX_ACTION_SEQUENCE_STEPS} direct steps; it currently has {found}.",
                    index + 1
                )
            }
            Self::SequenceCycle { index } => {
                format!("Remove the circular reference from sequence action {}.", index + 1)
            }
            Self::SequenceDepth { index, depth } => {
                format!(
                    "Reduce sequence action {} to at most {MAX_ACTION_SEQUENCE_DEPTH} nested levels; it currently reaches {depth}.",
                    index + 1
                )
            }
            Self::SequenceSteps { index, steps } => {
                format!(
                    "Reduce sequence action {} to at most {MAX_ACTION_SEQUENCE_STEPS} executed steps; it currently expands to {steps}.",
                    index + 1
                )
            }
            Self::SequenceDelay { index, milliseconds } => {
                format!(
                    "Reduce sequence action {} to at most {MAX_ACTION_SEQUENCE_DURATION_MS} ms of Delay; it currently totals {milliseconds} ms.",
                    index + 1
                )
            }
            Self::MissingSequenceStep { index, step } => {
                format!(
                    "Choose an available action for step {} of sequence action {}.",
                    step + 1,
                    index + 1
                )
            }
            Self::InvalidConfiguration => {
                "These settings cannot be saved yet. Review the editor fields.".to_owned()
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ActionEditorFeedback {
    Info(String),
    Error(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProfileEditorFeedback {
    Info(String),
    Error(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum ProfileTestState {
    #[default]
    Idle,
    CountingDown {
        deadline: Instant,
    },
    Captured(ProfileTestTarget),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum BindingTestState {
    #[default]
    Idle,
    Waiting {
        expected: ActivationBinding,
    },
    Complete(ActivationBindingTestResult),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ActionReferenceSummary {
    menu_slices: usize,
    sequences: usize,
}

impl ActionReferenceSummary {
    const fn is_referenced(self) -> bool {
        self.menu_slices > 0 || self.sequences > 0
    }

    fn message(self) -> String {
        match (self.menu_slices, self.sequences) {
            (menu_slices, 0) => format!(
                "This action is assigned to {menu_slices} menu {}. Clear those assignments before deleting it.",
                if menu_slices == 1 { "slice" } else { "slices" }
            ),
            (0, sequences) => format!(
                "This action is used by {sequences} sequence {}. Remove those steps before deleting it.",
                if sequences == 1 { "step" } else { "steps" }
            ),
            (menu_slices, sequences) => format!(
                "This action is assigned to {menu_slices} menu {} and used by {sequences} sequence {}. Clear those references before deleting it.",
                if menu_slices == 1 { "slice" } else { "slices" },
                if sequences == 1 { "step" } else { "steps" }
            ),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct PrivatePath(PathBuf);

impl PrivatePath {
    fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub(crate) fn as_path(&self) -> &Path {
        &self.0
    }
}

impl std::fmt::Debug for PrivatePath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PrivatePath(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) enum SettingsEvent {
    Save(Configuration),
    Import(PrivatePath),
    Export { path: PrivatePath, configuration: Configuration },
    ImportIcon { path: PrivatePath, profile_id: Uuid, slice_index: usize },
    RefreshDiagnostics,
    ExportDiagnostics { path: PrivatePath, request: DiagnosticBundleRequest },
    Closed,
}

impl std::fmt::Debug for SettingsEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Save(_) => formatter.write_str("Save(<configuration>)"),
            Self::Import(_) => formatter.write_str("Import(<path>)"),
            Self::Export { .. } => formatter.write_str("Export(<path>, <configuration>)"),
            Self::ImportIcon { profile_id, slice_index, .. } => formatter
                .debug_struct("ImportIcon")
                .field("path", &"<redacted>")
                .field("profile_id", profile_id)
                .field("slice_index", slice_index)
                .finish(),
            Self::RefreshDiagnostics => formatter.write_str("RefreshDiagnostics"),
            Self::ExportDiagnostics { request, .. } => formatter
                .debug_struct("ExportDiagnostics")
                .field("path", &"<redacted>")
                .field("request", request)
                .finish(),
            Self::Closed => formatter.write_str("Closed"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct SettingsEditor {
    state: Arc<Mutex<SettingsState>>,
}

impl SettingsEditor {
    pub(crate) fn new(recovery_warning: bool, icons: Arc<IconManager>) -> Self {
        Self { state: Arc::new(Mutex::new(SettingsState::new(recovery_warning, icons))) }
    }

    pub(crate) fn open(&self, configuration: &Configuration) -> bool {
        self.state().open(configuration)
    }

    pub(crate) fn show(&self, context: &egui::Context) {
        if self.state().lifecycle == EditorLifecycle::Closed {
            return;
        }
        let state = Arc::clone(&self.state);
        context.show_viewport_deferred(
            settings_viewport_id(),
            settings_viewport_builder(),
            move |ui, viewport_class| {
                if viewport_class == ViewportClass::EmbeddedWindow {
                    log::error!(
                        target: "settings",
                        "component=settings event_code=settings_viewport_embedded \
                         result_code=failure config_revision=0"
                    );
                }
                let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let event = state.render(ui);
                if let Some(event) = event {
                    state.pending_event = Some(event);
                    drop(state);
                    ui.ctx().request_repaint_of(ViewportId::ROOT);
                }
            },
        );
    }

    pub(crate) fn try_event(&self) -> Option<SettingsEvent> {
        self.state().pending_event.take()
    }

    pub(crate) fn is_open(&self) -> bool {
        self.state().lifecycle != EditorLifecycle::Closed
    }

    pub(crate) fn proposed_configuration(&self) -> Option<Configuration> {
        let state = self.state();
        if state.lifecycle == EditorLifecycle::Closed {
            None
        } else {
            state.draft.as_ref().map(|draft| draft.proposed.clone())
        }
    }

    pub(crate) fn accept_saved(&self, configuration: &Configuration) {
        self.state().accept_saved(configuration);
    }

    pub(crate) fn save_failed(&self) {
        self.state().save_failed();
    }

    pub(crate) fn accept_imported(
        &self,
        configuration: Configuration,
        migration: Option<ConfigurationMigration>,
        icons: PortableIconSummary,
    ) {
        self.state().accept_imported(configuration, migration, icons);
    }

    pub(crate) fn import_failed(&self, error: ConfigurationTransferError) {
        self.state().transfer_failed(TransferOperation::Import, error);
    }

    pub(crate) fn accept_exported(&self, icons: PortableIconSummary) {
        self.state().accept_exported(icons);
    }

    pub(crate) fn export_failed(&self, error: ConfigurationTransferError) {
        self.state().transfer_failed(TransferOperation::Export, error);
    }

    pub(crate) fn accept_icon_import(
        &self,
        profile_id: Uuid,
        slice_index: usize,
        icon: IconReference,
    ) {
        self.state().accept_icon_import(profile_id, slice_index, icon);
    }

    pub(crate) fn icon_import_failed(
        &self,
        profile_id: Uuid,
        slice_index: usize,
        error: IconImportError,
    ) {
        self.state().icon_import_failed(profile_id, slice_index, error);
    }

    pub(crate) fn accept_diagnostic_preview(&self, snapshot: DiagnosticSnapshot) {
        self.state().accept_diagnostic_preview(snapshot);
    }

    pub(crate) fn diagnostic_preview_failed(&self, error: DiagnosticBundleError) {
        self.state().diagnostic_failed(error);
    }

    pub(crate) fn accept_diagnostic_exported(&self, summary: DiagnosticBundleSummary) {
        self.state().accept_diagnostic_exported(summary);
    }

    pub(crate) fn diagnostic_export_failed(&self, error: DiagnosticBundleError) {
        self.state().diagnostic_failed(error);
    }

    fn state(&self) -> MutexGuard<'_, SettingsState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Debug)]
struct SettingsState {
    lifecycle: EditorLifecycle,
    draft: Option<SettingsDraft>,
    section: SettingsSection,
    selected_slice: usize,
    selected_profile: Option<Uuid>,
    selected_action: Option<Uuid>,
    profile_feedback: Option<ProfileEditorFeedback>,
    profile_test: ProfileTestState,
    binding_test: BindingTestState,
    action_feedback: Option<ActionEditorFeedback>,
    shortcut_recorder: ShortcutRecorder,
    recording_action_id: Option<Uuid>,
    save_feedback: SaveFeedback,
    transfer_feedback: Option<TransferFeedback>,
    icon_feedback: Option<IconEditorFeedback>,
    pending_icon_import: Option<PendingIconImport>,
    pending_import_path: Option<PrivatePath>,
    diagnostic_snapshot: Option<Arc<DiagnosticSnapshot>>,
    diagnostic_include_process_paths: bool,
    diagnostic_feedback: Option<DiagnosticFeedback>,
    recovery_warning: bool,
    preview_renderer: FoundationOverlayRenderer,
    pending_event: Option<SettingsEvent>,
}

impl SettingsState {
    fn new(recovery_warning: bool, icons: Arc<IconManager>) -> Self {
        Self {
            lifecycle: EditorLifecycle::Closed,
            draft: None,
            section: SettingsSection::Menu,
            selected_slice: 0,
            selected_profile: None,
            selected_action: None,
            profile_feedback: None,
            profile_test: ProfileTestState::Idle,
            binding_test: BindingTestState::Idle,
            action_feedback: None,
            shortcut_recorder: ShortcutRecorder::default(),
            recording_action_id: None,
            save_feedback: SaveFeedback::None,
            transfer_feedback: None,
            icon_feedback: None,
            pending_icon_import: None,
            pending_import_path: None,
            diagnostic_snapshot: None,
            diagnostic_include_process_paths: false,
            diagnostic_feedback: None,
            recovery_warning,
            preview_renderer: FoundationOverlayRenderer::new(icons),
            pending_event: None,
        }
    }

    fn open(&mut self, configuration: &Configuration) -> bool {
        let newly_opened = self.lifecycle == EditorLifecycle::Closed;
        if newly_opened {
            self.lifecycle = EditorLifecycle::Editing;
            self.draft = Some(SettingsDraft::new(configuration));
            self.section = SettingsSection::Menu;
            self.selected_slice = 0;
            self.selected_profile = configuration
                .application_profiles
                .first()
                .map(|application| application.profile.id);
            self.selected_action = configuration.actions.first().map(|action| action.id);
            self.profile_feedback = None;
            self.profile_test = ProfileTestState::Idle;
            self.cancel_binding_test();
            self.action_feedback = None;
            self.shortcut_recorder.cancel();
            self.recording_action_id = None;
            self.save_feedback = SaveFeedback::None;
            self.transfer_feedback = None;
            self.icon_feedback = None;
            self.pending_icon_import = None;
            self.pending_import_path = None;
            self.diagnostic_snapshot = None;
            self.diagnostic_include_process_paths = false;
            self.diagnostic_feedback = None;
            log::info!(
                target: "settings",
                "component=settings event_code=settings_opened result_code=ok \
                 config_revision={}",
                configuration.config_revision
            );
        }
        newly_opened
    }

    fn accept_saved(&mut self, configuration: &Configuration) {
        if let Some(draft) = &mut self.draft {
            draft.accept_saved(configuration);
        }
        self.save_feedback = SaveFeedback::Saved;
        self.transfer_feedback = None;
        self.icon_feedback = None;
        self.pending_icon_import = None;
        self.profile_feedback = None;
        self.action_feedback = None;
        self.cancel_binding_test();
        self.recovery_warning = false;
        self.diagnostic_snapshot = None;
        self.diagnostic_include_process_paths = false;
        self.diagnostic_feedback = None;
    }

    fn save_failed(&mut self) {
        self.save_feedback = SaveFeedback::Error(
            "Gradial could not save these settings. Your draft is still open.".to_owned(),
        );
    }

    fn accept_imported(
        &mut self,
        configuration: Configuration,
        migration: Option<ConfigurationMigration>,
        icons: PortableIconSummary,
    ) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        draft.replace_proposed(configuration);
        let imported_dirty = draft.is_dirty();
        self.section = SettingsSection::Menu;
        self.selected_slice = 0;
        self.selected_profile =
            draft.proposed.application_profiles.first().map(|application| application.profile.id);
        self.selected_action = draft.proposed.actions.first().map(|action| action.id);
        self.profile_feedback = None;
        self.profile_test = ProfileTestState::Idle;
        self.cancel_binding_test();
        self.action_feedback = None;
        self.shortcut_recorder.cancel();
        self.recording_action_id = None;
        self.save_feedback = SaveFeedback::None;
        self.pending_import_path = None;
        self.pending_icon_import = None;
        self.icon_feedback = None;
        let mut message = if imported_dirty {
            migration.map_or_else(
                || "Settings imported into this draft. Save changes to apply them.".to_owned(),
                |migration| {
                    format!(
                        "Settings imported and updated from version {}. Save changes to apply them.",
                        migration.from_schema
                    )
                },
            )
        } else {
            "Settings imported. They match the settings already loaded in Gradial.".to_owned()
        };
        if icons.missing > 0 {
            let _written = write!(
                &mut message,
                " {} referenced {} not included; Gradial will show {} until the assets are restored.",
                icons.missing,
                if icons.missing == 1 { "icon was" } else { "icons were" },
                if icons.missing == 1 { "a placeholder" } else { "placeholders" }
            );
        } else if icons.copied > 0 || icons.reused > 0 {
            let _written = write!(
                &mut message,
                " {} portable icon {} ready.",
                icons.copied.saturating_add(icons.reused),
                if icons.copied.saturating_add(icons.reused) == 1 { "is" } else { "are" }
            );
        }
        self.transfer_feedback = Some(TransferFeedback::Info(message));
    }

    fn accept_exported(&mut self, icons: PortableIconSummary) {
        let message = if icons.referenced == 0 {
            "Current draft exported. It does not reference custom icons.".to_owned()
        } else {
            format!(
                "Current draft and {} custom {} exported.",
                icons.referenced,
                if icons.referenced == 1 { "icon" } else { "icons" }
            )
        };
        self.transfer_feedback = Some(TransferFeedback::Info(message));
    }

    fn transfer_failed(&mut self, operation: TransferOperation, error: ConfigurationTransferError) {
        self.pending_import_path = None;
        self.transfer_feedback =
            Some(TransferFeedback::Error(transfer_error_message(operation, error).to_owned()));
    }

    fn accept_diagnostic_preview(&mut self, snapshot: DiagnosticSnapshot) {
        let log_count = snapshot.logs().len();
        let display_count = snapshot.displays().len();
        self.section = SettingsSection::Diagnostics;
        self.diagnostic_snapshot = Some(Arc::new(snapshot));
        self.diagnostic_include_process_paths = false;
        self.diagnostic_feedback = Some(DiagnosticFeedback::Info(format!(
            "Preview ready with {log_count} log {} and {display_count} display {}.",
            if log_count == 1 { "file" } else { "files" },
            if display_count == 1 { "record" } else { "records" }
        )));
    }

    fn accept_diagnostic_exported(&mut self, summary: DiagnosticBundleSummary) {
        self.diagnostic_feedback = Some(DiagnosticFeedback::Info(format!(
            "Diagnostic bundle exported with {} archive {}, {} log {}, and {} configured application {}.",
            summary.archive_entries,
            if summary.archive_entries == 1 { "entry" } else { "entries" },
            summary.logs,
            if summary.logs == 1 { "file" } else { "files" },
            summary.process_paths,
            if summary.process_paths == 1 { "path" } else { "paths" }
        )));
    }

    fn diagnostic_failed(&mut self, error: DiagnosticBundleError) {
        self.diagnostic_feedback =
            Some(DiagnosticFeedback::Error(diagnostic_error_message(error).to_owned()));
    }

    fn accept_icon_import(&mut self, profile_id: Uuid, slice_index: usize, icon: IconReference) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        let Some(slice) = profile_menu_mut(&mut draft.proposed, profile_id)
            .and_then(|menu| menu.slices.get_mut(slice_index))
        else {
            self.icon_feedback = Some(IconEditorFeedback::error(
                profile_id,
                slice_index,
                "That menu direction is no longer available.",
            ));
            return;
        };
        slice.icon = Some(icon);
        self.save_feedback = SaveFeedback::None;
        self.icon_feedback = Some(IconEditorFeedback::info(
            profile_id,
            slice_index,
            "Icon imported into this draft. Save changes to apply it.",
        ));
    }

    fn icon_import_failed(&mut self, profile_id: Uuid, slice_index: usize, error: IconImportError) {
        self.icon_feedback = Some(IconEditorFeedback::error(
            profile_id,
            slice_index,
            icon_import_error_message(error),
        ));
    }

    #[cfg(debug_assertions)]
    fn debug_apply_fixture_edit(&mut self) -> bool {
        let Some(draft) = &mut self.draft else {
            return false;
        };
        draft.set_slice_count(SliceCount::Four);
        for (slice, label) in draft.proposed.global_profile.menu.slices.iter_mut().zip([
            "North edited",
            "East edited",
            "South edited",
            "West edited",
        ]) {
            label.clone_into(&mut slice.label);
        }
        self.selected_slice = 1;
        self.save_feedback = SaveFeedback::None;
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_draft_edited result_code=ok \
             config_revision={} slice_count=4",
            draft.original.config_revision
        );
        true
    }

    #[cfg(debug_assertions)]
    fn debug_apply_binding_fixture_edit(&mut self) -> bool {
        let Some(draft) = &mut self.draft else {
            return false;
        };
        let binding = ActivationBinding {
            button: ActivationBindingId::MouseButton5,
            modifiers: PhysicalModifiers { control: true, alt: false, shift: false },
        };
        draft.proposed.activation_binding = binding;
        self.section = SettingsSection::Binding;
        self.binding_test = BindingTestState::Waiting { expected: binding };
        input::begin_activation_binding_test(binding);
        self.save_feedback = SaveFeedback::None;
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_binding_draft_edited result_code=ok \
             config_revision={} activation_button={} activation_control=true \
             activation_alt=false activation_shift=false tester_armed=true",
            draft.original.config_revision,
            binding.button.short_label()
        );
        true
    }

    #[cfg(debug_assertions)]
    fn debug_apply_appearance_fixture_edit(&mut self) -> bool {
        let Some(draft) = &mut self.draft else {
            return false;
        };
        draft.proposed.appearance = Appearance {
            accent_color: AccentColor { red: 224, green: 96, blue: 72 },
            label_theme: LabelTheme::Dark,
            size_preset: SizePreset::Large,
        };
        self.section = SettingsSection::Appearance;
        self.save_feedback = SaveFeedback::None;
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_appearance_draft_edited \
             result_code=ok config_revision={} accent_red=224 accent_green=96 \
             accent_blue=72 label_theme=dark size_preset=large",
            draft.original.config_revision
        );
        true
    }

    #[cfg(debug_assertions)]
    #[allow(clippy::too_many_lines)]
    fn debug_apply_action_fixture_edit(&mut self) -> bool {
        let Some(draft) = &mut self.draft else {
            return false;
        };
        if !draft.proposed.actions.iter().any(|action| action.id == DEBUG_SHORTCUT_ACTION_ID) {
            let _created = create_action(
                &mut draft.proposed,
                DEBUG_SHORTCUT_ACTION_ID,
                "Editor shortcut",
                ActionKind::SendShortcut {
                    modifiers: vec![ShortcutModifier::LeftControl],
                    key: ShortcutKey { scan_code: 0x64, extended: false },
                },
            );
        }
        if !draft.proposed.actions.iter().any(|action| action.id == DEBUG_TEXT_ACTION_ID) {
            let _created = create_action(
                &mut draft.proposed,
                DEBUG_TEXT_ACTION_ID,
                "Editor text",
                ActionKind::TypeText { text: "Gradial editor text \u{1F642}".to_owned() },
            );
        }
        for (action_id, name, kind) in [
            (
                DEBUG_URL_ACTION_ID,
                "Editor website",
                ActionKind::OpenUrl { url: "https://example.com/gradial-editor".to_owned() },
            ),
            (
                DEBUG_PATH_ACTION_ID,
                "Editor folder",
                ActionKind::OpenPath { path: PathBuf::from(r"C:\Windows") },
            ),
            (
                DEBUG_EXECUTABLE_ACTION_ID,
                "Editor application",
                ActionKind::RunExecutable {
                    executable: PathBuf::from(r"C:\Windows\System32\notepad.exe"),
                    arguments: vec![
                        "--editor-smoke".to_owned(),
                        "literal argument with spaces".to_owned(),
                        String::new(),
                    ],
                    working_directory: Some(PathBuf::from(r"C:\Windows")),
                },
            ),
            (DEBUG_DELAY_ACTION_ID, "Editor delay", ActionKind::Delay { milliseconds: 275 }),
            (
                DEBUG_WINDOW_ACTION_ID,
                "Editor window",
                ActionKind::Window { command: WindowCommand::Restore },
            ),
        ] {
            if !draft.proposed.actions.iter().any(|action| action.id == action_id) {
                let _created = create_action(&mut draft.proposed, action_id, name, kind);
            }
        }
        if !draft.proposed.actions.iter().any(|action| action.id == DEBUG_SEQUENCE_ACTION_ID) {
            let _created = create_action(
                &mut draft.proposed,
                DEBUG_SEQUENCE_ACTION_ID,
                "Editor sequence",
                ActionKind::Sequence {
                    actions: vec![
                        DEBUG_SHORTCUT_ACTION_ID,
                        DEBUG_TEXT_ACTION_ID,
                        DEBUG_DELAY_ACTION_ID,
                    ],
                },
            );
        }
        if !draft.proposed.actions.iter().any(|action| action.id == DEBUG_NESTED_SEQUENCE_ACTION_ID)
        {
            let _created = create_action(
                &mut draft.proposed,
                DEBUG_NESTED_SEQUENCE_ACTION_ID,
                "Editor nested sequence",
                ActionKind::Sequence {
                    actions: vec![DEBUG_SEQUENCE_ACTION_ID, DEBUG_WINDOW_ACTION_ID],
                },
            );
            if let Some(ActionDefinition { kind: ActionKind::Sequence { actions }, .. }) = draft
                .proposed
                .actions
                .iter_mut()
                .find(|action| action.id == DEBUG_NESTED_SEQUENCE_ACTION_ID)
            {
                let _added = add_sequence_step(actions, DEBUG_DELAY_ACTION_ID);
                let _removed = remove_sequence_step(actions, 2);
                let _moved = move_sequence_step(actions, 0, SequenceMove::Down);
            }
        }
        let _duplicated =
            duplicate_action(&mut draft.proposed, DEBUG_TEXT_ACTION_ID, DEBUG_DUPLICATE_ACTION_ID);
        let _deleted = delete_action(&mut draft.proposed, DEBUG_DUPLICATE_ACTION_ID);
        if let Some(north) = draft.proposed.global_profile.menu.slices.first_mut() {
            north.action_id = Some(DEBUG_SHORTCUT_ACTION_ID);
        }
        self.section = SettingsSection::Actions;
        self.selected_action = Some(DEBUG_SHORTCUT_ACTION_ID);
        self.action_feedback = None;
        self.save_feedback = SaveFeedback::None;
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_action_draft_edited result_code=ok \
             config_revision={} action_count={}",
            draft.original.config_revision,
            draft.proposed.actions.len()
        );
        true
    }

    #[cfg(debug_assertions)]
    fn debug_apply_profile_fixture_edit(&mut self) -> bool {
        let Some(draft) = &mut self.draft else {
            return false;
        };
        if !draft
            .proposed
            .application_profiles
            .iter()
            .any(|application| application.profile.id == DEBUG_APPLICATION_PROFILE_ID)
        {
            let _created = create_application_profile(
                &mut draft.proposed,
                DEBUG_APPLICATION_PROFILE_ID,
                PathBuf::from(r"C:\Windows\System32\notepad.exe"),
            );
            if let Some(application) =
                application_profile_mut(&mut draft.proposed, DEBUG_APPLICATION_PROFILE_ID)
            {
                "Editor Notepad".clone_into(&mut application.profile.name);
                application.rule.priority = 72;
                application.rule.window_class = Some("Notepad".to_owned());
                application.rule.title_regex = Some("^Profile test title".to_owned());
            }
            draft.set_profile_slice_count(DEBUG_APPLICATION_PROFILE_ID, SliceCount::Four);
            if let Some(application) =
                application_profile_mut(&mut draft.proposed, DEBUG_APPLICATION_PROFILE_ID)
            {
                "Profile east".clone_into(&mut application.profile.menu.slices[1].label);
                application.profile.menu.slices[1].action_id =
                    Some(DEBUG_NESTED_SEQUENCE_ACTION_ID);
            }
            draft.set_profile_slice_count(DEBUG_APPLICATION_PROFILE_ID, SliceCount::Eight);
        }
        let _duplicated = duplicate_application_profile(
            &mut draft.proposed,
            DEBUG_APPLICATION_PROFILE_ID,
            DEBUG_DUPLICATE_PROFILE_ID,
        );
        let _deleted = delete_application_profile(&mut draft.proposed, DEBUG_DUPLICATE_PROFILE_ID);
        self.section = SettingsSection::Profiles;
        self.selected_profile = Some(DEBUG_APPLICATION_PROFILE_ID);
        self.selected_slice = 2;
        self.profile_feedback = None;
        self.save_feedback = SaveFeedback::None;
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_profile_draft_edited result_code=ok \
             config_revision={} profile_count={} profile_id={DEBUG_APPLICATION_PROFILE_ID} \
             priority=72 slice_count=8",
            draft.original.config_revision,
            draft.proposed.application_profiles.len()
        );
        true
    }

    #[cfg(debug_assertions)]
    fn debug_show_profile_test(&mut self) -> bool {
        if !self.draft.as_ref().is_some_and(|draft| {
            draft
                .proposed
                .application_profiles
                .iter()
                .any(|application| application.profile.id == DEBUG_APPLICATION_PROFILE_ID)
        }) {
            return false;
        }
        self.section = SettingsSection::Profiles;
        self.selected_profile = Some(DEBUG_APPLICATION_PROFILE_ID);
        self.profile_test = ProfileTestState::Captured(ProfileTestTarget {
            executable_path: Some(PathBuf::from(r"C:\Windows\System32\notepad.exe")),
            window_class: "Notepad".to_owned(),
            window_title: DEBUG_PROFILE_TEST_TITLE.to_owned(),
        });
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_profile_test_shown result_code=ok \
             config_revision={} executable_path_available=true window_class_available=true \
             window_title_available=true",
            self.draft.as_ref().map_or(0, |draft| draft.original.config_revision)
        );
        true
    }

    #[cfg(debug_assertions)]
    fn debug_begin_profile_test_capture(&mut self) -> bool {
        if self.lifecycle != EditorLifecycle::Editing {
            return false;
        }
        self.section = SettingsSection::Profiles;
        self.profile_test =
            ProfileTestState::CountingDown { deadline: Instant::now() + PROFILE_TEST_COUNTDOWN };
        true
    }

    #[cfg(debug_assertions)]
    fn debug_begin_shortcut_recording(&mut self) -> bool {
        let Some(action_id) = self.selected_action else {
            return false;
        };
        if !self.draft.as_ref().is_some_and(|draft| {
            draft.proposed.actions.iter().any(|action| {
                action.id == action_id && matches!(action.kind, ActionKind::SendShortcut { .. })
            })
        }) {
            return false;
        }
        self.recording_action_id = Some(action_id);
        self.shortcut_recorder.begin();
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_shortcut_recording_started \
             result_code=ok config_revision={} action_id={action_id}",
            self.draft.as_ref().map_or(0, |draft| draft.original.config_revision)
        );
        true
    }

    #[cfg(debug_assertions)]
    fn debug_select_leaf_action(&mut self) -> bool {
        if !self.draft.as_ref().is_some_and(|draft| {
            draft.proposed.actions.iter().any(|action| action.id == DEBUG_EXECUTABLE_ACTION_ID)
        }) {
            return false;
        }
        self.section = SettingsSection::Actions;
        self.selected_action = Some(DEBUG_EXECUTABLE_ACTION_ID);
        self.action_feedback = None;
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_leaf_editor_selected result_code=ok \
             config_revision={} action_id={DEBUG_EXECUTABLE_ACTION_ID}",
            self.draft.as_ref().map_or(0, |draft| draft.original.config_revision)
        );
        true
    }

    #[cfg(debug_assertions)]
    fn debug_open_native_application_picker(&mut self) {
        let result_code = match choose_path(PathDialogKind::Executable) {
            PathDialogOutcome::Selected(_) => "selected",
            PathDialogOutcome::Cancelled => "cancelled",
            PathDialogOutcome::Failed => "failed",
        };
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_native_picker result_code={result_code} \
             config_revision={} picker_kind=application",
            self.draft.as_ref().map_or(0, |draft| draft.original.config_revision)
        );
    }

    #[cfg(debug_assertions)]
    fn debug_open_native_configuration_picker(&mut self, kind: PathDialogKind) {
        let result_code = match choose_path(kind) {
            PathDialogOutcome::Selected(_) => "selected",
            PathDialogOutcome::Cancelled => "cancelled",
            PathDialogOutcome::Failed => "failed",
        };
        let picker_kind = match kind {
            PathDialogKind::ConfigurationImport => "config_import",
            PathDialogKind::ConfigurationExport => "config_export",
            PathDialogKind::DiagnosticBundleExport => "diagnostic_export",
            _ => "unsupported",
        };
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_native_picker result_code={result_code} \
             config_revision={} picker_kind={picker_kind}",
            self.draft.as_ref().map_or(0, |draft| draft.original.config_revision)
        );
    }

    #[cfg(debug_assertions)]
    fn debug_import_event(&self) -> Option<SettingsEvent> {
        let path = AppPaths::discover().ok()?.root.join("settings-import-smoke.json");
        (self.lifecycle == EditorLifecycle::Editing)
            .then(|| SettingsEvent::Import(PrivatePath::new(path)))
    }

    #[cfg(debug_assertions)]
    fn debug_export_event(&self) -> Option<SettingsEvent> {
        let draft = self.draft.as_ref()?;
        let path =
            AppPaths::discover().ok()?.root.join("settings-export-smoke").join("settings.json");
        (self.lifecycle == EditorLifecycle::Editing && validate_draft(&draft.proposed).is_ok())
            .then(|| SettingsEvent::Export {
                path: PrivatePath::new(path),
                configuration: draft.proposed.clone(),
            })
    }

    #[cfg(debug_assertions)]
    fn debug_refresh_diagnostics_event(&mut self) -> Option<SettingsEvent> {
        self.section = SettingsSection::Diagnostics;
        self.diagnostic_snapshot = None;
        self.diagnostic_include_process_paths = false;
        self.diagnostic_feedback = None;
        (self.lifecycle == EditorLifecycle::Editing).then_some(SettingsEvent::RefreshDiagnostics)
    }

    #[cfg(debug_assertions)]
    fn debug_export_diagnostics_event(
        &mut self,
        include_process_paths: bool,
    ) -> Option<SettingsEvent> {
        let snapshot = self.diagnostic_snapshot.clone()?;
        let root = AppPaths::discover().ok()?.root;
        let parent = root.parent()?;
        let name = if include_process_paths {
            "gradial-diagnostics-paths-smoke.zip"
        } else {
            "gradial-diagnostics-default-smoke.zip"
        };
        self.section = SettingsSection::Diagnostics;
        self.diagnostic_include_process_paths = include_process_paths;
        Some(SettingsEvent::ExportDiagnostics {
            path: PrivatePath::new(parent.join(name)),
            request: DiagnosticBundleRequest { snapshot, include_process_paths },
        })
    }

    #[cfg(debug_assertions)]
    fn debug_icon_import_event(&mut self) -> Option<SettingsEvent> {
        let draft = self.draft.as_ref()?;
        let profile_id = draft.proposed.global_profile.id;
        let config_revision = draft.original.config_revision;
        let path = AppPaths::discover().ok()?.root.join("settings-icon-smoke.png");
        self.section = SettingsSection::Menu;
        self.selected_slice = 0;
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_icon_import_requested \
             result_code=ok config_revision={config_revision} profile_id={profile_id} slice_index=0"
        );
        Some(SettingsEvent::ImportIcon { path: PrivatePath::new(path), profile_id, slice_index: 0 })
    }

    #[cfg(debug_assertions)]
    fn debug_select_sequence_action(&mut self) -> bool {
        if !self.draft.as_ref().is_some_and(|draft| {
            draft.proposed.actions.iter().any(|action| action.id == DEBUG_NESTED_SEQUENCE_ACTION_ID)
        }) {
            return false;
        }
        self.section = SettingsSection::Actions;
        self.selected_action = Some(DEBUG_NESTED_SEQUENCE_ACTION_ID);
        self.action_feedback = None;
        log::info!(
            target: "settings",
            "component=settings event_code=settings_debug_sequence_editor_selected result_code=ok \
             config_revision={} action_id={DEBUG_NESTED_SEQUENCE_ACTION_ID}",
            self.draft.as_ref().map_or(0, |draft| draft.original.config_revision)
        );
        true
    }

    #[cfg(debug_assertions)]
    fn debug_keep_editing(&mut self) -> bool {
        if self.lifecycle != EditorLifecycle::ConfirmingClose {
            return false;
        }
        self.lifecycle = EditorLifecycle::Editing;
        log::info!(
            target: "settings",
            "component=settings event_code=settings_close_cancelled result_code=ok \
             config_revision={}",
            self.draft.as_ref().map_or(0, |draft| draft.original.config_revision)
        );
        true
    }

    #[cfg(debug_assertions)]
    fn debug_save_event(&self) -> Option<SettingsEvent> {
        let draft = self.draft.as_ref()?;
        (self.lifecycle == EditorLifecycle::Editing
            && draft.is_dirty()
            && validate_draft(&draft.proposed).is_ok())
        .then(|| SettingsEvent::Save(draft.proposed.clone()))
    }

    fn cancel_binding_test(&mut self) {
        input::cancel_activation_binding_test();
        self.binding_test = BindingTestState::Idle;
    }

    fn poll_binding_test(&mut self, context: &egui::Context) {
        if !matches!(self.binding_test, BindingTestState::Waiting { .. }) {
            return;
        }
        if let Some(result) = input::take_activation_binding_test_result() {
            let selection = binding_test_selection(result);
            log::info!(
                target: "settings",
                "component=settings event_code=settings_binding_test_completed result_code={} \
                 config_revision={} matched={} activation_button={} activation_control={} \
                 activation_alt={} activation_shift={} selection={}",
                if result.matched { "ok" } else { "mismatch" },
                self.draft.as_ref().map_or(0, |draft| draft.original.config_revision),
                result.matched,
                result.actual.button.short_label(),
                result.actual.modifiers.control,
                result.actual.modifiers.alt,
                result.actual.modifiers.shift,
                selection.map_or("none", |index| direction_name(index, SliceCount::Eight))
            );
            self.binding_test = BindingTestState::Complete(result);
            context.request_repaint();
        } else {
            context.request_repaint_after(Duration::from_millis(50));
        }
    }

    #[allow(clippy::too_many_lines)]
    fn render(&mut self, ui: &mut egui::Ui) -> Option<SettingsEvent> {
        #[cfg(debug_assertions)]
        let debug_event = if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F1))
        {
            let _edited = self.debug_apply_binding_fixture_edit();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F5))
        {
            let _edited = self.debug_apply_fixture_edit();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F17))
        {
            let _edited = self.debug_apply_appearance_fixture_edit();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F18))
        {
            self.debug_icon_import_event()
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F9))
        {
            let _edited = self.debug_apply_action_fixture_edit();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F4))
        {
            let _edited = self.debug_apply_profile_fixture_edit();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F3))
        {
            let _shown = self.debug_show_profile_test();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F2))
        {
            let _started = self.debug_begin_profile_test_capture();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F10))
        {
            let _recording = self.debug_begin_shortcut_recording();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F6))
        {
            let _selected = self.debug_select_leaf_action();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F11))
        {
            self.debug_open_native_application_picker();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F12))
        {
            let _selected = self.debug_select_sequence_action();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F7))
        {
            let _kept = self.debug_keep_editing();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F8))
        {
            self.debug_save_event()
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F13))
        {
            self.debug_import_event()
        } else if !self.shortcut_recorder.is_recording()
            && ui.ctx().input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F14))
        {
            self.debug_export_event()
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F15))
        {
            self.debug_open_native_configuration_picker(PathDialogKind::ConfigurationImport);
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F16))
        {
            self.debug_open_native_configuration_picker(PathDialogKind::ConfigurationExport);
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F19))
        {
            self.debug_refresh_diagnostics_event()
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F20))
        {
            self.debug_export_diagnostics_event(false)
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F21))
        {
            self.debug_export_diagnostics_event(true)
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F22))
        {
            self.debug_open_native_configuration_picker(PathDialogKind::DiagnosticBundleExport);
            None
        } else {
            None
        };
        #[cfg(not(debug_assertions))]
        let debug_event = None;

        self.poll_binding_test(ui.ctx());
        let close_requested = ui.ctx().input(|input| input.viewport().close_requested());
        if close_requested {
            self.pending_import_path = None;
            self.pending_icon_import = None;
            self.cancel_binding_test();
            self.shortcut_recorder.cancel();
            self.recording_action_id = None;
            match self.request_close() {
                CloseOutcome::Closed => return Some(SettingsEvent::Closed),
                CloseOutcome::ConfirmationShown => {
                    ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
                }
            }
        }

        let Some(mut draft) = self.draft.take() else {
            self.lifecycle = EditorLifecycle::Closed;
            return None;
        };
        let mut event = debug_event;
        let validation = validate_draft(&draft.proposed);
        let dirty = draft.is_dirty();

        egui::Panel::top("settings_header").show(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.heading("Gradial settings");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(if dirty { "Unsaved changes" } else { "All changes saved" });
                });
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.section, SettingsSection::Binding, "Activation");
                ui.selectable_value(&mut self.section, SettingsSection::Appearance, "Appearance");
                ui.selectable_value(&mut self.section, SettingsSection::Menu, "Menu");
                ui.selectable_value(&mut self.section, SettingsSection::Profiles, "Profiles");
                ui.selectable_value(&mut self.section, SettingsSection::Actions, "Actions");
                ui.selectable_value(
                    &mut self.section,
                    SettingsSection::Diagnostics,
                    "Diagnostics",
                );
            });
            if self.section != SettingsSection::Diagnostics {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Import settings\u{2026}").clicked() {
                        self.transfer_feedback = None;
                        match choose_path(PathDialogKind::ConfigurationImport) {
                            PathDialogOutcome::Selected(path) if dirty => {
                                self.pending_import_path = Some(PrivatePath::new(path));
                            }
                            PathDialogOutcome::Selected(path) => {
                                event = Some(SettingsEvent::Import(PrivatePath::new(path)));
                            }
                            PathDialogOutcome::Cancelled => {}
                            PathDialogOutcome::Failed => {
                                self.transfer_feedback = Some(TransferFeedback::Error(
                                    "Gradial could not open the import picker.".to_owned(),
                                ));
                            }
                        }
                    }
                    let export = ui.add_enabled(
                        validation.is_ok(),
                        egui::Button::new("Export current draft\u{2026}"),
                    );
                    if export.clicked() {
                        self.transfer_feedback = None;
                        match choose_path(PathDialogKind::ConfigurationExport) {
                            PathDialogOutcome::Selected(path) => {
                                event = Some(SettingsEvent::Export {
                                    path: PrivatePath::new(path),
                                    configuration: draft.proposed.clone(),
                                });
                            }
                            PathDialogOutcome::Cancelled => {}
                            PathDialogOutcome::Failed => {
                                self.transfer_feedback = Some(TransferFeedback::Error(
                                    "Gradial could not open the export picker.".to_owned(),
                                ));
                            }
                        }
                    }
                    ui.weak("Import replaces only this draft. Export includes unsaved edits.");
                });
                if let Some(feedback) = &self.transfer_feedback {
                    match feedback {
                        TransferFeedback::Info(message) => {
                            ui.colored_label(Color32::from_rgb(112, 205, 151), message);
                        }
                        TransferFeedback::Error(message) => {
                            ui.colored_label(Color32::from_rgb(239, 113, 113), message);
                        }
                    }
                }
                if self.recovery_warning {
                    ui.add_space(6.0);
                    ui.colored_label(
                        Color32::from_rgb(245, 188, 78),
                        "Gradial recovered a previous configuration. Saving will replace the broken file.",
                    );
                }
            }
            ui.add_space(8.0);
        });
        if self.section != SettingsSection::Binding
            && !matches!(self.binding_test, BindingTestState::Idle)
        {
            self.cancel_binding_test();
        }

        if self.section != SettingsSection::Diagnostics {
            egui::Panel::bottom("settings_actions").show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let save_enabled = dirty && validation.is_ok();
                    if ui.add_enabled(save_enabled, egui::Button::new("Save changes")).clicked() {
                        event = Some(SettingsEvent::Save(draft.proposed.clone()));
                        self.save_feedback = SaveFeedback::None;
                    }
                    if ui.add_enabled(dirty, egui::Button::new("Discard edits")).clicked() {
                        draft.discard();
                        self.selected_slice = self
                            .selected_slice
                            .min(draft.proposed.global_profile.menu.slices.len() - 1);
                        self.selected_profile = draft
                            .proposed
                            .application_profiles
                            .first()
                            .map(|application| application.profile.id);
                        self.selected_action =
                            draft.proposed.actions.first().map(|action| action.id);
                        self.profile_feedback = None;
                        self.action_feedback = None;
                        self.cancel_binding_test();
                        self.shortcut_recorder.cancel();
                        self.recording_action_id = None;
                        self.save_feedback = SaveFeedback::None;
                        self.transfer_feedback = None;
                        self.icon_feedback = None;
                        self.pending_icon_import = None;
                        self.pending_import_path = None;
                    }
                    if let Err(error) = &validation {
                        ui.colored_label(Color32::from_rgb(239, 113, 113), error.message());
                    } else {
                        match &self.save_feedback {
                            SaveFeedback::None => {}
                            SaveFeedback::Saved => {
                                ui.colored_label(
                                    Color32::from_rgb(112, 205, 151),
                                    "Changes saved.",
                                );
                            }
                            SaveFeedback::Error(error) => {
                                ui.colored_label(Color32::from_rgb(239, 113, 113), error);
                            }
                        }
                    }
                });
                ui.add_space(8.0);
            });
        }

        match self.section {
            SettingsSection::Binding => self.render_binding_section(ui, &mut draft),
            SettingsSection::Appearance => self.render_appearance_section(ui, &mut draft),
            SettingsSection::Menu => self.render_menu_section(ui, &mut draft),
            SettingsSection::Profiles => self.render_profiles_section(ui, &mut draft),
            SettingsSection::Actions => self.render_actions_section(ui, &mut draft),
            SettingsSection::Diagnostics => {
                let diagnostic_event = self.render_diagnostics_section(ui);
                if event.is_none() {
                    event = diagnostic_event;
                }
            }
        }
        if event.is_none()
            && let Some(pending) = self.pending_icon_import.take()
        {
            event = Some(SettingsEvent::ImportIcon {
                path: pending.path,
                profile_id: pending.profile_id,
                slice_index: pending.slice_index,
            });
        }

        if self.pending_import_path.is_some() {
            let mut replace_draft = false;
            let mut keep_draft = false;
            let frame = egui::Frame::popup(ui.style()).fill(Color32::from_rgb(28, 28, 28));
            Modal::new(Id::new("replace_settings_draft_confirmation"))
                .frame(frame)
                .backdrop_color(Color32::from_black_alpha(150))
                .show(ui.ctx(), |ui| {
                    ui.set_min_width(360.0);
                    ui.heading("Replace the current draft?");
                    ui.label(
                        "Importing will replace your unsaved edits in this settings window. \
                         Gradial will not apply anything until you save.",
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Import file").clicked() {
                            replace_draft = true;
                        }
                        if ui.button("Keep current draft").clicked() {
                            keep_draft = true;
                        }
                    });
                });
            if replace_draft {
                if let Some(path) = self.pending_import_path.take() {
                    event = Some(SettingsEvent::Import(path));
                }
            } else if keep_draft {
                self.pending_import_path = None;
            }
        }

        if self.lifecycle == EditorLifecycle::ConfirmingClose {
            let mut discard = false;
            let mut keep_editing = false;
            let frame = egui::Frame::popup(ui.style()).fill(Color32::from_rgb(28, 28, 28));
            Modal::new(Id::new("discard_settings_confirmation"))
                .frame(frame)
                .backdrop_color(Color32::from_black_alpha(150))
                .show(ui.ctx(), |ui| {
                    ui.set_min_width(340.0);
                    ui.heading("Discard unsaved changes?");
                    ui.label("Your settings edits have not been saved.");
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Discard changes").clicked() {
                            discard = true;
                        }
                        if ui.button("Keep editing").clicked() {
                            keep_editing = true;
                        }
                    });
                });
            if discard {
                draft.discard();
                self.profile_test = ProfileTestState::Idle;
                self.cancel_binding_test();
                self.clear_diagnostic_preview();
                self.lifecycle = EditorLifecycle::Closed;
                event = Some(SettingsEvent::Closed);
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                log::info!(
                    target: "settings",
                    "component=settings event_code=settings_closed result_code=ok \
                     config_revision={} unsaved_changes_discarded=true",
                    draft.original.config_revision
                );
            } else if keep_editing {
                self.lifecycle = EditorLifecycle::Editing;
                log::info!(
                    target: "settings",
                    "component=settings event_code=settings_close_cancelled result_code=ok \
                     config_revision={}",
                    draft.original.config_revision
                );
            }
        }

        self.draft = Some(draft);
        event
    }

    #[allow(clippy::too_many_lines)]
    fn render_diagnostics_section(&mut self, ui: &mut egui::Ui) -> Option<SettingsEvent> {
        let mut event = None;
        let snapshot = self.diagnostic_snapshot.clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Diagnostic bundle");
                ui.label(
                    "Create a local ZIP you can inspect before sharing. Gradial never uploads it.",
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let label =
                        if snapshot.is_some() { "Refresh preview" } else { "Prepare preview" };
                    if ui.button(label).clicked() {
                        self.diagnostic_feedback = None;
                        event = Some(SettingsEvent::RefreshDiagnostics);
                    }
                    if let Some(snapshot) = snapshot.as_deref() {
                        ui.weak(format!(
                            "Frozen preview · configuration revision {}",
                            snapshot.config_revision(),
                        ));
                    }
                });

                let Some(snapshot) = snapshot.as_deref() else {
                    ui.add_space(12.0);
                    ui.label(
                        "Prepare a preview to see every archive entry and choose whether configured application paths are included.",
                    );
                    render_diagnostic_feedback(ui, self.diagnostic_feedback.as_ref());
                    return;
                };

                ui.add_space(12.0);
                ui.group(|ui| {
                    ui.heading("Privacy choice");
                    ui.checkbox(
                        &mut self.diagnostic_include_process_paths,
                        "Include configured application paths",
                    );
                    if self.diagnostic_include_process_paths {
                        ui.colored_label(
                            Color32::from_rgb(245, 188, 78),
                            "Explicitly included: the following paths will appear in configured-process-paths.json.",
                        );
                        if snapshot.process_paths().is_empty() {
                            ui.weak("No configured application paths are present.");
                        } else {
                            for path in snapshot.process_paths() {
                                ui.monospace(path.as_os_str().to_string_lossy());
                            }
                        }
                    } else {
                        ui.label(format!(
                            "Excluded by default: {} configured application {}.",
                            snapshot.process_paths().len(),
                            if snapshot.process_paths().len() == 1 { "path" } else { "paths" }
                        ));
                    }
                    ui.weak(
                        "Arguments, working folders, typed text, URLs, file/folder paths, window titles, clipboard data, settings, and icons are always excluded.",
                    );
                });

                ui.add_space(12.0);
                ui.heading("Archive preview");
                ui.label(format!(
                    "{} entries · about {}",
                    snapshot.archive_entry_count(self.diagnostic_include_process_paths),
                    display_byte_count(
                        snapshot.estimated_size_bytes(self.diagnostic_include_process_paths)
                    )
                ));
                ui.monospace("manifest.json");
                for log in snapshot.logs() {
                    ui.monospace(format!(
                        "logs/{} ({})",
                        log.name,
                        display_byte_count(log.size_bytes)
                    ));
                }
                if self.diagnostic_include_process_paths {
                    ui.monospace("configured-process-paths.json");
                }

                ui.add_space(12.0);
                ui.heading("Included status");
                ui.label(snapshot.input_status().user_text());
                let windows = snapshot.windows_version().map_or_else(
                    || format!("Windows {}", std::env::consts::ARCH),
                    |version| {
                        format!(
                            "Windows {}.{} build {} · {}",
                            version.major,
                            version.minor,
                            version.build,
                            std::env::consts::ARCH
                        )
                    },
                );
                ui.label(format!(
                    "Gradial {} · {} · configuration revision {}",
                    env!("CARGO_PKG_VERSION"),
                    windows,
                    snapshot.config_revision()
                ));
                ui.label(format!(
                    "{} display {}:",
                    snapshot.displays().len(),
                    if snapshot.displays().len() == 1 { "record" } else { "records" }
                ));
                for (index, display) in snapshot.displays().iter().enumerate() {
                    ui.monospace(format!(
                        "{}. bounds ({}, {})–({}, {}), work area ({}, {})–({}, {}), {} DPI",
                        index + 1,
                        display.left,
                        display.top,
                        display.right,
                        display.bottom,
                        display.work_left,
                        display.work_top,
                        display.work_right,
                        display.work_bottom,
                        display.dpi
                    ));
                }
                ui.weak(
                    "“Hook thread responsive” reports the thread heartbeat; it does not claim Windows has retained the hook.",
                );

                ui.add_space(12.0);
                if ui.button("Export this preview\u{2026}").clicked() {
                    self.diagnostic_feedback = None;
                    match choose_path(PathDialogKind::DiagnosticBundleExport) {
                        PathDialogOutcome::Selected(path) => {
                            event = Some(SettingsEvent::ExportDiagnostics {
                                path: PrivatePath::new(path),
                                request: DiagnosticBundleRequest {
                                    snapshot: Arc::clone(
                                        self.diagnostic_snapshot
                                            .as_ref()
                                            .expect("preview is present"),
                                    ),
                                    include_process_paths: self
                                        .diagnostic_include_process_paths,
                                },
                            });
                        }
                        PathDialogOutcome::Cancelled => {}
                        PathDialogOutcome::Failed => {
                            self.diagnostic_feedback = Some(DiagnosticFeedback::Error(
                                "Gradial could not open the diagnostic export picker.".to_owned(),
                            ));
                        }
                    }
                }
                ui.weak("Export uses exactly this frozen preview.");
                render_diagnostic_feedback(ui, self.diagnostic_feedback.as_ref());
            });
        });
        event
    }

    #[allow(clippy::too_many_lines)]
    fn render_binding_section(&mut self, ui: &mut egui::Ui, draft: &mut SettingsDraft) {
        let original_binding = draft.proposed.activation_binding;
        let mut binding = original_binding;
        let mut start_test = false;
        let mut cancel_test = false;

        egui::CentralPanel::default().show(ui, |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(18.0);
                ui.heading("Activation");
                ui.label("Choose the mouse chord that opens Gradial.");
                ui.add_space(18.0);

                ui.label(RichText::new("Side button").strong());
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut binding.button,
                        ActivationBindingId::MouseButton4,
                        "Mouse button 4 (M4)",
                    );
                    ui.selectable_value(
                        &mut binding.button,
                        ActivationBindingId::MouseButton5,
                        "Mouse button 5 (M5)",
                    );
                });
                ui.weak("Only M4 and M5 can activate Gradial.");

                ui.add_space(18.0);
                ui.label(RichText::new("Required keyboard keys").strong());
                ui.horizontal(|ui| {
                    ui.checkbox(&mut binding.modifiers.control, "Ctrl");
                    ui.checkbox(&mut binding.modifiers.alt, "Alt");
                    ui.checkbox(&mut binding.modifiers.shift, "Shift");
                });
                ui.weak(
                    "The chord is exact. Extra Ctrl, Alt, or Shift keys will not activate Gradial.",
                );

                ui.add_space(18.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(520.0);
                    ui.label(RichText::new("Test this draft safely").strong());
                    ui.label(format!(
                        "Test chord: {}",
                        activation_binding_display(binding)
                    ));
                    ui.weak(
                        "The next side-button attempt is consumed once. It cannot launch an action or navigate another app.",
                    );
                    ui.add_space(10.0);

                    match self.binding_test {
                        BindingTestState::Idle => {
                            if ui.button("Start hold–flick–release test").clicked() {
                                start_test = true;
                            }
                        }
                        BindingTestState::Waiting { expected } => {
                            ui.colored_label(
                                Color32::from_rgb(124, 154, 255),
                                if input::activation_test_gesture_active() {
                                    "Chord recognized. Flick in any direction, then release."
                                        .to_owned()
                                } else {
                                    format!(
                                        "Waiting for {}. Hold it, flick, then release.",
                                        activation_binding_display(expected)
                                    )
                                },
                            );
                            if ui.button("Cancel test").clicked() {
                                cancel_test = true;
                            }
                        }
                        BindingTestState::Complete(result) if result.matched => {
                            let message = binding_test_selection(result).map_or_else(
                                || {
                                    "Chord recognized. The release stayed in the center, so the gesture cancelled safely."
                                        .to_owned()
                                },
                                |selection| {
                                    format!(
                                        "Chord recognized with a {} flick. No action was launched.",
                                        direction_name(selection, SliceCount::Eight)
                                    )
                                },
                            );
                            ui.colored_label(Color32::from_rgb(112, 205, 151), message);
                            if ui.button("Test again").clicked() {
                                start_test = true;
                            }
                        }
                        BindingTestState::Complete(result) => {
                            ui.colored_label(
                                Color32::from_rgb(239, 113, 113),
                                format!(
                                    "That was {}. Try {}.",
                                    activation_binding_display(result.actual),
                                    activation_binding_display(binding)
                                ),
                            );
                            if ui.button("Try again").clicked() {
                                start_test = true;
                            }
                        }
                    }
                });

                ui.add_space(18.0);
                ui.label(RichText::new("When it takes effect").strong());
                ui.label(
                    "This is still a draft. Gradial keeps using the saved activation chord until you choose Save changes.",
                );
                ui.add_space(8.0);
                ui.weak(match binding.modifiers {
                    PhysicalModifiers { control: false, alt: false, shift: false } => {
                        "Without modifiers, the chosen side button is dedicated to Gradial while input is healthy."
                    }
                    _ => {
                        "With required modifiers, only the complete chord is dedicated; the side button alone continues to work normally."
                    }
                });
            });
        });

        if binding != original_binding {
            draft.proposed.activation_binding = binding;
            self.cancel_binding_test();
            self.save_feedback = SaveFeedback::None;
        }
        if cancel_test {
            self.cancel_binding_test();
        } else if start_test {
            input::begin_activation_binding_test(binding);
            self.binding_test = BindingTestState::Waiting { expected: binding };
            self.save_feedback = SaveFeedback::None;
        }
    }

    fn render_appearance_section(&mut self, ui: &mut egui::Ui, draft: &mut SettingsDraft) {
        let original = draft.proposed.appearance;
        let mut appearance = original;

        egui::Panel::left("appearance_editor").resizable(false).exact_size(350.0).show(ui, |ui| {
            ui.add_space(12.0);
            ui.heading("Appearance");
            ui.label("Customize the radial menu without changing how flicks are recognized.");

            ui.add_space(18.0);
            ui.label(RichText::new("Accent color").strong());
            let mut accent = appearance.accent_color.components();
            ui.horizontal(|ui| {
                if ui.color_edit_button_srgb(&mut accent).changed() {
                    appearance.accent_color =
                        AccentColor { red: accent[0], green: accent[1], blue: accent[2] };
                }
                ui.monospace(format!("#{:02X}{:02X}{:02X}", accent[0], accent[1], accent[2]));
            });
            ui.weak("Used for outlines, dividers, and the selected direction.");

            ui.add_space(18.0);
            ui.label(RichText::new("Labels").strong());
            ui.horizontal(|ui| {
                ui.selectable_value(&mut appearance.label_theme, LabelTheme::Light, "Light labels");
                ui.selectable_value(&mut appearance.label_theme, LabelTheme::Dark, "Dark labels");
            });
            ui.weak(match appearance.label_theme {
                LabelTheme::Light => "Light labels use the original dark ring.",
                LabelTheme::Dark => "Dark labels use a light ring for readable contrast.",
            });

            ui.add_space(18.0);
            ui.label(RichText::new("Menu size").strong());
            ui.horizontal(|ui| {
                ui.selectable_value(&mut appearance.size_preset, SizePreset::Compact, "Compact");
                ui.selectable_value(&mut appearance.size_preset, SizePreset::Standard, "Standard");
                ui.selectable_value(&mut appearance.size_preset, SizePreset::Large, "Large");
            });
            ui.weak("Size affects only visuals. Gesture thresholds stay consistent.");

            ui.add_space(20.0);
            if ui.button("Restore original appearance").clicked() {
                appearance = Appearance::default();
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(12.0);
            ui.heading("Live preview");
            ui.label("Appearance applies to Global and every application profile.");
            ui.add_space(18.0);
            ui.vertical_centered(|ui| {
                self.render_preview(
                    ui,
                    &draft.proposed.global_profile.menu,
                    draft.proposed.activation_binding.button,
                    appearance,
                );
            });
        });

        if appearance != original {
            draft.proposed.appearance = appearance;
            self.save_feedback = SaveFeedback::None;
        }
    }

    fn render_menu_section(&mut self, ui: &mut egui::Ui, draft: &mut SettingsDraft) {
        let global_profile_id = draft.proposed.global_profile.id;
        let global_slice_count =
            draft.proposed.global_profile.menu.resolved_slice_count().unwrap_or_default();
        self.selected_slice = self.selected_slice.min(global_slice_count.get().saturating_sub(1));
        egui::Panel::left("menu_editor").resizable(false).exact_size(350.0).show(ui, |ui| {
            ui.add_space(12.0);
            ui.heading("Menu");
            ui.label("Choose the number of directions, then select one to edit.");
            ui.add_space(12.0);

            let current_count =
                draft.proposed.global_profile.menu.resolved_slice_count().unwrap_or_default();
            let mut selected_count = current_count;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut selected_count, SliceCount::Four, "4 directions");
                ui.selectable_value(&mut selected_count, SliceCount::Eight, "8 directions");
            });
            if selected_count != current_count {
                draft.set_slice_count(selected_count);
                self.selected_slice =
                    map_selected_slice(self.selected_slice, current_count, selected_count);
                self.save_feedback = SaveFeedback::None;
            }

            ui.add_space(18.0);
            ui.label(RichText::new("Direction").strong());
            direction_selector(ui, selected_count, &mut self.selected_slice);

            ui.add_space(18.0);
            let direction = direction_name(self.selected_slice, selected_count);
            ui.label(RichText::new(direction).strong().size(18.0));
            let action_options = action_options(&draft.proposed);
            let action_catalog_empty = draft.proposed.actions.is_empty();
            if let Some(slice) =
                draft.proposed.global_profile.menu.slices.get_mut(self.selected_slice)
            {
                ui.label("Label");
                if ui.text_edit_singleline(&mut slice.label).changed() {
                    self.save_feedback = SaveFeedback::None;
                }

                ui.add_space(10.0);
                ui.label("Action");
                let selected_text = action_option_text(slice.action_id, &action_options);
                ComboBox::from_id_salt(("slice_action", self.selected_slice))
                    .width(310.0)
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(&mut slice.action_id, None, "No action").changed() {
                            self.save_feedback = SaveFeedback::None;
                        }
                        for (action_id, label) in &action_options {
                            if ui
                                .selectable_value(&mut slice.action_id, Some(*action_id), label)
                                .changed()
                            {
                                self.save_feedback = SaveFeedback::None;
                            }
                        }
                    });
                if action_catalog_empty {
                    ui.add_space(6.0);
                    if ui.link("Create an action first.").clicked() {
                        self.section = SettingsSection::Actions;
                    }
                }
                ui.add_space(12.0);
                self.render_slice_icon_editor(ui, global_profile_id, self.selected_slice, slice);
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(12.0);
            ui.heading("Live preview");
            ui.label("Click a direction to select it.");
            ui.add_space(18.0);
            ui.vertical_centered(|ui| {
                self.render_preview(
                    ui,
                    &draft.proposed.global_profile.menu,
                    draft.proposed.activation_binding.button,
                    draft.proposed.appearance,
                );
            });
        });
    }

    #[allow(clippy::too_many_lines)]
    fn render_profiles_section(&mut self, ui: &mut egui::Ui, draft: &mut SettingsDraft) {
        if self.selected_profile.is_some_and(|selected| {
            !draft
                .proposed
                .application_profiles
                .iter()
                .any(|application| application.profile.id == selected)
        }) {
            self.selected_profile = draft
                .proposed
                .application_profiles
                .first()
                .map(|application| application.profile.id);
        }
        self.render_profile_catalog(ui, &mut draft.proposed);
        let Some(profile_id) = self.selected_profile else {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.add_space(18.0);
                ui.heading("Application profiles");
                ui.label("Add a profile to use a different menu in one application.");
                ui.add_space(8.0);
                ui.weak("The Global menu remains active whenever no application profile matches.");
                if let Some(feedback) = &self.profile_feedback {
                    ui.add_space(8.0);
                    match feedback {
                        ProfileEditorFeedback::Info(message) => {
                            ui.colored_label(Color32::from_rgb(112, 205, 151), message);
                        }
                        ProfileEditorFeedback::Error(message) => {
                            ui.colored_label(Color32::from_rgb(239, 113, 113), message);
                        }
                    }
                }
            });
            return;
        };

        let profile_slice_count = profile_menu(&draft.proposed, profile_id)
            .and_then(Menu::resolved_slice_count)
            .unwrap_or_default();
        self.selected_slice = self.selected_slice.min(profile_slice_count.get().saturating_sub(1));

        egui::Panel::left("profile_editor").resizable(false).exact_size(340.0).show(ui, |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(12.0);
                ui.heading("Profile");

                let Some(profile_index) = application_profile_index(&draft.proposed, profile_id)
                else {
                    ui.label("Select an available profile.");
                    return;
                };
                {
                    let application = &mut draft.proposed.application_profiles[profile_index];
                    ui.label("Name");
                    if ui.text_edit_singleline(&mut application.profile.name).changed() {
                        self.profile_feedback = None;
                        self.save_feedback = SaveFeedback::None;
                    }
                    ui.add_space(10.0);
                    ui.label("Exact application");
                    ui.label(
                        RichText::new(application.rule.executable_path.to_string_lossy())
                            .monospace(),
                    );
                    if ui.button("Choose application...").clicked() {
                        match choose_path(PathDialogKind::Executable) {
                            PathDialogOutcome::Selected(path) => {
                                application.rule.executable_path = path;
                                self.profile_feedback = Some(ProfileEditorFeedback::Info(
                                    "Application updated.".to_owned(),
                                ));
                                self.save_feedback = SaveFeedback::None;
                            }
                            PathDialogOutcome::Cancelled => {}
                            PathDialogOutcome::Failed => {
                                self.profile_feedback = Some(ProfileEditorFeedback::Error(
                                    "Gradial could not open the application chooser.".to_owned(),
                                ));
                            }
                        }
                    }
                    ui.weak("This profile matches that complete application path.");

                    ui.add_space(10.0);
                    ui.label("Priority");
                    if ui
                        .add(DragValue::new(&mut application.rule.priority).range(0..=100).speed(1))
                        .changed()
                    {
                        self.profile_feedback = None;
                        self.save_feedback = SaveFeedback::None;
                    }
                    ui.weak("Higher priority wins when more than one profile matches.");

                    ui.add_space(10.0);
                    let mut class_enabled = application.rule.window_class.is_some();
                    if ui.checkbox(&mut class_enabled, "Match window class").changed() {
                        application.rule.window_class = class_enabled.then(String::new);
                        self.profile_feedback = None;
                        self.save_feedback = SaveFeedback::None;
                    }
                    if let Some(window_class) = &mut application.rule.window_class {
                        if ui.text_edit_singleline(window_class).changed() {
                            self.profile_feedback = None;
                            self.save_feedback = SaveFeedback::None;
                        }
                        if window_class.trim().is_empty() {
                            ui.colored_label(
                                Color32::from_rgb(239, 113, 113),
                                "Enter a window class or turn off this match.",
                            );
                        } else {
                            ui.weak("Exact match; letter case is ignored.");
                        }
                    }

                    ui.add_space(8.0);
                    let mut title_enabled = application.rule.title_regex.is_some();
                    if ui.checkbox(&mut title_enabled, "Match window title pattern").changed() {
                        application.rule.title_regex = title_enabled.then(String::new);
                        self.profile_feedback = None;
                        self.save_feedback = SaveFeedback::None;
                    }
                    if let Some(pattern) = &mut application.rule.title_regex {
                        if ui.text_edit_singleline(pattern).changed() {
                            self.profile_feedback = None;
                            self.save_feedback = SaveFeedback::None;
                        }
                        if pattern.is_empty() {
                            ui.colored_label(
                                Color32::from_rgb(239, 113, 113),
                                "Enter a title pattern or turn off this match.",
                            );
                        } else if Regex::new(pattern).is_err() {
                            ui.colored_label(
                                Color32::from_rgb(239, 113, 113),
                                "This title pattern is not valid.",
                            );
                        } else {
                            ui.weak("Regular expression; letter case is significant.");
                        }
                    }
                }

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(10.0);
                ui.label(RichText::new("Profile menu").strong());
                let current_count = profile_menu(&draft.proposed, profile_id)
                    .and_then(Menu::resolved_slice_count)
                    .unwrap_or_default();
                let mut selected_count = current_count;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut selected_count, SliceCount::Four, "4 directions");
                    ui.selectable_value(&mut selected_count, SliceCount::Eight, "8 directions");
                });
                if selected_count != current_count {
                    draft.set_profile_slice_count(profile_id, selected_count);
                    self.selected_slice =
                        map_selected_slice(self.selected_slice, current_count, selected_count);
                    self.profile_feedback = None;
                    self.save_feedback = SaveFeedback::None;
                }

                ui.add_space(10.0);
                direction_selector(ui, selected_count, &mut self.selected_slice);
                ui.add_space(10.0);
                ui.label(
                    RichText::new(direction_name(self.selected_slice, selected_count)).strong(),
                );
                let action_options = action_options(&draft.proposed);
                let action_catalog_empty = action_options.is_empty();
                if let Some(menu) = profile_menu_mut(&mut draft.proposed, profile_id)
                    && let Some(slice) = menu.slices.get_mut(self.selected_slice)
                {
                    ui.label("Label");
                    if ui.text_edit_singleline(&mut slice.label).changed() {
                        self.profile_feedback = None;
                        self.save_feedback = SaveFeedback::None;
                    }
                    ui.label("Action");
                    ComboBox::from_id_salt((
                        "profile_slice_action",
                        profile_id,
                        self.selected_slice,
                    ))
                    .width(310.0)
                    .selected_text(action_option_text(slice.action_id, &action_options))
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(&mut slice.action_id, None, "No action").changed() {
                            self.profile_feedback = None;
                            self.save_feedback = SaveFeedback::None;
                        }
                        for (action_id, label) in &action_options {
                            if ui
                                .selectable_value(&mut slice.action_id, Some(*action_id), label)
                                .changed()
                            {
                                self.profile_feedback = None;
                                self.save_feedback = SaveFeedback::None;
                            }
                        }
                    });
                    ui.add_space(12.0);
                    self.render_slice_icon_editor(ui, profile_id, self.selected_slice, slice);
                }
                if action_catalog_empty && ui.link("Create an action first.").clicked() {
                    self.section = SettingsSection::Actions;
                }
                if let Some(feedback) = &self.profile_feedback {
                    ui.add_space(8.0);
                    match feedback {
                        ProfileEditorFeedback::Info(message) => {
                            ui.colored_label(Color32::from_rgb(112, 205, 151), message);
                        }
                        ProfileEditorFeedback::Error(message) => {
                            ui.colored_label(Color32::from_rgb(239, 113, 113), message);
                        }
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(12.0);
                self.render_profile_tester(ui, &draft.proposed);
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(12.0);
                ui.heading("Profile preview");
                ui.label("This is a complete menu, independent from Global.");
                ui.add_space(18.0);
                if let Some(menu) = profile_menu(&draft.proposed, profile_id) {
                    ui.vertical_centered(|ui| {
                        self.render_preview(
                            ui,
                            menu,
                            draft.proposed.activation_binding.button,
                            draft.proposed.appearance,
                        );
                    });
                }
                ui.add_space(16.0);
            });
        });
    }

    fn render_profile_catalog(&mut self, ui: &mut egui::Ui, configuration: &mut Configuration) {
        egui::Panel::left("profile_catalog").resizable(false).exact_size(220.0).show(ui, |ui| {
            ui.add_space(12.0);
            ui.heading("Profiles");
            ui.weak("Global is the fallback.");
            ui.add_space(10.0);
            let rows = configuration
                .application_profiles
                .iter()
                .map(|application| {
                    (
                        application.profile.id,
                        application.profile.name.clone(),
                        application.rule.priority,
                    )
                })
                .collect::<Vec<_>>();
            ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                if rows.is_empty() {
                    ui.weak("No application profiles yet.");
                }
                for (profile_id, name, priority) in rows {
                    let selected = self.selected_profile;
                    if ui
                        .selectable_value(
                            &mut self.selected_profile,
                            Some(profile_id),
                            format!("{name}\nPriority {priority}"),
                        )
                        .changed()
                        && self.selected_profile != selected
                    {
                        self.selected_slice = 0;
                        self.profile_feedback = None;
                    }
                }
            });
            ui.separator();
            if ui.button("+ New profile...").clicked() {
                match choose_path(PathDialogKind::Executable) {
                    PathDialogOutcome::Selected(path) => {
                        let profile_id = Uuid::new_v4();
                        if create_application_profile(configuration, profile_id, path) {
                            self.selected_profile = Some(profile_id);
                            self.selected_slice = 0;
                            self.profile_feedback = Some(ProfileEditorFeedback::Info(
                                "Profile created with a copy of the Global menu.".to_owned(),
                            ));
                            self.save_feedback = SaveFeedback::None;
                        }
                    }
                    PathDialogOutcome::Cancelled => {}
                    PathDialogOutcome::Failed => {
                        self.profile_feedback = Some(ProfileEditorFeedback::Error(
                            "Gradial could not open the application chooser.".to_owned(),
                        ));
                    }
                }
            }
            let has_selection = self.selected_profile.is_some();
            if ui.add_enabled(has_selection, egui::Button::new("Duplicate selected")).clicked()
                && let Some(source_id) = self.selected_profile
            {
                let duplicate_id = Uuid::new_v4();
                if duplicate_application_profile(configuration, source_id, duplicate_id) {
                    self.selected_profile = Some(duplicate_id);
                    self.selected_slice = 0;
                    self.profile_feedback =
                        Some(ProfileEditorFeedback::Info("Profile duplicated.".to_owned()));
                    self.save_feedback = SaveFeedback::None;
                }
            }
            if ui.add_enabled(has_selection, egui::Button::new("Delete selected")).clicked()
                && let Some(profile_id) = self.selected_profile
            {
                self.selected_profile = delete_application_profile(configuration, profile_id);
                self.selected_slice = 0;
                if self.selected_profile.is_none() {
                    self.profile_test = ProfileTestState::Idle;
                }
                self.profile_feedback =
                    Some(ProfileEditorFeedback::Info("Profile deleted.".to_owned()));
                self.save_feedback = SaveFeedback::None;
            }
        });
    }

    fn render_profile_tester(&mut self, ui: &mut egui::Ui, configuration: &Configuration) {
        let now = Instant::now();
        let capture_due = match self.profile_test {
            ProfileTestState::CountingDown { deadline } => (now >= deadline).then_some(deadline),
            _ => None,
        };
        if capture_due.is_some() {
            let target = platform::capture_profile_test_target();
            #[cfg(debug_assertions)]
            log::info!(
                target: "settings",
                "component=settings event_code=settings_debug_profile_test_capture_completed \
                 result_code=ok config_revision={} executable_path_available={} \
                 window_class_available={} window_title_available={}",
                configuration.config_revision,
                target.executable_path.is_some(),
                !target.window_class.is_empty(),
                !target.window_title.is_empty()
            );
            self.profile_test = ProfileTestState::Captured(target);
        }

        ui.heading("Test profiles");
        ui.label("Capture another app, then test every current draft rule.");
        ui.weak(
            "Captured path, class, and title stay only in this window and are cleared when settings closes.",
        );
        ui.add_space(8.0);

        let mut start_capture = false;
        let mut cancel_capture = false;
        match &self.profile_test {
            ProfileTestState::Idle => {
                start_capture = ui.button("Capture target in 3 seconds").clicked();
            }
            ProfileTestState::CountingDown { deadline } => {
                ui.label(
                    RichText::new(format!(
                        "Switch to the target app — capturing in {}…",
                        countdown_seconds_remaining(*deadline, now)
                    ))
                    .strong(),
                );
                cancel_capture = ui.button("Cancel capture").clicked();
                ui.ctx().request_repaint_after(Duration::from_millis(50));
            }
            ProfileTestState::Captured(_) => {
                start_capture = ui.button("Capture another target in 3 seconds").clicked();
            }
        }
        if start_capture {
            self.profile_test =
                ProfileTestState::CountingDown { deadline: now + PROFILE_TEST_COUNTDOWN };
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        } else if cancel_capture {
            self.profile_test = ProfileTestState::Idle;
        }

        let mut clear_capture = false;
        if let ProfileTestState::Captured(target) = &self.profile_test {
            ui.add_space(12.0);
            render_profile_test_result(ui, configuration, target);
            ui.add_space(8.0);
            clear_capture = ui.button("Clear captured details").clicked();
        }
        if clear_capture {
            self.profile_test = ProfileTestState::Idle;
        }
    }

    fn render_actions_section(&mut self, ui: &mut egui::Ui, draft: &mut SettingsDraft) {
        if self.selected_action.is_some_and(|selected| {
            !draft.proposed.actions.iter().any(|action| action.id == selected)
        }) {
            self.selected_action = draft.proposed.actions.first().map(|action| action.id);
        }
        self.render_action_catalog(ui, &mut draft.proposed);
        self.render_action_details(ui, &mut draft.proposed);
        self.render_shortcut_recorder(ui.ctx(), &mut draft.proposed);
    }

    fn render_slice_icon_editor(
        &mut self,
        ui: &mut egui::Ui,
        profile_id: Uuid,
        slice_index: usize,
        slice: &mut MenuSlice,
    ) {
        ui.label(RichText::new("Icon").strong());
        ui.label(match slice.icon {
            Some(icon) => format!("{} icon assigned", icon.format.extension().to_uppercase()),
            None => "No custom icon".to_owned(),
        });
        ui.horizontal(|ui| {
            if ui
                .button(if slice.icon.is_some() { "Replace icon..." } else { "Choose icon..." })
                .clicked()
            {
                match choose_path(PathDialogKind::Icon) {
                    PathDialogOutcome::Selected(path) => {
                        self.pending_icon_import = Some(PendingIconImport {
                            path: PrivatePath::new(path),
                            profile_id,
                            slice_index,
                        });
                        self.icon_feedback = None;
                    }
                    PathDialogOutcome::Cancelled => {}
                    PathDialogOutcome::Failed => {
                        self.icon_feedback = Some(IconEditorFeedback::error(
                            profile_id,
                            slice_index,
                            "Gradial could not open the icon chooser.",
                        ));
                    }
                }
            }
            if slice.icon.is_some() && ui.button("Remove icon").clicked() {
                slice.icon = None;
                self.pending_icon_import = None;
                self.icon_feedback = Some(IconEditorFeedback::info(
                    profile_id,
                    slice_index,
                    "Icon removed from this draft.",
                ));
                self.save_feedback = SaveFeedback::None;
            }
        });
        ui.weak("PNG or SVG, up to 512×512 and 256 KiB. Imported files are copied into Gradial.");
        if let Some(feedback) = self.icon_feedback.as_ref().filter(|feedback| {
            feedback.profile_id == profile_id && feedback.slice_index == slice_index
        }) {
            ui.colored_label(
                if feedback.error {
                    Color32::from_rgb(239, 113, 113)
                } else {
                    Color32::from_rgb(112, 205, 151)
                },
                &feedback.message,
            );
        }
    }

    fn render_action_catalog(&mut self, ui: &mut egui::Ui, configuration: &mut Configuration) {
        let action_rows = configuration
            .actions
            .iter()
            .map(|action| (action.id, action.name.clone(), action_kind_name(&action.kind)))
            .collect::<Vec<_>>();
        egui::Panel::left("action_catalog").resizable(false).exact_size(280.0).show(ui, |ui| {
            ui.add_space(12.0);
            ui.heading("Actions");
            ui.label("Create reusable actions, then assign them from the Menu tab.");
            ui.add_space(10.0);
            ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                if action_rows.is_empty() {
                    ui.weak("No actions yet.");
                }
                for (action_id, name, kind) in &action_rows {
                    let label = format!("{name}\n{kind}");
                    ui.selectable_value(&mut self.selected_action, Some(*action_id), label);
                }
            });
            ui.separator();
            let mut new_action = None;
            let initial_sequence_child = configuration
                .actions
                .iter()
                .find(|action| !matches!(action.kind, ActionKind::Sequence { .. }))
                .map(|action| action.id);
            ui.menu_button("+ New action", |ui| {
                for template in NewActionTemplate::ALL {
                    let enabled =
                        template != NewActionTemplate::Sequence || initial_sequence_child.is_some();
                    if ui.add_enabled(enabled, egui::Button::new(template.label())).clicked() {
                        new_action = Some(template);
                        ui.close();
                    }
                }
                if initial_sequence_child.is_none() {
                    ui.weak("Create a regular action before creating a sequence.");
                }
            });
            if let Some(template) = new_action {
                let action_id = Uuid::new_v4();
                let Some(kind) = template.initial_kind(initial_sequence_child) else {
                    return;
                };
                create_action(configuration, action_id, template.preferred_name(), kind);
                self.selected_action = Some(action_id);
                self.action_feedback = Some(ActionEditorFeedback::Info(format!(
                    "{} action created.",
                    template.label()
                )));
                self.save_feedback = SaveFeedback::None;
            }
            ui.add_space(8.0);
            let has_selection = self.selected_action.is_some();
            if ui.add_enabled(has_selection, egui::Button::new("Duplicate selected")).clicked()
                && let Some(source_id) = self.selected_action
            {
                let duplicate_id = Uuid::new_v4();
                if duplicate_action(configuration, source_id, duplicate_id) {
                    self.selected_action = Some(duplicate_id);
                    self.action_feedback =
                        Some(ActionEditorFeedback::Info("Action duplicated.".to_owned()));
                    self.save_feedback = SaveFeedback::None;
                }
            }
            if ui.add_enabled(has_selection, egui::Button::new("Delete selected")).clicked()
                && let Some(action_id) = self.selected_action
            {
                match delete_action(configuration, action_id) {
                    Ok(next_selection) => {
                        self.selected_action = next_selection;
                        self.action_feedback =
                            Some(ActionEditorFeedback::Info("Action deleted.".to_owned()));
                        self.save_feedback = SaveFeedback::None;
                    }
                    Err(references) => {
                        self.action_feedback =
                            Some(ActionEditorFeedback::Error(references.message()));
                    }
                }
            }
        });
    }

    #[allow(clippy::too_many_lines)]
    fn render_action_details(&mut self, ui: &mut egui::Ui, configuration: &mut Configuration) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(12.0);
            ui.heading("Action editor");
            let Some(action_id) = self.selected_action else {
                ui.label("Create or select an action to edit it.");
                return;
            };
            let references = action_references(configuration, action_id);
            let sequence_context = configuration
                .actions
                .iter()
                .find(|action| action.id == action_id)
                .and_then(|action| {
                    matches!(action.kind, ActionKind::Sequence { .. })
                        .then(|| SequenceEditorContext::new(configuration.clone(), action_id))
                });
            let Some(action) = configuration.actions.iter_mut().find(|action| action.id == action_id)
            else {
                ui.label("Select an available action.");
                return;
            };

            ui.label("Name");
            if ui.text_edit_singleline(&mut action.name).changed() {
                self.action_feedback = None;
                self.save_feedback = SaveFeedback::None;
            }
            ui.add_space(10.0);
            ui.label("Type");
            ui.label(RichText::new(action_kind_name(&action.kind)).strong());
            ui.add_space(12.0);

            let mut payload_changed = false;
            let mut payload_feedback = None;
            match &mut action.kind {
                ActionKind::SendShortcut { modifiers, key } => {
                    ui.label("Shortcut");
                    ui.label(RichText::new(shortcut_display(modifiers, *key)).monospace().strong());
                    ui.add_space(8.0);
                    if ui.button("Record shortcut").clicked() {
                        self.recording_action_id = Some(action_id);
                        self.shortcut_recorder.begin();
                        self.action_feedback = None;
                    }
                    ui.weak(
                        "Press modifiers first, then one letter, number, navigation key, or F1–F24.",
                    );
                }
                ActionKind::TypeText { text } => {
                    ui.label("Text");
                    if ui
                        .add(
                            TextEdit::multiline(text)
                                .desired_width(f32::INFINITY)
                                .desired_rows(10),
                        )
                        .changed()
                    {
                        payload_changed = true;
                    }
                    let bytes = text.len();
                    let count = format!("{bytes} / {MAX_TYPED_TEXT_BYTES} bytes");
                    if bytes > MAX_TYPED_TEXT_BYTES {
                        ui.colored_label(Color32::from_rgb(239, 113, 113), count);
                    } else {
                        ui.weak(count);
                    }
                    ui.weak("Text is sent literally and is never copied to the clipboard.");
                }
                ActionKind::OpenUrl { url } => {
                    payload_changed = render_url_editor(ui, url);
                }
                ActionKind::OpenPath { path } => {
                    let outcome = render_open_path_editor(ui, path);
                    payload_changed = outcome.changed;
                    payload_feedback = outcome.feedback;
                }
                ActionKind::RunExecutable {
                    executable,
                    arguments,
                    working_directory,
                } => {
                    let outcome =
                        render_executable_editor(ui, executable, arguments, working_directory);
                    payload_changed = outcome.changed;
                    payload_feedback = outcome.feedback;
                }
                ActionKind::Window { command } => {
                    payload_changed = render_window_editor(ui, command);
                }
                ActionKind::Delay { milliseconds } => {
                    payload_changed = render_delay_editor(ui, milliseconds);
                }
                ActionKind::Sequence { actions } => {
                    let Some(context) = sequence_context.as_ref() else {
                        ui.colored_label(
                            Color32::from_rgb(239, 113, 113),
                            "The sequence editor is unavailable.",
                        );
                        return;
                    };
                    payload_changed = render_sequence_editor(ui, actions, context);
                }
            }
            if payload_changed {
                self.action_feedback = None;
                self.save_feedback = SaveFeedback::None;
            }
            if let Some(feedback) = payload_feedback {
                self.action_feedback = Some(feedback);
            }

            ui.add_space(14.0);
            if references.is_referenced() {
                ui.weak(references.message());
            }
            if let Some(feedback) = &self.action_feedback {
                match feedback {
                    ActionEditorFeedback::Info(message) => {
                        ui.colored_label(Color32::from_rgb(112, 205, 151), message);
                    }
                    ActionEditorFeedback::Error(message) => {
                        ui.colored_label(Color32::from_rgb(239, 113, 113), message);
                    }
                }
            }
        });
    }

    fn render_shortcut_recorder(
        &mut self,
        context: &egui::Context,
        configuration: &mut Configuration,
    ) {
        if !self.shortcut_recorder.is_recording() {
            return;
        }
        let outcome = self.shortcut_recorder.update(context);
        let mut cancel = false;
        let frame = egui::Frame::popup(&context.style_of(context.theme()))
            .fill(Color32::from_rgb(28, 28, 28));
        Modal::new(Id::new("shortcut_recorder"))
            .frame(frame)
            .backdrop_color(Color32::from_black_alpha(150))
            .show(context, |ui| {
                ui.set_min_width(380.0);
                ui.heading("Record shortcut");
                ui.label("Press the shortcut now.");
                ui.label("Press Escape or choose Cancel to stop recording.");
                if let Some(error) = self.shortcut_recorder.error() {
                    ui.add_space(8.0);
                    ui.colored_label(Color32::from_rgb(239, 113, 113), error);
                }
                ui.add_space(12.0);
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        if cancel {
            self.shortcut_recorder.cancel();
            self.recording_action_id = None;
            return;
        }

        match outcome {
            Some(CaptureOutcome::Captured(recorded)) => {
                let Some(action_id) = self.recording_action_id.take() else {
                    return;
                };
                let Some(action) =
                    configuration.actions.iter_mut().find(|action| action.id == action_id)
                else {
                    return;
                };
                let ActionKind::SendShortcut { modifiers, key } = &mut action.kind else {
                    return;
                };
                *modifiers = recorded.modifiers;
                *key = recorded.key;
                self.action_feedback =
                    Some(ActionEditorFeedback::Info("Shortcut recorded.".to_owned()));
                self.save_feedback = SaveFeedback::None;
                log::info!(
                    target: "settings",
                    "component=settings event_code=settings_shortcut_recorded result_code=ok \
                     config_revision={} action_id={} modifier_count={} scan_code={} extended={}",
                    configuration.config_revision,
                    action_id,
                    modifiers.len(),
                    key.scan_code,
                    key.extended
                );
            }
            Some(CaptureOutcome::Cancelled) => {
                self.recording_action_id = None;
            }
            None => {}
        }
    }

    fn render_preview(
        &mut self,
        ui: &mut egui::Ui,
        configured_menu: &Menu,
        activation_button: ActivationBindingId,
        appearance: Appearance,
    ) {
        let Some(menu) = resolved_menu(configured_menu) else {
            ui.colored_label(Color32::from_rgb(239, 113, 113), "Preview unavailable.");
            return;
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(PREVIEW_SIZE), Sense::click());
        let mut preview_ui =
            ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Center)));
        preview_ui.set_clip_rect(rect);
        let now = Instant::now();
        self.preview_renderer.render(
            &mut preview_ui,
            OverlayRenderFrame {
                invocation_id: Some(PREVIEW_INVOCATION_ID),
                menu: Some(menu),
                activation_button,
                appearance,
                selected_slice: Some(self.selected_slice),
                selection_changed_at: Some(now),
                opacity: 1.0,
                scale: 1.0,
                animation_mode: AnimationMode::ReducedMotion,
                now,
            },
        );
        if response.clicked()
            && let Some(pointer) = response.interact_pointer_pos()
            && let Some(slice) = preview_slice_at(
                normalize_preview_offset(pointer - rect.center(), appearance),
                configured_menu.resolved_slice_count().unwrap_or_default(),
            )
        {
            self.selected_slice = slice;
        }
    }

    fn request_close(&mut self) -> CloseOutcome {
        self.cancel_binding_test();
        if matches!(self.profile_test, ProfileTestState::CountingDown { .. }) {
            self.profile_test = ProfileTestState::Idle;
        }
        if self.draft.as_ref().is_some_and(SettingsDraft::is_dirty) {
            self.lifecycle = EditorLifecycle::ConfirmingClose;
            log::info!(
                target: "settings",
                "component=settings event_code=settings_close_confirmation_shown \
                 result_code=ok config_revision={}",
                self.draft.as_ref().map_or(0, |draft| draft.original.config_revision)
            );
            CloseOutcome::ConfirmationShown
        } else {
            self.profile_test = ProfileTestState::Idle;
            self.clear_diagnostic_preview();
            self.lifecycle = EditorLifecycle::Closed;
            log::info!(
                target: "settings",
                "component=settings event_code=settings_closed result_code=ok \
                 config_revision={} unsaved_changes_discarded=false",
                self.draft.as_ref().map_or(0, |draft| draft.original.config_revision)
            );
            CloseOutcome::Closed
        }
    }

    fn clear_diagnostic_preview(&mut self) {
        self.diagnostic_snapshot = None;
        self.diagnostic_include_process_paths = false;
        self.diagnostic_feedback = None;
    }
}

#[derive(Debug, Default)]
struct PayloadEditOutcome {
    changed: bool,
    feedback: Option<ActionEditorFeedback>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SequenceMove {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SequenceRowEdit {
    Move { index: usize, direction: SequenceMove },
    Remove { index: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SequenceMetrics {
    direct_steps: usize,
    expanded_steps: usize,
    depth: u8,
    delay_ms: u32,
}

#[derive(Clone, Debug)]
struct SequenceEditorContext {
    configuration: Configuration,
    sequence_id: Uuid,
}

impl SequenceEditorContext {
    fn new(configuration: Configuration, sequence_id: Uuid) -> Self {
        Self { configuration, sequence_id }
    }

    fn action_label(&self, action_id: Uuid) -> String {
        self.configuration.actions.iter().find(|action| action.id == action_id).map_or_else(
            || "Unavailable action".to_owned(),
            |action| format!("{} — {}", action.name, action_kind_name(&action.kind)),
        )
    }

    fn valid_append_candidates(&self, current_steps: &[Uuid]) -> Vec<(Uuid, String)> {
        if current_steps.len() >= MAX_ACTION_SEQUENCE_STEPS {
            return Vec::new();
        }
        self.configuration
            .actions
            .iter()
            .filter(|action| action.id != self.sequence_id)
            .filter_map(|action| {
                let mut proposed = current_steps.to_vec();
                proposed.push(action.id);
                self.configuration_with_steps(&proposed)
                    .is_some_and(|configuration| configuration.validate().is_ok())
                    .then(|| (action.id, self.action_label(action.id)))
            })
            .collect()
    }

    fn metrics(&self, steps: &[Uuid]) -> Option<SequenceMetrics> {
        let configuration = self.configuration_with_steps(steps)?;
        let compiler = ActionPlanCompiler::from_configuration(&configuration).ok()?;
        let plan = compiler.compile_plan(self.sequence_id, configuration.config_revision).ok()?;
        let delay_ms = plan.steps().iter().fold(0_u32, |total, step| {
            total.saturating_add(match step.operation() {
                ExecutionOperation::Delay { milliseconds } => *milliseconds,
                _ => 0,
            })
        });
        Some(SequenceMetrics {
            direct_steps: steps.len(),
            expanded_steps: plan.steps().len(),
            depth: sequence_depth(&configuration, self.sequence_id, &mut Vec::new())?,
            delay_ms,
        })
    }

    fn configuration_with_steps(&self, steps: &[Uuid]) -> Option<Configuration> {
        let mut configuration = self.configuration.clone();
        let sequence =
            configuration.actions.iter_mut().find(|action| action.id == self.sequence_id)?;
        let ActionKind::Sequence { actions } = &mut sequence.kind else {
            return None;
        };
        *actions = steps.to_vec();
        Some(configuration)
    }
}

fn render_sequence_editor(
    ui: &mut egui::Ui,
    actions: &mut Vec<Uuid>,
    context: &SequenceEditorContext,
) -> bool {
    let mut changed = false;
    let mut row_edit = None;
    ui.label("Steps");
    ScrollArea::vertical()
        .id_salt("sequence_steps")
        .max_height(230.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (index, action_id) in actions.iter().copied().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", index + 1));
                    ui.label(context.action_label(action_id));
                    if ui.add_enabled(index > 0, egui::Button::new("Up").small()).clicked() {
                        row_edit =
                            Some(SequenceRowEdit::Move { index, direction: SequenceMove::Up });
                    }
                    if ui
                        .add_enabled(index + 1 < actions.len(), egui::Button::new("Down").small())
                        .clicked()
                    {
                        row_edit =
                            Some(SequenceRowEdit::Move { index, direction: SequenceMove::Down });
                    }
                    if ui
                        .add_enabled(actions.len() > 1, egui::Button::new("Remove").small())
                        .clicked()
                    {
                        row_edit = Some(SequenceRowEdit::Remove { index });
                    }
                });
            }
        });
    if let Some(edit) = row_edit {
        changed = match edit {
            SequenceRowEdit::Move { index, direction } => {
                move_sequence_step(actions, index, direction)
            }
            SequenceRowEdit::Remove { index } => remove_sequence_step(actions, index),
        };
    }

    let candidates = context.valid_append_candidates(actions);
    let mut added_action = None;
    ui.add_enabled_ui(!candidates.is_empty(), |ui| {
        ui.menu_button("+ Add step", |ui| {
            for (action_id, label) in &candidates {
                if ui.button(label).clicked() {
                    added_action = Some(*action_id);
                    ui.close();
                }
            }
        });
    });
    if let Some(action_id) = added_action {
        changed |= add_sequence_step(actions, action_id);
    }
    if candidates.is_empty() {
        ui.weak("No additional action fits the current sequence limits.");
    } else {
        ui.weak(
            "Only actions that avoid cycles and stay within depth, step, and Delay limits are listed.",
        );
    }

    ui.add_space(10.0);
    if let Some(metrics) = context.metrics(actions) {
        ui.label(RichText::new("Expanded plan").strong());
        ui.label(format!(
            "{} direct steps · {} executed steps · depth {} / {}",
            metrics.direct_steps, metrics.expanded_steps, metrics.depth, MAX_ACTION_SEQUENCE_DEPTH
        ));
        ui.label(format!(
            "Delay total: {} / {} ms",
            metrics.delay_ms, MAX_ACTION_SEQUENCE_DURATION_MS
        ));
    } else {
        ui.colored_label(
            Color32::from_rgb(239, 113, 113),
            "This sequence exceeds a safety limit. Remove or reorder a step.",
        );
    }
    changed
}

fn add_sequence_step(actions: &mut Vec<Uuid>, action_id: Uuid) -> bool {
    if actions.len() >= MAX_ACTION_SEQUENCE_STEPS {
        return false;
    }
    actions.push(action_id);
    true
}

fn remove_sequence_step(actions: &mut Vec<Uuid>, index: usize) -> bool {
    if actions.len() <= 1 || index >= actions.len() {
        return false;
    }
    actions.remove(index);
    true
}

fn move_sequence_step(actions: &mut [Uuid], index: usize, direction: SequenceMove) -> bool {
    let destination = match direction {
        SequenceMove::Up => index.checked_sub(1),
        SequenceMove::Down => {
            index.checked_add(1).filter(|destination| *destination < actions.len())
        }
    };
    let Some(destination) = destination else {
        return false;
    };
    actions.swap(index, destination);
    true
}

fn sequence_depth(
    configuration: &Configuration,
    action_id: Uuid,
    stack: &mut Vec<Uuid>,
) -> Option<u8> {
    let action = configuration.actions.iter().find(|action| action.id == action_id)?;
    let ActionKind::Sequence { actions } = &action.kind else {
        return Some(0);
    };
    if stack.contains(&action_id) {
        return None;
    }
    stack.push(action_id);
    let child_depth = (|| {
        let mut maximum = 0;
        for child in actions {
            maximum = maximum.max(sequence_depth(configuration, *child, stack)?);
        }
        Some(maximum)
    })();
    stack.pop();
    child_depth.map(|depth| depth.saturating_add(1))
}

fn render_url_editor(ui: &mut egui::Ui, url: &mut String) -> bool {
    ui.label("Website address");
    let changed = ui.add(TextEdit::singleline(url).desired_width(f32::INFINITY)).changed();
    ui.weak("Only complete http:// and https:// addresses are accepted.");
    changed
}

fn render_open_path_editor(ui: &mut egui::Ui, path: &mut PathBuf) -> PayloadEditOutcome {
    ui.label("File or folder path");
    let mut outcome =
        PayloadEditOutcome { changed: render_path_text_field(ui, path), feedback: None };
    ui.horizontal(|ui| {
        if ui.button("Choose file...").clicked() {
            apply_dialog_outcome(
                &mut outcome,
                path,
                choose_path(PathDialogKind::File),
                "Gradial could not open the file chooser.",
            );
        }
        if ui.button("Choose folder...").clicked() {
            apply_dialog_outcome(
                &mut outcome,
                path,
                choose_path(PathDialogKind::Folder),
                "Gradial could not open the folder chooser.",
            );
        }
    });
    ui.weak("The path must be absolute. Gradial checks that it exists when the action runs.");
    outcome
}

fn render_executable_editor(
    ui: &mut egui::Ui,
    executable: &mut PathBuf,
    arguments: &mut Vec<String>,
    working_directory: &mut Option<PathBuf>,
) -> PayloadEditOutcome {
    let mut outcome = PayloadEditOutcome::default();
    ui.label("Application");
    outcome.changed |= render_path_text_field(ui, executable);
    if ui.button("Choose application...").clicked() {
        apply_dialog_outcome(
            &mut outcome,
            executable,
            choose_path(PathDialogKind::Executable),
            "Gradial could not open the application chooser.",
        );
    }
    ui.weak("Choose an absolute .exe path. Scripts and shell commands are not accepted.");

    ui.add_space(12.0);
    ui.label("Arguments");
    let mut remove_argument = None;
    ScrollArea::vertical()
        .id_salt("application_arguments")
        .max_height(130.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (index, argument) in arguments.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", index + 1));
                    outcome.changed |=
                        ui.add(TextEdit::singleline(argument).desired_width(260.0)).changed();
                    if ui.small_button("Remove").clicked() {
                        remove_argument = Some(index);
                    }
                });
            }
        });
    if let Some(index) = remove_argument {
        outcome.changed |= remove_executable_argument(arguments, index);
    }
    if ui.button("+ Add argument").clicked() {
        add_executable_argument(arguments);
        outcome.changed = true;
    }
    ui.weak("Each row is one literal argument. Empty rows are preserved; no shell parsing occurs.");

    ui.add_space(12.0);
    let mut use_working_directory = working_directory.is_some();
    if ui.checkbox(&mut use_working_directory, "Use a working folder").changed() {
        if use_working_directory {
            let initial = executable
                .parent()
                .filter(|parent| parent.is_absolute())
                .map_or_else(default_browse_path, Path::to_path_buf);
            *working_directory = Some(initial);
        } else {
            *working_directory = None;
        }
        outcome.changed = true;
    }
    if let Some(directory) = working_directory {
        outcome.changed |= render_path_text_field(ui, directory);
        if ui.button("Choose working folder...").clicked() {
            apply_dialog_outcome(
                &mut outcome,
                directory,
                choose_path(PathDialogKind::Folder),
                "Gradial could not open the working-folder chooser.",
            );
        }
        ui.weak("The working folder is passed directly to the application.");
    }
    outcome
}

fn render_window_editor(ui: &mut egui::Ui, command: &mut WindowCommand) -> bool {
    ui.label("Command");
    let previous = *command;
    ComboBox::from_id_salt("window_action_command")
        .selected_text(window_command_name(*command))
        .show_ui(ui, |ui| {
            for candidate in [
                WindowCommand::Close,
                WindowCommand::Minimize,
                WindowCommand::Maximize,
                WindowCommand::Restore,
            ] {
                ui.selectable_value(command, candidate, window_command_name(candidate));
            }
        });
    ui.weak("The command applies only to the window captured when the M4 gesture begins.");
    *command != previous
}

fn render_delay_editor(ui: &mut egui::Ui, milliseconds: &mut u32) -> bool {
    ui.label("Duration");
    let changed =
        ui.add(DragValue::new(milliseconds).range(10..=10_000).speed(10).suffix(" ms")).changed();
    ui.weak("Allowed range: 10 through 10000 milliseconds.");
    changed
}

fn render_path_text_field(ui: &mut egui::Ui, path: &mut PathBuf) -> bool {
    let mut text = path.to_string_lossy().into_owned();
    let changed = ui.add(TextEdit::singleline(&mut text).desired_width(f32::INFINITY)).changed();
    if changed {
        *path = PathBuf::from(text);
    }
    changed
}

fn apply_dialog_outcome(
    current: &mut PayloadEditOutcome,
    path: &mut PathBuf,
    dialog: PathDialogOutcome,
    failure_message: &'static str,
) {
    match dialog {
        PathDialogOutcome::Selected(selected) => {
            *path = selected;
            current.changed = true;
            current.feedback = None;
        }
        PathDialogOutcome::Cancelled => {}
        PathDialogOutcome::Failed => {
            current.feedback = Some(ActionEditorFeedback::Error(failure_message.to_owned()));
        }
    }
}

fn add_executable_argument(arguments: &mut Vec<String>) {
    arguments.push(String::new());
}

fn remove_executable_argument(arguments: &mut Vec<String>, index: usize) -> bool {
    if index >= arguments.len() {
        return false;
    }
    arguments.remove(index);
    true
}

fn default_browse_path() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(r"C:\"))
}

fn default_executable_path() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("notepad.exe")
}

const fn window_command_name(command: WindowCommand) -> &'static str {
    match command {
        WindowCommand::Close => "Close",
        WindowCommand::Minimize => "Minimize",
        WindowCommand::Maximize => "Maximize",
        WindowCommand::Restore => "Restore",
    }
}

fn action_field_name(field: &'static str) -> &'static str {
    match field {
        "executable" => "application path",
        "working_directory" => "working-folder path",
        _ => "file or folder path",
    }
}

fn settings_viewport_builder() -> ViewportBuilder {
    ViewportBuilder::default()
        .with_app_id("gradial-settings")
        .with_title("Gradial settings")
        .with_inner_size(SETTINGS_SIZE)
        .with_min_inner_size(SETTINGS_MIN_SIZE)
        .with_resizable(true)
        .with_decorations(true)
        .with_taskbar(true)
        .with_active(true)
        .with_visible(true)
}

fn resize_menu(
    menu: &mut Menu,
    new_slice_count: SliceCount,
    eight_slice_stash: &mut Option<Vec<MenuSlice>>,
) {
    let Some(current_slice_count) = menu.resolved_slice_count() else {
        return;
    };
    if current_slice_count == new_slice_count {
        return;
    }
    match (current_slice_count, new_slice_count) {
        (SliceCount::Eight, SliceCount::Four) => {
            *eight_slice_stash = Some(menu.slices.clone());
            menu.slices = [0, 2, 4, 6].map(|index| menu.slices[index].clone()).into();
            menu.slice_count = 4;
        }
        (SliceCount::Four, SliceCount::Eight) => {
            let cardinal_slices = menu.slices.clone();
            let mut slices = eight_slice_stash
                .take()
                .filter(|slices| slices.len() == 8)
                .unwrap_or_else(|| Menu::eight_slice_default().slices);
            for (cardinal_index, eight_index) in [0, 2, 4, 6].into_iter().enumerate() {
                slices[eight_index] = cardinal_slices[cardinal_index].clone();
            }
            menu.slices = slices;
            menu.slice_count = 8;
        }
        _ => {}
    }
}

fn application_profile_index(configuration: &Configuration, profile_id: Uuid) -> Option<usize> {
    configuration
        .application_profiles
        .iter()
        .position(|application| application.profile.id == profile_id)
}

fn application_profile_mut(
    configuration: &mut Configuration,
    profile_id: Uuid,
) -> Option<&mut ApplicationProfile> {
    configuration
        .application_profiles
        .iter_mut()
        .find(|application| application.profile.id == profile_id)
}

fn profile_menu(configuration: &Configuration, profile_id: Uuid) -> Option<&Menu> {
    configuration.profile(profile_id).map(|profile| &profile.menu)
}

fn profile_menu_mut(configuration: &mut Configuration, profile_id: Uuid) -> Option<&mut Menu> {
    if configuration.global_profile.id == profile_id {
        return Some(&mut configuration.global_profile.menu);
    }
    application_profile_mut(configuration, profile_id)
        .map(|application| &mut application.profile.menu)
}

fn create_application_profile(
    configuration: &mut Configuration,
    profile_id: Uuid,
    executable_path: PathBuf,
) -> bool {
    if configuration.profile(profile_id).is_some() || executable_path.as_os_str().is_empty() {
        return false;
    }
    let preferred_name = executable_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|stem| !stem.is_empty())
        .unwrap_or("Application");
    configuration.application_profiles.push(ApplicationProfile {
        profile: Profile {
            id: profile_id,
            name: unique_profile_name(configuration, preferred_name),
            menu: configuration.global_profile.menu.clone(),
        },
        rule: ProfileRule { executable_path, window_class: None, title_regex: None, priority: 50 },
    });
    true
}

fn duplicate_application_profile(
    configuration: &mut Configuration,
    source_id: Uuid,
    duplicate_id: Uuid,
) -> bool {
    if configuration.profile(duplicate_id).is_some() {
        return false;
    }
    let Some(mut duplicate) = configuration
        .application_profiles
        .iter()
        .find(|application| application.profile.id == source_id)
        .cloned()
    else {
        return false;
    };
    duplicate.profile.id = duplicate_id;
    duplicate.profile.name =
        unique_profile_name(configuration, &format!("{} copy", duplicate.profile.name));
    configuration.application_profiles.push(duplicate);
    true
}

fn delete_application_profile(configuration: &mut Configuration, profile_id: Uuid) -> Option<Uuid> {
    let index = application_profile_index(configuration, profile_id)?;
    configuration.application_profiles.remove(index);
    configuration
        .application_profiles
        .get(index)
        .or_else(|| configuration.application_profiles.last())
        .map(|application| application.profile.id)
}

fn unique_profile_name(configuration: &Configuration, preferred_name: &str) -> String {
    let name_exists = |candidate: &str| {
        configuration.global_profile.name == candidate
            || configuration
                .application_profiles
                .iter()
                .any(|application| application.profile.name == candidate)
    };
    if !name_exists(preferred_name) {
        return preferred_name.to_owned();
    }
    for suffix in 2_u32.. {
        let candidate = format!("{preferred_name} {suffix}");
        if !name_exists(&candidate) {
            return candidate;
        }
    }
    unreachable!("the finite profile catalog cannot exhaust every u32 name suffix")
}

fn countdown_seconds_remaining(deadline: Instant, now: Instant) -> u64 {
    let remaining = deadline.saturating_duration_since(now);
    remaining.as_secs().saturating_add(u64::from(remaining.subsec_nanos() > 0))
}

fn render_profile_test_result(
    ui: &mut egui::Ui,
    configuration: &Configuration,
    target: &ProfileTestTarget,
) {
    ui.label(RichText::new("Captured target").strong());
    ui.label("Application");
    ui.label(
        RichText::new(
            target.executable_path.as_ref().map_or_else(
                || "Unavailable".to_owned(),
                |path| path.to_string_lossy().into_owned(),
            ),
        )
        .monospace(),
    );
    ui.label("Window class");
    ui.label(RichText::new(display_captured_value(&target.window_class)).monospace());
    ui.label("Window title");
    ui.label(RichText::new(display_captured_value(&target.window_title)).monospace());

    ui.add_space(10.0);
    let Ok(resolver) = ProfileResolver::compile(configuration) else {
        ui.colored_label(
            Color32::from_rgb(239, 113, 113),
            "Fix the highlighted settings before testing these draft rules.",
        );
        return;
    };
    let resolution = resolver.resolve(ProfileTarget {
        executable_path: target.executable_path.as_deref(),
        window_class: &target.window_class,
        window_title: &target.window_title,
    });

    ui.label(RichText::new("Matching profiles").strong());
    if resolution.matching_profiles.is_empty() {
        ui.label("No application profile matched every enabled rule.");
    } else {
        for matched in &resolution.matching_profiles {
            ui.label(format!(
                "{} — priority {}, {} of 3 match fields",
                profile_name(configuration, matched.profile_id),
                matched.priority,
                matched.specificity
            ));
            ui.weak(format!("ID {}", matched.profile_id));
        }
    }

    ui.add_space(8.0);
    ui.label(RichText::new("Winner").strong());
    ui.label(profile_name(configuration, resolution.profile_id));
    ui.label(resolution_decision_text(resolution.decision));
}

fn display_captured_value(value: &str) -> &str {
    if value.is_empty() { "(empty)" } else { value }
}

fn profile_name(configuration: &Configuration, profile_id: Uuid) -> &str {
    configuration.profile(profile_id).map_or("Unavailable profile", |profile| profile.name.as_str())
}

const fn resolution_decision_text(decision: ResolutionDecision) -> &'static str {
    match decision {
        ResolutionDecision::ExecutablePathUnavailable => {
            "Global won because the executable path was unavailable."
        }
        ResolutionDecision::NoApplicationProfileMatched => {
            "Global won because no application profile matched every enabled rule."
        }
        ResolutionDecision::OnlyApplicationProfileMatched => {
            "This was the only matching application profile."
        }
        ResolutionDecision::HighestPriority => {
            "This profile won at the highest configured priority."
        }
        ResolutionDecision::HighestSpecificity => {
            "Priorities tied; this profile enabled more matching fields."
        }
        ResolutionDecision::LexicographicallySmallestUuid => {
            "Priority and specificity tied; the smallest immutable profile ID won."
        }
    }
}

fn map_selected_slice(selected_slice: usize, from: SliceCount, to: SliceCount) -> usize {
    match (from, to) {
        (SliceCount::Eight, SliceCount::Four) => selected_slice.div_ceil(2) % 4,
        (SliceCount::Four, SliceCount::Eight) => (selected_slice % 4) * 2,
        _ => selected_slice.min(to.get() - 1),
    }
}

fn direction_selector(ui: &mut egui::Ui, slice_count: SliceCount, selected: &mut usize) {
    ui.horizontal_wrapped(|ui| {
        for index in 0..slice_count.get() {
            ui.selectable_value(selected, index, direction_abbreviation(index, slice_count));
        }
    });
}

fn direction_abbreviation(index: usize, slice_count: SliceCount) -> &'static str {
    match slice_count {
        SliceCount::Four => ["N", "E", "S", "W"].get(index).copied().unwrap_or("?"),
        SliceCount::Eight => {
            ["N", "NE", "E", "SE", "S", "SW", "W", "NW"].get(index).copied().unwrap_or("?")
        }
    }
}

fn direction_name(index: usize, slice_count: SliceCount) -> &'static str {
    match slice_count {
        SliceCount::Four => {
            ["North", "East", "South", "West"].get(index).copied().unwrap_or("Direction")
        }
        SliceCount::Eight => [
            "North",
            "North-east",
            "East",
            "South-east",
            "South",
            "South-west",
            "West",
            "North-west",
        ]
        .get(index)
        .copied()
        .unwrap_or("Direction"),
    }
}

fn create_action(
    configuration: &mut Configuration,
    action_id: Uuid,
    preferred_name: &str,
    kind: ActionKind,
) -> bool {
    if configuration.actions.iter().any(|action| action.id == action_id) {
        return false;
    }
    let name = unique_action_name(configuration, preferred_name);
    configuration.actions.push(ActionDefinition { id: action_id, name, kind });
    true
}

fn duplicate_action(
    configuration: &mut Configuration,
    source_id: Uuid,
    duplicate_id: Uuid,
) -> bool {
    if configuration.actions.iter().any(|action| action.id == duplicate_id) {
        return false;
    }
    let Some(mut duplicate) =
        configuration.actions.iter().find(|action| action.id == source_id).cloned()
    else {
        return false;
    };
    duplicate.id = duplicate_id;
    duplicate.name = unique_action_name(configuration, &format!("{} copy", duplicate.name));
    configuration.actions.push(duplicate);
    true
}

fn unique_action_name(configuration: &Configuration, preferred_name: &str) -> String {
    if !configuration.actions.iter().any(|action| action.name == preferred_name) {
        return preferred_name.to_owned();
    }
    for suffix in 2_u32.. {
        let candidate = format!("{preferred_name} {suffix}");
        if !configuration.actions.iter().any(|action| action.name == candidate) {
            return candidate;
        }
    }
    unreachable!("the finite action catalog cannot exhaust every u32 name suffix")
}

fn action_references(configuration: &Configuration, action_id: Uuid) -> ActionReferenceSummary {
    let global_references = configuration
        .global_profile
        .menu
        .slices
        .iter()
        .filter(|slice| slice.action_id == Some(action_id))
        .count();
    let application_references = configuration
        .application_profiles
        .iter()
        .flat_map(|application| &application.profile.menu.slices)
        .filter(|slice| slice.action_id == Some(action_id))
        .count();
    let sequence_references = configuration
        .actions
        .iter()
        .filter_map(|action| match &action.kind {
            ActionKind::Sequence { actions } => Some(actions),
            _ => None,
        })
        .flatten()
        .filter(|child_id| **child_id == action_id)
        .count();
    ActionReferenceSummary {
        menu_slices: global_references.saturating_add(application_references),
        sequences: sequence_references,
    }
}

fn delete_action(
    configuration: &mut Configuration,
    action_id: Uuid,
) -> Result<Option<Uuid>, ActionReferenceSummary> {
    let references = action_references(configuration, action_id);
    if references.is_referenced() {
        return Err(references);
    }
    let Some(index) = configuration.actions.iter().position(|action| action.id == action_id) else {
        return Ok(configuration.actions.first().map(|action| action.id));
    };
    configuration.actions.remove(index);
    Ok(configuration
        .actions
        .get(index)
        .or_else(|| configuration.actions.last())
        .map(|action| action.id))
}

fn action_options(configuration: &Configuration) -> Vec<(Uuid, String)> {
    configuration
        .actions
        .iter()
        .map(|action| (action.id, format!("{} — {}", action.name, action_kind_name(&action.kind))))
        .collect()
}

fn action_option_text(action_id: Option<Uuid>, options: &[(Uuid, String)]) -> String {
    let Some(action_id) = action_id else {
        return "No action".to_owned();
    };
    options
        .iter()
        .find(|(candidate, _)| *candidate == action_id)
        .map_or_else(|| "Unavailable action".to_owned(), |(_, label)| label.clone())
}

const fn action_kind_name(kind: &ActionKind) -> &'static str {
    match kind {
        ActionKind::SendShortcut { .. } => "Keyboard shortcut",
        ActionKind::TypeText { .. } => "Text",
        ActionKind::OpenUrl { .. } => "Website",
        ActionKind::OpenPath { .. } => "File or folder",
        ActionKind::RunExecutable { .. } => "Application",
        ActionKind::Window { .. } => "Window",
        ActionKind::Delay { .. } => "Delay",
        ActionKind::Sequence { .. } => "Sequence",
    }
}

fn resolved_menu(menu: &Menu) -> Option<Arc<ResolvedMenu>> {
    ResolvedMenu::new_with_icons(
        menu.resolved_slice_count()?,
        menu.slices.iter().map(|slice| slice.label.clone()).collect(),
        menu.slices.iter().map(|slice| slice.action_id).collect(),
        menu.slices.iter().map(|slice| slice.icon).collect(),
    )
    .map(Arc::new)
}

fn activation_binding_display(binding: ActivationBinding) -> String {
    let mut parts = Vec::with_capacity(4);
    if binding.modifiers.control {
        parts.push("Ctrl");
    }
    if binding.modifiers.alt {
        parts.push("Alt");
    }
    if binding.modifiers.shift {
        parts.push("Shift");
    }
    parts.push(binding.button.short_label());
    parts.join(" + ")
}

fn binding_test_selection(result: ActivationBindingTestResult) -> Option<usize> {
    if !result.matched {
        return None;
    }
    let dx = result.release.x.saturating_sub(result.origin.x);
    let dy = result.release.y.saturating_sub(result.origin.y);
    if f64::from(dx).hypot(f64::from(dy)) < 36.0 {
        return None;
    }
    Some(slice_for_delta(dx, dy, SliceCount::Eight))
}

fn preview_slice_at(offset: Vec2, slice_count: SliceCount) -> Option<usize> {
    let radius = offset.length();
    if !(28.0..=116.0).contains(&radius) {
        return None;
    }
    let width = match slice_count {
        SliceCount::Four => std::f32::consts::FRAC_PI_2,
        SliceCount::Eight => std::f32::consts::FRAC_PI_4,
    };
    let angle = offset.x.atan2(-offset.y).rem_euclid(std::f32::consts::TAU);
    let rotated = (angle + width / 2.0).rem_euclid(std::f32::consts::TAU);
    Some(match slice_count {
        SliceCount::Four => sector_index::<4>(rotated),
        SliceCount::Eight => sector_index::<8>(rotated),
    })
}

fn normalize_preview_offset(offset: Vec2, appearance: Appearance) -> Vec2 {
    offset / appearance.size_preset.visual_scale()
}

fn sector_index<const N: usize>(angle: f32) -> usize {
    let width = std::f32::consts::TAU
        / match N {
            4 => 4.0,
            8 => 8.0,
            _ => return 0,
        };
    (1..=N)
        .find_map(|upper| (angle < width * index_as_f32(upper)).then_some(upper - 1))
        .unwrap_or(0)
}

const fn index_as_f32(index: usize) -> f32 {
    match index {
        1 => 1.0,
        2 => 2.0,
        3 => 3.0,
        4 => 4.0,
        5 => 5.0,
        6 => 6.0,
        7 => 7.0,
        8 => 8.0,
        _ => 0.0,
    }
}

fn validate_draft(configuration: &Configuration) -> Result<(), DraftValidationError> {
    let slice_count = configuration
        .global_profile
        .menu
        .resolved_slice_count()
        .ok_or(DraftValidationError::InvalidConfiguration)?;
    if let Some((index, _)) = configuration
        .global_profile
        .menu
        .slices
        .iter()
        .enumerate()
        .find(|(_, slice)| slice.label.trim().is_empty())
    {
        return Err(DraftValidationError::EmptyLabel { index, slice_count });
    }
    for (profile, application) in configuration.application_profiles.iter().enumerate() {
        let profile_slice_count = application
            .profile
            .menu
            .resolved_slice_count()
            .ok_or(DraftValidationError::InvalidConfiguration)?;
        if let Some((index, _)) = application
            .profile
            .menu
            .slices
            .iter()
            .enumerate()
            .find(|(_, slice)| slice.label.trim().is_empty())
        {
            return Err(DraftValidationError::EmptyApplicationLabel {
                profile,
                index,
                slice_count: profile_slice_count,
            });
        }
    }
    if let Some((index, _)) =
        configuration.actions.iter().enumerate().find(|(_, action)| action.name.trim().is_empty())
    {
        return Err(DraftValidationError::EmptyActionName { index });
    }
    if let Some((index, bytes)) =
        configuration.actions.iter().enumerate().find_map(|(index, action)| {
            let ActionKind::TypeText { text } = &action.kind else {
                return None;
            };
            (text.len() > MAX_TYPED_TEXT_BYTES).then_some((index, text.len()))
        })
    {
        return Err(DraftValidationError::TypedTextTooLong { index, bytes });
    }
    configuration.validate().map_err(|error| draft_validation_error(configuration, &error))?;
    ProfileResolver::compile(configuration)
        .map_err(|_error| DraftValidationError::InvalidConfiguration)?;
    ActionPlanCompiler::from_configuration(configuration)
        .map_err(|_error| DraftValidationError::InvalidConfiguration)?;
    Ok(())
}

fn render_diagnostic_feedback(ui: &mut egui::Ui, feedback: Option<&DiagnosticFeedback>) {
    let Some(feedback) = feedback else {
        return;
    };
    ui.add_space(8.0);
    match feedback {
        DiagnosticFeedback::Info(message) => {
            ui.colored_label(Color32::from_rgb(112, 205, 151), message);
        }
        DiagnosticFeedback::Error(message) => {
            ui.colored_label(Color32::from_rgb(239, 113, 113), message);
        }
    }
}

fn display_byte_count(bytes: usize) -> String {
    if bytes < 1_024 {
        return format!("{bytes} B");
    }
    let scaled = bytes.saturating_mul(10).saturating_add(512) / 1_024;
    if scaled < 10_240 {
        return format!("{}.{:01} KiB", scaled / 10, scaled % 10);
    }
    let scaled = bytes.saturating_mul(10).saturating_add(524_288) / 1_048_576;
    format!("{}.{:01} MiB", scaled / 10, scaled % 10)
}

const fn diagnostic_error_message(error: DiagnosticBundleError) -> &'static str {
    match error {
        DiagnosticBundleError::Preview => {
            "Gradial could not prepare the diagnostic preview. The previous preview is unchanged."
        }
        DiagnosticBundleError::InvalidPath => {
            "Choose an available folder and a file name ending in .zip."
        }
        DiagnosticBundleError::ProtectedDestination => {
            "Choose another location. Diagnostic export cannot replace Gradial's active files."
        }
        DiagnosticBundleError::Write => {
            "Gradial could not write the diagnostic bundle. The preview is still available."
        }
    }
}

const fn transfer_error_message(
    operation: TransferOperation,
    error: ConfigurationTransferError,
) -> &'static str {
    match (operation, error) {
        (TransferOperation::Import, ConfigurationTransferError::InvalidPath) => {
            "Choose a settings file from an available location."
        }
        (TransferOperation::Import, ConfigurationTransferError::Missing) => {
            "That settings file no longer exists."
        }
        (TransferOperation::Import, ConfigurationTransferError::Read) => {
            "Gradial could not read that settings file."
        }
        (TransferOperation::Import, ConfigurationTransferError::Parse) => {
            "That file is not valid Gradial settings."
        }
        (TransferOperation::Import, ConfigurationTransferError::UnsupportedSchema) => {
            "That file was made by a newer, unsupported version of Gradial."
        }
        (TransferOperation::Import, ConfigurationTransferError::Invalid) => {
            "Those settings contain invalid profiles, menus, or actions."
        }
        (TransferOperation::Import, ConfigurationTransferError::IconMissing) => {
            "A portable icon disappeared while Gradial was importing it."
        }
        (TransferOperation::Import, ConfigurationTransferError::IconRead) => {
            "Gradial could not read one of the portable icons."
        }
        (TransferOperation::Import, ConfigurationTransferError::IconInvalid) => {
            "A portable icon is damaged or does not meet Gradial's icon limits."
        }
        (TransferOperation::Import, ConfigurationTransferError::IconConflict) => {
            "A portable icon conflicts with a different managed icon that has the same ID."
        }
        (TransferOperation::Import, ConfigurationTransferError::IconWrite) => {
            "Gradial could not copy a portable icon into managed storage."
        }
        (TransferOperation::Import, _) => "Gradial could not import that settings file.",
        (TransferOperation::Export, ConfigurationTransferError::InvalidPath) => {
            "Choose an available folder and file name for the export."
        }
        (TransferOperation::Export, ConfigurationTransferError::ProtectedDestination) => {
            "Choose another file name. Export cannot replace Gradial's active files."
        }
        (TransferOperation::Export, ConfigurationTransferError::Invalid) => {
            "Fix the current draft before exporting it."
        }
        (TransferOperation::Export, ConfigurationTransferError::Write) => {
            "Gradial could not write the settings export."
        }
        (TransferOperation::Export, ConfigurationTransferError::IconMissing) => {
            "A custom icon used by this draft is missing from Gradial's managed storage."
        }
        (TransferOperation::Export, ConfigurationTransferError::IconRead) => {
            "Gradial could not read a custom icon used by this draft."
        }
        (TransferOperation::Export, ConfigurationTransferError::IconInvalid) => {
            "A custom icon used by this draft is damaged or no longer valid."
        }
        (TransferOperation::Export, ConfigurationTransferError::IconConflict) => {
            "The export folder already contains a different icon with the same ID."
        }
        (TransferOperation::Export, ConfigurationTransferError::IconWrite) => {
            "Gradial could not write the portable icon files."
        }
        (TransferOperation::Export, _) => "Gradial could not export the current draft.",
    }
}

const fn icon_import_error_message(error: IconImportError) -> &'static str {
    match error {
        IconImportError::InvalidPath => "Choose a regular icon file.",
        IconImportError::Missing => "That icon file is no longer available.",
        IconImportError::Read => "Gradial could not read that icon file.",
        IconImportError::UnsupportedType => "Choose a PNG or SVG icon.",
        IconImportError::Empty => "That icon file is empty.",
        IconImportError::SourceTooLarge => "Choose an icon no larger than 256 KiB.",
        IconImportError::InvalidPng => "That file is not a valid PNG icon.",
        IconImportError::InvalidSvg => "That file is not a valid SVG icon.",
        IconImportError::UnsupportedSvgContent => {
            "Convert SVG text and embedded images to vector paths before importing."
        }
        IconImportError::Dimensions => "Choose an icon between 1×1 and 512×512 logical pixels.",
        IconImportError::Write => "Gradial could not copy that icon into managed storage.",
    }
}

fn draft_validation_error(
    configuration: &Configuration,
    error: &ConfigurationError,
) -> DraftValidationError {
    let action_index = |action_id| {
        configuration.actions.iter().position(|action| action.id == action_id).unwrap_or(0)
    };
    let profile_index = |profile_id| {
        configuration
            .application_profiles
            .iter()
            .position(|application| application.profile.id == profile_id)
    };
    match error {
        ConfigurationError::EmptyProfileName { profile_id } => profile_index(*profile_id)
            .map_or(DraftValidationError::InvalidConfiguration, |profile| {
                DraftValidationError::EmptyApplicationProfileName { profile }
            }),
        ConfigurationError::EmptyMenuLabel { profile_id, slice_index } => {
            empty_menu_label_error(configuration, *profile_id, *slice_index)
        }
        ConfigurationError::EmptyExecutablePath { profile_id } => profile_index(*profile_id)
            .map_or(DraftValidationError::InvalidConfiguration, |profile| {
                DraftValidationError::EmptyApplicationProfilePath { profile }
            }),
        ConfigurationError::InvalidPriority { profile_id, priority } => profile_index(*profile_id)
            .map_or(DraftValidationError::InvalidConfiguration, |profile| {
                DraftValidationError::InvalidApplicationProfilePriority {
                    profile,
                    priority: *priority,
                }
            }),
        ConfigurationError::EmptyWindowClass { profile_id } => profile_index(*profile_id)
            .map_or(DraftValidationError::InvalidConfiguration, |profile| {
                DraftValidationError::EmptyApplicationWindowClass { profile }
            }),
        ConfigurationError::EmptyTitleRegex { profile_id } => profile_index(*profile_id)
            .map_or(DraftValidationError::InvalidConfiguration, |profile| {
                DraftValidationError::EmptyApplicationTitleRegex { profile }
            }),
        ConfigurationError::InvalidTitleRegex { profile_id, .. } => profile_index(*profile_id)
            .map_or(DraftValidationError::InvalidConfiguration, |profile| {
                DraftValidationError::InvalidApplicationTitleRegex { profile }
            }),
        ConfigurationError::InvalidActionUrl { action_id } => {
            DraftValidationError::InvalidUrl { index: action_index(*action_id) }
        }
        ConfigurationError::ActionPathNotAbsolute { action_id, field }
        | ConfigurationError::ActionPathContainsNul { action_id, field } => {
            DraftValidationError::InvalidPath { index: action_index(*action_id), field }
        }
        ConfigurationError::ExecutableExtensionNotAllowed { action_id } => {
            DraftValidationError::InvalidExecutable { index: action_index(*action_id) }
        }
        ConfigurationError::ExecutableArgumentContainsNul { action_id, index } => {
            DraftValidationError::InvalidArgument {
                index: action_index(*action_id),
                argument: *index,
            }
        }
        ConfigurationError::InvalidDelay { action_id, milliseconds } => {
            DraftValidationError::InvalidDelay {
                index: action_index(*action_id),
                milliseconds: *milliseconds,
            }
        }
        ConfigurationError::InvalidSequenceLength { action_id, found, .. } => {
            DraftValidationError::InvalidSequenceLength {
                index: action_index(*action_id),
                found: *found,
            }
        }
        ConfigurationError::SequenceCycle { action_id } => {
            DraftValidationError::SequenceCycle { index: action_index(*action_id) }
        }
        ConfigurationError::SequenceDepthLimitExceeded { action_id, depth, .. } => {
            DraftValidationError::SequenceDepth { index: action_index(*action_id), depth: *depth }
        }
        ConfigurationError::SequenceStepLimitExceeded { action_id, steps, .. } => {
            DraftValidationError::SequenceSteps { index: action_index(*action_id), steps: *steps }
        }
        ConfigurationError::SequenceDelayLimitExceeded { action_id, milliseconds, .. } => {
            DraftValidationError::SequenceDelay {
                index: action_index(*action_id),
                milliseconds: *milliseconds,
            }
        }
        ConfigurationError::MissingSequenceActionReference { sequence_id, index, .. } => {
            DraftValidationError::MissingSequenceStep {
                index: action_index(*sequence_id),
                step: *index,
            }
        }
        _ => DraftValidationError::InvalidConfiguration,
    }
}

fn empty_menu_label_error(
    configuration: &Configuration,
    profile_id: Uuid,
    slice_index: usize,
) -> DraftValidationError {
    if profile_id == configuration.global_profile.id {
        return DraftValidationError::EmptyLabel {
            index: slice_index,
            slice_count: configuration
                .global_profile
                .menu
                .resolved_slice_count()
                .unwrap_or_default(),
        };
    }
    configuration
        .application_profiles
        .iter()
        .position(|application| application.profile.id == profile_id)
        .map_or(DraftValidationError::InvalidConfiguration, |profile| {
            DraftValidationError::EmptyApplicationLabel {
                profile,
                index: slice_index,
                slice_count: configuration.application_profiles[profile]
                    .profile
                    .menu
                    .resolved_slice_count()
                    .unwrap_or_default(),
            }
        })
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::Arc,
        time::{Duration, Instant},
    };

    use eframe::egui::Vec2;
    use gradial_core::{
        AccentColor, ActionDefinition, ActionKind, ActivationBinding, ActivationBindingId,
        Appearance, Configuration, IconFormat, IconReference, LabelTheme, MAX_TYPED_TEXT_BYTES,
        MenuSlice, PhysicalModifiers, ResolutionDecision, ScreenPoint, ShortcutKey,
        ShortcutModifier, SizePreset, SliceCount, WindowCommand,
    };
    use uuid::Uuid;

    use super::{
        ActionReferenceSummary, CloseOutcome, DraftValidationError, EditorLifecycle,
        NewActionTemplate, PayloadEditOutcome, PortableIconSummary, PrivatePath, ProfileTestState,
        SequenceEditorContext, SequenceMetrics, SequenceMove, SettingsDraft, SettingsEvent,
        SettingsSection, SettingsState, TransferFeedback, action_references,
        activation_binding_display, add_executable_argument, add_sequence_step,
        apply_dialog_outcome, binding_test_selection, countdown_seconds_remaining, create_action,
        create_application_profile, delete_action, delete_application_profile, duplicate_action,
        duplicate_application_profile, map_selected_slice, move_sequence_step,
        normalize_preview_offset, preview_slice_at, remove_executable_argument,
        remove_sequence_step, resize_menu, resolution_decision_text, validate_draft,
    };
    use crate::{
        configuration::ConfigurationMigration,
        diagnostic_bundle::{DiagnosticBundleRequest, DiagnosticSnapshot},
        icons::{IconImportError, IconManager},
        input::ActivationBindingTestResult,
        native_file_dialog::PathDialogOutcome,
        platform::ProfileTestTarget,
    };

    #[test]
    fn switching_slice_counts_preserves_cardinals_and_restores_diagonals_within_the_draft() {
        let mut menu = gradial_core::Menu::eight_slice_default();
        for (index, slice) in menu.slices.iter_mut().enumerate() {
            slice.label = format!("Slice {index}");
            slice.action_id = Some(Uuid::from_u128(index as u128 + 1));
        }
        let original = menu.clone();
        let mut stash = None;

        resize_menu(&mut menu, SliceCount::Four, &mut stash);
        assert_eq!(menu.slice_count, 4);
        assert_eq!(
            menu.slices.iter().map(|slice| slice.label.as_str()).collect::<Vec<_>>(),
            ["Slice 0", "Slice 2", "Slice 4", "Slice 6"]
        );
        menu.slices[1].label = "Changed east".to_owned();

        resize_menu(&mut menu, SliceCount::Eight, &mut stash);
        assert_eq!(menu.slice_count, 8);
        assert_eq!(menu.slices[2].label, "Changed east");
        assert_eq!(menu.slices[1], original.slices[1]);
        assert_eq!(menu.slices[3], original.slices[3]);
    }

    #[test]
    fn switching_from_a_saved_four_slice_menu_creates_default_diagonals() {
        let mut menu = gradial_core::Menu {
            slice_count: 4,
            slices: ["One", "Two", "Three", "Four"]
                .map(|label| MenuSlice { label: label.to_owned(), action_id: None, icon: None })
                .into(),
        };
        let mut stash = None;

        resize_menu(&mut menu, SliceCount::Eight, &mut stash);

        assert_eq!(menu.slices[0].label, "One");
        assert_eq!(menu.slices[1].label, "NE");
        assert_eq!(menu.slices[2].label, "Two");
        assert_eq!(menu.slices[7].label, "NW");
    }

    #[test]
    fn new_application_profile_copies_global_menu_and_uses_a_readable_unique_name() {
        let mut configuration = Configuration::default();
        configuration.global_profile.name = "notepad".to_owned();
        configuration.global_profile.menu.slices[0].label = "Global north".to_owned();
        let profile_id = Uuid::from_u128(41);

        assert!(create_application_profile(
            &mut configuration,
            profile_id,
            PathBuf::from(r"C:\Windows\System32\notepad.exe"),
        ));

        let application = &configuration.application_profiles[0];
        assert_eq!(application.profile.id, profile_id);
        assert_eq!(application.profile.name, "notepad 2");
        assert_eq!(application.profile.menu, configuration.global_profile.menu);
        assert_eq!(
            application.rule.executable_path,
            PathBuf::from(r"C:\Windows\System32\notepad.exe")
        );
        assert_eq!(application.rule.priority, 50);
        assert_eq!(validate_draft(&configuration), Ok(()));
    }

    #[test]
    fn duplicate_and_delete_application_profiles_preserve_complete_independent_data() {
        let source_id = Uuid::from_u128(51);
        let duplicate_id = Uuid::from_u128(52);
        let mut configuration = Configuration::default();
        assert!(create_application_profile(
            &mut configuration,
            source_id,
            PathBuf::from(r"C:\Apps\Editor.exe"),
        ));
        configuration.application_profiles[0].profile.name = "Editor".to_owned();
        configuration.application_profiles[0].profile.menu.slices[2].label =
            "Profile east".to_owned();
        configuration.application_profiles[0].rule.window_class = Some("EditorWindow".to_owned());
        configuration.application_profiles[0].rule.priority = 88;

        assert!(duplicate_application_profile(&mut configuration, source_id, duplicate_id));
        let duplicate = &configuration.application_profiles[1];
        assert_eq!(duplicate.profile.id, duplicate_id);
        assert_eq!(duplicate.profile.name, "Editor copy");
        assert_eq!(duplicate.profile.menu, configuration.application_profiles[0].profile.menu);
        assert_eq!(duplicate.rule, configuration.application_profiles[0].rule);
        assert_eq!(delete_application_profile(&mut configuration, source_id), Some(duplicate_id));
        assert_eq!(configuration.application_profiles.len(), 1);
        assert_eq!(configuration.application_profiles[0].profile.id, duplicate_id);
    }

    #[test]
    fn profile_slice_resizing_does_not_mutate_global_and_restores_its_own_diagonals() {
        let profile_id = Uuid::from_u128(61);
        let mut configuration = Configuration::default();
        for (index, slice) in configuration.global_profile.menu.slices.iter_mut().enumerate() {
            slice.label = format!("Global {index}");
        }
        assert!(create_application_profile(
            &mut configuration,
            profile_id,
            PathBuf::from(r"C:\Apps\Paint.exe"),
        ));
        let original_global = configuration.global_profile.menu.clone();
        let mut draft = SettingsDraft::new(&configuration);

        draft.set_profile_slice_count(profile_id, SliceCount::Four);
        draft.proposed.application_profiles[0].profile.menu.slices[1].label =
            "Profile east".to_owned();
        draft.set_profile_slice_count(profile_id, SliceCount::Eight);

        assert_eq!(draft.proposed.global_profile.menu, original_global);
        let menu = &draft.proposed.application_profiles[0].profile.menu;
        assert_eq!(menu.slice_count, 8);
        assert_eq!(menu.slices[1].label, "Global 1");
        assert_eq!(menu.slices[2].label, "Profile east");
    }

    #[test]
    fn application_profile_validation_reports_editor_specific_fields() {
        let profile_id = Uuid::from_u128(71);
        let mut configuration = Configuration::default();
        assert!(create_application_profile(
            &mut configuration,
            profile_id,
            PathBuf::from(r"C:\Apps\Profile.exe"),
        ));

        configuration.application_profiles[0].profile.name = " ".to_owned();
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::EmptyApplicationProfileName { profile: 0 })
        );
        configuration.application_profiles[0].profile.name = "Profile".to_owned();
        configuration.application_profiles[0].rule.executable_path = PathBuf::new();
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::EmptyApplicationProfilePath { profile: 0 })
        );
        configuration.application_profiles[0].rule.executable_path =
            PathBuf::from(r"C:\Apps\Profile.exe");
        configuration.application_profiles[0].rule.priority = 101;
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::InvalidApplicationProfilePriority {
                profile: 0,
                priority: 101,
            })
        );
        configuration.application_profiles[0].rule.priority = 50;
        configuration.application_profiles[0].rule.window_class = Some(" ".to_owned());
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::EmptyApplicationWindowClass { profile: 0 })
        );
        configuration.application_profiles[0].rule.window_class = None;
        configuration.application_profiles[0].rule.title_regex = Some(String::new());
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::EmptyApplicationTitleRegex { profile: 0 })
        );
        configuration.application_profiles[0].rule.title_regex = Some("(".to_owned());
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::InvalidApplicationTitleRegex { profile: 0 })
        );
        configuration.application_profiles[0].rule.title_regex = Some("^Profile".to_owned());
        configuration.application_profiles[0].profile.menu.slices[2].label = String::new();
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::EmptyApplicationLabel {
                profile: 0,
                index: 2,
                slice_count: SliceCount::Eight,
            })
        );
    }

    #[test]
    fn dirty_close_requires_an_explicit_choice_and_keep_editing_preserves_the_draft() {
        let configuration = Configuration::default();
        let mut editor = SettingsState::new(false, Arc::new(IconManager::new(PathBuf::new())));
        editor.open(&configuration);
        editor.draft.as_mut().expect("open draft").proposed.global_profile.menu.slices[0].label =
            "Changed".to_owned();

        assert_eq!(editor.request_close(), CloseOutcome::ConfirmationShown);
        assert_eq!(editor.lifecycle, EditorLifecycle::ConfirmingClose);

        editor.lifecycle = EditorLifecycle::Editing;
        assert_eq!(
            editor.draft.as_ref().expect("draft retained").proposed.global_profile.menu.slices[0]
                .label,
            "Changed"
        );
    }

    #[test]
    fn clean_close_does_not_prompt() {
        let configuration = Configuration::default();
        let mut editor = SettingsState::new(false, Arc::new(IconManager::new(PathBuf::new())));
        editor.open(&configuration);
        let private_target = ProfileTestTarget {
            executable_path: Some(PathBuf::from(r"C:\Private\Target.exe")),
            window_class: "PrivateClass".to_owned(),
            window_title: "Private title".to_owned(),
        };
        let debug_output = format!("{private_target:?}");
        assert!(!debug_output.contains("Private"));
        assert!(!debug_output.contains("Target.exe"));
        editor.profile_test = ProfileTestState::Captured(private_target);

        assert_eq!(editor.request_close(), CloseOutcome::Closed);
        assert_eq!(editor.lifecycle, EditorLifecycle::Closed);
        assert_eq!(editor.profile_test, ProfileTestState::Idle);
    }

    #[test]
    fn clean_close_clears_the_private_diagnostic_preview_and_path_opt_in() {
        let configuration = Configuration::default();
        let mut editor = SettingsState::new(false, Arc::new(IconManager::new(PathBuf::new())));
        editor.open(&configuration);
        editor.accept_diagnostic_preview(DiagnosticSnapshot::test_fixture(PathBuf::from(
            r"C:\Private\Target.exe",
        )));
        editor.diagnostic_include_process_paths = true;

        assert_eq!(editor.request_close(), CloseOutcome::Closed);
        assert!(editor.diagnostic_snapshot.is_none());
        assert!(!editor.diagnostic_include_process_paths);
        assert!(editor.diagnostic_feedback.is_none());
    }

    #[test]
    fn activation_binding_edits_stay_in_the_draft_until_accept_or_discard() {
        let configuration = Configuration::default();
        let mut draft = SettingsDraft::new(&configuration);
        let edited = ActivationBinding {
            button: ActivationBindingId::MouseButton5,
            modifiers: PhysicalModifiers { control: true, alt: false, shift: false },
        };

        draft.proposed.activation_binding = edited;
        assert!(draft.is_dirty());
        assert_eq!(draft.original.activation_binding, ActivationBinding::default());

        draft.discard();
        assert_eq!(draft.proposed.activation_binding, ActivationBinding::default());
        assert!(!draft.is_dirty());
    }

    #[test]
    fn appearance_edits_are_draft_only_and_size_normalizes_preview_hits() {
        let configuration = Configuration::default();
        let mut draft = SettingsDraft::new(&configuration);
        let edited = Appearance {
            accent_color: AccentColor { red: 224, green: 96, blue: 72 },
            label_theme: LabelTheme::Dark,
            size_preset: SizePreset::Large,
        };

        draft.proposed.appearance = edited;
        assert!(draft.is_dirty());
        assert_eq!(draft.original.appearance, Appearance::default());
        assert_eq!(
            preview_slice_at(
                normalize_preview_offset(Vec2::new(96.6, 0.0), edited),
                SliceCount::Eight,
            ),
            Some(2)
        );

        draft.discard();
        assert_eq!(draft.proposed.appearance, Appearance::default());
        assert!(!draft.is_dirty());
    }

    #[test]
    fn icon_import_and_failure_feedback_remain_private_to_the_draft() {
        let configuration = Configuration::default();
        let profile_id = configuration.global_profile.id;
        let mut editor = SettingsState::new(false, Arc::new(IconManager::new(PathBuf::new())));
        assert!(editor.open(&configuration));
        let icon = IconReference { asset_id: Uuid::from_u128(77), format: IconFormat::Png };

        editor.accept_icon_import(profile_id, 0, icon);

        let draft = editor.draft.as_ref().expect("draft");
        assert_eq!(draft.original.global_profile.menu.slices[0].icon, None);
        assert_eq!(draft.proposed.global_profile.menu.slices[0].icon, Some(icon));
        assert!(draft.is_dirty());
        assert!(editor.icon_feedback.as_ref().is_some_and(|feedback| !feedback.error));

        editor.icon_import_failed(profile_id, 0, IconImportError::SourceTooLarge);
        assert!(
            editor
                .icon_feedback
                .as_ref()
                .is_some_and(|feedback| feedback.error && feedback.message.contains("256 KiB"))
        );
        editor.draft.as_mut().expect("draft").discard();
        assert_eq!(
            editor.draft.as_ref().expect("draft").proposed.global_profile.menu.slices[0].icon,
            None
        );
    }

    #[test]
    fn imported_settings_replace_only_the_private_draft_and_reset_editor_state() {
        let original = Configuration { config_revision: 7, ..Configuration::default() };
        let mut editor = SettingsState::new(false, Arc::new(IconManager::new(PathBuf::new())));
        assert!(editor.open(&original));
        let prior_stash_id = original.global_profile.id;
        editor
            .draft
            .as_mut()
            .expect("open draft")
            .eight_slice_stashes
            .insert(prior_stash_id, original.global_profile.menu.slices.clone());
        editor.section = SettingsSection::Actions;
        editor.selected_slice = 6;

        let mut imported = Configuration { config_revision: 999, ..Configuration::default() };
        imported.global_profile.name = "Imported privately".to_owned();
        imported.global_profile.menu.slices[0].label = "Imported north".to_owned();
        imported.actions.push(ActionDefinition {
            id: Uuid::from_u128(801),
            name: "Imported delay".to_owned(),
            kind: ActionKind::Delay { milliseconds: 100 },
        });

        editor.accept_imported(
            imported,
            Some(ConfigurationMigration {
                from_schema: 2,
                to_schema: gradial_core::CONFIG_SCHEMA_VERSION,
            }),
            PortableIconSummary { referenced: 1, missing: 1, ..PortableIconSummary::default() },
        );

        let draft = editor.draft.as_ref().expect("retained draft");
        assert_eq!(draft.original, original);
        assert_eq!(draft.proposed.config_revision, original.config_revision);
        assert_eq!(draft.proposed.global_profile.name, "Imported privately");
        assert!(draft.is_dirty());
        assert!(draft.eight_slice_stashes.is_empty());
        assert_eq!(editor.section, SettingsSection::Menu);
        assert_eq!(editor.selected_slice, 0);
        assert_eq!(editor.selected_action, Some(Uuid::from_u128(801)));
        assert!(matches!(
            editor.transfer_feedback,
            Some(TransferFeedback::Info(ref message))
                if message.contains("version 2")
                    && message.contains("Save changes")
                    && message.contains("placeholder")
        ));
    }

    #[test]
    fn transfer_events_redact_paths_and_configuration_values_from_debug_output() {
        let private_path = PrivatePath::new(PathBuf::from(r"C:\Private\Export.json"));
        let mut configuration = Configuration::default();
        configuration.global_profile.name = "Private profile value".to_owned();
        let event = SettingsEvent::Export { path: private_path, configuration };

        let debug_output = format!("{event:?}");

        assert!(!debug_output.contains("Private"));
        assert!(!debug_output.contains("Export.json"));
        assert_eq!(debug_output, "Export(<path>, <configuration>)");

        let icon_event = SettingsEvent::ImportIcon {
            path: PrivatePath::new(PathBuf::from(r"C:\Private\Icon.png")),
            profile_id: Uuid::from_u128(42),
            slice_index: 3,
        };
        let icon_debug = format!("{icon_event:?}");
        assert!(!icon_debug.contains(r"C:\Private"));
        assert!(!icon_debug.contains("Icon.png"));
        assert!(icon_debug.contains("slice_index: 3"));

        let diagnostic_event = SettingsEvent::ExportDiagnostics {
            path: PrivatePath::new(PathBuf::from(r"C:\Private\Diagnostics.zip")),
            request: DiagnosticBundleRequest {
                snapshot: Arc::new(DiagnosticSnapshot::test_fixture(PathBuf::from(
                    r"C:\Private\Target.exe",
                ))),
                include_process_paths: true,
            },
        };
        let diagnostic_debug = format!("{diagnostic_event:?}");
        assert!(!diagnostic_debug.contains(r"C:\Private"));
        assert!(!diagnostic_debug.contains("Target.exe"));
        assert!(!diagnostic_debug.contains("Diagnostics.zip"));
        assert!(diagnostic_debug.contains("include_process_paths: true"));
    }

    #[test]
    fn binding_test_reports_exact_chords_and_eight_way_flicks() {
        let actual = ActivationBinding {
            button: ActivationBindingId::MouseButton5,
            modifiers: PhysicalModifiers { control: true, alt: true, shift: false },
        };
        assert_eq!(activation_binding_display(actual), "Ctrl + Alt + M5");

        let east = ActivationBindingTestResult {
            actual,
            matched: true,
            origin: ScreenPoint::new(100, 100),
            release: ScreenPoint::new(180, 100),
        };
        assert_eq!(binding_test_selection(east), Some(2));

        let centered = ActivationBindingTestResult { release: ScreenPoint::new(120, 100), ..east };
        assert_eq!(binding_test_selection(centered), None);

        let mismatch = ActivationBindingTestResult { matched: false, ..east };
        assert_eq!(binding_test_selection(mismatch), None);
    }

    #[test]
    fn profile_capture_countdown_rounds_up_and_decision_text_explains_every_tie_break() {
        let now = Instant::now();
        assert_eq!(countdown_seconds_remaining(now + Duration::from_millis(2_001), now), 3);
        assert_eq!(countdown_seconds_remaining(now + Duration::from_secs(2), now), 2);
        assert_eq!(countdown_seconds_remaining(now, now), 0);

        for decision in [
            ResolutionDecision::ExecutablePathUnavailable,
            ResolutionDecision::NoApplicationProfileMatched,
            ResolutionDecision::OnlyApplicationProfileMatched,
            ResolutionDecision::HighestPriority,
            ResolutionDecision::HighestSpecificity,
            ResolutionDecision::LexicographicallySmallestUuid,
        ] {
            assert!(!resolution_decision_text(decision).is_empty());
        }
    }

    #[test]
    fn opening_an_existing_editor_preserves_its_unsaved_draft() {
        let configuration = Configuration { config_revision: 1, ..Configuration::default() };
        let mut editor = SettingsState::new(false, Arc::new(IconManager::new(PathBuf::new())));
        assert!(editor.open(&configuration));
        editor.draft.as_mut().expect("open draft").proposed.global_profile.menu.slices[0].label =
            "Unsaved".to_owned();

        let mut newer_configuration = configuration;
        newer_configuration.config_revision = 2;
        assert!(!editor.open(&newer_configuration));
        let draft = editor.draft.as_ref().expect("existing draft");
        assert_eq!(draft.original.config_revision, 1);
        assert_eq!(draft.proposed.global_profile.menu.slices[0].label, "Unsaved");
    }

    #[test]
    fn empty_labels_block_save_with_a_field_specific_error() {
        let mut configuration = Configuration::default();
        configuration.global_profile.menu.slices[2].label = "   ".to_owned();

        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::EmptyLabel { index: 2, slice_count: SliceCount::Eight })
        );
    }

    #[test]
    fn existing_action_assignment_remains_valid() {
        let action_id = Uuid::from_u128(44);
        let mut configuration = Configuration::default();
        configuration.actions.push(ActionDefinition {
            id: action_id,
            name: "Pause".to_owned(),
            kind: ActionKind::Delay { milliseconds: 50 },
        });
        configuration.global_profile.menu.slices[0].action_id = Some(action_id);

        assert_eq!(validate_draft(&configuration), Ok(()));
        let draft = SettingsDraft::new(&configuration);
        assert_eq!(draft.proposed.global_profile.menu.slices[0].action_id, Some(action_id));
    }

    #[test]
    fn preview_hit_testing_uses_production_clockwise_slice_order() {
        assert_eq!(preview_slice_at(Vec2::new(0.0, -84.0), SliceCount::Eight), Some(0));
        assert_eq!(preview_slice_at(Vec2::new(84.0, 0.0), SliceCount::Eight), Some(2));
        assert_eq!(preview_slice_at(Vec2::new(0.0, 0.0), SliceCount::Eight), None);
    }

    #[test]
    fn selection_mapping_keeps_the_same_cardinal_direction() {
        assert_eq!(map_selected_slice(2, SliceCount::Eight, SliceCount::Four), 1);
        assert_eq!(map_selected_slice(1, SliceCount::Four, SliceCount::Eight), 2);
    }

    #[test]
    fn action_creation_and_duplication_assign_fresh_ids_and_readable_names() {
        let mut configuration = Configuration::default();
        let source_id = Uuid::from_u128(101);
        let duplicate_id = Uuid::from_u128(102);
        assert!(create_action(
            &mut configuration,
            source_id,
            "Keyboard shortcut",
            ActionKind::SendShortcut {
                modifiers: vec![ShortcutModifier::LeftControl],
                key: ShortcutKey { scan_code: 0x64, extended: false },
            }
        ));
        assert!(create_action(
            &mut configuration,
            Uuid::from_u128(103),
            "Keyboard shortcut",
            ActionKind::TypeText { text: "literal".to_owned() }
        ));
        assert!(duplicate_action(&mut configuration, source_id, duplicate_id));

        assert_eq!(
            configuration.actions.iter().map(|action| action.name.as_str()).collect::<Vec<_>>(),
            ["Keyboard shortcut", "Keyboard shortcut 2", "Keyboard shortcut copy"]
        );
        let duplicate = configuration
            .actions
            .iter()
            .find(|action| action.id == duplicate_id)
            .expect("duplicate");
        assert_eq!(duplicate.kind, configuration.actions[0].kind);
        assert_eq!(validate_draft(&configuration), Ok(()));
    }

    #[test]
    fn referenced_action_deletion_is_blocked_without_clearing_any_reference() {
        let target_id = Uuid::from_u128(201);
        let sequence_id = Uuid::from_u128(202);
        let mut configuration = Configuration {
            actions: vec![
                ActionDefinition {
                    id: target_id,
                    name: "Target".to_owned(),
                    kind: ActionKind::TypeText { text: "private".to_owned() },
                },
                ActionDefinition {
                    id: sequence_id,
                    name: "Sequence".to_owned(),
                    kind: ActionKind::Sequence { actions: vec![target_id] },
                },
            ],
            ..Configuration::default()
        };
        configuration.global_profile.menu.slices[0].action_id = Some(target_id);

        assert_eq!(
            action_references(&configuration, target_id),
            ActionReferenceSummary { menu_slices: 1, sequences: 1 }
        );
        assert_eq!(
            delete_action(&mut configuration, target_id),
            Err(ActionReferenceSummary { menu_slices: 1, sequences: 1 })
        );
        assert!(configuration.actions.iter().any(|action| action.id == target_id));
        assert_eq!(configuration.global_profile.menu.slices[0].action_id, Some(target_id));
    }

    #[test]
    fn unreferenced_action_deletion_selects_the_next_available_action() {
        let first_id = Uuid::from_u128(301);
        let second_id = Uuid::from_u128(302);
        let mut configuration = Configuration {
            actions: [first_id, second_id]
                .map(|id| ActionDefinition {
                    id,
                    name: format!("Action {id}"),
                    kind: ActionKind::TypeText { text: String::new() },
                })
                .into(),
            ..Configuration::default()
        };

        assert_eq!(delete_action(&mut configuration, first_id), Ok(Some(second_id)));
        assert_eq!(configuration.actions.len(), 1);
        assert_eq!(configuration.actions[0].id, second_id);
    }

    #[test]
    fn action_validation_reports_empty_names_and_oversized_text_fields() {
        let mut configuration = Configuration::default();
        configuration.actions.push(ActionDefinition {
            id: Uuid::from_u128(401),
            name: " ".to_owned(),
            kind: ActionKind::TypeText { text: String::new() },
        });
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::EmptyActionName { index: 0 })
        );

        configuration.actions[0].name = "Large text".to_owned();
        configuration.actions[0].kind =
            ActionKind::TypeText { text: "x".repeat(MAX_TYPED_TEXT_BYTES + 1) };
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::TypedTextTooLong {
                index: 0,
                bytes: MAX_TYPED_TEXT_BYTES + 1,
            })
        );
    }

    #[test]
    fn every_action_template_creates_a_valid_payload() {
        let mut configuration = Configuration::default();
        for (index, template) in NewActionTemplate::ALL.into_iter().enumerate() {
            let initial_sequence_child = configuration.actions.first().map(|action| action.id);
            assert!(create_action(
                &mut configuration,
                Uuid::from_u128(500 + index as u128),
                template.preferred_name(),
                template
                    .initial_kind(initial_sequence_child)
                    .expect("regular action exists before the sequence template"),
            ));
        }

        assert_eq!(configuration.actions.len(), NewActionTemplate::ALL.len());
        assert_eq!(validate_draft(&configuration), Ok(()));
        assert!(matches!(configuration.actions[5].kind, ActionKind::Delay { milliseconds: 250 }));
        assert!(matches!(
            configuration.actions[6].kind,
            ActionKind::Window { command: WindowCommand::Minimize }
        ));
        assert!(matches!(
            configuration.actions[7].kind,
            ActionKind::Sequence { ref actions } if actions == &[Uuid::from_u128(500)]
        ));
    }

    #[test]
    fn sequence_rows_preserve_order_and_never_remove_the_last_step() {
        let first = Uuid::from_u128(701);
        let second = Uuid::from_u128(702);
        let third = Uuid::from_u128(703);
        let mut actions = vec![first, second];

        assert!(add_sequence_step(&mut actions, third));
        assert!(move_sequence_step(&mut actions, 2, SequenceMove::Up));
        assert_eq!(actions, [first, third, second]);
        assert!(move_sequence_step(&mut actions, 0, SequenceMove::Down));
        assert_eq!(actions, [third, first, second]);
        assert!(remove_sequence_step(&mut actions, 1));
        assert!(remove_sequence_step(&mut actions, 1));
        assert_eq!(actions, [third]);
        assert!(!remove_sequence_step(&mut actions, 0));
        assert!(!move_sequence_step(&mut actions, 0, SequenceMove::Up));
    }

    #[test]
    fn sequence_metrics_expand_nested_steps_depth_and_delay() {
        let text_id = Uuid::from_u128(711);
        let delay_id = Uuid::from_u128(712);
        let inner_id = Uuid::from_u128(713);
        let outer_id = Uuid::from_u128(714);
        let configuration = Configuration {
            actions: vec![
                ActionDefinition {
                    id: text_id,
                    name: "Text".to_owned(),
                    kind: ActionKind::TypeText { text: "ok".to_owned() },
                },
                ActionDefinition {
                    id: delay_id,
                    name: "Delay".to_owned(),
                    kind: ActionKind::Delay { milliseconds: 200 },
                },
                ActionDefinition {
                    id: inner_id,
                    name: "Inner".to_owned(),
                    kind: ActionKind::Sequence { actions: vec![text_id, delay_id] },
                },
                ActionDefinition {
                    id: outer_id,
                    name: "Outer".to_owned(),
                    kind: ActionKind::Sequence { actions: vec![inner_id, delay_id] },
                },
            ],
            ..Configuration::default()
        };
        let context = SequenceEditorContext::new(configuration, outer_id);

        assert_eq!(
            context.metrics(&[inner_id, delay_id]),
            Some(SequenceMetrics { direct_steps: 2, expanded_steps: 3, depth: 2, delay_ms: 400 })
        );
    }

    #[test]
    fn sequence_candidates_hide_cycles_and_limit_overflows() {
        let text_id = Uuid::from_u128(721);
        let delay_id = Uuid::from_u128(722);
        let inner_id = Uuid::from_u128(723);
        let outer_id = Uuid::from_u128(724);
        let configuration = Configuration {
            actions: vec![
                ActionDefinition {
                    id: text_id,
                    name: "Text".to_owned(),
                    kind: ActionKind::TypeText { text: String::new() },
                },
                ActionDefinition {
                    id: delay_id,
                    name: "Delay".to_owned(),
                    kind: ActionKind::Delay { milliseconds: 10_000 },
                },
                ActionDefinition {
                    id: inner_id,
                    name: "Inner".to_owned(),
                    kind: ActionKind::Sequence { actions: vec![delay_id, delay_id, delay_id] },
                },
                ActionDefinition {
                    id: outer_id,
                    name: "Outer".to_owned(),
                    kind: ActionKind::Sequence { actions: vec![inner_id] },
                },
            ],
            ..Configuration::default()
        };

        let inner_context = SequenceEditorContext::new(configuration, inner_id);
        let candidates = inner_context.valid_append_candidates(&[delay_id, delay_id, delay_id]);
        let candidate_ids = candidates.iter().map(|(action_id, _)| *action_id).collect::<Vec<_>>();
        assert!(candidate_ids.contains(&text_id));
        assert!(!candidate_ids.contains(&delay_id));
        assert!(!candidate_ids.contains(&inner_id));
        assert!(!candidate_ids.contains(&outer_id));
    }

    #[test]
    fn sequence_validation_reports_empty_and_circular_steps() {
        let sequence_id = Uuid::from_u128(731);
        let mut configuration = Configuration {
            actions: vec![ActionDefinition {
                id: sequence_id,
                name: "Sequence".to_owned(),
                kind: ActionKind::Sequence { actions: Vec::new() },
            }],
            ..Configuration::default()
        };
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::InvalidSequenceLength { index: 0, found: 0 })
        );

        configuration.actions[0].kind = ActionKind::Sequence { actions: vec![sequence_id] };
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::SequenceCycle { index: 0 })
        );
    }

    #[test]
    fn executable_argument_rows_preserve_literal_boundaries_and_empty_values() {
        let mut arguments = vec!["--first".to_owned(), "literal value with spaces".to_owned()];

        add_executable_argument(&mut arguments);
        assert_eq!(arguments, ["--first", "literal value with spaces", ""]);
        assert!(remove_executable_argument(&mut arguments, 0));
        assert_eq!(arguments, ["literal value with spaces", ""]);
        assert!(!remove_executable_argument(&mut arguments, 9));
    }

    #[test]
    fn native_dialog_selection_replaces_only_the_edited_path() {
        let mut path = PathBuf::from(r"C:\before");
        let mut outcome = PayloadEditOutcome::default();

        apply_dialog_outcome(
            &mut outcome,
            &mut path,
            PathDialogOutcome::Selected(PathBuf::from(r"D:\after")),
            "fixed failure",
        );
        assert_eq!(path, PathBuf::from(r"D:\after"));
        assert!(outcome.changed);
        assert_eq!(outcome.feedback, None);

        apply_dialog_outcome(
            &mut outcome,
            &mut path,
            PathDialogOutcome::Cancelled,
            "fixed failure",
        );
        assert_eq!(path, PathBuf::from(r"D:\after"));
        assert_eq!(outcome.feedback, None);
    }

    #[test]
    fn leaf_action_validation_reports_the_specific_editor_field() {
        let action_id = Uuid::from_u128(601);
        let mut configuration = Configuration {
            actions: vec![ActionDefinition {
                id: action_id,
                name: "Edited action".to_owned(),
                kind: ActionKind::OpenUrl { url: "file:///private".to_owned() },
            }],
            ..Configuration::default()
        };
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::InvalidUrl { index: 0 })
        );

        configuration.actions[0].kind = ActionKind::OpenPath { path: PathBuf::from("relative") };
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::InvalidPath { index: 0, field: "path" })
        );

        configuration.actions[0].kind = ActionKind::RunExecutable {
            executable: PathBuf::from(r"C:\tool.cmd"),
            arguments: Vec::new(),
            working_directory: None,
        };
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::InvalidExecutable { index: 0 })
        );

        configuration.actions[0].kind = ActionKind::RunExecutable {
            executable: PathBuf::from(r"C:\tool.exe"),
            arguments: vec!["bad\0argument".to_owned()],
            working_directory: None,
        };
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::InvalidArgument { index: 0, argument: 0 })
        );

        configuration.actions[0].kind = ActionKind::Delay { milliseconds: 1 };
        assert_eq!(
            validate_draft(&configuration),
            Err(DraftValidationError::InvalidDelay { index: 0, milliseconds: 1 })
        );
    }
}
