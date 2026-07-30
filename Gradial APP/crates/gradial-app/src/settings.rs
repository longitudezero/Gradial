use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};

use eframe::egui::{
    self, Align, Color32, ComboBox, DragValue, Id, Layout, Modal, RichText, ScrollArea, Sense,
    TextEdit, UiBuilder, Vec2, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId,
};
#[cfg(debug_assertions)]
use gradial_core::ShortcutModifier;
use gradial_core::{
    ActionDefinition, ActionKind, ActionPlanCompiler, Configuration, ConfigurationError,
    ExecutionOperation, MAX_ACTION_SEQUENCE_DEPTH, MAX_ACTION_SEQUENCE_DURATION_MS,
    MAX_ACTION_SEQUENCE_STEPS, MAX_TYPED_TEXT_BYTES, Menu, MenuSlice, ProfileResolver,
    ResolvedMenu, ShortcutKey, SliceCount, WindowCommand,
};
use uuid::Uuid;

use crate::{
    native_file_dialog::{PathDialogKind, PathDialogOutcome, choose_path},
    overlay::{AnimationMode, FoundationOverlayRenderer, OverlayRenderFrame, OverlayRenderer},
    shortcut_recorder::{CaptureOutcome, ShortcutRecorder, shortcut_display},
};

const SETTINGS_SIZE: [f32; 2] = [820.0, 620.0];
const SETTINGS_MIN_SIZE: [f32; 2] = [720.0, 540.0];
const PREVIEW_SIZE: f32 = 288.0;
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
pub(crate) fn settings_viewport_id() -> ViewportId {
    ViewportId::from_hash_of("gradial-settings")
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SettingsDraft {
    original: Configuration,
    proposed: Configuration,
    eight_slice_stash: Option<Vec<MenuSlice>>,
}

impl SettingsDraft {
    fn new(configuration: &Configuration) -> Self {
        Self {
            original: configuration.clone(),
            proposed: configuration.clone(),
            eight_slice_stash: None,
        }
    }

    fn is_dirty(&self) -> bool {
        self.proposed != self.original
    }

    fn discard(&mut self) {
        self.proposed = self.original.clone();
        self.eight_slice_stash = None;
    }

    fn accept_saved(&mut self, configuration: &Configuration) {
        self.original = configuration.clone();
        self.proposed = configuration.clone();
        self.eight_slice_stash = None;
    }

    fn set_slice_count(&mut self, slice_count: SliceCount) {
        resize_menu(
            &mut self.proposed.global_profile.menu,
            slice_count,
            &mut self.eight_slice_stash,
        );
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
    Actions,
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
enum DraftValidationError {
    EmptyLabel { index: usize, slice_count: SliceCount },
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
    fn message(&self) -> String {
        match self {
            Self::EmptyLabel { index, slice_count } => {
                format!("Give {} a label before saving.", direction_name(*index, *slice_count))
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SettingsEvent {
    SaveRequested(Configuration),
}

#[derive(Debug)]
pub(crate) struct SettingsEditor {
    state: Arc<Mutex<SettingsState>>,
}

impl SettingsEditor {
    pub(crate) fn new(recovery_warning: bool) -> Self {
        Self { state: Arc::new(Mutex::new(SettingsState::new(recovery_warning))) }
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

    pub(crate) fn accept_saved(&self, configuration: &Configuration) {
        self.state().accept_saved(configuration);
    }

    pub(crate) fn save_failed(&self) {
        self.state().save_failed();
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
    selected_action: Option<Uuid>,
    action_feedback: Option<ActionEditorFeedback>,
    shortcut_recorder: ShortcutRecorder,
    recording_action_id: Option<Uuid>,
    save_feedback: SaveFeedback,
    recovery_warning: bool,
    preview_renderer: FoundationOverlayRenderer,
    pending_event: Option<SettingsEvent>,
}

impl SettingsState {
    fn new(recovery_warning: bool) -> Self {
        Self {
            lifecycle: EditorLifecycle::Closed,
            draft: None,
            section: SettingsSection::Menu,
            selected_slice: 0,
            selected_action: None,
            action_feedback: None,
            shortcut_recorder: ShortcutRecorder::default(),
            recording_action_id: None,
            save_feedback: SaveFeedback::None,
            recovery_warning,
            preview_renderer: FoundationOverlayRenderer::new(),
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
            self.selected_action = configuration.actions.first().map(|action| action.id);
            self.action_feedback = None;
            self.shortcut_recorder.cancel();
            self.recording_action_id = None;
            self.save_feedback = SaveFeedback::None;
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
        self.action_feedback = None;
        self.recovery_warning = false;
    }

    fn save_failed(&mut self) {
        self.save_feedback = SaveFeedback::Error(
            "Gradial could not save these settings. Your draft is still open.".to_owned(),
        );
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
        .then(|| SettingsEvent::SaveRequested(draft.proposed.clone()))
    }

    #[allow(clippy::too_many_lines)]
    fn render(&mut self, ui: &mut egui::Ui) -> Option<SettingsEvent> {
        #[cfg(debug_assertions)]
        let debug_event = if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F5))
        {
            let _edited = self.debug_apply_fixture_edit();
            None
        } else if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F9))
        {
            let _edited = self.debug_apply_action_fixture_edit();
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
        } else {
            None
        };
        #[cfg(not(debug_assertions))]
        let debug_event = None;

        let close_requested = ui.ctx().input(|input| input.viewport().close_requested());
        if close_requested {
            self.shortcut_recorder.cancel();
            self.recording_action_id = None;
            match self.request_close() {
                CloseOutcome::Closed => return None,
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
                ui.selectable_value(&mut self.section, SettingsSection::Menu, "Menu");
                ui.selectable_value(&mut self.section, SettingsSection::Actions, "Actions");
            });
            if self.recovery_warning {
                ui.add_space(6.0);
                ui.colored_label(
                    Color32::from_rgb(245, 188, 78),
                    "Gradial recovered a previous configuration. Saving will replace the broken file.",
                );
            }
            ui.add_space(8.0);
        });

        egui::Panel::bottom("settings_actions").show(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let save_enabled = dirty && validation.is_ok();
                if ui.add_enabled(save_enabled, egui::Button::new("Save changes")).clicked() {
                    event = Some(SettingsEvent::SaveRequested(draft.proposed.clone()));
                    self.save_feedback = SaveFeedback::None;
                }
                if ui.add_enabled(dirty, egui::Button::new("Discard edits")).clicked() {
                    draft.discard();
                    self.selected_slice = self
                        .selected_slice
                        .min(draft.proposed.global_profile.menu.slices.len() - 1);
                    self.selected_action = draft.proposed.actions.first().map(|action| action.id);
                    self.action_feedback = None;
                    self.shortcut_recorder.cancel();
                    self.recording_action_id = None;
                    self.save_feedback = SaveFeedback::None;
                }
                if let Err(error) = &validation {
                    ui.colored_label(Color32::from_rgb(239, 113, 113), error.message());
                } else {
                    match &self.save_feedback {
                        SaveFeedback::None => {}
                        SaveFeedback::Saved => {
                            ui.colored_label(Color32::from_rgb(112, 205, 151), "Changes saved.");
                        }
                        SaveFeedback::Error(error) => {
                            ui.colored_label(Color32::from_rgb(239, 113, 113), error);
                        }
                    }
                }
            });
            ui.add_space(8.0);
        });

        match self.section {
            SettingsSection::Menu => self.render_menu_section(ui, &mut draft),
            SettingsSection::Actions => self.render_actions_section(ui, &mut draft),
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
                self.lifecycle = EditorLifecycle::Closed;
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

    fn render_menu_section(&mut self, ui: &mut egui::Ui, draft: &mut SettingsDraft) {
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
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(12.0);
            ui.heading("Live preview");
            ui.label("Click a direction to select it.");
            ui.add_space(18.0);
            ui.vertical_centered(|ui| {
                self.render_preview(ui, &draft.proposed);
            });
        });
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

    fn render_preview(&mut self, ui: &mut egui::Ui, configuration: &Configuration) {
        let Some(menu) = resolved_global_menu(configuration) else {
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
                pointer - rect.center(),
                configuration.global_profile.menu.resolved_slice_count().unwrap_or_default(),
            )
        {
            self.selected_slice = slice;
        }
    }

    fn request_close(&mut self) -> CloseOutcome {
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

fn resolved_global_menu(configuration: &Configuration) -> Option<Arc<ResolvedMenu>> {
    let menu = &configuration.global_profile.menu;
    ResolvedMenu::new(
        menu.resolved_slice_count()?,
        menu.slices.iter().map(|slice| slice.label.clone()).collect(),
        menu.slices.iter().map(|slice| slice.action_id).collect(),
    )
    .map(Arc::new)
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

fn draft_validation_error(
    configuration: &Configuration,
    error: &ConfigurationError,
) -> DraftValidationError {
    let action_index = |action_id| {
        configuration.actions.iter().position(|action| action.id == action_id).unwrap_or(0)
    };
    match error {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use eframe::egui::Vec2;
    use gradial_core::{
        ActionDefinition, ActionKind, Configuration, MAX_TYPED_TEXT_BYTES, MenuSlice, ShortcutKey,
        ShortcutModifier, SliceCount, WindowCommand,
    };
    use uuid::Uuid;

    use super::{
        ActionReferenceSummary, CloseOutcome, DraftValidationError, EditorLifecycle,
        NewActionTemplate, PayloadEditOutcome, SequenceEditorContext, SequenceMetrics,
        SequenceMove, SettingsDraft, SettingsState, action_references, add_executable_argument,
        add_sequence_step, apply_dialog_outcome, create_action, delete_action, duplicate_action,
        map_selected_slice, move_sequence_step, preview_slice_at, remove_executable_argument,
        remove_sequence_step, resize_menu, validate_draft,
    };
    use crate::native_file_dialog::PathDialogOutcome;

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
                .map(|label| MenuSlice { label: label.to_owned(), action_id: None })
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
    fn dirty_close_requires_an_explicit_choice_and_keep_editing_preserves_the_draft() {
        let configuration = Configuration::default();
        let mut editor = SettingsState::new(false);
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
        let mut editor = SettingsState::new(false);
        editor.open(&configuration);

        assert_eq!(editor.request_close(), CloseOutcome::Closed);
        assert_eq!(editor.lifecycle, EditorLifecycle::Closed);
    }

    #[test]
    fn opening_an_existing_editor_preserves_its_unsaved_draft() {
        let configuration = Configuration { config_revision: 1, ..Configuration::default() };
        let mut editor = SettingsState::new(false);
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
