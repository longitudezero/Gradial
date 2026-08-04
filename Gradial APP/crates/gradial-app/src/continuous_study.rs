use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use eframe::egui::{
    self, Align, Color32, Layout, RichText, ViewportBuilder, ViewportClass, ViewportCommand,
    ViewportId,
};
use gradial_core::{
    ActivationBinding, CancelReason, InteractionOutcome, InvocationSnapshot, ResolvedMenu,
    ScreenPoint, ScreenRect, SliceCount,
};
use serde::Serialize;
use uuid::Uuid;
use windows::{
    Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW},
    core::PCWSTR,
};

const STUDY_SCHEMA_VERSION: u32 = 3;
const DISTANCE_STUDY_SCHEMA_VERSION: u32 = 1;
const STUDY_EVENT_CAPACITY: usize = 32;
const STUDY_SIZE: [f32; 2] = [760.0, 680.0];
const STUDY_MIN_SIZE: [f32; 2] = [680.0, 600.0];
const TARGETS: [u8; 4] = [25, 50, 75, 100];
const DISTANCE_TARGET_PERCENT: u8 = 25;
const DISTANCE_CANDIDATES_DIP: [u32; 4] = [160, 200, 250, 300];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StudyProtocol {
    Usability,
    DistanceComparison,
}

impl StudyProtocol {
    const fn heading(self) -> &'static str {
        match self {
            Self::Usability => "Gradial continuous-input study",
            Self::DistanceComparison => "Gradial distance comparison",
        }
    }

    const fn subtitle(self) -> &'static str {
        match self {
            Self::Usability => "A guided, no-side-effect test of the candidate distance gesture.",
            Self::DistanceComparison => {
                "Compare four no-side-effect full-scale distances using the same target."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum StudyPhase {
    Practice,
    Scalar,
    DirectionChange,
    Cancellation,
    DistanceComparison,
}

impl StudyPhase {
    const fn title(self) -> &'static str {
        match self {
            Self::Practice => "Practice",
            Self::Scalar => "Measured target",
            Self::DirectionChange => "Direction-change check",
            Self::Cancellation => "Cancellation check",
            Self::DistanceComparison => "Distance comparison",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum StudyPlacement {
    Center,
    Edge,
    Corner,
}

impl StudyPlacement {
    const fn instruction(self) -> &'static str {
        match self {
            Self::Center => "Place the pointer comfortably away from every screen edge.",
            Self::Edge => "Place the pointer directly against any one screen edge.",
            Self::Corner => "Place the pointer directly in any screen corner.",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StudyTrial {
    id: u32,
    phase: StudyPhase,
    slices: SliceCount,
    target_percent: Option<u8>,
    direction_index: usize,
    change_from_index: Option<usize>,
    change_after_arm: bool,
    placement: StudyPlacement,
    full_scale_radius_dip: u32,
}

impl StudyTrial {
    fn direction(&self) -> &'static str {
        direction_label(self.slices, self.direction_index)
    }

    fn change_from_direction(&self) -> Option<&'static str> {
        self.change_from_index.map(|index| direction_label(self.slices, index))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StudyInvocationSetup {
    pub(crate) trial_id: u32,
    pub(crate) menu: Arc<ResolvedMenu>,
    pub(crate) full_scale_radius_dip: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct ContinuousStudySink {
    current: Arc<Mutex<Option<StudyInvocationSetup>>>,
    sender: Sender<StudyObservation>,
}

impl ContinuousStudySink {
    pub(crate) fn invocation_if_enabled(&self) -> Option<StudyInvocationSetup> {
        self.current.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    pub(crate) fn completed(
        &self,
        trial_id: Option<u32>,
        snapshot: &InvocationSnapshot,
        outcome: InteractionOutcome,
        duration_ms: u32,
    ) {
        let Some(trial_id) = trial_id else {
            return;
        };
        let terminal = match outcome {
            InteractionOutcome::ContinuousCommitted { slice_index, value } => {
                StudyTerminal::Committed { slice_index, value }
            }
            InteractionOutcome::Cancelled { reason } => StudyTerminal::Cancelled { reason },
            InteractionOutcome::Selected { slice_index } => {
                StudyTerminal::DiscreteSelected { slice_index }
            }
        };
        let observation = StudyObservation {
            trial_id,
            placement: classify_placement(
                snapshot.cursor_physical_px,
                snapshot.context.work_area_physical_px,
                snapshot.monitor_dpi,
            ),
            monitor_dpi: snapshot.monitor_dpi,
            duration_ms,
            terminal,
        };
        match self.sender.try_send(observation) {
            Ok(()) => crate::platform::request_controller_repaint(),
            Err(error) => {
                let status = match error {
                    TrySendError::Full(_) => "full",
                    TrySendError::Disconnected(_) => "disconnected",
                };
                log::error!(
                    target: "study",
                    "component=continuous_study event_code=study_observation_dropped \
                     result_code=failure error_code=channel_{status} config_revision=0"
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum StudyTerminal {
    Committed { slice_index: usize, value: f64 },
    DiscreteSelected { slice_index: usize },
    Cancelled { reason: CancelReason },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StudyObservation {
    trial_id: u32,
    placement: StudyPlacement,
    monitor_dpi: u32,
    duration_ms: u32,
    terminal: StudyTerminal,
}

#[derive(Clone, Debug, Serialize)]
struct StudyAttemptRecord {
    trial_id: u32,
    phase: StudyPhase,
    accepted: bool,
    requested_slice_count: u8,
    requested_direction: String,
    requested_change_from_direction: Option<String>,
    requested_change_after_arm: bool,
    requested_target: Option<f64>,
    requested_full_scale_radius_dip: u32,
    requested_placement: StudyPlacement,
    observed_placement: StudyPlacement,
    placement_matches: bool,
    result: String,
    observed_direction: Option<String>,
    direction_correct: Option<bool>,
    observed_value: Option<f64>,
    absolute_scalar_error: Option<f64>,
    cancel_reason: Option<String>,
    duration_ms: u32,
    monitor_dpi: u32,
}

impl StudyAttemptRecord {
    fn from_observation(trial: &StudyTrial, observation: StudyObservation) -> Self {
        let placement_matches = trial.placement == observation.placement;
        let requested_target = trial.target_percent.map(|value| f64::from(value) / 100.0);
        let (result, observed_index, observed_value, cancel_reason) = match observation.terminal {
            StudyTerminal::Committed { slice_index, value } => {
                ("committed".to_owned(), Some(slice_index), Some(value), None)
            }
            StudyTerminal::DiscreteSelected { slice_index } => {
                ("discrete_selected".to_owned(), Some(slice_index), None, None)
            }
            StudyTerminal::Cancelled { reason } => {
                ("cancelled".to_owned(), None, None, Some(cancel_reason_name(reason).to_owned()))
            }
        };
        let direction_correct = observed_index.map(|index| index == trial.direction_index);
        let observed_direction =
            observed_index.map(|index| direction_label(trial.slices, index).to_owned());
        let absolute_scalar_error =
            observed_value.zip(requested_target).map(|(actual, target)| (actual - target).abs());
        let terminal_is_accepted = match trial.phase {
            StudyPhase::Practice | StudyPhase::Scalar | StudyPhase::DistanceComparison => {
                observed_value.is_some()
            }
            StudyPhase::DirectionChange if trial.change_after_arm => observed_value.is_some(),
            StudyPhase::DirectionChange => {
                result == "discrete_selected"
                    || observed_value.is_some()
                    || cancel_reason.as_deref() == Some("no_selection")
            }
            StudyPhase::Cancellation => {
                observed_value.is_some()
                    || matches!(
                        cancel_reason.as_deref(),
                        Some(
                            "escape" | "other_mouse_button" | "no_selection" | "returned_to_center"
                        )
                    )
            }
        };

        Self {
            trial_id: trial.id,
            phase: trial.phase,
            accepted: placement_matches && terminal_is_accepted,
            requested_slice_count: slice_count_value(trial.slices),
            requested_direction: trial.direction().to_owned(),
            requested_change_from_direction: trial.change_from_direction().map(str::to_owned),
            requested_change_after_arm: trial.change_after_arm,
            requested_target,
            requested_full_scale_radius_dip: trial.full_scale_radius_dip,
            requested_placement: trial.placement,
            observed_placement: observation.placement,
            placement_matches,
            result,
            observed_direction,
            direction_correct,
            observed_value,
            absolute_scalar_error,
            cancel_reason,
            duration_ms: observation.duration_ms,
            monitor_dpi: observation.monitor_dpi,
        }
    }

    fn cancellation_succeeded(&self) -> bool {
        self.phase == StudyPhase::Cancellation
            && matches!(self.cancel_reason.as_deref(), Some("no_selection" | "returned_to_center"))
    }

    fn accidentally_armed(&self) -> bool {
        self.phase == StudyPhase::DirectionChange
            && !self.requested_change_after_arm
            && self.result == "committed"
    }

    fn armed_direction_switch_succeeded(&self) -> bool {
        self.phase == StudyPhase::DirectionChange
            && self.requested_change_after_arm
            && self.result == "committed"
            && self.direction_correct == Some(true)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
struct StudySummary {
    accepted_trials: usize,
    measured_scalar_samples: usize,
    eight_direction_samples: usize,
    eight_direction_errors: usize,
    eight_direction_error_rate: Option<f64>,
    median_absolute_scalar_error: Option<f64>,
    samples_within_0_15: usize,
    fraction_within_0_15: Option<f64>,
    accidental_arming_trials: usize,
    accidental_arms: usize,
    accidental_arming_rate: Option<f64>,
    armed_direction_switch_trials: usize,
    armed_direction_switch_successes: usize,
    armed_direction_switch_success_rate: Option<f64>,
    cancellation_trials: usize,
    cancellation_successes: usize,
    center_median_completion_ms: Option<f64>,
    edge_median_completion_ms: Option<f64>,
    corner_median_completion_ms: Option<f64>,
    edge_corner_slowdown_fraction: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
struct StudyReport {
    schema_version: u32,
    protocol: &'static str,
    session_id: Uuid,
    privacy: StudyPrivacy,
    candidate_geometry_dip: CandidateGeometry,
    planned_trials: usize,
    attempts: Vec<StudyAttemptRecord>,
    summary: StudySummary,
}

#[derive(Clone, Debug, Serialize)]
struct DistanceStudyReport {
    schema_version: u32,
    protocol: &'static str,
    session_id: Uuid,
    privacy: StudyPrivacy,
    direction: &'static str,
    target: f64,
    continuous_boundary_dip: f64,
    candidates_dip: [u32; 4],
    results: Vec<DistanceStudyResult>,
    rejected_attempt_count: usize,
    preferred_full_scale_radius_dip: u32,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct DistanceStudyResult {
    full_scale_radius_dip: u32,
    observed_value: f64,
    absolute_scalar_error: f64,
    duration_ms: u32,
    monitor_dpi: u32,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct StudyPrivacy {
    identity: &'static str,
    excluded_data: [&'static str; 6],
}

#[derive(Clone, Copy, Debug, Serialize)]
struct CandidateGeometry {
    cancel: u32,
    direction: u32,
    continuous_boundary: CandidateBoundaries,
    full_scale: u32,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct CandidateBoundaries {
    compact: f64,
    standard: f64,
    large: f64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum StudyStage {
    #[default]
    Closed,
    Intro,
    Running,
    Results,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StudyUiEvent {
    Start,
    RetryExport,
    ChooseDistance(u32),
    Close,
}

#[derive(Debug)]
struct StudyState {
    protocol: StudyProtocol,
    stage: StudyStage,
    session_id: Uuid,
    plan: Vec<StudyTrial>,
    trial_index: usize,
    attempts: Vec<StudyAttemptRecord>,
    privacy_acknowledged: bool,
    feedback: Option<String>,
    pending_event: Option<StudyUiEvent>,
    export_pending: bool,
    report_path: Option<PathBuf>,
    preferred_full_scale_radius_dip: Option<u32>,
}

impl StudyState {
    fn new(enabled: bool, distance_comparison: bool) -> Self {
        let protocol = if distance_comparison {
            StudyProtocol::DistanceComparison
        } else {
            StudyProtocol::Usability
        };
        Self {
            protocol,
            stage: if enabled { StudyStage::Intro } else { StudyStage::Closed },
            session_id: Uuid::new_v4(),
            plan: match protocol {
                StudyProtocol::Usability => study_plan(),
                StudyProtocol::DistanceComparison => distance_study_plan(),
            },
            trial_index: 0,
            attempts: Vec::new(),
            privacy_acknowledged: false,
            feedback: None,
            pending_event: None,
            export_pending: false,
            report_path: None,
            preferred_full_scale_radius_dip: None,
        }
    }

    fn current_trial(&self) -> Option<&StudyTrial> {
        (self.stage == StudyStage::Running).then(|| self.plan.get(self.trial_index)).flatten()
    }

    fn start(&mut self) {
        self.stage = StudyStage::Running;
        self.trial_index = 0;
        self.attempts.clear();
        self.feedback = None;
        self.export_pending = false;
        self.report_path = None;
        self.preferred_full_scale_radius_dip = None;
    }

    fn record(&mut self, observation: StudyObservation) {
        let Some(trial) = self.current_trial().cloned() else {
            return;
        };
        if observation.trial_id != trial.id {
            return;
        }
        let record = StudyAttemptRecord::from_observation(&trial, observation);
        let accepted = record.accepted;
        self.feedback = if !record.placement_matches {
            Some(format!(
                "That attempt began at {:?}, but this trial requires {:?}. Reposition and try again.",
                record.observed_placement, record.requested_placement
            ))
        } else if !accepted {
            Some(
                "That attempt ended before a value was committed. Try the same target again."
                    .to_owned(),
            )
        } else {
            None
        };
        self.attempts.push(record);
        if !accepted {
            return;
        }

        self.trial_index += 1;
        if self.trial_index >= self.plan.len() {
            self.stage = StudyStage::Results;
            self.export_pending = self.protocol == StudyProtocol::Usability;
        }
    }

    fn report(&self) -> StudyReport {
        StudyReport {
            schema_version: STUDY_SCHEMA_VERSION,
            protocol: "gradial-continuous-usability-v3",
            session_id: self.session_id,
            privacy: study_privacy(),
            candidate_geometry_dip: CandidateGeometry {
                cancel: 24,
                direction: 36,
                continuous_boundary: CandidateBoundaries {
                    compact: 90.95,
                    standard: 107.0,
                    large: 123.05,
                },
                full_scale: 160,
            },
            planned_trials: self.plan.len(),
            attempts: self.attempts.clone(),
            summary: summarize_attempts(&self.attempts),
        }
    }

    fn distance_report(&self) -> Option<DistanceStudyReport> {
        let preference = self
            .preferred_full_scale_radius_dip
            .filter(|candidate| DISTANCE_CANDIDATES_DIP.contains(candidate))?;
        let accepted: Vec<_> = self
            .attempts
            .iter()
            .filter(|attempt| attempt.phase == StudyPhase::DistanceComparison && attempt.accepted)
            .collect();
        let results = accepted
            .iter()
            .filter_map(|attempt| {
                Some(DistanceStudyResult {
                    full_scale_radius_dip: attempt.requested_full_scale_radius_dip,
                    observed_value: attempt.observed_value?,
                    absolute_scalar_error: attempt.absolute_scalar_error?,
                    duration_ms: attempt.duration_ms,
                    monitor_dpi: attempt.monitor_dpi,
                })
            })
            .collect();
        Some(DistanceStudyReport {
            schema_version: DISTANCE_STUDY_SCHEMA_VERSION,
            protocol: "gradial-continuous-distance-comparison-v1",
            session_id: self.session_id,
            privacy: study_privacy(),
            direction: "E",
            target: f64::from(DISTANCE_TARGET_PERCENT) / 100.0,
            continuous_boundary_dip: 107.0,
            candidates_dip: DISTANCE_CANDIDATES_DIP,
            results,
            rejected_attempt_count: self.attempts.len().saturating_sub(accepted.len()),
            preferred_full_scale_radius_dip: preference,
        })
    }

    fn render(&mut self, ui: &mut egui::Ui, binding: ActivationBinding) -> Option<StudyUiEvent> {
        if ui.ctx().input(|input| input.viewport().close_requested()) {
            return Some(StudyUiEvent::Close);
        }

        #[cfg(debug_assertions)]
        let debug_event = if self.stage == StudyStage::Intro
            && ui.ctx().input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F1))
        {
            self.privacy_acknowledged = true;
            Some(StudyUiEvent::Start)
        } else {
            None
        };
        #[cfg(not(debug_assertions))]
        let debug_event = None;
        let mut event = debug_event;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.heading(self.protocol.heading());
                ui.label(self.protocol.subtitle());
            });
            ui.add_space(18.0);

            match self.stage {
                StudyStage::Intro => self.render_intro(ui, binding, &mut event),
                StudyStage::Running => self.render_trial(ui, binding),
                StudyStage::Results => self.render_results(ui, &mut event),
                StudyStage::Closed => {}
            }

            if let Some(feedback) = &self.feedback {
                ui.add_space(14.0);
                ui.colored_label(Color32::from_rgb(239, 113, 113), feedback);
            }
        });
        event
    }

    fn render_intro(
        &mut self,
        ui: &mut egui::Ui,
        binding: ActivationBinding,
        event: &mut Option<StudyUiEvent>,
    ) {
        ui.heading("Before you begin");
        ui.label(format!(
            "The {} uses {} and the real production overlay, but every direction is inert.",
            if self.protocol == StudyProtocol::DistanceComparison { "comparison" } else { "study" },
            binding_display(binding)
        ));
        ui.add_space(12.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(650.0);
            ui.label(RichText::new("Privacy").strong());
            ui.label("• A random session ID is generated; you are not asked for a name.");
            ui.label("• No raw cursor coordinates, application identity, paths, or window details are exported.");
            ui.label("• The report stays on this computer until you choose to share the JSON file.");
        });
        ui.add_space(12.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            if self.protocol == StudyProtocol::DistanceComparison {
                ui.label(RichText::new("How the comparison works").strong());
                ui.label("• Four trials use E at 25%, changing only the 100% distance.");
                ui.label("• Keep the pointer comfortably away from every screen edge.");
                ui.label("• Each measured result remains visible while you continue.");
                ui.label("• At the end, choose the distance that felt best.");
            } else {
                ui.label(RichText::new("Cancellation instruction — read once").strong());
                ui.label("To cancel, return to the center while still holding, then release there.");
                ui.label(
                    "Cancellation stops further effects; it does not undo effects already produced.",
                );
            }
        });
        ui.add_space(14.0);
        ui.checkbox(
            &mut self.privacy_acknowledged,
            if self.protocol == StudyProtocol::DistanceComparison {
                "I understand the privacy summary and comparison instructions."
            } else {
                "I understand the privacy summary and the cancellation instruction."
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add_enabled(
                    self.privacy_acknowledged,
                    egui::Button::new(if self.protocol == StudyProtocol::DistanceComparison {
                        "Start comparison"
                    } else {
                        "Start practice"
                    }),
                )
                .clicked()
            {
                *event = Some(StudyUiEvent::Start);
            }
        });
    }

    fn render_trial(&self, ui: &mut egui::Ui, binding: ActivationBinding) {
        let Some(trial) = self.current_trial() else {
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(trial.phase.title()).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(format!("Trial {} of {}", self.trial_index + 1, self.plan.len()));
            });
        });
        ui.add(egui::ProgressBar::new(progress_fraction(self.trial_index, self.plan.len())));
        ui.add_space(18.0);
        ui.label(RichText::new(trial.placement.instruction()).size(18.0));
        ui.add_space(18.0);

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(650.0);
            match trial.phase {
                StudyPhase::Practice | StudyPhase::Scalar => {
                    ui.label(RichText::new(format!(
                        "Direction {}  •  Target {}%  •  {} slices",
                        trial.direction(),
                        trial.target_percent.unwrap_or_default(),
                        slice_count_value(trial.slices)
                    )).size(24.0).strong());
                    ui.label(format!(
                        "Hold {}, move toward the requested direction until the live percentage matches the target, then release.",
                        binding_display(binding)
                    ));
                }
                StudyPhase::DirectionChange => {
                    if trial.change_after_arm {
                        ui.label(RichText::new(format!(
                            "Move outside toward {} near 50%, then turn to {}",
                            trial.change_from_direction().unwrap_or("?"),
                            trial.direction()
                        )).size(24.0).strong());
                        ui.label(format!(
                            "Hold {}, move beyond the outer edge, turn to the requested final wedge while remaining outside, then release.",
                            binding_display(binding)
                        ));
                    } else {
                        ui.label(RichText::new(format!(
                            "Begin toward {}, then change to {}",
                            trial.change_from_direction().unwrap_or("?"),
                            trial.direction()
                        )).size(24.0).strong());
                        ui.label(format!(
                            "Hold {}, change direction inside the visible outer edge, and release over the requested wedge.",
                            binding_display(binding)
                        ));
                    }
                }
                StudyPhase::Cancellation => {
                    ui.label(RichText::new(format!(
                        "Move outside toward {} near {}%, then clear",
                        trial.direction(),
                        trial.target_percent.unwrap_or_default()
                    )).size(24.0).strong());
                    ui.label("Use the cancellation method described once at the beginning of the study.");
                }
                StudyPhase::DistanceComparison => {
                    ui.label(RichText::new(format!(
                        "Direction E  ·  Target 25%  ·  {} DIP at 100%",
                        trial.full_scale_radius_dip
                    )).size(24.0).strong());
                    ui.label(format!(
                        "Hold {}, move east until the live percentage reads 25%, then release.",
                        binding_display(binding)
                    ));
                }
            }
        });
        if self.protocol == StudyProtocol::DistanceComparison {
            ui.add_space(16.0);
            self.render_distance_result_rows(ui);
        }
        ui.add_space(16.0);
        ui.weak("The next trial appears automatically after a valid attempt. A misplaced or incomplete attempt is retained but must be retried.");
    }

    fn render_results(&self, ui: &mut egui::Ui, event: &mut Option<StudyUiEvent>) {
        if self.protocol == StudyProtocol::DistanceComparison {
            self.render_distance_results(ui, event);
            return;
        }
        let summary = summarize_attempts(&self.attempts);
        ui.heading("Study complete");
        ui.label("Thank you. The configured actions were never available to this study.");
        ui.add_space(14.0);
        egui::Grid::new("continuous-study-summary").num_columns(2).spacing([24.0, 8.0]).show(
            ui,
            |ui| {
                summary_row(ui, "Measured samples", summary.measured_scalar_samples.to_string());
                summary_row(
                    ui,
                    "Median scalar error",
                    format_optional_percent(summary.median_absolute_scalar_error),
                );
                summary_row(
                    ui,
                    "Samples within 15%",
                    format_optional_percent(summary.fraction_within_0_15),
                );
                summary_row(
                    ui,
                    "Accidental continuous entry",
                    format!("{} of {}", summary.accidental_arms, summary.accidental_arming_trials),
                );
                summary_row(
                    ui,
                    "Outside direction switches",
                    format!(
                        "{} of {}",
                        summary.armed_direction_switch_successes,
                        summary.armed_direction_switch_trials
                    ),
                );
                summary_row(
                    ui,
                    "Cancellation",
                    format!(
                        "{} of {}",
                        summary.cancellation_successes, summary.cancellation_trials
                    ),
                );
            },
        );
        ui.add_space(16.0);
        if let Some(path) = &self.report_path {
            ui.label(RichText::new("Anonymous JSON report saved locally:").strong());
            ui.monospace(path.display().to_string());
        } else if self.export_pending {
            ui.label("Saving the anonymous report…");
        } else {
            ui.colored_label(Color32::from_rgb(239, 113, 113), "The report could not be saved.");
            if ui.button("Retry export").clicked() {
                *event = Some(StudyUiEvent::RetryExport);
            }
        }
        ui.add_space(18.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Close study").clicked() {
                *event = Some(StudyUiEvent::Close);
            }
        });
    }

    fn render_distance_result_rows(&self, ui: &mut egui::Ui) {
        let accepted: Vec<_> = self
            .attempts
            .iter()
            .filter(|attempt| attempt.phase == StudyPhase::DistanceComparison && attempt.accepted)
            .collect();
        if accepted.is_empty() {
            return;
        }
        ui.label(RichText::new("Measured so far").strong());
        egui::Grid::new("continuous-distance-results").num_columns(3).spacing([24.0, 6.0]).show(
            ui,
            |ui| {
                ui.strong("100% distance");
                ui.strong("Measured");
                ui.strong("Error from 25%");
                ui.end_row();
                for attempt in accepted {
                    ui.label(format!("{} DIP", attempt.requested_full_scale_radius_dip));
                    ui.label(format_optional_percent(attempt.observed_value));
                    ui.label(format_optional_percent(attempt.absolute_scalar_error));
                    ui.end_row();
                }
            },
        );
    }

    fn render_distance_results(&self, ui: &mut egui::Ui, event: &mut Option<StudyUiEvent>) {
        ui.heading("Comparison complete");
        ui.label("All four attempts used the same direction and 25% target.");
        ui.add_space(14.0);
        self.render_distance_result_rows(ui);
        ui.add_space(18.0);
        if self.preferred_full_scale_radius_dip.is_none() {
            ui.label(RichText::new("Which distance felt best?").size(18.0).strong());
            ui.horizontal_wrapped(|ui| {
                for candidate in DISTANCE_CANDIDATES_DIP {
                    if ui.button(format!("{candidate} DIP")).clicked() {
                        *event = Some(StudyUiEvent::ChooseDistance(candidate));
                    }
                }
            });
        } else if let Some(path) = &self.report_path {
            ui.label(
                RichText::new(format!(
                    "Preferred distance: {} DIP",
                    self.preferred_full_scale_radius_dip.unwrap_or_default()
                ))
                .strong(),
            );
            ui.label("Comparison report saved locally:");
            ui.monospace(path.display().to_string());
        } else if self.export_pending {
            ui.label("Saving the comparison report…");
        } else {
            ui.colored_label(Color32::from_rgb(239, 113, 113), "The report could not be saved.");
            if ui.button("Retry export").clicked() {
                *event = Some(StudyUiEvent::RetryExport);
            }
        }
        ui.add_space(18.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Close comparison").clicked() {
                *event = Some(StudyUiEvent::Close);
            }
        });
    }
}

#[derive(Debug)]
pub(crate) struct ContinuousStudy {
    state: Arc<Mutex<StudyState>>,
    current: Arc<Mutex<Option<StudyInvocationSetup>>>,
    observations: Receiver<StudyObservation>,
    sink: ContinuousStudySink,
    binding: ActivationBinding,
    report_directory: PathBuf,
}

impl ContinuousStudy {
    pub(crate) fn new(
        enabled: bool,
        distance_comparison: bool,
        binding: ActivationBinding,
        report_directory: PathBuf,
    ) -> Self {
        let (sender, observations) = bounded(STUDY_EVENT_CAPACITY);
        let current = Arc::new(Mutex::new(None));
        Self {
            state: Arc::new(Mutex::new(StudyState::new(enabled, distance_comparison))),
            current: Arc::clone(&current),
            observations,
            sink: ContinuousStudySink { current, sender },
            binding,
            report_directory,
        }
    }

    pub(crate) fn sink(&self) -> ContinuousStudySink {
        self.sink.clone()
    }

    pub(crate) fn is_open(&self) -> bool {
        self.state().stage != StudyStage::Closed
    }

    pub(crate) fn settings_open_blocked(&self) {
        self.state().feedback =
            Some("Finish or close the study before opening Settings or onboarding.".to_owned());
    }

    pub(crate) fn update(&self, context: &egui::Context) {
        let mut changed = false;
        while let Ok(observation) = self.observations.try_recv() {
            self.state().record(observation);
            changed = true;
        }

        let event = self.state().pending_event.take();
        match event {
            Some(StudyUiEvent::Start) => {
                self.state().start();
                changed = true;
                log::info!(
                    target: "study",
                    "component=continuous_study event_code=study_started result_code=ok \
                     config_revision=0 participant_identity_collected=false"
                );
            }
            Some(StudyUiEvent::RetryExport) => {
                self.state().export_pending = true;
                changed = true;
            }
            Some(StudyUiEvent::ChooseDistance(candidate)) => {
                if DISTANCE_CANDIDATES_DIP.contains(&candidate) {
                    let mut state = self.state();
                    state.preferred_full_scale_radius_dip = Some(candidate);
                    state.export_pending = true;
                    state.feedback = None;
                    changed = true;
                }
            }
            Some(StudyUiEvent::Close) => {
                self.state().stage = StudyStage::Closed;
                self.set_current(None);
                context.send_viewport_cmd_to(study_viewport_id(), ViewportCommand::Close);
                return;
            }
            None => {}
        }

        if changed {
            self.publish_current_trial();
            context.request_repaint_of(study_viewport_id());
        }

        if self.state().export_pending {
            self.export_report();
            context.request_repaint_of(study_viewport_id());
        }
    }

    pub(crate) fn show(&self, context: &egui::Context) {
        if !self.is_open() {
            return;
        }
        let state = Arc::clone(&self.state);
        let binding = self.binding;
        let protocol = self.state().protocol;
        context.show_viewport_deferred(
            study_viewport_id(),
            study_viewport_builder(protocol),
            move |ui, viewport_class| {
                if viewport_class == ViewportClass::EmbeddedWindow {
                    log::error!(
                        target: "study",
                        "component=continuous_study event_code=study_viewport_embedded \
                         result_code=failure config_revision=0"
                    );
                }
                let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(event) = state.render(ui, binding) {
                    state.pending_event = Some(event);
                    drop(state);
                    ui.ctx().request_repaint_of(ViewportId::ROOT);
                }
            },
        );
    }

    fn publish_current_trial(&self) {
        let setup = self.state().current_trial().map(|trial| StudyInvocationSetup {
            trial_id: trial.id,
            menu: Arc::new(study_menu(trial)),
            full_scale_radius_dip: f64::from(trial.full_scale_radius_dip),
        });
        self.set_current(setup);
    }

    fn set_current(&self, setup: Option<StudyInvocationSetup>) {
        *self.current.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = setup;
    }

    fn export_report(&self) {
        let protocol = self.state().protocol;
        let (result, schema_version, session_id) = match protocol {
            StudyProtocol::Usability => {
                let report = self.state().report();
                (
                    write_report(
                        &self.report_directory,
                        "continuous-study",
                        report.session_id,
                        &report,
                    ),
                    STUDY_SCHEMA_VERSION,
                    report.session_id,
                )
            }
            StudyProtocol::DistanceComparison => {
                let report = self.state().distance_report();
                match report {
                    Some(report) => (
                        write_report(
                            &self.report_directory,
                            "continuous-distance-study",
                            report.session_id,
                            &report,
                        ),
                        DISTANCE_STUDY_SCHEMA_VERSION,
                        report.session_id,
                    ),
                    None => (
                        Err(anyhow::anyhow!("a preferred distance has not been selected")),
                        DISTANCE_STUDY_SCHEMA_VERSION,
                        self.state().session_id,
                    ),
                }
            }
        };
        match result {
            Ok(path) => {
                let mut state = self.state();
                state.report_path = Some(path);
                state.export_pending = false;
                state.feedback = None;
                log::info!(
                    target: "study",
                    "component=continuous_study event_code=study_report_saved result_code=ok \
                     config_revision=0 schema_version={schema_version} session_id={session_id} \
                     protocol={protocol:?} participant_identity_collected=false"
                );
            }
            Err(error) => {
                let mut state = self.state();
                state.export_pending = false;
                state.feedback = Some(
                    "Gradial could not save the anonymous report. Check the local data folder and retry."
                        .to_owned(),
                );
                log::error!(
                    target: "study",
                    "component=continuous_study event_code=study_report_save_failed \
                     result_code=failure error_code=write_failed config_revision=0 error={error}"
                );
            }
        }
    }

    fn state(&self) -> MutexGuard<'_, StudyState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn study_plan() -> Vec<StudyTrial> {
    let mut trials = Vec::with_capacity(37);
    let mut next_id = 1_u32;
    for (index, target) in TARGETS.into_iter().enumerate() {
        trials.push(StudyTrial {
            id: next_id,
            phase: StudyPhase::Practice,
            slices: SliceCount::Eight,
            target_percent: Some(target),
            direction_index: index * 2,
            change_from_index: None,
            change_after_arm: false,
            placement: StudyPlacement::Center,
            full_scale_radius_dip: 160,
        });
        next_id += 1;
    }

    for (target_index, target) in TARGETS.into_iter().enumerate() {
        for (placement_index, placement) in
            [StudyPlacement::Center, StudyPlacement::Edge, StudyPlacement::Corner]
                .into_iter()
                .enumerate()
        {
            for slices in [SliceCount::Four, SliceCount::Eight] {
                let direction_index = match slices {
                    SliceCount::Four => (target_index + placement_index) % 4,
                    SliceCount::Eight => (target_index * 2 + placement_index) % 8,
                };
                trials.push(StudyTrial {
                    id: next_id,
                    phase: StudyPhase::Scalar,
                    slices,
                    target_percent: Some(target),
                    direction_index,
                    change_from_index: None,
                    change_after_arm: false,
                    placement,
                    full_scale_radius_dip: 160,
                });
                next_id += 1;
            }
        }
    }

    for initial_direction in 0..8 {
        let change_after_arm = initial_direction % 2 == 1;
        trials.push(StudyTrial {
            id: next_id,
            phase: StudyPhase::DirectionChange,
            slices: SliceCount::Eight,
            target_percent: change_after_arm.then_some(50),
            direction_index: (initial_direction + 2) % 8,
            change_from_index: Some(initial_direction),
            change_after_arm,
            placement: StudyPlacement::Center,
            full_scale_radius_dip: 160,
        });
        next_id += 1;
    }
    trials.push(StudyTrial {
        id: next_id,
        phase: StudyPhase::Cancellation,
        slices: SliceCount::Eight,
        target_percent: Some(50),
        direction_index: 2,
        change_from_index: None,
        change_after_arm: false,
        placement: StudyPlacement::Center,
        full_scale_radius_dip: 160,
    });
    trials
}

fn distance_study_plan() -> Vec<StudyTrial> {
    DISTANCE_CANDIDATES_DIP
        .into_iter()
        .enumerate()
        .map(|(index, full_scale_radius_dip)| StudyTrial {
            id: u32::try_from(index + 1).expect("four distance candidates fit u32"),
            phase: StudyPhase::DistanceComparison,
            slices: SliceCount::Eight,
            target_percent: Some(DISTANCE_TARGET_PERCENT),
            direction_index: 2,
            change_from_index: None,
            change_after_arm: false,
            placement: StudyPlacement::Center,
            full_scale_radius_dip,
        })
        .collect()
}

fn study_menu(trial: &StudyTrial) -> ResolvedMenu {
    let count = usize::from(slice_count_value(trial.slices));
    let labels: Arc<[String]> = (0..count)
        .map(|index| match trial.target_percent {
            Some(target) if trial.phase == StudyPhase::DistanceComparison => format!(
                "{} · {target}% · {} DIP",
                direction_label(trial.slices, index),
                trial.full_scale_radius_dip
            ),
            Some(target) => format!("{} · {target}%", direction_label(trial.slices, index)),
            None => direction_label(trial.slices, index).to_owned(),
        })
        .collect::<Vec<_>>()
        .into();
    ResolvedMenu::new(trial.slices, labels, vec![None; count].into())
        .expect("the built-in study menu is complete")
}

fn summarize_attempts(attempts: &[StudyAttemptRecord]) -> StudySummary {
    let accepted: Vec<_> = attempts.iter().filter(|attempt| attempt.accepted).collect();
    let measured: Vec<_> =
        accepted.iter().copied().filter(|attempt| attempt.phase == StudyPhase::Scalar).collect();
    let scalar_errors: Vec<_> =
        measured.iter().filter_map(|attempt| attempt.absolute_scalar_error).collect();
    let eight_direction: Vec<_> =
        measured.iter().copied().filter(|attempt| attempt.requested_slice_count == 8).collect();
    let eight_direction_errors =
        eight_direction.iter().filter(|attempt| attempt.direction_correct == Some(false)).count();
    let within = scalar_errors.iter().filter(|error| **error <= 0.15).count();
    let direction_changes: Vec<_> = accepted
        .iter()
        .copied()
        .filter(|attempt| attempt.phase == StudyPhase::DirectionChange)
        .collect();
    let accidental: Vec<_> = direction_changes
        .iter()
        .copied()
        .filter(|attempt| !attempt.requested_change_after_arm)
        .collect();
    let accidental_arms = accidental.iter().filter(|attempt| attempt.accidentally_armed()).count();
    let armed_direction_switches: Vec<_> = direction_changes
        .iter()
        .copied()
        .filter(|attempt| attempt.requested_change_after_arm)
        .collect();
    let armed_direction_switch_successes = armed_direction_switches
        .iter()
        .filter(|attempt| attempt.armed_direction_switch_succeeded())
        .count();
    let cancellation: Vec<_> = accepted
        .iter()
        .copied()
        .filter(|attempt| attempt.phase == StudyPhase::Cancellation)
        .collect();
    let cancellation_successes =
        cancellation.iter().filter(|attempt| attempt.cancellation_succeeded()).count();
    let center = completion_times(&measured, StudyPlacement::Center);
    let edge = completion_times(&measured, StudyPlacement::Edge);
    let corner = completion_times(&measured, StudyPlacement::Corner);
    let center_median = median(&center);
    let edge_median = median(&edge);
    let corner_median = median(&corner);
    let edge_corner_slowdown_fraction = center_median.and_then(|center_value| {
        if center_value <= f64::EPSILON {
            None
        } else {
            edge_median
                .into_iter()
                .chain(corner_median)
                .reduce(f64::max)
                .map(|outer| outer / center_value - 1.0)
        }
    });

    StudySummary {
        accepted_trials: accepted.len(),
        measured_scalar_samples: measured.len(),
        eight_direction_samples: eight_direction.len(),
        eight_direction_errors,
        eight_direction_error_rate: fraction(eight_direction_errors, eight_direction.len()),
        median_absolute_scalar_error: median(&scalar_errors),
        samples_within_0_15: within,
        fraction_within_0_15: fraction(within, scalar_errors.len()),
        accidental_arming_trials: accidental.len(),
        accidental_arms,
        accidental_arming_rate: fraction(accidental_arms, accidental.len()),
        armed_direction_switch_trials: armed_direction_switches.len(),
        armed_direction_switch_successes,
        armed_direction_switch_success_rate: fraction(
            armed_direction_switch_successes,
            armed_direction_switches.len(),
        ),
        cancellation_trials: cancellation.len(),
        cancellation_successes,
        center_median_completion_ms: center_median,
        edge_median_completion_ms: edge_median,
        corner_median_completion_ms: corner_median,
        edge_corner_slowdown_fraction,
    }
}

fn completion_times(attempts: &[&StudyAttemptRecord], placement: StudyPlacement) -> Vec<f64> {
    attempts
        .iter()
        .filter(|attempt| attempt.requested_placement == placement)
        .map(|attempt| f64::from(attempt.duration_ms))
        .collect()
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let midpoint = ordered.len() / 2;
    if ordered.len().is_multiple_of(2) {
        Some(f64::midpoint(ordered[midpoint - 1], ordered[midpoint]))
    } else {
        Some(ordered[midpoint])
    }
}

fn fraction(numerator: usize, denominator: usize) -> Option<f64> {
    let numerator = u32::try_from(numerator).ok()?;
    let denominator = u32::try_from(denominator).ok()?;
    (denominator != 0).then(|| f64::from(numerator) / f64::from(denominator))
}

fn progress_fraction(completed: usize, total: usize) -> f32 {
    let completed = u16::try_from(completed).unwrap_or(u16::MAX);
    let total = u16::try_from(total).unwrap_or(u16::MAX);
    if total == 0 { 0.0 } else { f32::from(completed) / f32::from(total) }
}

fn classify_placement(cursor: ScreenPoint, work_area: ScreenRect, dpi: u32) -> StudyPlacement {
    let threshold =
        i32::try_from((32_u64 * u64::from(dpi.max(96))).div_ceil(96)).unwrap_or(i32::MAX);
    let near_horizontal = cursor.x.saturating_sub(work_area.left).abs() <= threshold
        || cursor.x.saturating_sub(work_area.right).abs() <= threshold;
    let near_vertical = cursor.y.saturating_sub(work_area.top).abs() <= threshold
        || cursor.y.saturating_sub(work_area.bottom).abs() <= threshold;
    match (near_horizontal, near_vertical) {
        (true, true) => StudyPlacement::Corner,
        (true, false) | (false, true) => StudyPlacement::Edge,
        (false, false) => StudyPlacement::Center,
    }
}

fn write_report<T: Serialize>(
    directory: &Path,
    prefix: &str,
    session_id: Uuid,
    report: &T,
) -> Result<PathBuf> {
    fs::create_dir_all(directory).context("could not create the study-report directory")?;
    let destination = directory.join(format!("{prefix}-{session_id}.json"));
    let temporary = directory.join(format!(".{prefix}-{session_id}.tmp"));
    if destination.exists() || temporary.exists() {
        bail!("the anonymous study report already exists");
    }
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .context("could not create the temporary study report")?;
        serde_json::to_writer_pretty(&mut file, report)
            .context("could not serialize the study report")?;
        file.write_all(b"\n").context("could not finish the study report")?;
        file.sync_all().context("could not flush the study report")?;
        drop(file);
        let temporary_wide = wide_path(&temporary);
        let destination_wide = wide_path(&destination);
        // SAFETY: both buffers are live, null-terminated paths in the same
        // Gradial-owned directory. The unique destination is never replaced.
        unsafe {
            MoveFileExW(
                PCWSTR::from_raw(temporary_wide.as_ptr()),
                PCWSTR::from_raw(destination_wide.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        }
        .context("could not atomically publish the study report")?;
        Ok(())
    })();
    if result.is_err() {
        let _removed = fs::remove_file(&temporary);
    }
    result.map(|()| destination)
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

const fn study_privacy() -> StudyPrivacy {
    StudyPrivacy {
        identity: "random_session_id_only",
        excluded_data: [
            "participant_name",
            "raw_cursor_coordinates",
            "application_identity",
            "file_paths",
            "window_details",
            "monitor_device_names",
        ],
    }
}

fn direction_label(slices: SliceCount, index: usize) -> &'static str {
    match slices {
        SliceCount::Four => ["N", "E", "S", "W"].get(index).copied().unwrap_or("?"),
        SliceCount::Eight => {
            ["N", "NE", "E", "SE", "S", "SW", "W", "NW"].get(index).copied().unwrap_or("?")
        }
    }
}

const fn slice_count_value(slices: SliceCount) -> u8 {
    match slices {
        SliceCount::Four => 4,
        SliceCount::Eight => 8,
    }
}

fn cancel_reason_name(reason: CancelReason) -> &'static str {
    match reason {
        CancelReason::Escape => "escape",
        CancelReason::OtherMouseButton => "other_mouse_button",
        CancelReason::NoSelection => "no_selection",
        CancelReason::ReturnedToCenter => "returned_to_center",
        CancelReason::InputQueueOverflow => "input_queue_overflow",
        CancelReason::HookUnresponsive => "hook_unresponsive",
        CancelReason::InputReregistration => "input_reregistration",
        CancelReason::DisplayTopologyChange => "display_topology_change",
        CancelReason::Shutdown => "shutdown",
    }
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
    parts.push(binding.button.short_label());
    parts.join(" + ")
}

fn summary_row(ui: &mut egui::Ui, label: &str, value: String) {
    ui.label(label);
    ui.label(RichText::new(value).strong());
    ui.end_row();
}

fn format_optional_percent(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("{:.1}%", value * 100.0))
}

pub(crate) fn study_viewport_id() -> ViewportId {
    ViewportId::from_hash_of("gradial-continuous-study")
}

fn study_viewport_builder(protocol: StudyProtocol) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_app_id("gradial-continuous-study")
        .with_title(protocol.heading())
        .with_inner_size(STUDY_SIZE)
        .with_min_inner_size(STUDY_MIN_SIZE)
        .with_resizable(true)
        .with_decorations(true)
        .with_taskbar(true)
        .with_active(true)
        .with_visible(true)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gradial_core::{CancelReason, ScreenPoint, ScreenRect, SliceCount};

    use super::{
        DISTANCE_CANDIDATES_DIP, DISTANCE_TARGET_PERCENT, StudyAttemptRecord, StudyObservation,
        StudyPhase, StudyPlacement, StudyTerminal, classify_placement, distance_study_plan,
        study_plan, summarize_attempts, write_report,
    };

    fn observation(
        trial_id: u32,
        placement: StudyPlacement,
        terminal: StudyTerminal,
    ) -> StudyObservation {
        StudyObservation { trial_id, placement, monitor_dpi: 144, duration_ms: 100, terminal }
    }

    #[test]
    fn plan_balances_required_conditions_and_keeps_practice_separate() {
        let plan = study_plan();
        assert_eq!(plan.len(), 37);
        assert_eq!(plan.iter().filter(|trial| trial.phase == StudyPhase::Practice).count(), 4);
        assert_eq!(plan.iter().filter(|trial| trial.phase == StudyPhase::Scalar).count(), 24);
        assert_eq!(
            plan.iter().filter(|trial| trial.phase == StudyPhase::DirectionChange).count(),
            8
        );
        assert_eq!(
            plan.iter()
                .filter(|trial| trial.phase == StudyPhase::DirectionChange && trial.change_after_arm)
                .count(),
            4
        );
        assert_eq!(plan.iter().filter(|trial| trial.phase == StudyPhase::Cancellation).count(), 1);
        for placement in [StudyPlacement::Center, StudyPlacement::Edge, StudyPlacement::Corner] {
            for slices in [SliceCount::Four, SliceCount::Eight] {
                let targets: Vec<_> = plan
                    .iter()
                    .filter(|trial| {
                        trial.phase == StudyPhase::Scalar
                            && trial.placement == placement
                            && trial.slices == slices
                    })
                    .filter_map(|trial| trial.target_percent)
                    .collect();
                assert_eq!(targets, vec![25, 50, 75, 100]);
            }
        }
    }

    #[test]
    fn placement_classification_uses_dpi_scaled_edges_without_exporting_coordinates() {
        let work_area = ScreenRect::new(-1920, -200, 0, 880);
        assert_eq!(
            classify_placement(ScreenPoint::new(-960, 340), work_area, 144),
            StudyPlacement::Center
        );
        assert_eq!(
            classify_placement(ScreenPoint::new(-1919, 340), work_area, 144),
            StudyPlacement::Edge
        );
        assert_eq!(
            classify_placement(ScreenPoint::new(-1919, -199), work_area, 144),
            StudyPlacement::Corner
        );
    }

    #[test]
    fn summary_excludes_practice_and_rejected_attempts() {
        let plan = study_plan();
        let scalar = plan.iter().find(|trial| trial.phase == StudyPhase::Scalar).expect("scalar");
        let accidental = plan
            .iter()
            .find(|trial| trial.phase == StudyPhase::DirectionChange && !trial.change_after_arm)
            .expect("pre-arm direction change");
        let armed_switch = plan
            .iter()
            .find(|trial| trial.phase == StudyPhase::DirectionChange && trial.change_after_arm)
            .expect("armed direction change");
        let cancellation = plan
            .iter()
            .find(|trial| trial.phase == StudyPhase::Cancellation)
            .expect("cancellation");
        let attempts = vec![
            StudyAttemptRecord::from_observation(
                scalar,
                observation(
                    scalar.id,
                    scalar.placement,
                    StudyTerminal::Committed {
                        slice_index: scalar.direction_index,
                        value: f64::from(scalar.target_percent.unwrap_or_default()) / 100.0 + 0.04,
                    },
                ),
            ),
            StudyAttemptRecord::from_observation(
                accidental,
                observation(
                    accidental.id,
                    accidental.placement,
                    StudyTerminal::DiscreteSelected { slice_index: accidental.direction_index },
                ),
            ),
            StudyAttemptRecord::from_observation(
                armed_switch,
                observation(
                    armed_switch.id,
                    armed_switch.placement,
                    StudyTerminal::Committed {
                        slice_index: armed_switch.direction_index,
                        value: 0.5,
                    },
                ),
            ),
            StudyAttemptRecord::from_observation(
                cancellation,
                observation(
                    cancellation.id,
                    cancellation.placement,
                    StudyTerminal::Cancelled { reason: CancelReason::NoSelection },
                ),
            ),
        ];
        let summary = summarize_attempts(&attempts);
        assert_eq!(summary.measured_scalar_samples, 1);
        assert_eq!(summary.accidental_arms, 0);
        assert_eq!(summary.armed_direction_switch_successes, 1);
        assert_eq!(summary.cancellation_successes, 1);
        assert!(summary.median_absolute_scalar_error.is_some_and(|error| error < 0.041));
    }

    #[test]
    fn complete_guided_session_advances_every_trial_and_builds_a_complete_report() {
        let mut state = super::StudyState::new(true, false);
        state.start();
        let plan = state.plan.clone();
        for trial in &plan {
            let terminal = match trial.phase {
                StudyPhase::Practice | StudyPhase::Scalar => StudyTerminal::Committed {
                    slice_index: trial.direction_index,
                    value: f64::from(trial.target_percent.unwrap_or_default()) / 100.0,
                },
                StudyPhase::DirectionChange if !trial.change_after_arm => {
                    StudyTerminal::DiscreteSelected { slice_index: trial.direction_index }
                }
                StudyPhase::DirectionChange => {
                    StudyTerminal::Committed { slice_index: trial.direction_index, value: 0.5 }
                }
                StudyPhase::Cancellation => {
                    StudyTerminal::Cancelled { reason: CancelReason::NoSelection }
                }
                StudyPhase::DistanceComparison => unreachable!("usability plan has no comparison"),
            };
            state.record(observation(trial.id, trial.placement, terminal));
        }

        let report = state.report();
        assert_eq!(state.stage, super::StudyStage::Results);
        assert!(state.export_pending);
        assert_eq!(report.summary.accepted_trials, 37);
        assert_eq!(report.summary.measured_scalar_samples, 24);
        assert_eq!(report.summary.accidental_arms, 0);
        assert_eq!(report.summary.armed_direction_switch_successes, 4);
        assert_eq!(report.summary.cancellation_successes, 1);
    }

    #[test]
    fn distance_comparison_freezes_four_endpoints_and_requires_a_preference() {
        let plan = distance_study_plan();
        assert_eq!(plan.len(), DISTANCE_CANDIDATES_DIP.len());
        assert_eq!(
            plan.iter().map(|trial| trial.full_scale_radius_dip).collect::<Vec<_>>(),
            DISTANCE_CANDIDATES_DIP
        );
        assert!(plan.iter().all(|trial| {
            trial.phase == StudyPhase::DistanceComparison
                && trial.slices == SliceCount::Eight
                && trial.direction_index == 2
                && trial.target_percent == Some(DISTANCE_TARGET_PERCENT)
                && trial.placement == StudyPlacement::Center
        }));

        let mut state = super::StudyState::new(true, true);
        state.start();
        for (index, trial) in plan.iter().enumerate() {
            state.record(observation(
                trial.id,
                StudyPlacement::Center,
                StudyTerminal::Committed {
                    slice_index: 2,
                    value: 0.25 + f64::from(u32::try_from(index).expect("small index")) / 100.0,
                },
            ));
        }
        assert_eq!(state.stage, super::StudyStage::Results);
        assert!(!state.export_pending, "preference must be chosen before export");
        assert!(state.distance_report().is_none());

        state.preferred_full_scale_radius_dip = Some(250);
        let report = state.distance_report().expect("distance report");
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.results.len(), 4);
        assert_eq!(report.preferred_full_scale_radius_dip, 250);
        assert_eq!(report.results[0].full_scale_radius_dip, 160);
        assert!((report.results[3].observed_value - 0.28).abs() < f64::EPSILON);
    }

    #[test]
    fn report_is_atomic_and_contains_no_identity_or_raw_position_fields() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let state = super::StudyState::new(true, false);
        let report = state.report();
        let path = write_report(directory.path(), "continuous-study", report.session_id, &report)
            .expect("study report");
        let json = fs::read_to_string(path).expect("report JSON");
        for prohibited in [
            "participant_name_collected",
            "cursor_physical_px",
            "executable_path",
            "window_title",
            "\"monitor_device_name\":",
        ] {
            assert!(!json.contains(prohibited), "found prohibited field {prohibited}");
        }
        assert!(json.contains("\"identity\": \"random_session_id_only\""));
        assert!(json.contains("gradial-continuous-usability-v3"));
        assert!(directory.path().read_dir().expect("report directory").all(|entry| {
            entry
                .expect("report entry")
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        }));
    }
}
