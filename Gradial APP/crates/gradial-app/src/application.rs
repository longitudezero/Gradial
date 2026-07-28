use std::sync::Arc;

use anyhow::{Context, Result};
use gradial_core::{GeometryMode, SliceCount};

use crate::{
    diagnostics::TimingMarkers,
    input::InputConfiguration,
    interaction::{InteractionRuntime, RuntimeEvent},
    options::StartupOptions,
    overlay::OverlayPool,
    platform,
};

pub(crate) struct GradialApp {
    geometry_mode: GeometryMode,
    slice_count: SliceCount,
    overlay_pool: OverlayPool,
    interaction: Option<InteractionRuntime>,
    native_style_passes: u8,
    pool_generation: u64,
    rebuild_in_progress: bool,
    timing_markers: Arc<TimingMarkers>,
}

impl GradialApp {
    const REQUIRED_NATIVE_STYLE_PASSES: u8 = 2;

    pub(crate) fn new(
        creation_context: &eframe::CreationContext<'_>,
        startup_options: StartupOptions,
        timing_markers: Arc<TimingMarkers>,
    ) -> Result<Self> {
        platform::set_display_topology_monitoring(false);
        platform::configure_overlay_window(creation_context)?;
        let monitors =
            platform::monitor_work_areas().context("could not read the display work areas")?;
        log::info!(
            target: "startup",
            "component=app event_code=input_geometry_ready result_code=ok config_revision=0 \
             geometry_mode={} slice_count={} monitor_count={}",
            startup_options.geometry_mode.as_str(),
            startup_options.slice_count.as_str(),
            monitors.len()
        );
        let overlay_pool =
            OverlayPool::new(0, monitors, startup_options.slice_count, Arc::clone(&timing_markers));
        creation_context.egui_ctx.request_repaint();

        Ok(Self {
            geometry_mode: startup_options.geometry_mode,
            slice_count: startup_options.slice_count,
            overlay_pool,
            interaction: None,
            native_style_passes: 0,
            pool_generation: 0,
            rebuild_in_progress: false,
            timing_markers,
        })
    }

    fn fail(context: &eframe::egui::Context, event_code: &str, error: &anyhow::Error) {
        log::error!(
            target: "renderer",
            "component=renderer event_code={event_code} result_code=failure \
             config_revision=0 error={error}"
        );
        context.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
    }

    fn begin_display_rebuild(&mut self, context: &eframe::egui::Context) {
        let Some(interaction) = self.interaction.as_ref() else {
            return;
        };
        platform::set_display_topology_monitoring(false);
        if let Err(error) = interaction.begin_display_rebuild() {
            Self::fail(context, "display_rebuild_request_failed", &error);
            return;
        }
        self.rebuild_in_progress = true;
    }

    fn replace_pool(&mut self, context: &eframe::egui::Context) {
        self.interaction = None;
        let monitors = match platform::monitor_work_areas() {
            Ok(monitors) => monitors,
            Err(error) => {
                Self::fail(context, "display_enumeration_failed", &error);
                return;
            }
        };
        self.pool_generation = self.pool_generation.saturating_add(1);
        self.overlay_pool = OverlayPool::new(
            self.pool_generation,
            monitors,
            self.slice_count,
            Arc::clone(&self.timing_markers),
        );
        self.rebuild_in_progress = false;
        log::info!(
            target: "renderer",
            "component=renderer event_code=overlay_pool_rebuild_started result_code=ok \
             config_revision=0 generation={} monitor_count={}",
            self.pool_generation,
            self.overlay_pool.monitors().len()
        );
        context.request_repaint();
    }

    fn start_interaction_if_ready(&mut self, context: &eframe::egui::Context) {
        if self.interaction.is_some()
            || self.rebuild_in_progress
            || self.native_style_passes < Self::REQUIRED_NATIVE_STYLE_PASSES
        {
            return;
        }

        let Some(routes) = self.overlay_pool.routes() else {
            return;
        };
        let current_monitors = match platform::monitor_work_areas() {
            Ok(monitors) => monitors,
            Err(error) => {
                Self::fail(context, "display_enumeration_failed", &error);
                return;
            }
        };
        if current_monitors != self.overlay_pool.monitors() {
            self.pool_generation = self.pool_generation.saturating_add(1);
            self.overlay_pool = OverlayPool::new(
                self.pool_generation,
                current_monitors,
                self.slice_count,
                Arc::clone(&self.timing_markers),
            );
            context.request_repaint();
            return;
        }

        let input_configuration =
            InputConfiguration { geometry_mode: self.geometry_mode, monitors: current_monitors };
        let interaction = match InteractionRuntime::start(
            context.clone(),
            input_configuration,
            Arc::clone(&self.timing_markers),
            routes,
            self.slice_count,
        ) {
            Ok(interaction) => interaction,
            Err(error) => {
                Self::fail(context, "interaction_start_failed", &error);
                return;
            }
        };
        self.interaction = Some(interaction);
        platform::set_display_topology_monitoring(true);
        log::info!(
            target: "renderer",
            "component=renderer event_code=overlay_pool_ready result_code=ok \
             config_revision=0 generation={} monitor_count={}",
            self.pool_generation,
            self.overlay_pool.monitors().len()
        );
    }
}

impl eframe::App for GradialApp {
    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, context: &eframe::egui::Context, frame: &mut eframe::Frame) {
        if self.native_style_passes < Self::REQUIRED_NATIVE_STYLE_PASSES {
            if let Err(error) = platform::enforce_overlay_window_styles(frame) {
                Self::fail(context, "renderer_unavailable", &error);
                return;
            }

            self.native_style_passes += 1;
            if self.native_style_passes < Self::REQUIRED_NATIVE_STYLE_PASSES {
                context.request_repaint();
            } else if let Err(error) = platform::hide_overlay(frame) {
                Self::fail(context, "initial_overlay_hide_failed", &error);
                return;
            }
        }

        if platform::take_display_topology_changed() && !self.rebuild_in_progress {
            self.begin_display_rebuild(context);
        }

        if self.rebuild_in_progress
            && self.interaction.as_ref().and_then(InteractionRuntime::try_event)
                == Some(RuntimeEvent::DisplayRebuildReady)
        {
            self.replace_pool(context);
        }

        self.start_interaction_if_ready(context);
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.overlay_pool.show(ui.ctx());
    }
}
