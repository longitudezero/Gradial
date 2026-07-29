use std::{
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Instant,
};

use anyhow::{Context as _, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use eframe::egui::{Context, ViewportBuilder, ViewportClass, ViewportId};
use gradial_core::{InvocationSnapshot, MonitorGeometry, ScreenPoint, SliceCount};
use uuid::Uuid;

use crate::{
    diagnostics::TimingMarkers,
    focus,
    platform::{self, OverlayWakeHandle},
};

use super::{
    AnimationMode, FoundationOverlayRenderer, OVERLAY_SIZE_DIP, OverlayRenderFrame,
    OverlayRenderer,
    animation::{CLOSE_DURATION, FRAME_INTERVAL, PresenceFrame, closing_frame, opening_frame},
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
    selected_slice: Option<usize>,
    selection_changed_at: Option<Instant>,
    phase: VisualPhase,
    pending_open_marker: bool,
    pending_first_frame: Option<InvocationSnapshot>,
    pending_highlight_marker: Option<(Uuid, Option<usize>)>,
}

impl OverlayViewportState {
    fn reset_visuals(&mut self) -> Option<InvocationSnapshot> {
        let snapshot = self.snapshot.take();
        self.selected_slice = None;
        self.selection_changed_at = None;
        self.phase = VisualPhase::Hidden;
        self.pending_open_marker = false;
        self.pending_first_frame = None;
        self.pending_highlight_marker = None;
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
    timing_markers: Arc<TimingMarkers>,
    close_coordinator: CloseCoordinator,
}

impl OverlayPool {
    pub(crate) fn new(
        generation: u64,
        monitors: Vec<MonitorGeometry>,
        slice_count: SliceCount,
        animation_mode: AnimationMode,
        timing_markers: Arc<TimingMarkers>,
    ) -> Self {
        let process_id = std::process::id();
        let close_coordinator = CloseCoordinator::start(Arc::clone(&timing_markers));
        let slots: Vec<_> = monitors
            .iter()
            .copied()
            .enumerate()
            .map(|(index, monitor)| OverlaySlot {
                viewport_id: ViewportId::from_hash_of(("gradial-overlay", generation, index)),
                title: format!("Gradial Overlay {process_id}-{generation}-{index}"),
                monitor,
                state: Arc::new(Mutex::new(OverlayViewportState::default())),
                renderer: Arc::new(Mutex::new(FoundationOverlayRenderer::new(slice_count))),
            })
            .collect();
        log::info!(
            target: "renderer",
            "component=renderer event_code=renderer_geometry_cache_ready result_code=ok \
             config_revision=0 generation={generation} monitor_count={} slice_count={} \
             reduced_motion={} open_ms=60 selection_ms=35 close_ms=50",
            slots.len(),
            slice_count.as_str(),
            animation_mode.is_reduced()
        );

        Self { monitors, slots, animation_mode, timing_markers, close_coordinator }
    }

    pub(crate) fn monitors(&self) -> &[MonitorGeometry] {
        &self.monitors
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
            let builder = viewport_builder(&title);

            context.show_viewport_deferred(viewport_id, builder, move |ui, viewport_class| {
                if viewport_class == ViewportClass::EmbeddedWindow {
                    log::error!(
                        target: "renderer",
                        "component=renderer event_code=overlay_viewport_embedded \
                         result_code=failure config_revision=0 monitor_index={index}"
                    );
                    return;
                }

                prewarm_viewport(ui.ctx(), viewport_id, &title, monitor, index, &state);
                let prepared = prepare_overlay_frame(
                    ui.ctx(),
                    viewport_id,
                    &state,
                    animation_mode,
                    &timing_markers,
                );
                if let Some(repaint_after) = prepared.repaint_after {
                    ui.ctx().request_repaint_after_for(repaint_after, viewport_id);
                }

                let selection_animating = renderer
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .render(ui, prepared.render);
                if selection_animating {
                    ui.ctx().request_repaint_after_for(FRAME_INTERVAL, viewport_id);
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
        snapshot: InvocationSnapshot,
    ) -> Result<()> {
        let started_at = Instant::now();
        let interrupted_snapshot = {
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let interrupted =
                state.snapshot.filter(|active| active.invocation_id != snapshot.invocation_id);
            state.snapshot = Some(snapshot);
            state.selected_slice = None;
            state.selection_changed_at = Some(started_at);
            state.phase = VisualPhase::Opening { started_at };
            state.pending_open_marker = true;
            state.pending_first_frame = None;
            state.pending_highlight_marker = None;
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
        platform::request_overlay_wake(
            self.native_handle,
            snapshot.geometry.overlay_origin_physical_px,
        )
        .inspect_err(|_| {
            self.clear_immediately(snapshot.invocation_id);
        })
        .context("could not wake the routed display overlay")?;
        timing_markers.emit(
            "overlay_wake_requested",
            Some(snapshot.invocation_id),
            snapshot.qpc_timestamp,
            "",
        );
        timing_markers.emit(
            "open_animation_started",
            Some(snapshot.invocation_id),
            snapshot.qpc_timestamp,
            &format!("duration_ms=60 reduced_motion={}", self.animation_mode.is_reduced()),
        );
        log::debug!(
            target: "renderer",
            "component=renderer event_code=open_animation_started result_code=ok \
             config_revision=0 invocation_id={} duration_ms=60 reduced_motion={}",
            snapshot.invocation_id,
            self.animation_mode.is_reduced()
        );
        context.request_repaint_of(self.viewport_id);
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
            if state.snapshot.is_some_and(|snapshot| snapshot.invocation_id == invocation_id) {
                state.selected_slice = selected_slice;
                state.selection_changed_at = Some(Instant::now());
                state.pending_highlight_marker = Some((invocation_id, selected_slice));
                true
            } else {
                false
            }
        };
        if applied {
            context.request_repaint_of(self.viewport_id);
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
            if state.snapshot.is_some_and(|snapshot| snapshot.invocation_id == invocation_id) {
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
                     result_code=failure config_revision=0 invocation_id={invocation_id} \
                     error={error}"
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
                 config_revision=0 invocation_id={invocation_id} duration_ms=50 \
                 reduced_motion={}",
                self.animation_mode.is_reduced()
            );
            context.request_repaint_of(self.viewport_id);
            if let Err(error) = platform::request_overlay_repaint(self.native_handle) {
                log::error!(
                    target: "renderer",
                    "component=renderer event_code=close_repaint_wake_failed \
                     result_code=failure config_revision=0 invocation_id={invocation_id} \
                     error={error}"
                );
            }
        }
    }

    fn clear_immediately(&self, invocation_id: Uuid) -> Option<InvocationSnapshot> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.snapshot.is_some_and(|snapshot| snapshot.invocation_id == invocation_id) {
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

#[derive(Clone, Copy, Debug)]
struct PreparedOverlayFrame {
    render: OverlayRenderFrame,
    repaint_after: Option<std::time::Duration>,
    completed_close: Option<CompletedClose>,
}

#[derive(Clone, Copy, Debug)]
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
                if let Some(snapshot) = state.snapshot {
                    timing_markers.emit(
                        "open_animation_completed",
                        Some(snapshot.invocation_id),
                        0,
                        "duration_ms=60",
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
            invocation_id: state.snapshot.map(|snapshot| snapshot.invocation_id),
            selected_slice: state.selected_slice,
            selection_changed_at: state.selection_changed_at,
            opacity: presence.opacity,
            scale: presence.scale,
            animation_mode,
            now,
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
        && state.snapshot == Some(snapshot)
    {
        timing_markers.emit(
            "first_frame_checkpoint",
            Some(snapshot.invocation_id),
            0,
            "correlate_with_presentmon=true",
        );
        focus::checkpoint(snapshot, "after_first_frame");
    }
    if state.pending_open_marker {
        state.pending_open_marker = false;
        if let Some(snapshot) = state.snapshot {
            timing_markers.emit(
                "overlay_shown",
                Some(snapshot.invocation_id),
                snapshot.qpc_timestamp,
                &format!("geometry={}", snapshot.geometry.behavior.as_str()),
            );
            state.pending_first_frame = Some(snapshot);
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
        focus::checkpoint(snapshot, "after_close");
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
             config_revision=0 monitor_index={monitor_index} error={error}"
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
             config_revision=0 monitor_index={monitor_index} dpi={actual_dpi}"
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
