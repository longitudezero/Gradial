use std::sync::Arc;

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::{
    diagnostics::TimingMarkers,
    focus::FocusProbe,
    input::InputConfiguration,
    interaction::{InteractionRuntime, OverlayCommand},
    options::StartupOptions,
    overlay::{FoundationOverlayRenderer, OverlayRenderer},
    platform,
};

pub(crate) struct GradialApp {
    renderer: FoundationOverlayRenderer,
    interaction: InteractionRuntime,
    native_style_passes: u8,
    timing_markers: Arc<TimingMarkers>,
    focus_probe: FocusProbe,
    first_frame_pending: Option<Uuid>,
}

impl GradialApp {
    const REQUIRED_NATIVE_STYLE_PASSES: u8 = 2;

    pub(crate) fn new(
        creation_context: &eframe::CreationContext<'_>,
        startup_options: StartupOptions,
        timing_markers: Arc<TimingMarkers>,
    ) -> Result<Self> {
        platform::configure_overlay_window(creation_context)?;
        let overlay_wake_handle = platform::overlay_wake_handle(creation_context)?;
        let monitors =
            platform::monitor_work_areas().context("could not read the display work areas")?;
        log::info!(
            target: "startup",
            "component=app event_code=input_geometry_ready result_code=ok config_revision=0 \
             geometry_mode={} monitor_count={}",
            startup_options.geometry_mode.as_str(),
            monitors.len()
        );
        let interaction = InteractionRuntime::start(
            creation_context.egui_ctx.clone(),
            InputConfiguration { geometry_mode: startup_options.geometry_mode, monitors },
            Arc::clone(&timing_markers),
            overlay_wake_handle,
        )?;
        creation_context.egui_ctx.request_repaint();

        Ok(Self {
            renderer: FoundationOverlayRenderer::default(),
            interaction,
            native_style_passes: 0,
            timing_markers,
            focus_probe: FocusProbe::default(),
            first_frame_pending: None,
        })
    }
}

impl eframe::App for GradialApp {
    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, context: &eframe::egui::Context, frame: &mut eframe::Frame) {
        if let Some(invocation_id) = self.first_frame_pending.take() {
            self.timing_markers.emit(
                "first_frame_checkpoint",
                Some(invocation_id),
                0,
                "correlate_with_presentmon=true",
            );
            self.focus_probe.after_first_frame(invocation_id);
        }

        if self.native_style_passes < Self::REQUIRED_NATIVE_STYLE_PASSES {
            if let Err(error) = platform::enforce_overlay_window_styles(frame) {
                log::error!(
                    target: "renderer",
                    "component=renderer event_code=renderer_unavailable result_code=failure \
                     config_revision=0 error={error}"
                );
                context.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                return;
            }

            self.native_style_passes += 1;
            if self.native_style_passes < Self::REQUIRED_NATIVE_STYLE_PASSES {
                context.request_repaint();
            } else {
                // eframe deliberately shows its root window after the first
                // completed paint, even when the viewport requested hidden.
                // Reassert Gradial's idle state after that initialization pass.
                if let Err(error) = platform::hide_overlay(frame) {
                    log::error!(
                        target: "renderer",
                        "component=renderer event_code=initial_overlay_hide_failed \
                         result_code=failure config_revision=0 error={error}"
                    );
                    context.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                    return;
                }
                log::info!(
                    target: "renderer",
                    "component=renderer event_code=overlay_ready result_code=ok config_revision=0"
                );
            }
        }

        while let Some(command) = self.interaction.try_command() {
            let result = match command {
                OverlayCommand::Open { snapshot } => {
                    self.renderer.set_selected_slice(None);
                    self.focus_probe.begin(snapshot);
                    platform::position_and_show_overlay(
                        frame,
                        snapshot.geometry.overlay_origin_physical_px,
                    )
                    .map(|()| {
                        self.timing_markers.emit(
                            "overlay_shown",
                            Some(snapshot.invocation_id),
                            snapshot.qpc_timestamp,
                            &format!("geometry={}", snapshot.geometry.behavior.as_str()),
                        );
                        self.first_frame_pending = Some(snapshot.invocation_id);
                        context.request_repaint();
                    })
                }
                OverlayCommand::SelectionChanged { invocation_id, selected_slice } => {
                    self.renderer.set_selected_slice(selected_slice);
                    self.timing_markers.emit(
                        "highlight_applied",
                        Some(invocation_id),
                        0,
                        &format!("selected_slice={selected_slice:?}"),
                    );
                    Ok(())
                }
                OverlayCommand::Close { invocation_id } => {
                    self.renderer.set_selected_slice(None);
                    self.first_frame_pending = None;
                    platform::hide_overlay(frame).map(|()| {
                        self.timing_markers.emit("overlay_hidden", Some(invocation_id), 0, "");
                        self.focus_probe.after_close(invocation_id);
                    })
                }
            };

            if let Err(error) = result {
                log::error!(
                    target: "renderer",
                    "component=renderer event_code=overlay_command_failed result_code=failure \
                     config_revision=0 error={error}"
                );
                context.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                return;
            }
        }
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        // eframe makes its root window visible after the first rendered frame.
        // Keep that initialization frame fully transparent; the second logic
        // pass re-hides the native window before Gradial paints menu content.
        if self.native_style_passes >= Self::REQUIRED_NATIVE_STYLE_PASSES {
            self.renderer.render(ui);
        }
    }
}
