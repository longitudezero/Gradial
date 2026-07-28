mod pool;
mod renderer;

pub(crate) use pool::{OverlayPool, OverlayRoute};
pub(crate) use renderer::{FoundationOverlayRenderer, OverlayRenderer};

pub(super) const OVERLAY_SIZE_DIP: f32 = 288.0;

pub(crate) fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id("gradial")
            .with_title("Gradial Overlay")
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
            .with_visible(false),
        renderer: eframe::Renderer::Wgpu,
        centered: true,
        persist_window: false,
        ..Default::default()
    }
}
