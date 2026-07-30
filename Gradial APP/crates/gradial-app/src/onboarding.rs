use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, bounded};
use eframe::egui::{
    self, Align, Color32, Layout, RichText, ViewportBuilder, ViewportClass, ViewportCommand,
    ViewportId,
};
use gradial_core::{
    ActivationBinding, ActivationBindingId, AppPaths, CancelReason, ResolvedMenu, SliceCount,
};
use serde::{Deserialize, Serialize};
use windows::{
    Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    core::PCWSTR,
};

const ONBOARDING_SCHEMA_VERSION: u32 = 1;
const ONBOARDING_SIZE: [f32; 2] = [700.0, 590.0];
const ONBOARDING_MIN_SIZE: [f32; 2] = [620.0, 520.0];
const PRACTICE_EVENT_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OnboardingLoad {
    Complete,
    Required,
}

#[derive(Clone, Debug)]
pub(crate) struct OnboardingStore {
    marker: PathBuf,
    temporary: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct OnboardingStartup {
    pub(crate) store: OnboardingStore,
    pub(crate) required: bool,
}

impl OnboardingStartup {
    pub(crate) fn new(store: OnboardingStore, required: bool) -> Self {
        Self { store, required }
    }
}

impl OnboardingStore {
    pub(crate) fn new(paths: &AppPaths) -> Self {
        Self { marker: paths.onboarding.clone(), temporary: paths.onboarding_temporary.clone() }
    }

    pub(crate) fn load(&self) -> OnboardingLoad {
        let Ok(bytes) = fs::read(&self.marker) else {
            return OnboardingLoad::Required;
        };
        let Ok(marker) = serde_json::from_slice::<OnboardingMarker>(&bytes) else {
            return OnboardingLoad::Required;
        };
        if marker.schema_version == ONBOARDING_SCHEMA_VERSION && marker.completed {
            OnboardingLoad::Complete
        } else {
            OnboardingLoad::Required
        }
    }

    pub(crate) fn set_completed(&self, completed: bool) -> Result<()> {
        let marker = OnboardingMarker { schema_version: ONBOARDING_SCHEMA_VERSION, completed };
        let mut bytes =
            serde_json::to_vec_pretty(&marker).context("could not serialize onboarding state")?;
        bytes.push(b'\n');
        let mut temporary = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.temporary)
            .context("could not create temporary onboarding state")?;
        temporary.write_all(&bytes).context("could not write temporary onboarding state")?;
        temporary.sync_all().context("could not flush temporary onboarding state")?;
        drop(temporary);

        let temporary_wide = wide_path(&self.temporary);
        let marker_wide = wide_path(&self.marker);
        // SAFETY: both buffers are live, null-terminated paths in the same
        // Gradial-owned directory. Replacement is one flushed Windows move.
        let replace_result = unsafe {
            MoveFileExW(
                PCWSTR::from_raw(temporary_wide.as_ptr()),
                PCWSTR::from_raw(marker_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .context("Windows could not replace onboarding state")
        };
        if replace_result.is_err() {
            let _removed = fs::remove_file(&self.temporary);
        }
        replace_result
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OnboardingMarker {
    schema_version: u32,
    completed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct OnboardingPractice {
    enabled: Arc<AtomicBool>,
    sender: Sender<PracticeEvent>,
    menu: Arc<ResolvedMenu>,
}

impl OnboardingPractice {
    pub(crate) fn menu_if_enabled(&self) -> Option<Arc<ResolvedMenu>> {
        self.enabled.load(Ordering::Acquire).then(|| Arc::clone(&self.menu))
    }

    pub(crate) fn opened(&self) {
        self.send(PracticeEvent::Opened);
    }

    pub(crate) fn selected(&self, slice_index: usize) {
        self.send(PracticeEvent::Selected { slice_index });
    }

    pub(crate) fn cancelled(&self, reason: CancelReason) {
        if matches!(reason, CancelReason::NoSelection | CancelReason::Escape) {
            self.send(PracticeEvent::Cancelled { reason });
        }
    }

    fn send(&self, event: PracticeEvent) {
        if self.enabled.load(Ordering::Acquire) {
            let _sent = self.sender.try_send(event);
        }
    }

    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PracticeEvent {
    Opened,
    Selected { slice_index: usize },
    Cancelled { reason: CancelReason },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OnboardingEvent {
    BeginPractice(ActivationBinding),
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OnboardingStartError {
    SettingsOpen,
    SaveFailed,
}

#[derive(Debug)]
pub(crate) struct OnboardingEditor {
    state: Arc<Mutex<OnboardingState>>,
    practice: OnboardingPractice,
    practice_events: Receiver<PracticeEvent>,
}

impl OnboardingEditor {
    pub(crate) fn new(required: bool, binding: ActivationBinding) -> Self {
        let (sender, practice_events) = bounded(PRACTICE_EVENT_CAPACITY);
        let practice = OnboardingPractice {
            enabled: Arc::new(AtomicBool::new(false)),
            sender,
            menu: Arc::new(practice_menu()),
        };
        Self {
            state: Arc::new(Mutex::new(OnboardingState::new(required, binding))),
            practice,
            practice_events,
        }
    }

    pub(crate) fn practice(&self) -> OnboardingPractice {
        self.practice.clone()
    }

    pub(crate) fn is_open(&self) -> bool {
        self.state().stage != OnboardingStage::Closed
    }

    pub(crate) fn settings_open_blocked(&self) {
        self.state().feedback = Some("Finish onboarding before opening the full Settings window.");
    }

    pub(crate) fn restart(&self, binding: ActivationBinding) {
        self.practice.set_enabled(false);
        self.state().restart(binding);
    }

    pub(crate) fn show(&self, context: &egui::Context) {
        if self.state().stage == OnboardingStage::Closed {
            return;
        }
        let state = Arc::clone(&self.state);
        context.show_viewport_deferred(
            onboarding_viewport_id(),
            onboarding_viewport_builder(),
            move |ui, viewport_class| {
                if viewport_class == ViewportClass::EmbeddedWindow {
                    log::error!(
                        target: "onboarding",
                        "component=onboarding event_code=onboarding_viewport_embedded \
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

    pub(crate) fn poll_practice_events(&self, context: &egui::Context) {
        while let Ok(event) = self.practice_events.try_recv() {
            let (event_code, event_value) = match event {
                PracticeEvent::Opened => ("opened", 0),
                PracticeEvent::Selected { slice_index } => ("selected", slice_index),
                PracticeEvent::Cancelled { reason } => {
                    ("cancelled", usize::from(reason == CancelReason::Escape))
                }
            };
            let mut state = self.state();
            state.record_practice_event(event);
            let progress = state.progress;
            drop(state);
            log::info!(
                target: "onboarding",
                "component=onboarding event_code=onboarding_practice_progress result_code=ok \
                 config_revision=0 practice_event={event_code} event_value={event_value} \
                 opened={} selected={} cancelled={}",
                progress.opened,
                progress.selected,
                progress.cancelled_after_selection
            );
            context.request_repaint_of(onboarding_viewport_id());
        }
    }

    pub(crate) fn try_event(&self) -> Option<OnboardingEvent> {
        self.state().pending_event.take()
    }

    pub(crate) fn practice_started(&self, binding: ActivationBinding) {
        self.state().practice_started(binding);
        self.practice.set_enabled(true);
    }

    pub(crate) fn practice_start_failed(&self, error: OnboardingStartError) {
        self.practice.set_enabled(false);
        self.state().practice_start_failed(error);
    }

    pub(crate) fn completion_saved(&self, context: &egui::Context) {
        self.practice.set_enabled(false);
        self.state().stage = OnboardingStage::Closed;
        context.send_viewport_cmd_to(onboarding_viewport_id(), ViewportCommand::Close);
    }

    pub(crate) fn completion_failed(&self) {
        self.state().feedback = Some(
            "Gradial could not save onboarding completion. Your practice progress is retained.",
        );
    }

    fn state(&self) -> MutexGuard<'_, OnboardingState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum OnboardingStage {
    #[default]
    Closed,
    ChooseBinding,
    Practice,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PracticeProgress {
    opened: bool,
    selected: bool,
    cancelled_after_selection: bool,
}

impl PracticeProgress {
    const fn complete(self) -> bool {
        self.opened && self.selected && self.cancelled_after_selection
    }
}

#[derive(Debug)]
struct OnboardingState {
    stage: OnboardingStage,
    binding: ActivationBinding,
    dedication_acknowledged: bool,
    progress: PracticeProgress,
    feedback: Option<&'static str>,
    pending_event: Option<OnboardingEvent>,
}

impl OnboardingState {
    fn new(required: bool, binding: ActivationBinding) -> Self {
        Self {
            stage: if required { OnboardingStage::ChooseBinding } else { OnboardingStage::Closed },
            binding,
            dedication_acknowledged: false,
            progress: PracticeProgress::default(),
            feedback: None,
            pending_event: None,
        }
    }

    fn restart(&mut self, binding: ActivationBinding) {
        self.stage = OnboardingStage::ChooseBinding;
        self.binding = binding;
        self.dedication_acknowledged = false;
        self.progress = PracticeProgress::default();
        self.feedback = None;
        self.pending_event = None;
    }

    fn practice_started(&mut self, binding: ActivationBinding) {
        self.stage = OnboardingStage::Practice;
        self.binding = binding;
        self.progress = PracticeProgress::default();
        self.feedback = None;
    }

    fn practice_start_failed(&mut self, error: OnboardingStartError) {
        self.feedback = Some(match error {
            OnboardingStartError::SettingsOpen => {
                "Close the Settings window before changing the onboarding activation chord."
            }
            OnboardingStartError::SaveFailed => {
                "Gradial could not save that activation chord. Check the local data folder and retry."
            }
        });
    }

    fn record_practice_event(&mut self, event: PracticeEvent) {
        if self.stage != OnboardingStage::Practice {
            return;
        }
        match event {
            PracticeEvent::Opened => self.progress.opened = true,
            PracticeEvent::Selected { .. } => self.progress.selected = true,
            PracticeEvent::Cancelled { .. } if self.progress.selected => {
                self.progress.cancelled_after_selection = true;
            }
            PracticeEvent::Cancelled { .. } => {
                self.feedback = Some(
                    "Good cancellation. Select a sample once, then practise cancellation again.",
                );
            }
        }
    }

    fn render(&mut self, ui: &mut egui::Ui) -> Option<OnboardingEvent> {
        #[cfg(debug_assertions)]
        let debug_event =
            if ui.ctx().input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F1))
                && self.stage == OnboardingStage::ChooseBinding
            {
                self.binding = ActivationBinding {
                    button: ActivationBindingId::MouseButton5,
                    modifiers: gradial_core::PhysicalModifiers {
                        control: true,
                        alt: false,
                        shift: false,
                    },
                };
                self.dedication_acknowledged = true;
                log::info!(
                    target: "onboarding",
                    "component=onboarding event_code=onboarding_debug_choice_ready result_code=ok \
                     config_revision=0 activation_button=M5 activation_control=true"
                );
                Some(OnboardingEvent::BeginPractice(self.binding))
            } else if ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F8))
                && self.stage == OnboardingStage::Practice
                && self.progress.complete()
            {
                Some(OnboardingEvent::Complete)
            } else {
                None
            };
        #[cfg(not(debug_assertions))]
        let debug_event = None;

        if ui.ctx().input(|input| input.viewport().close_requested()) {
            ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
            self.feedback = Some(
                "Finish the required practice, or exit Gradial from its notification-area menu.",
            );
        }

        let mut event = debug_event;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(22.0);
            ui.vertical_centered(|ui| {
                ui.heading("Welcome to Gradial");
                ui.label("A quick practice makes the radial gesture predictable.");
            });
            ui.add_space(20.0);

            match self.stage {
                OnboardingStage::ChooseBinding => self.render_binding_choice(ui, &mut event),
                OnboardingStage::Practice => self.render_practice(ui, &mut event),
                OnboardingStage::Closed => {}
            }

            if let Some(feedback) = self.feedback {
                ui.add_space(14.0);
                ui.colored_label(Color32::from_rgb(239, 113, 113), feedback);
            }
        });
        event
    }

    fn render_binding_choice(&mut self, ui: &mut egui::Ui, event: &mut Option<OnboardingEvent>) {
        ui.heading("1. Choose your activation chord");
        ui.label("The side button and exact optional keys below will open Gradial.");
        ui.add_space(14.0);
        ui.label(RichText::new("Side button").strong());
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.binding.button,
                ActivationBindingId::MouseButton4,
                "Mouse button 4 (M4)",
            );
            ui.selectable_value(
                &mut self.binding.button,
                ActivationBindingId::MouseButton5,
                "Mouse button 5 (M5)",
            );
        });
        ui.add_space(12.0);
        ui.label(RichText::new("Optional required keys").strong());
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.binding.modifiers.control, "Ctrl");
            ui.checkbox(&mut self.binding.modifiers.alt, "Alt");
            ui.checkbox(&mut self.binding.modifiers.shift, "Shift");
        });
        ui.add_space(20.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(610.0);
            ui.label(RichText::new("Important").strong().color(Color32::from_rgb(245, 188, 78)));
            ui.label(format!(
                "{} becomes dedicated to Gradial while Gradial is running.",
                binding_display(self.binding)
            ));
            ui.label(
                "Applications will not receive that exact chord, even when a quick press cancels the menu.",
            );
            ui.add_space(10.0);
            ui.checkbox(
                &mut self.dedication_acknowledged,
                "I understand that this activation chord is dedicated to Gradial.",
            );
        });
        ui.add_space(18.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(
                    self.dedication_acknowledged,
                    egui::Button::new("Save chord and start practice"),
                )
                .clicked()
            {
                self.feedback = None;
                *event = Some(OnboardingEvent::BeginPractice(self.binding));
            }
        });
    }

    fn render_practice(&mut self, ui: &mut egui::Ui, event: &mut Option<OnboardingEvent>) {
        ui.heading("2. Practise with the real radial menu");
        ui.label(format!(
            "Use {}. Practice uses the production input and overlay, but cannot run your configured actions.",
            binding_display(self.binding)
        ));
        ui.add_space(18.0);
        checklist_row(
            ui,
            self.progress.opened,
            "Open the sample menu",
            "Press and hold the activation chord.",
        );
        checklist_row(
            ui,
            self.progress.selected,
            "Select one safe sample",
            "Flick in any direction, then release.",
        );
        checklist_row(
            ui,
            self.progress.cancelled_after_selection,
            "Cancel once",
            "Open again, then release in the center or press Escape.",
        );
        ui.add_space(18.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(RichText::new("Gesture tips").strong());
            ui.label("• Press, flick, and release as one continuous motion.");
            ui.label("• Returning to the center clears the highlighted direction.");
            ui.label("• Escape cancels without running anything.");
        });
        ui.add_space(18.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(self.progress.complete(), egui::Button::new("Finish onboarding"))
                .clicked()
            {
                self.feedback = None;
                *event = Some(OnboardingEvent::Complete);
            }
        });
    }
}

fn checklist_row(ui: &mut egui::Ui, complete: bool, title: &str, explanation: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(if complete { "✓" } else { "○" }).size(22.0).color(if complete {
            Color32::from_rgb(112, 205, 151)
        } else {
            Color32::from_rgb(170, 170, 170)
        }));
        ui.vertical(|ui| {
            ui.label(RichText::new(title).strong());
            ui.weak(explanation);
        });
    });
    ui.add_space(10.0);
}

fn practice_menu() -> ResolvedMenu {
    let labels: Arc<[String]> =
        ["Try N", "Try NE", "Try E", "Try SE", "Try S", "Try SW", "Try W", "Try NW"]
            .map(str::to_owned)
            .into();
    ResolvedMenu::new(SliceCount::Eight, labels, vec![None; 8].into())
        .expect("the built-in onboarding menu is complete")
}

fn binding_display(binding: ActivationBinding) -> String {
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
    parts.push(match binding.button {
        ActivationBindingId::MouseButton4 => "M4",
        ActivationBindingId::MouseButton5 => "M5",
    });
    parts.join(" + ")
}

pub(crate) fn onboarding_viewport_id() -> ViewportId {
    ViewportId::from_hash_of("gradial-onboarding")
}

fn onboarding_viewport_builder() -> ViewportBuilder {
    ViewportBuilder::default()
        .with_app_id("gradial-onboarding")
        .with_title("Welcome to Gradial")
        .with_inner_size(ONBOARDING_SIZE)
        .with_min_inner_size(ONBOARDING_MIN_SIZE)
        .with_resizable(true)
        .with_decorations(true)
        .with_taskbar(true)
        .with_active(true)
        .with_visible(true)
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gradial_core::{ActivationBinding, AppPaths, CancelReason};

    use super::{
        OnboardingLoad, OnboardingStage, OnboardingState, OnboardingStore, PracticeEvent,
        practice_menu,
    };

    fn store() -> (tempfile::TempDir, AppPaths, OnboardingStore) {
        let temporary_directory = tempfile::tempdir().expect("temporary directory");
        let paths = AppPaths::from_local_app_data(temporary_directory.path());
        paths.ensure_directories().expect("application directories");
        let store = OnboardingStore::new(&paths);
        (temporary_directory, paths, store)
    }

    #[test]
    fn missing_invalid_unsupported_and_incomplete_markers_require_onboarding() {
        let (_temporary_directory, paths, store) = store();
        assert_eq!(store.load(), OnboardingLoad::Required);

        fs::write(&paths.onboarding, b"{ invalid private marker").expect("invalid marker");
        assert_eq!(store.load(), OnboardingLoad::Required);

        fs::write(&paths.onboarding, br#"{"schema_version":2,"completed":true}"#)
            .expect("unsupported marker");
        assert_eq!(store.load(), OnboardingLoad::Required);

        store.set_completed(false).expect("incomplete marker");
        assert_eq!(store.load(), OnboardingLoad::Required);
    }

    #[test]
    fn completion_marker_is_flushed_and_replaces_an_incomplete_marker() {
        let (_temporary_directory, paths, store) = store();
        store.set_completed(false).expect("incomplete");
        store.set_completed(true).expect("complete");

        assert_eq!(store.load(), OnboardingLoad::Complete);
        assert!(paths.onboarding.exists());
        assert!(!paths.onboarding_temporary.exists());
    }

    #[test]
    fn practice_requires_selection_before_the_counted_cancellation() {
        let mut state = OnboardingState::new(true, ActivationBinding::default());
        state.stage = OnboardingStage::Practice;

        state.record_practice_event(PracticeEvent::Opened);
        state.record_practice_event(PracticeEvent::Cancelled { reason: CancelReason::NoSelection });
        assert!(!state.progress.complete());

        state.record_practice_event(PracticeEvent::Selected { slice_index: 2 });
        state.record_practice_event(PracticeEvent::Cancelled { reason: CancelReason::Escape });
        assert!(state.progress.complete());
    }

    #[test]
    fn practice_menu_has_eight_samples_and_no_dispatchable_actions() {
        let menu = practice_menu();
        assert_eq!(menu.labels().len(), 8);
        assert!((0..8).all(|index| menu.action_id(index).is_none()));
    }
}
