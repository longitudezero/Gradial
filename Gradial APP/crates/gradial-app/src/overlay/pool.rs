use std::{
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Instant,
};

use anyhow::{Context as _, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use eframe::egui::{Context, ViewportBuilder, ViewportClass, ViewportId};
use gradial_core::{
    Appearance, ContinuousSelection, ContinuousVisualState, InvocationSnapshot, MonitorGeometry,
    Point, ResolvedGeometry, ResolvedMenu, ScreenPoint, continuous_arm_radius_for_size,
};
use uuid::Uuid;

use crate::{
    diagnostics::TimingMarkers,
    focus,
    icons::IconManager,
    platform::{self, OverlayWakeHandle},
};

use super::{
    AnimationMode, FoundationOverlayRenderer, OVERLAY_SIZE_DIP, OverlayRenderFrame,
    OverlayRenderer,
    animation::{CLOSE_DURATION, PresenceFrame, closing_frame, opening_frame},
};

const REQUIRED_PREWARM_PASSES: u8 = 3;
const CLOSE_COORDINATOR_CAPACITY: usize = 128;

#[derive(Clone, Copy, Debug, Default)]
enum VisualPhase {
    #[default]
    Hidden,
    Opening {
        started_at: Instant,
    },
    Visible,
    Closing {
        started_at: Instant,
    },
}

#[derive(Debug, Default)]
struct OverlayViewportState {
    native_handle: Option<OverlayWakeHandle>,
    native_passes: u8,
    ready: bool,
    snapshot: Option<InvocationSnapshot>,
    menu: Option<Arc<ResolvedMenu>>,
    selected_slice: Option<usize>,
    continuous_selection: Option<ContinuousSelection>,
    continuous_pointer: Option<Point>,
    phase: VisualPhase,
    pending_open_marker: bool,
    pending_first_frame: Option<InvocationSnapshot>,
    pending_highlight_marker: Option<(Uuid, Option<usize>)>,
    pending_continuous_marker: Option<(Uuid, ContinuousVisualState)>,
}

impl OverlayViewportState {
    fn reset_visuals(&mut self) -> Option<InvocationSnapshot> {
        let snapshot = self.snapshot.take();
        self.menu = None;
        self.selected_slice = None;
        self.continuous_selection = None;
        self.continuous_pointer = None;
        self.phase = VisualPhase::Hidden;
        self.pending_open_marker = false;
        self.pending_first_frame = None;
        self.pending_highlight_marker = None;
        self.pending_continuous_marker = None;
        snapshot
    }
}

#[derive(Debug)]
struct OverlaySlot {
    viewport_id: ViewportId,
    title: String,
    monitor: MonitorGeometry,
    state: Arc<Mutex<OverlayViewportState>>,
    renderer: Arc<Mutex<FoundationOverlayRenderer>>,
}

/// One hidden, initialized overlay viewport for every active display.
#[derive(Debug)]
pub(crate) struct OverlayPool {
    monitors: Vec<MonitorGeometry>,
    slots: Vec<OverlaySlot>,
    animation_mode: AnimationMode,
    appearance: Appearance,
    config_revision: u64,
    timing_markers: Arc<TimingMarkers>,
    close_coordinator: CloseCoordinator,
}

impl OverlayPool {
    pub(crate) fn new(
        generation: u64,
        monitors: Vec<MonitorGeometry>,
        animation_mode: AnimationMode,
        appearance: Appearance,
        icons: &Arc<IconManager>,
        config_revision: u64,
        timing_markers: Arc<TimingMarkers>,
    ) -> Self {
        let process_id = std::process::id();
        let close_coordinator = CloseCoordinator::start(Arc::clone(&timing_markers));
        let slots: Vec<_> = monitors
            .iter()
            .copied()
            .enumerate()
            .map(|(index, monitor)| {
                let icons = Arc::clone(icons);
                OverlaySlot {
                    viewport_id: ViewportId::from_hash_of(("gradial-overlay", generation, index)),
                    title: format!("Gradial Overlay {process_id}-{generation}-{index}"),
                    monitor,
                    state: Arc::new(Mutex::new(OverlayViewportState::default())),
                    renderer: Arc::new(Mutex::new(FoundationOverlayRenderer::new(icons))),
                }
            })
            .collect();
        let continuous_arm_radius_dip = continuous_arm_radius_for_size(appearance.size_preset);
        log::info!(
            target: "renderer",
            "component=renderer event_code=renderer_geometry_cache_ready result_code=ok \
             config_revision={config_revision} generation={generation} monitor_count={} \
             slice_counts=4,8 \
             reduced_motion={} accent_red={} accent_green={} accent_blue={} \
             label_theme={} size_preset={} cached_icon_count={} \
             open_ms=60 selection_ms=35 close_ms=50 \
             continuous_arm_radius_dip={} continuous_full_scale_radius_dip=160 \
             overlay_size_dip=400",
            slots.len(),
            animation_mode.is_reduced(),
            appearance.accent_color.red,
            appearance.accent_color.green,
            appearance.accent_color.blue,
            appearance.label_theme.as_str(),
            appearance.size_preset.as_str(),
            icons.cached_count(),
            continuous_arm_radius_dip
        );

        Self {
            monitors,
            slots,
            animation_mode,
            appearance,
            config_revision,
            timing_markers,
            close_coordinator,
        }
    }

    pub(crate) fn monitors(&self) -> &[MonitorGeometry] {
        &self.monitors
    }

    pub(crate) fn set_configuration(&mut self, config_revision: u64, appearance: Appearance) {
        self.config_revision = config_revision;
        self.appearance = appearance;
        log::info!(
            target: "renderer",
            "component=renderer event_code=renderer_appearance_updated result_code=ok \
             config_revision={config_revision} accent_red={} accent_green={} accent_blue={} \
             label_theme={} size_preset={}",
            appearance.accent_color.red,
            appearance.accent_color.green,
            appearance.accent_color.blue,
            appearance.label_theme.as_str(),
            appearance.size_preset.as_str()
        );
    }

    pub(crate) fn show(&self, context: &Context) {
        for (index, slot) in self.slots.iter().enumerate() {
            let viewport_id = slot.viewport_id;
            let title = slot.title.clone();
            let monitor = slot.monitor;
            let state = Arc::clone(&slot.state);
            let renderer = Arc::clone(&slot.renderer);
            let timing_markers = Arc::clone(&self.timing_markers);
            let animation_mode = self.animation_mode;
            let appearance = self.appearance;
            let config_revision = self.config_revision;
            let builder = viewport_builder(&title);

            context.show_viewport_deferred(viewport_id, builder, move |ui, viewport_class| {
                if viewport_class == ViewportClass::EmbeddedWindow {
                    log::error!(
                        target: "renderer",
                        "component=renderer event_code=overlay_viewport_embedded \
                         result_code=failure config_revision={config_revision} \
                         monitor_index={index}"
                    );
                    return;
                }

                prewarm_viewport(
                    ui.ctx(),
                    viewport_id,
                    &title,
                    monitor,
                    index,
                    config_revision,
                    &state,
                );
                let prepared = prepare_overlay_frame(
                    ui.ctx(),
                    viewport_id,
                    &state,
                    animation_mode,
                    appearance,
                    &timing_markers,
                );
                if let Some(repaint_after) = prepared.repaint_after {
                    ui.ctx().request_repaint_after_for(repaint_after, viewport_id);
                }
                let presence_animating = prepared.repaint_after.is_some();

                renderer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .render(ui, prepared.render);
                if presence_animating {
                    request_native_animation_frame(&state, config_revision, index);
                }
                if let Some(completed_close) = prepared.completed_close {
                    finish_close(completed_close, &timing_markers);
                }
            });
        }
    }

    pub(crate) fn routes(&self) -> Option<Vec<OverlayRoute>> {
        self.slots
            .iter()
            .map(|slot| {
                let state = slot.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let native_handle = state.native_handle.filter(|_| state.ready)?;
                Some(OverlayRoute {
                    viewport_id: slot.viewport_id,
                    monitor: slot.monitor,
                    native_handle,
                    animation_mode: self.animation_mode,
                    config_revision: self.config_revision,
                    close_sender: self.close_coordinator.sender.clone(),
                    state: Arc::clone(&slot.state),
                })
            })
            .collect()
    }
}

impl Drop for OverlayPool {
    fn drop(&mut self) {
        self.close_coordinator.stop();
        for slot in &self.slots {
            let completed_close = {
                let mut state =
                    slot.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let native_handle = state.native_handle;
                let hide_requested = native_handle.map(|handle| request_native_hide(handle, true));
                let snapshot = state.reset_visuals();
                native_handle.map(|native_handle| CompletedClose {
                    native_handle,
                    snapshot,
                    forced: true,
                    hide_requested,
                })
            };
            if let Some(completed_close) = completed_close {
                finish_close(completed_close, &self.timing_markers);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OverlayRoute {
    viewport_id: ViewportId,
    monitor: MonitorGeometry,
    native_handle: OverlayWakeHandle,
    animation_mode: AnimationMode,
    config_revision: u64,
    close_sender: Sender<CloseCommand>,
    state: Arc<Mutex<OverlayViewportState>>,
}

impl OverlayRoute {
    pub(crate) const fn monitor(&self) -> MonitorGeometry {
        self.monitor
    }

    pub(crate) fn open(
        &self,
        context: &Context,
        timing_markers: &TimingMarkers,
        snapshot: &InvocationSnapshot,
        menu: Arc<ResolvedMenu>,
    ) -> Result<()> {
        let started_at = Instant::now();
        let invocation_id = snapshot.invocation_id;
        let qpc_timestamp = snapshot.qpc_timestamp;
        let overlay_origin = snapshot.geometry.overlay_origin_physical_px;
        let interrupted_snapshot = {
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let interrupted = state
                .snapshot
                .as_ref()
                .filter(|active| active.invocation_id != invocation_id)
                .cloned();
            state.snapshot = Some(snapshot.clone());
            state.menu = Some(menu);
            state.selected_slice = None;
            state.continuous_selection = None;
            state.continuous_pointer = None;
            state.phase = VisualPhase::Opening { started_at };
            state.pending_open_marker = true;
            state.pending_first_frame = None;
            state.pending_highlight_marker = None;
            state.pending_continuous_marker = None;
            interrupted
        };
        if let Some(interrupted_snapshot) = interrupted_snapshot {
            finish_close(
                CompletedClose {
                    native_handle: self.native_handle,
                    snapshot: Some(interrupted_snapshot),
                    forced: true,
                    hide_requested: None,
                },
                timing_markers,
            );
        }
        focus::activation(snapshot);
        platform::request_overlay_wake(self.native_handle, overlay_origin)
            .inspect_err(|_| {
                self.clear_immediately(invocation_id);
            })
            .context("could not wake the routed display overlay")?;
        timing_markers.emit("overlay_wake_requested", Some(invocation_id), qpc_timestamp, "");
        timing_markers.emit(
            "open_animation_started",
            Some(invocation_id),
            qpc_timestamp,
            &format!("duration_ms=60 reduced_motion={}", self.animation_mode.is_reduced()),
        );
        log::debug!(
            target: "renderer",
            "component=renderer event_code=open_animation_started result_code=ok \
             config_revision={} invocation_id={} duration_ms=60 reduced_motion={}",
            snapshot.context.config_revision,
            invocation_id,
            self.animation_mode.is_reduced()
        );
        self.request_repaint(context, "overlay_open_repaint_wake_failed", invocation_id);
        Ok(())
    }

    pub(crate) fn set_selection(
        &self,
        context: &Context,
        invocation_id: Uuid,
        selected_slice: Option<usize>,
    ) {
        let applied = {
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.invocation_id == invocation_id)
            {
                state.selected_slice = selected_slice;
                state.continuous_selection = None;
                state.continuous_pointer = None;
                state.pending_highlight_marker = Some((invocation_id, selected_slice));
                true
            } else {
                false
            }
        };
        if applied {
            self.request_repaint(context, "selection_repaint_wake_failed", invocation_id);
        }
    }

    pub(crate) fn set_continuous(
        &self,
        context: &Context,
        invocation_id: Uuid,
        visual: ContinuousVisualState,
    ) {
        let applied = {
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let edge_assist = state
                .snapshot
                .as_ref()
                .filter(|snapshot| snapshot.invocation_id == invocation_id)
                .map(|snapshot| snapshot.geometry.behavior == ResolvedGeometry::EdgeAssist);
            if let Some(edge_assist) = edge_assist {
                state.selected_slice = Some(visual.selection.slice_index);
                state.continuous_selection = visual.armed.then_some(visual.selection);
                state.continuous_pointer = edge_assist.then_some(visual.pointer_offset_dip);
                state.pending_continuous_marker = Some((invocation_id, visual));
                true
            } else {
                false
            }
        };
        if applied {
            self.request_repaint(context, "continuous_repaint_wake_failed", invocation_id);
        }
    }

    pub(crate) fn close(
        &self,
        context: &Context,
        timing_markers: &TimingMarkers,
        invocation_id: Uuid,
    ) {
        let started_at = Instant::now();
        let scheduled = {
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.invocation_id == invocation_id)
            {
                state.phase = VisualPhase::Closing { started_at };
                true
            } else {
                false
            }
        };
        if scheduled {
            if let Err(error) = self.close_sender.try_send(CloseCommand::Schedule(ScheduledClose {
                deadline: started_at + CLOSE_DURATION,
                invocation_id,
                native_handle: self.native_handle,
                state: Arc::clone(&self.state),
            })) {
                log::error!(
                    target: "renderer",
                    "component=renderer event_code=close_watchdog_schedule_failed \
                     result_code=failure config_revision={} invocation_id={invocation_id} \
                     error={error}",
                    self.config_revision
                );
            }
            timing_markers.emit(
                "close_animation_started",
                Some(invocation_id),
                0,
                &format!("duration_ms=50 reduced_motion={}", self.animation_mode.is_reduced()),
            );
            log::debug!(
                target: "renderer",
                "component=renderer event_code=close_animation_started result_code=ok \
                 config_revision={} invocation_id={invocation_id} duration_ms=50 \
                 reduced_motion={}",
                self.config_revision,
                self.animation_mode.is_reduced()
            );
            self.request_repaint(context, "close_repaint_wake_failed", invocation_id);
        }
    }

    fn request_repaint(&self, context: &Context, failure_event_code: &str, invocation_id: Uuid) {
        context.request_repaint_of(self.viewport_id);
        if let Err(error) = platform::request_overlay_repaint(self.native_handle) {
            log::error!(
                target: "renderer",
                "component=renderer event_code={failure_event_code} result_code=failure \
                 config_revision={} invocation_id={invocation_id} error={error}",
                self.config_revision,
            );
        }
    }

    fn clear_immediately(&self, invocation_id: Uuid) -> Option<InvocationSnapshot> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.snapshot.as_ref().is_some_and(|snapshot| snapshot.invocation_id == invocation_id) {
            state.reset_visuals()
        } else {
            None
        }
    }
}

#[derive(Debug)]
enum CloseCommand {
    Schedule(ScheduledClose),
    Stop,
}

#[derive(Debug)]
struct ScheduledClose {
    deadline: Instant,
    invocation_id: Uuid,
    native_handle: OverlayWakeHandle,
    state: Arc<Mutex<OverlayViewportState>>,
}

#[derive(Debug)]
struct CloseCoordinator {
    sender: Sender<CloseCommand>,
    join_handle: Option<JoinHandle<()>>,
}

impl CloseCoordinator {
    fn start(timing_markers: Arc<TimingMarkers>) -> Self {
        let (sender, receiver) = bounded(CLOSE_COORDINATOR_CAPACITY);
        let join_handle = match thread::Builder::new()
            .name("gradial-overlay-close".to_owned())
            .spawn(move || run_close_coordinator(&receiver, &timing_markers))
        {
            Ok(join_handle) => Some(join_handle),
            Err(error) => {
                log::error!(
                    target: "renderer",
                    "component=renderer event_code=close_watchdog_start_failed \
                     result_code=failure config_revision=0 error={error}"
                );
                None
            }
        };
        Self { sender, join_handle }
    }

    fn stop(&mut self) {
        if self.join_handle.is_none() {
            return;
        }
        if let Err(error) = self.sender.send(CloseCommand::Stop) {
            log::warn!(
                target: "renderer",
                "component=renderer event_code=close_watchdog_stop_signal_failed \
                 result_code=failure config_revision=0 error={error}"
            );
        }
        if let Some(join_handle) = self.join_handle.take()
            && join_handle.join().is_err()
        {
            log::error!(
                target: "renderer",
                "component=renderer event_code=close_watchdog_panicked result_code=failure \
                 config_revision=0"
            );
        }
    }
}

impl Drop for CloseCoordinator {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_close_coordinator(receiver: &Receiver<CloseCommand>, timing_markers: &TimingMarkers) {
    let mut pending: Vec<ScheduledClose> = Vec::new();
    loop {
        let command_result =
            if let Some(next_deadline) = pending.iter().map(|close| close.deadline).min() {
                receiver.recv_timeout(next_deadline.saturating_duration_since(Instant::now()))
            } else {
                match receiver.recv() {
                    Ok(command) => Ok(command),
                    Err(_) => break,
                }
            };

        match command_result {
            Ok(CloseCommand::Schedule(scheduled)) => {
                pending.retain(|existing| !Arc::ptr_eq(&existing.state, &scheduled.state));
                pending.push(scheduled);
            }
            Ok(CloseCommand::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }

        let now = Instant::now();
        let mut index = 0;
        while index < pending.len() {
            if pending[index].deadline > now {
                index += 1;
                continue;
            }
            let scheduled = pending.swap_remove(index);
            complete_scheduled_close(&scheduled, timing_markers);
        }
    }
}

fn complete_scheduled_close(scheduled: &ScheduledClose, timing_markers: &TimingMarkers) {
    let completion = {
        let mut state = scheduled.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let matching = state
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.invocation_id == scheduled.invocation_id)
            && matches!(state.phase, VisualPhase::Closing { .. });
        if !matching {
            return;
        }
        let hidden = request_native_hide(scheduled.native_handle, false);
        let snapshot = state.reset_visuals();
        snapshot.map(|snapshot| {
            (
                CompletedClose {
                    native_handle: scheduled.native_handle,
                    snapshot: Some(snapshot),
                    forced: false,
                    hide_requested: Some(hidden),
                },
                hidden,
            )
        })
    };

    if let Some((completed, hidden)) = completion {
        record_finished_close(completed, hidden, timing_markers);
    }
}

#[derive(Clone, Debug)]
struct PreparedOverlayFrame {
    render: OverlayRenderFrame,
    repaint_after: Option<std::time::Duration>,
    completed_close: Option<CompletedClose>,
}

#[derive(Clone, Debug)]
struct CompletedClose {
    native_handle: OverlayWakeHandle,
    snapshot: Option<InvocationSnapshot>,
    forced: bool,
    hide_requested: Option<bool>,
}

fn prepare_overlay_frame(
    context: &Context,
    viewport_id: ViewportId,
    state: &Mutex<OverlayViewportState>,
    animation_mode: AnimationMode,
    appearance: Appearance,
    timing_markers: &TimingMarkers,
) -> PreparedOverlayFrame {
    let now = Instant::now();
    let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let phase = state.phase;
    let presence = match phase {
        VisualPhase::Opening { started_at } => {
            let frame = opening_frame(now.saturating_duration_since(started_at), animation_mode);
            if frame.complete {
                state.phase = VisualPhase::Visible;
                if let Some(snapshot) = state.snapshot.as_ref() {
                    timing_markers.emit(
                        "open_animation_completed",
                        Some(snapshot.invocation_id),
                        0,
                        "duration_ms=60",
                    );
                    log::info!(
                        target: "renderer",
                        "component=renderer event_code=open_animation_completed result_code=ok \
                         config_revision={} invocation_id={} duration_ms=60",
                        snapshot.context.config_revision,
                        snapshot.invocation_id
                    );
                }
            }
            frame
        }
        VisualPhase::Closing { started_at } => {
            closing_frame(now.saturating_duration_since(started_at), animation_mode)
        }
        VisualPhase::Hidden | VisualPhase::Visible => {
            PresenceFrame { opacity: 1.0, scale: 1.0, complete: true, repaint_after: None }
        }
    };

    let completed_close = if matches!(phase, VisualPhase::Closing { .. }) && presence.complete {
        let native_handle = state.native_handle;
        let hide_requested = native_handle.map(|handle| request_native_hide(handle, false));
        let snapshot = state.reset_visuals();
        native_handle.map(|native_handle| CompletedClose {
            native_handle,
            snapshot,
            forced: false,
            hide_requested,
        })
    } else {
        process_pending_markers(context, viewport_id, &mut state, timing_markers);
        None
    };

    PreparedOverlayFrame {
        render: OverlayRenderFrame {
            menu: state.menu.clone(),
            activation_button: state
                .snapshot
                .as_ref()
                .map_or(gradial_core::ActivationBindingId::default(), |snapshot| {
                    snapshot.context.activation_binding
                }),
            appearance,
            selected_slice: state.selected_slice,
            continuous_selection: state.continuous_selection,
            continuous_pointer: state.continuous_pointer,
            opacity: presence.opacity,
            scale: presence.scale,
        },
        repaint_after: presence.repaint_after,
        completed_close,
    }
}

fn process_pending_markers(
    context: &Context,
    viewport_id: ViewportId,
    state: &mut OverlayViewportState,
    timing_markers: &TimingMarkers,
) {
    if let Some(snapshot) = state.pending_first_frame.take()
        && state.snapshot.as_ref() == Some(&snapshot)
    {
        timing_markers.emit(
            "first_frame_checkpoint",
            Some(snapshot.invocation_id),
            0,
            "correlate_with_presentmon=true",
        );
        focus::checkpoint(&snapshot, "after_first_frame");
    }
    if state.pending_open_marker {
        state.pending_open_marker = false;
        if let (Some(snapshot), Some(menu)) = (state.snapshot.as_ref(), state.menu.as_ref()) {
            timing_markers.emit(
                "overlay_shown",
                Some(snapshot.invocation_id),
                snapshot.qpc_timestamp,
                &format!(
                    "geometry={} slice_count={} label_count={}",
                    snapshot.geometry.behavior.as_str(),
                    menu.slice_count().as_str(),
                    menu.labels().len()
                ),
            );
            log::info!(
                target: "renderer",
                "component=renderer event_code=profile_menu_frame_ready result_code=ok \
                 config_revision={} invocation_id={} slice_count={} label_count={}",
                snapshot.context.config_revision,
                snapshot.invocation_id,
                menu.slice_count().as_str(),
                menu.labels().len()
            );
            state.pending_first_frame = Some(snapshot.clone());
            context.request_repaint_of(viewport_id);
        }
    }
    if let Some((invocation_id, selected_slice)) = state.pending_highlight_marker.take() {
        timing_markers.emit(
            "highlight_applied",
            Some(invocation_id),
            0,
            &format!("selected_slice={selected_slice:?} transition_ms=35"),
        );
    }
    if let Some((invocation_id, visual)) = state.pending_continuous_marker.take() {
        timing_markers.emit(
            "continuous_value_applied",
            Some(invocation_id),
            0,
            &format!(
                "slice_index={} normalized_value={:.6} armed={}",
                visual.selection.slice_index, visual.selection.value, visual.armed
            ),
        );
    }
}

fn finish_close(completed: CompletedClose, timing_markers: &TimingMarkers) {
    let hidden = completed
        .hide_requested
        .unwrap_or_else(|| request_native_hide(completed.native_handle, completed.forced));
    record_finished_close(completed, hidden, timing_markers);
}

fn request_native_hide(native_handle: OverlayWakeHandle, forced: bool) -> bool {
    match platform::request_overlay_hide(native_handle) {
        Ok(()) => true,
        Err(error) => {
            log::error!(
                target: "renderer",
                "component=renderer event_code=overlay_hide_request_failed result_code=failure \
                 config_revision=0 forced={forced} error={error}"
            );
            false
        }
    }
}

fn request_native_animation_frame(
    state: &Mutex<OverlayViewportState>,
    config_revision: u64,
    monitor_index: usize,
) {
    let native_handle =
        state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).native_handle;
    let Some(native_handle) = native_handle else {
        return;
    };
    if let Err(error) = platform::request_overlay_repaint(native_handle) {
        log::error!(
            target: "renderer",
            "component=renderer event_code=animation_repaint_wake_failed result_code=failure \
             config_revision={config_revision} monitor_index={monitor_index} error={error}"
        );
    }
}

fn record_finished_close(completed: CompletedClose, hidden: bool, timing_markers: &TimingMarkers) {
    if let Some(snapshot) = completed.snapshot {
        timing_markers.emit(
            "overlay_hidden",
            Some(snapshot.invocation_id),
            0,
            if completed.forced {
                "close_animation_completed=false forced=true"
            } else {
                "close_animation_completed=true duration_ms=50"
            },
        );
        if hidden {
            let duration_ms = if completed.forced { 0 } else { 50 };
            log::info!(
                target: "renderer",
                "component=renderer event_code=close_animation_completed result_code=ok \
                 config_revision=0 invocation_id={} duration_ms={duration_ms} forced={}",
                snapshot.invocation_id,
                completed.forced
            );
        }
        focus::checkpoint(&snapshot, "after_close");
    }
}

fn viewport_builder(title: &str) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([OVERLAY_SIZE_DIP, OVERLAY_SIZE_DIP])
        .with_min_inner_size([OVERLAY_SIZE_DIP, OVERLAY_SIZE_DIP])
        .with_max_inner_size([OVERLAY_SIZE_DIP, OVERLAY_SIZE_DIP])
        .with_decorations(false)
        .with_resizable(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_mouse_passthrough(true)
        .with_active(false)
        .with_visible(false)
}

fn prewarm_viewport(
    context: &Context,
    viewport_id: ViewportId,
    title: &str,
    monitor: MonitorGeometry,
    monitor_index: usize,
    config_revision: u64,
    state: &Mutex<OverlayViewportState>,
) {
    let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.ready {
        return;
    }

    let Some(handle) = state.native_handle.or_else(|| platform::overlay_window_by_title(title))
    else {
        context.request_repaint_of(viewport_id);
        return;
    };
    state.native_handle = Some(handle);

    let center = rect_center(monitor.work_area);
    let configured = platform::configure_overlay_handle(handle)
        .and_then(|()| platform::position_hidden_overlay(handle, center));
    if let Err(error) = configured {
        log::error!(
            target: "renderer",
            "component=renderer event_code=overlay_prewarm_failed result_code=failure \
             config_revision={config_revision} monitor_index={monitor_index} error={error}"
        );
        context.request_repaint_of(viewport_id);
        return;
    }

    state.native_passes = state.native_passes.saturating_add(1);
    let actual_dpi = platform::overlay_window_dpi(handle);
    if state.native_passes >= REQUIRED_PREWARM_PASSES && actual_dpi == monitor.dpi.max(96) {
        state.ready = true;
        log::info!(
            target: "renderer",
            "component=renderer event_code=overlay_pool_slot_ready result_code=ok \
             config_revision={config_revision} monitor_index={monitor_index} dpi={actual_dpi}"
        );
        context.request_repaint_of(ViewportId::ROOT);
    } else {
        context.request_repaint_of(viewport_id);
    }
}

const fn rect_center(rectangle: gradial_core::ScreenRect) -> ScreenPoint {
    ScreenPoint::new(
        rectangle.left.saturating_add(rectangle.right).div_euclid(2),
        rectangle.top.saturating_add(rectangle.bottom).div_euclid(2),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gradial_core::{ScreenPoint, ScreenRect};

    use super::{VisualPhase, rect_center};

    #[test]
    fn monitor_center_preserves_negative_virtual_screen_coordinates() {
        assert_eq!(
            rect_center(ScreenRect::new(-2_560, -1_400, 0, 0)),
            ScreenPoint::new(-1_280, -700)
        );
    }

    #[test]
    fn visual_phase_carries_monotonic_animation_start() {
        let started_at = std::time::Instant::now();
        let phase = VisualPhase::Closing { started_at };
        let VisualPhase::Closing { started_at: observed } = phase else {
            panic!("closing phase");
        };
        assert!(observed.elapsed() < Duration::from_secs(1));
    }
}
