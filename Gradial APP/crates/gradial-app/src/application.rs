use anyhow::Result;

use crate::{
    overlay::{FoundationOverlayRenderer, OverlayRenderer},
    platform,
};

pub(crate) struct GradialApp {
    renderer: FoundationOverlayRenderer,
    native_style_passes: u8,
}

impl GradialApp {
    const REQUIRED_NATIVE_STYLE_PASSES: u8 = 2;

    pub(crate) fn new(creation_context: &eframe::CreationContext<'_>) -> Result<Self> {
        platform::configure_overlay_window(creation_context)?;

        Ok(Self { renderer: FoundationOverlayRenderer, native_style_passes: 0 })
    }
}

impl eframe::App for GradialApp {
    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        if self.native_style_passes < Self::REQUIRED_NATIVE_STYLE_PASSES {
            if let Err(error) = platform::enforce_overlay_window_styles(frame) {
                log::error!(
                    target: "renderer",
                    "component=renderer event_code=renderer_unavailable result_code=failure \
                     config_revision=0 error={error}"
                );
                ui.ctx().send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                return;
            }

            self.native_style_passes += 1;
            if self.native_style_passes < Self::REQUIRED_NATIVE_STYLE_PASSES {
                ui.ctx().request_repaint();
            } else {
                log::info!(
                    target: "renderer",
                    "component=renderer event_code=overlay_ready result_code=ok config_revision=0"
                );
            }
        }

        self.renderer.render(ui);
    }
}
