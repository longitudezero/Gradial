use std::sync::Arc;

use anyhow::{Context, Result};
use gradial_core::{ActionPlanCompiler, GeometryMode, ProfileResolver, ResolvedMenu};

use crate::{
    action_executor::ActionExecutor,
    diagnostics::TimingMarkers,
    input::InputConfiguration,
    interaction::{InteractionRuntime, RuntimeEvent},
    options::StartupOptions,
    overlay::{AnimationMode, OverlayPool},
    platform,
    tray::{TrayCommand, TrayHealth, TrayIcon},
};

pub(crate) struct GradialApp {
    geometry_mode: GeometryMode,
    debug_menu_override: Option<Arc<ResolvedMenu>>,
    animation_mode: AnimationMode,
    overlay_pool: OverlayPool,
    interaction: Option<InteractionRuntime>,
    action_executor: ActionExecutor,
    native_style_passes: u8,
    pool_generation: u64,
    rebuild_in_progress: bool,
    input_recovery_in_progress: bool,
    manual_input_fault: bool,
    timing_markers: Arc<TimingMarkers>,
    profile_resolver: Arc<ProfileResolver>,
    action_plan_compiler: Arc<ActionPlanCompiler>,
    config_revision: u64,
    tray_icon: TrayIcon,
}

impl GradialApp {
    const REQUIRED_NATIVE_STYLE_PASSES: u8 = 2;

    pub(crate) fn new(
        creation_context: &eframe::CreationContext<'_>,
        startup_options: StartupOptions,
        timing_markers: Arc<TimingMarkers>,
        profile_resolver: Arc<ProfileResolver>,
        action_plan_compiler: Arc<ActionPlanCompiler>,
        config_revision: u64,
    ) -> Result<Self> {
        platform::set_display_topology_monitoring(false);
        platform::configure_overlay_window(creation_context)?;
        let tray_icon = TrayIcon::install(platform::controller_window_address())?;
        let action_executor = ActionExecutor::start(config_revision)?;
        let monitors =
            platform::monitor_work_areas().context("could not read the display work areas")?;
        #[cfg(debug_assertions)]
        let debug_menu_override =
            startup_options.debug_slice_override.map(ResolvedMenu::directional).map(Arc::new);
        #[cfg(not(debug_assertions))]
        let debug_menu_override = None;
        #[cfg(debug_assertions)]
        let debug_slice_override =
            startup_options.debug_slice_override.map_or("none", gradial_core::SliceCount::as_str);
        #[cfg(not(debug_assertions))]
        let debug_slice_override = "none";
        log::info!(
            target: "startup",
            "component=app event_code=input_geometry_ready result_code=ok \
             config_revision={config_revision} \
             geometry_mode={} debug_slice_override={} reduced_motion={} monitor_count={}",
            startup_options.geometry_mode.as_str(),
            debug_slice_override,
            startup_options.reduced_motion,
            monitors.len()
        );
        let animation_mode = if startup_options.reduced_motion {
            AnimationMode::ReducedMotion
        } else {
            AnimationMode::Standard
        };
        let overlay_pool = OverlayPool::new(
            0,
            monitors,
            animation_mode,
            config_revision,
            Arc::clone(&timing_markers),
        );
        creation_context.egui_ctx.request_repaint();

        Ok(Self {
            geometry_mode: startup_options.geometry_mode,
            debug_menu_override,
            animation_mode,
            overlay_pool,
            interaction: None,
            action_executor,
            native_style_passes: 0,
            pool_generation: 0,
            rebuild_in_progress: false,
            input_recovery_in_progress: false,
            manual_input_fault: false,
            timing_markers,
            profile_resolver,
            action_plan_compiler,
            config_revision,
            tray_icon,
        })
    }

    fn fail(
        context: &eframe::egui::Context,
        event_code: &str,
        error: &anyhow::Error,
        config_revision: u64,
    ) {
        log::error!(
            target: "renderer",
            "component=renderer event_code={event_code} result_code=failure \
             config_revision={config_revision} error={error}"
        );
        context.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
    }

    fn begin_display_rebuild(&mut self, context: &eframe::egui::Context) {
        if self.interaction.is_none() {
            return;
        }
        platform::set_display_topology_monitoring(false);
        self.set_tray_health(TrayHealth::Recovering);
        let interaction = self.interaction.as_ref().expect("interaction checked above");
        if let Err(error) = interaction.begin_display_rebuild() {
            Self::fail(context, "display_rebuild_request_failed", &error, self.config_revision);
            return;
        }
        self.rebuild_in_progress = true;
    }

    fn replace_pool(&mut self, context: &eframe::egui::Context) {
        self.interaction = None;
        let monitors = match platform::monitor_work_areas() {
            Ok(monitors) => monitors,
            Err(error) => {
                Self::fail(context, "display_enumeration_failed", &error, self.config_revision);
                return;
            }
        };
        self.pool_generation = self.pool_generation.saturating_add(1);
        self.overlay_pool = OverlayPool::new(
            self.pool_generation,
            monitors,
            self.animation_mode,
            self.config_revision,
            Arc::clone(&self.timing_markers),
        );
        self.rebuild_in_progress = false;
        log::info!(
            target: "renderer",
            "component=renderer event_code=overlay_pool_rebuild_started result_code=ok \
             config_revision={} generation={} monitor_count={}",
            self.config_revision,
            self.pool_generation,
            self.overlay_pool.monitors().len()
        );
        context.request_repaint();
    }

    fn set_tray_health(&mut self, health: TrayHealth) {
        if self.tray_icon.health() == health {
            return;
        }
        if let Err(error) = self.tray_icon.set_health(health) {
            log::error!(
                target: "tray",
                "component=tray event_code=tray_health_update_failed result_code=failure \
                 config_revision={} health={health:?} error={error}",
                self.config_revision
            );
        }
    }

    fn begin_input_recovery(&mut self, source: &str) {
        if self.rebuild_in_progress || self.input_recovery_in_progress || self.manual_input_fault {
            return;
        }

        self.set_tray_health(TrayHealth::Recovering);
        if self.interaction.is_none() {
            return;
        }
        let interaction = self.interaction.as_ref().expect("interaction checked above");
        if let Err(error) = interaction.begin_input_recovery() {
            log::error!(
                target: "input",
                "component=input event_code=input_reregistration_signal_failed \
                 result_code=failure config_revision={} source={source} error={error}",
                self.config_revision
            );
            self.interaction = None;
            return;
        }
        self.input_recovery_in_progress = true;
        log::info!(
            target: "input",
            "component=input event_code=input_recovery_started result_code=ok \
             config_revision={} source={source}",
            self.config_revision
        );
    }

    fn handle_runtime_event(&mut self, context: &eframe::egui::Context, event: RuntimeEvent) {
        match event {
            RuntimeEvent::DisplayRebuildReady if self.rebuild_in_progress => {
                self.replace_pool(context);
            }
            RuntimeEvent::InputRecoveryReady => {
                self.set_tray_health(TrayHealth::Recovering);
                self.interaction = None;
                self.input_recovery_in_progress = false;
                self.manual_input_fault = false;
                log::info!(
                    target: "input",
                    "component=input event_code=input_recovery_restart_ready result_code=ok \
                     config_revision={}",
                    self.config_revision
                );
            }
            RuntimeEvent::InputFaulted => {
                self.interaction = None;
                self.input_recovery_in_progress = false;
                self.manual_input_fault = true;
                self.set_tray_health(TrayHealth::Faulted);
                log::error!(
                    target: "input",
                    "component=input event_code=input_fault_requires_retry result_code=failure \
                     config_revision={} reason=queue_overflow",
                    self.config_revision
                );
            }
            RuntimeEvent::DisplayRebuildReady => {
                log::warn!(
                    target: "renderer",
                    "component=renderer event_code=unexpected_display_rebuild_ready \
                     result_code=failure config_revision={}",
                    self.config_revision
                );
            }
        }
    }

    fn retry_input(&mut self) {
        self.manual_input_fault = false;
        self.set_tray_health(TrayHealth::Recovering);
        if self.interaction.is_some() {
            self.begin_input_recovery("tray_retry");
        } else {
            log::info!(
                target: "input",
                "component=input event_code=input_retry_requested result_code=ok \
                 config_revision={} source=tray",
                self.config_revision
            );
        }
    }

    fn start_interaction_if_ready(&mut self, context: &eframe::egui::Context) {
        if self.interaction.is_some()
            || self.rebuild_in_progress
            || self.input_recovery_in_progress
            || self.manual_input_fault
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
                Self::fail(context, "display_enumeration_failed", &error, self.config_revision);
                return;
            }
        };
        if current_monitors != self.overlay_pool.monitors() {
            self.pool_generation = self.pool_generation.saturating_add(1);
            self.overlay_pool = OverlayPool::new(
                self.pool_generation,
                current_monitors,
                self.animation_mode,
                self.config_revision,
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
            Arc::clone(&self.profile_resolver),
            Arc::clone(&self.action_plan_compiler),
            self.action_executor.handle(),
            self.debug_menu_override.clone(),
            self.config_revision,
        ) {
            Ok(interaction) => interaction,
            Err(error) => {
                self.manual_input_fault = true;
                self.set_tray_health(TrayHealth::Faulted);
                log::error!(
                    target: "input",
                    "component=input event_code=interaction_start_failed result_code=failure \
                     config_revision={} error={error}",
                    self.config_revision
                );
                return;
            }
        };
        self.interaction = Some(interaction);
        self.input_recovery_in_progress = false;
        self.manual_input_fault = false;
        self.set_tray_health(TrayHealth::Healthy);
        platform::set_display_topology_monitoring(true);
        log::info!(
            target: "renderer",
            "component=renderer event_code=overlay_pool_ready result_code=ok \
             config_revision={} generation={} monitor_count={}",
            self.config_revision,
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
                Self::fail(context, "renderer_unavailable", &error, self.config_revision);
                return;
            }

            self.native_style_passes += 1;
            if self.native_style_passes < Self::REQUIRED_NATIVE_STYLE_PASSES {
                context.request_repaint();
            } else if let Err(error) = platform::hide_overlay(frame) {
                Self::fail(context, "initial_overlay_hide_failed", &error, self.config_revision);
                return;
            }
        }

        if platform::take_tray_readd_requested()
            && let Err(error) = self.tray_icon.readd()
        {
            log::error!(
                target: "tray",
                "component=tray event_code=tray_icon_restore_failed result_code=failure \
                 config_revision={} error={error}",
                self.config_revision
            );
        }

        if let Some(command) = crate::tray::try_command() {
            match command {
                TrayCommand::RetryInput => self.retry_input(),
                TrayCommand::CancelCurrentAction => {
                    let cancellation_requested = self.action_executor.cancel_current();
                    log::info!(
                        target: "actions",
                        "component=actions event_code=action_cancellation_requested \
                         result_code={} config_revision={} source=tray action_running={}",
                        if cancellation_requested { "ok" } else { "ignored" },
                        self.config_revision,
                        cancellation_requested
                    );
                }
                TrayCommand::ClearActivity => crate::tray::clear_activity(self.config_revision),
                TrayCommand::Exit => {
                    context.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                    return;
                }
            }
        }

        while let Some(completion) = self.action_executor.try_completion() {
            crate::tray::record_action_result(&completion.result, completion.config_revision);
        }
        while let Some(notification) = crate::tray::try_failure_notification() {
            if let Err(error) = self.tray_icon.show_failure_notification(notification) {
                log::error!(
                    target: "tray",
                    "component=tray event_code=action_failure_notification_failed \
                     result_code=failure error_code={} config_revision={} error={error}",
                    notification.error_code().as_str(),
                    self.config_revision
                );
            }
        }

        if let Some(event) = self.interaction.as_ref().and_then(InteractionRuntime::try_event) {
            self.handle_runtime_event(context, event);
        }

        if platform::take_display_topology_changed()
            && !self.rebuild_in_progress
            && !self.input_recovery_in_progress
        {
            self.begin_display_rebuild(context);
        }

        if platform::take_input_reregistration_requested() {
            self.begin_input_recovery("system_notification");
        }

        #[cfg(debug_assertions)]
        if let Some(failure) = platform::take_debug_input_failure()
            && let Some(interaction) = self.interaction.as_ref()
        {
            match failure {
                platform::DebugInputFailure::HookUnresponsive => {
                    interaction.debug_simulate_hook_unresponsive();
                }
                platform::DebugInputFailure::QueueOverflow => {
                    interaction.debug_simulate_queue_overflow();
                }
            }
        }

        self.start_interaction_if_ready(context);
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.overlay_pool.show(ui.ctx());
    }
}
