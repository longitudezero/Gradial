use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result};
use eframe::egui::{Context, ViewportBuilder, ViewportClass, ViewportId};
use gradial_core::{InvocationSnapshot, MonitorGeometry, ScreenPoint};
use uuid::Uuid;

use crate::{
    diagnostics::TimingMarkers,
    focus,
    platform::{self, OverlayWakeHandle},
};

use super::{FoundationOverlayRenderer, OVERLAY_SIZE_DIP, OverlayRenderer};

const REQUIRED_PREWARM_PASSES: u8 = 3;

#[derive(Debug, Default)]
struct OverlayViewportState {
    native_handle: Option<OverlayWakeHandle>,
    native_passes: u8,
    ready: bool,
    snapshot: Option<InvocationSnapshot>,
    selected_slice: Option<usize>,
    pending_open_marker: bool,
    pending_first_frame: Option<InvocationSnapshot>,
    pending_highlight_marker: Option<(Uuid, Option<usize>)>,
}

#[derive(Debug)]
struct OverlaySlot {
    viewport_id: ViewportId,
    title: String,
    monitor: MonitorGeometry,
    state: Arc<Mutex<OverlayViewportState>>,
}

/// One hidden, initialized overlay viewport for every active display.
#[derive(Debug)]
pub(crate) struct OverlayPool {
    monitors: Vec<MonitorGeometry>,
    slots: Vec<OverlaySlot>,
    timing_markers: Arc<TimingMarkers>,
}

impl OverlayPool {
    pub(crate) fn new(
        generation: u64,
        monitors: Vec<MonitorGeometry>,
        timing_markers: Arc<TimingMarkers>,
    ) -> Self {
        let process_id = std::process::id();
        let slots = monitors
            .iter()
            .copied()
            .enumerate()
            .map(|(index, monitor)| OverlaySlot {
                viewport_id: ViewportId::from_hash_of(("gradial-overlay", generation, index)),
                title: format!("Gradial Overlay {process_id}-{generation}-{index}"),
                monitor,
                state: Arc::new(Mutex::new(OverlayViewportState::default())),
            })
            .collect();

        Self { monitors, slots, timing_markers }
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
            let timing_markers = Arc::clone(&self.timing_markers);
            let builder = ViewportBuilder::default()
                .with_title(&title)
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
                .with_visible(false);

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

                let selected_slice = {
                    let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
                            ui.ctx().request_repaint_of(viewport_id);
                        }
                    }
                    if let Some((invocation_id, selected_slice)) =
                        state.pending_highlight_marker.take()
                    {
                        timing_markers.emit(
                            "highlight_applied",
                            Some(invocation_id),
                            0,
                            &format!("selected_slice={selected_slice:?}"),
                        );
                    }
                    state.selected_slice
                };

                let mut renderer = FoundationOverlayRenderer::default();
                renderer.set_selected_slice(selected_slice);
                renderer.render(ui);
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
                    state: Arc::clone(&slot.state),
                })
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OverlayRoute {
    viewport_id: ViewportId,
    monitor: MonitorGeometry,
    native_handle: OverlayWakeHandle,
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
        {
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.snapshot = Some(snapshot);
            state.selected_slice = None;
            state.pending_open_marker = true;
            state.pending_first_frame = None;
            state.pending_highlight_marker = None;
        }
        focus::activation(snapshot);
        platform::request_overlay_wake(
            self.native_handle,
            snapshot.geometry.overlay_origin_physical_px,
        )
        .inspect_err(|_| {
            self.clear(snapshot.invocation_id);
        })
        .context("could not wake the routed display overlay")?;
        timing_markers.emit(
            "overlay_wake_requested",
            Some(snapshot.invocation_id),
            snapshot.qpc_timestamp,
            "",
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
    ) -> Result<()> {
        let snapshot = self.clear(invocation_id);
        platform::request_overlay_hide(self.native_handle)
            .context("could not hide the routed display overlay")?;
        timing_markers.emit("overlay_hidden", Some(invocation_id), 0, "");
        if let Some(snapshot) = snapshot {
            focus::checkpoint(snapshot, "after_close");
        }
        context.request_repaint_of(self.viewport_id);
        Ok(())
    }

    fn clear(&self, invocation_id: Uuid) -> Option<InvocationSnapshot> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let snapshot = state.snapshot.filter(|snapshot| snapshot.invocation_id == invocation_id)?;
        state.snapshot = None;
        state.selected_slice = None;
        state.pending_open_marker = false;
        state.pending_first_frame = None;
        state.pending_highlight_marker = None;
        Some(snapshot)
    }
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
    use gradial_core::{ScreenPoint, ScreenRect};

    use super::rect_center;

    #[test]
    fn monitor_center_preserves_negative_virtual_screen_coordinates() {
        assert_eq!(
            rect_center(ScreenRect::new(-2_560, -1_400, 0, 0)),
            ScreenPoint::new(-1_280, -700)
        );
    }
}
