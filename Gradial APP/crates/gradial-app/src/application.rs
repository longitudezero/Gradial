use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
#[cfg(debug_assertions)]
use gradial_core::ActionKind;
use gradial_core::{
    ActionPlanCompiler, Configuration, DEFAULT_OVERLAY_HALF_EXTENT_DIP, GeometryMode,
    GestureConfig, ProfileResolver, ResolvedMenu, continuous_arm_radius_for_size,
};

use crate::{
    action_executor::ActionExecutor,
    configuration::ConfigStore,
    continuous_study::{ContinuousStudy, study_viewport_id},
    diagnostic_bundle::{DiagnosticBundleService, DiagnosticInputStatus},
    diagnostics::TimingMarkers,
    icons::IconManager,
    input::InputConfiguration,
    interaction::{InteractionRuntime, RuntimeEvent},
    onboarding::{
        OnboardingEditor, OnboardingEvent, OnboardingStartError, OnboardingStartup,
        OnboardingStore, onboarding_viewport_id,
    },
    options::{ContinuousMeterMode, StartupOptions},
    overlay::{AnimationMode, OverlayPool},
    platform,
    plugin_worker::{
        PluginWorkerError, PluginWorkerOperation, PluginWorkerRuntime, PluginWorkerSnapshotTicket,
        PluginWorkerTicket,
    },
    settings::{PluginSettingsAction, SettingsEditor, SettingsEvent, settings_viewport_id},
    start_at_login::{StartAtLoginAction, StartAtLoginService},
    tray::{TrayCommand, TrayHealth, TrayIcon},
};

const CONTINUOUS_OVERLAY_MARGIN_DIP: u32 = 20;
const FOUNDATION_OVERLAY_SIZE_DIP: f32 = 400.0;

const fn overlay_half_extent_dip(
    mode: ContinuousMeterMode,
    continuous_full_scale_radius_dip: u32,
) -> u32 {
    if mode.is_enabled() {
        continuous_full_scale_radius_dip.saturating_add(CONTINUOUS_OVERLAY_MARGIN_DIP)
    } else {
        DEFAULT_OVERLAY_HALF_EXTENT_DIP
    }
}

fn overlay_size_dip(mode: ContinuousMeterMode, continuous_full_scale_radius_dip: u32) -> f32 {
    if mode.is_enabled() {
        let half_extent =
            u16::try_from(overlay_half_extent_dip(mode, continuous_full_scale_radius_dip))
                .unwrap_or(u16::MAX);
        f32::from(half_extent) * 2.0
    } else {
        FOUNDATION_OVERLAY_SIZE_DIP
    }
}

#[derive(Debug)]
pub(crate) struct ApplicationServices {
    config_store: ConfigStore,
    diagnostic_bundles: DiagnosticBundleService,
    plugin_worker: PluginWorkerRuntime,
    start_at_login: StartAtLoginService,
    continuous_study_directory: PathBuf,
}

impl ApplicationServices {
    pub(crate) fn new(
        config_store: ConfigStore,
        diagnostic_bundles: DiagnosticBundleService,
        plugin_worker: PluginWorkerRuntime,
    ) -> Self {
        let continuous_study_directory = config_store.continuous_study_directory();
        Self {
            config_store,
            diagnostic_bundles,
            plugin_worker,
            start_at_login: StartAtLoginService::for_current_executable(),
            continuous_study_directory,
        }
    }

    fn into_parts(
        self,
    ) -> (ConfigStore, DiagnosticBundleService, PluginWorkerRuntime, StartAtLoginService, PathBuf)
    {
        (
            self.config_store,
            self.diagnostic_bundles,
            self.plugin_worker,
            self.start_at_login,
            self.continuous_study_directory,
        )
    }
}

fn resolve_debug_menu_options(
    startup_options: StartupOptions,
) -> (Option<Arc<ResolvedMenu>>, &'static str) {
    #[cfg(debug_assertions)]
    {
        let description =
            startup_options.debug_slice_override.map_or("none", gradial_core::SliceCount::as_str);
        let menu =
            startup_options.debug_slice_override.map(ResolvedMenu::directional).map(Arc::new);
        (menu, description)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = startup_options;
        (None, "none")
    }
}

fn create_continuous_study(
    startup_options: StartupOptions,
    binding: gradial_core::ActivationBinding,
    report_directory: PathBuf,
) -> ContinuousStudy {
    ContinuousStudy::new(
        startup_options.debug_continuous_meter.is_study(),
        startup_options.debug_continuous_meter.is_distance_study(),
        binding,
        report_directory,
    )
}

struct LearningTools {
    onboarding: OnboardingEditor,
    continuous_study: ContinuousStudy,
}

#[derive(Debug)]
enum PendingPluginRequest {
    Snapshot {
        ticket: PluginWorkerSnapshotTicket,
        rescan: bool,
    },
    Operation {
        ticket: PluginWorkerTicket,
        operation: PluginWorkerOperation,
        plugin_id: String,
        enable_after_approval: bool,
        session_retry_count: u8,
    },
    AwaitingContinuousCancellation {
        operation: PluginWorkerOperation,
        plugin_id: String,
        enable_after_approval: bool,
        reason: gradial_core::ContinuousCancelReason,
        session_retry_count: u8,
    },
    RetryOperation {
        operation: PluginWorkerOperation,
        plugin_id: String,
        enable_after_approval: bool,
        session_retry_count: u8,
        not_before: Instant,
    },
}

impl LearningTools {
    fn new(
        onboarding_required: bool,
        startup_options: StartupOptions,
        binding: gradial_core::ActivationBinding,
        report_directory: PathBuf,
    ) -> Self {
        Self {
            onboarding: OnboardingEditor::new(onboarding_required, binding),
            continuous_study: create_continuous_study(startup_options, binding, report_directory),
        }
    }
}

fn create_settings_editor(
    recovery_warning: bool,
    icons: &Arc<IconManager>,
    start_at_login: &StartAtLoginService,
) -> SettingsEditor {
    let settings = SettingsEditor::new(recovery_warning, Arc::clone(icons));
    settings.update_start_at_login_status(start_at_login.inspect());
    settings
}

#[cfg(debug_assertions)]
fn debug_plugin_operation_target(configuration: &Configuration) -> Option<String> {
    configuration.actions.iter().find_map(|action| match &action.kind {
        ActionKind::PluginContinuous { plugin_id, .. } => Some(plugin_id.clone()),
        _ => None,
    })
}

pub(crate) struct GradialApp {
    geometry_mode: GeometryMode,
    debug_menu_override: Option<Arc<ResolvedMenu>>,
    debug_continuous_meter: ContinuousMeterMode,
    debug_continuous_full_scale_radius_dip: Option<u32>,
    continuous_full_scale_radius_dip: u32,
    animation_mode: AnimationMode,
    overlay_pool: OverlayPool,
    interaction: Option<InteractionRuntime>,
    action_executor: ActionExecutor,
    plugin_worker: PluginWorkerRuntime,
    pending_plugin_request: Option<PendingPluginRequest>,
    native_style_passes: u8,
    pool_generation: u64,
    rebuild_in_progress: bool,
    input_recovery_in_progress: bool,
    manual_input_fault: bool,
    timing_markers: Arc<TimingMarkers>,
    profile_resolver: Arc<ProfileResolver>,
    action_plan_compiler: Arc<ActionPlanCompiler>,
    configuration: Configuration,
    config_store: ConfigStore,
    diagnostic_bundles: DiagnosticBundleService,
    start_at_login: StartAtLoginService,
    icons: Arc<IconManager>,
    settings: SettingsEditor,
    onboarding_store: OnboardingStore,
    onboarding: OnboardingEditor,
    continuous_study: ContinuousStudy,
    config_revision: u64,
    tray_icon: TrayIcon,
}

impl GradialApp {
    const REQUIRED_NATIVE_STYLE_PASSES: u8 = 2;

    #[allow(clippy::too_many_lines)]
    pub(crate) fn new(
        creation_context: &eframe::CreationContext<'_>,
        startup_options: StartupOptions,
        timing_markers: Arc<TimingMarkers>,
        services: ApplicationServices,
        onboarding_startup: OnboardingStartup,
        configuration: Configuration,
        recovery_warning: bool,
    ) -> Result<Self> {
        let (
            config_store,
            diagnostic_bundles,
            plugin_worker,
            start_at_login,
            continuous_study_directory,
        ) = services.into_parts();
        let config_revision = configuration.config_revision;
        let icons = Arc::new(IconManager::new(config_store.icon_directory()));
        let icon_protection = config_store.icon_protection_snapshot(&configuration, None);
        icons.reconcile_storage(
            &icon_protection.references,
            icon_protection.quarantine_allowed,
            config_revision,
        );
        icons.preload_configuration(&creation_context.egui_ctx, &configuration);
        let profile_resolver = Arc::new(
            ProfileResolver::compile(&configuration)
                .context("could not compile Gradial profiles")?,
        );
        let action_plan_compiler = Arc::new(
            ActionPlanCompiler::from_configuration(&configuration)
                .context("could not compile Gradial action catalog")?,
        );
        platform::set_display_topology_monitoring(false);
        platform::configure_overlay_window(creation_context)?;
        let tray_icon = TrayIcon::install(platform::controller_window_address())?;
        let action_executor = ActionExecutor::start(config_revision)?;
        let monitors =
            platform::monitor_work_areas().context("could not read the display work areas")?;
        let (debug_menu_override, debug_slice_override) =
            resolve_debug_menu_options(startup_options);
        let debug_continuous_full_scale_radius_dip =
            startup_options.debug_continuous_full_scale_radius_dip;
        let continuous_full_scale_radius_dip = debug_continuous_full_scale_radius_dip
            .unwrap_or(configuration.continuous_full_scale_radius_dip);
        let overlay_half_extent_dip = overlay_half_extent_dip(
            startup_options.debug_continuous_meter,
            continuous_full_scale_radius_dip,
        );
        log::info!(
            target: "startup",
            "component=app event_code=input_geometry_ready result_code=ok \
             config_revision={config_revision} \
             geometry_mode={} debug_slice_override={} debug_continuous_meter={} \
             debug_open_settings={} reduced_motion={} overlay_half_extent_dip={} \
             continuous_full_scale_radius_dip={} \
             monitor_count={}",
            startup_options.geometry_mode.as_str(),
            debug_slice_override,
            startup_options.debug_continuous_meter.is_enabled(),
            startup_options.debug_open_settings,
            startup_options.reduced_motion,
            overlay_half_extent_dip,
            continuous_full_scale_radius_dip,
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
            configuration.appearance,
            overlay_size_dip(
                startup_options.debug_continuous_meter,
                continuous_full_scale_radius_dip,
            ),
            &icons,
            config_revision,
            Arc::clone(&timing_markers),
        );
        creation_context.egui_ctx.request_repaint();
        let settings = create_settings_editor(recovery_warning, &icons, &start_at_login);
        let learning_tools = LearningTools::new(
            onboarding_startup.required,
            startup_options,
            configuration.activation_binding,
            continuous_study_directory,
        );
        if startup_options.debug_open_settings {
            settings.open(&configuration);
        }

        let mut app = Self {
            geometry_mode: startup_options.geometry_mode,
            debug_menu_override,
            debug_continuous_meter: startup_options.debug_continuous_meter,
            debug_continuous_full_scale_radius_dip,
            continuous_full_scale_radius_dip,
            animation_mode,
            overlay_pool,
            interaction: None,
            action_executor,
            plugin_worker,
            pending_plugin_request: None,
            native_style_passes: 0,
            pool_generation: 0,
            rebuild_in_progress: false,
            input_recovery_in_progress: false,
            manual_input_fault: false,
            timing_markers,
            profile_resolver,
            action_plan_compiler,
            configuration,
            config_store,
            diagnostic_bundles,
            start_at_login,
            icons,
            settings,
            onboarding_store: onboarding_startup.store,
            onboarding: learning_tools.onboarding,
            continuous_study: learning_tools.continuous_study,
            config_revision,
            tray_icon,
        };
        if startup_options.debug_open_settings {
            app.begin_plugin_snapshot(&creation_context.egui_ctx, false);
        }
        Ok(app)
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
            self.configuration.appearance,
            overlay_size_dip(self.debug_continuous_meter, self.continuous_full_scale_radius_dip),
            &self.icons,
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
            RuntimeEvent::ContinuousCancellationReady(reason) => {
                let Some(PendingPluginRequest::AwaitingContinuousCancellation {
                    operation,
                    plugin_id,
                    enable_after_approval,
                    reason: expected_reason,
                    session_retry_count,
                }) = self.pending_plugin_request.take()
                else {
                    log::warn!(
                        target: "plugins",
                        "component=plugins event_code=unexpected_plugin_cancellation_ready \
                         result_code=failure reason={reason:?}"
                    );
                    return;
                };
                if reason != expected_reason {
                    self.settings.plugin_cancellation_failed();
                    log::error!(
                        target: "plugins",
                        "component=plugins event_code=plugin_cancellation_reason_mismatch \
                         result_code=failure expected={expected_reason:?} actual={reason:?}"
                    );
                    return;
                }
                self.submit_plugin_operation(
                    context,
                    operation,
                    plugin_id,
                    enable_after_approval,
                    session_retry_count,
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

    fn open_settings(&mut self, context: &eframe::egui::Context) {
        if self.continuous_study.is_open() {
            self.continuous_study.settings_open_blocked();
            context.request_repaint_of(study_viewport_id());
            context.send_viewport_cmd_to(study_viewport_id(), eframe::egui::ViewportCommand::Focus);
            return;
        }
        if self.onboarding.is_open() {
            self.onboarding.settings_open_blocked();
            context.request_repaint_of(onboarding_viewport_id());
            context.send_viewport_cmd_to(
                onboarding_viewport_id(),
                eframe::egui::ViewportCommand::Focus,
            );
            return;
        }
        let newly_opened = self.settings.open(&self.configuration);
        self.refresh_start_at_login_status(context);
        self.begin_plugin_snapshot(context, false);
        if newly_opened {
            context.request_repaint();
        } else {
            context
                .send_viewport_cmd_to(settings_viewport_id(), eframe::egui::ViewportCommand::Focus);
        }
    }

    fn begin_plugin_snapshot(&mut self, context: &eframe::egui::Context, rescan: bool) {
        if self.pending_plugin_request.is_some() {
            return;
        }
        self.settings.plugin_snapshot_started(rescan);
        let request = if rescan {
            self.plugin_worker.request_rescan()
        } else {
            self.plugin_worker.request_snapshot()
        };
        match request {
            Ok(ticket) => {
                self.pending_plugin_request =
                    Some(PendingPluginRequest::Snapshot { ticket, rescan });
                context.request_repaint_after(Duration::from_millis(50));
            }
            Err(error) => self.settings.plugin_submission_failed(error),
        }
    }

    fn begin_plugin_operation(
        &mut self,
        context: &eframe::egui::Context,
        operation: PluginWorkerOperation,
        plugin_id: String,
        enable_after_approval: bool,
    ) {
        if self.pending_plugin_request.is_some() {
            return;
        }
        self.settings.plugin_operation_started(operation, &plugin_id);
        let cancellation_reason = match operation {
            PluginWorkerOperation::Reload => {
                Some(gradial_core::ContinuousCancelReason::ReloadRequested)
            }
            PluginWorkerOperation::Disable => {
                Some(gradial_core::ContinuousCancelReason::PluginDisabled)
            }
            PluginWorkerOperation::ApprovePermissions
            | PluginWorkerOperation::Enable
            | PluginWorkerOperation::MarkFaulted => None,
        };
        if let (Some(reason), Some(interaction)) = (cancellation_reason, self.interaction.as_ref())
        {
            match interaction.cancel_continuous(reason) {
                Ok(()) => {
                    self.pending_plugin_request =
                        Some(PendingPluginRequest::AwaitingContinuousCancellation {
                            operation,
                            plugin_id,
                            enable_after_approval,
                            reason,
                            session_retry_count: 0,
                        });
                    context.request_repaint_after(Duration::from_millis(10));
                }
                Err(error) => {
                    self.settings.plugin_cancellation_failed();
                    log::error!(
                        target: "plugins",
                        "component=plugins event_code=plugin_management_cancel_submit_failed \
                         result_code=failure operation={} error={error}",
                        operation.as_str()
                    );
                }
            }
            return;
        }
        self.submit_plugin_operation(context, operation, plugin_id, enable_after_approval, 0);
    }

    fn submit_plugin_operation(
        &mut self,
        context: &eframe::egui::Context,
        operation: PluginWorkerOperation,
        plugin_id: String,
        enable_after_approval: bool,
        session_retry_count: u8,
    ) {
        match self.plugin_worker.request(operation, plugin_id.clone()) {
            Ok(ticket) => {
                self.pending_plugin_request = Some(PendingPluginRequest::Operation {
                    ticket,
                    operation,
                    plugin_id,
                    enable_after_approval,
                    session_retry_count,
                });
                context.request_repaint_after(Duration::from_millis(50));
            }
            Err(error) => self.settings.plugin_submission_failed(error),
        }
    }

    #[allow(clippy::too_many_lines, reason = "polls the closed plugin request state machine")]
    fn poll_plugin_worker(&mut self, context: &eframe::egui::Context) {
        let Some(pending) = self.pending_plugin_request.take() else {
            return;
        };
        match pending {
            PendingPluginRequest::Snapshot { ticket, rescan } => match ticket.try_result() {
                None => {
                    self.pending_plugin_request =
                        Some(PendingPluginRequest::Snapshot { ticket, rescan });
                    context.request_repaint_after(Duration::from_millis(50));
                }
                Some(Ok(snapshot)) => {
                    self.settings.accept_plugin_snapshot(snapshot, rescan);
                    context.request_repaint_of(settings_viewport_id());
                }
                Some(Err(error)) => {
                    self.settings.plugin_operation_failed(error);
                    context.request_repaint_of(settings_viewport_id());
                }
            },
            PendingPluginRequest::Operation {
                ticket,
                operation,
                plugin_id,
                enable_after_approval,
                session_retry_count,
            } => match ticket.try_result() {
                None => {
                    self.pending_plugin_request = Some(PendingPluginRequest::Operation {
                        ticket,
                        operation,
                        plugin_id,
                        enable_after_approval,
                        session_retry_count,
                    });
                    context.request_repaint_after(Duration::from_millis(50));
                }
                Some(Ok(receipt)) if enable_after_approval => {
                    self.begin_plugin_operation(
                        context,
                        PluginWorkerOperation::Enable,
                        plugin_id,
                        false,
                    );
                    log::info!(
                        target: "plugins",
                        "component=plugins event_code=plugin_settings_approval_chained \
                         result_code=ok state_revision={}",
                        receipt.state_revision
                    );
                }
                Some(Ok(receipt)) => {
                    self.settings.plugin_operation_succeeded(&receipt);
                    self.begin_plugin_snapshot(context, false);
                    context.request_repaint_of(settings_viewport_id());
                }
                Some(Err(PluginWorkerError::SessionActive))
                    if matches!(
                        operation,
                        PluginWorkerOperation::Reload | PluginWorkerOperation::Disable
                    ) && session_retry_count < 20 =>
                {
                    self.pending_plugin_request = Some(PendingPluginRequest::RetryOperation {
                        operation,
                        plugin_id,
                        enable_after_approval,
                        session_retry_count: session_retry_count.saturating_add(1),
                        not_before: Instant::now() + Duration::from_millis(10),
                    });
                    context.request_repaint_after(Duration::from_millis(10));
                }
                Some(Err(error)) => {
                    self.settings.plugin_operation_failed(error);
                    self.begin_plugin_snapshot(context, false);
                    context.request_repaint_of(settings_viewport_id());
                }
            },
            PendingPluginRequest::AwaitingContinuousCancellation {
                operation,
                plugin_id,
                enable_after_approval,
                reason,
                session_retry_count,
            } => {
                self.pending_plugin_request =
                    Some(PendingPluginRequest::AwaitingContinuousCancellation {
                        operation,
                        plugin_id,
                        enable_after_approval,
                        reason,
                        session_retry_count,
                    });
                context.request_repaint_after(Duration::from_millis(10));
            }
            PendingPluginRequest::RetryOperation {
                operation,
                plugin_id,
                enable_after_approval,
                session_retry_count,
                not_before,
            } => {
                if Instant::now() < not_before {
                    self.pending_plugin_request = Some(PendingPluginRequest::RetryOperation {
                        operation,
                        plugin_id,
                        enable_after_approval,
                        session_retry_count,
                        not_before,
                    });
                    context.request_repaint_after(Duration::from_millis(10));
                } else {
                    self.submit_plugin_operation(
                        context,
                        operation,
                        plugin_id,
                        enable_after_approval,
                        session_retry_count,
                    );
                }
            }
        }
    }

    fn open_plugins_folder(&self, context: &eframe::egui::Context) {
        let opened = platform::open_folder(&self.config_store.plugin_directory()).is_ok();
        self.settings.plugins_folder_result(opened);
        if opened {
            log::info!(
                target: "plugins",
                "component=plugins event_code=plugins_folder_opened result_code=ok"
            );
        } else {
            log::warn!(
                target: "plugins",
                "component=plugins event_code=plugins_folder_opened result_code=failure \
                 error_code=shell_open_failed"
            );
        }
        context.request_repaint_of(settings_viewport_id());
    }

    fn persist_configuration(
        &mut self,
        context: &eframe::egui::Context,
        proposed: &Configuration,
    ) -> Result<Configuration> {
        let mut prepared = proposed.clone();
        prepared.config_revision = self
            .configuration
            .config_revision
            .checked_add(1)
            .context("configuration revision is exhausted")?;
        prepared.validate().context("settings draft is invalid")?;
        self.icons.preload_configuration(context, &prepared);
        let profile_resolver = Arc::new(
            ProfileResolver::compile(&prepared)
                .context("could not compile the settings profile rules")?,
        );
        let action_plan_compiler = Arc::new(
            ActionPlanCompiler::from_configuration(&prepared)
                .context("could not compile the settings action catalog")?,
        );
        let saved = self
            .config_store
            .save(&self.configuration, proposed)
            .context("could not atomically save the settings draft")?;
        if saved != prepared {
            bail!("saved configuration does not match its validated draft");
        }

        let previous_continuous_full_scale_radius_dip = self.continuous_full_scale_radius_dip;
        self.interaction = None;
        self.profile_resolver = profile_resolver;
        self.action_plan_compiler = action_plan_compiler;
        self.config_revision = saved.config_revision;
        self.continuous_full_scale_radius_dip = self
            .debug_continuous_full_scale_radius_dip
            .unwrap_or(saved.continuous_full_scale_radius_dip);
        let overlay_size_changed = self.debug_continuous_meter.is_enabled()
            && self.continuous_full_scale_radius_dip != previous_continuous_full_scale_radius_dip;
        self.configuration = saved.clone();
        self.reconcile_icon_storage(None);
        if overlay_size_changed {
            self.replace_pool(context);
        } else {
            self.overlay_pool.set_configuration(saved.config_revision, saved.appearance);
        }
        self.action_executor.set_config_revision(saved.config_revision);
        context.request_repaint();
        Ok(saved)
    }

    fn save_settings(
        &mut self,
        context: &eframe::egui::Context,
        proposed: &Configuration,
    ) -> Result<Configuration> {
        let saved = self.persist_configuration(context, proposed)?;
        let icon_assignment_count = saved
            .global_profile
            .menu
            .slices
            .iter()
            .chain(
                saved
                    .application_profiles
                    .iter()
                    .flat_map(|application| &application.profile.menu.slices),
            )
            .filter(|slice| slice.icon.is_some())
            .count();
        log::info!(
            target: "settings",
            "component=settings event_code=settings_saved result_code=ok \
             config_revision={} slice_count={} action_count={} activation_button={} \
             activation_control={} activation_alt={} activation_shift={} \
             accent_red={} accent_green={} accent_blue={} label_theme={} size_preset={} \
             icon_assignment_count={}",
            saved.config_revision,
            saved.global_profile.menu.slice_count,
            saved.actions.len(),
            saved.activation_binding.button.short_label(),
            saved.activation_binding.modifiers.control,
            saved.activation_binding.modifiers.alt,
            saved.activation_binding.modifiers.shift,
            saved.appearance.accent_color.red,
            saved.appearance.accent_color.green,
            saved.appearance.accent_color.blue,
            saved.appearance.label_theme.as_str(),
            saved.appearance.size_preset.as_str(),
            icon_assignment_count
        );
        Ok(saved)
    }

    fn handle_onboarding_event(&mut self, context: &eframe::egui::Context, event: OnboardingEvent) {
        match event {
            OnboardingEvent::BeginPractice(binding) => {
                if self.settings.is_open() {
                    self.onboarding.practice_start_failed(OnboardingStartError::SettingsOpen);
                    context.request_repaint_of(onboarding_viewport_id());
                    return;
                }
                let mut proposed = self.configuration.clone();
                proposed.activation_binding = binding;
                let binding_changed = binding != self.configuration.activation_binding;
                let result = if binding_changed {
                    self.persist_configuration(context, &proposed)
                } else {
                    Ok(self.configuration.clone())
                };
                match result {
                    Ok(saved) => {
                        self.onboarding.practice_started(saved.activation_binding);
                        log::info!(
                            target: "onboarding",
                            "component=onboarding event_code=onboarding_practice_started \
                             result_code=ok config_revision={} binding_changed={} \
                             activation_button={} activation_control={} activation_alt={} \
                             activation_shift={}",
                            saved.config_revision,
                            binding_changed,
                            saved.activation_binding.button.short_label(),
                            saved.activation_binding.modifiers.control,
                            saved.activation_binding.modifiers.alt,
                            saved.activation_binding.modifiers.shift
                        );
                    }
                    Err(error) => {
                        self.onboarding.practice_start_failed(OnboardingStartError::SaveFailed);
                        log::error!(
                            target: "onboarding",
                            "component=onboarding event_code=onboarding_binding_save_failed \
                             result_code=failure error_code=config_write_failed \
                             config_revision={} error={error}",
                            self.config_revision
                        );
                    }
                }
                context.request_repaint_of(onboarding_viewport_id());
            }
            OnboardingEvent::Complete => match self.onboarding_store.set_completed(true) {
                Ok(()) => {
                    self.onboarding.completion_saved(context);
                    log::info!(
                        target: "onboarding",
                        "component=onboarding event_code=onboarding_completed result_code=ok \
                         config_revision={}",
                        self.config_revision
                    );
                }
                Err(error) => {
                    self.onboarding.completion_failed();
                    log::error!(
                        target: "onboarding",
                        "component=onboarding event_code=onboarding_completion_save_failed \
                         result_code=failure error_code=state_write_failed \
                         config_revision={} error={error}",
                        self.config_revision
                    );
                    context.request_repaint_of(onboarding_viewport_id());
                }
            },
        }
    }

    fn restart_onboarding(&mut self, context: &eframe::egui::Context) {
        if self.continuous_study.is_open() {
            self.continuous_study.settings_open_blocked();
            context.request_repaint_of(study_viewport_id());
            context.send_viewport_cmd_to(study_viewport_id(), eframe::egui::ViewportCommand::Focus);
            return;
        }
        self.onboarding.restart(self.configuration.activation_binding);
        context.request_repaint_of(onboarding_viewport_id());
        context
            .send_viewport_cmd_to(onboarding_viewport_id(), eframe::egui::ViewportCommand::Focus);
        log::info!(
            target: "onboarding",
            "component=onboarding event_code=onboarding_restarted result_code=ok \
             config_revision={} source=tray",
            self.config_revision
        );
    }

    fn handle_settings_event(&mut self, context: &eframe::egui::Context, event: SettingsEvent) {
        match event {
            SettingsEvent::Save(proposed) => match self.save_settings(context, &proposed) {
                Ok(saved) => self.settings.accept_saved(&saved),
                Err(error) => {
                    self.settings.save_failed();
                    log::error!(
                        target: "settings",
                        "component=settings event_code=settings_save_failed result_code=failure \
                         error_code=config_invalid config_revision={} error={error}",
                        self.config_revision
                    );
                }
            },
            SettingsEvent::Import(path) => self.import_settings_draft(context, path.as_path()),
            SettingsEvent::ImportIcon { path, profile_id, slice_index } => {
                match self.icons.import(context, path.as_path(), self.config_revision) {
                    Ok(icon) => {
                        self.settings.accept_icon_import(profile_id, slice_index, icon);
                        let draft = self.settings.proposed_configuration();
                        self.reconcile_icon_storage(draft.as_ref());
                    }
                    Err(error) => {
                        self.settings.icon_import_failed(profile_id, slice_index, error);
                        log::warn!(
                            target: "settings",
                            "component=settings event_code=icon_import_failed \
                             result_code=failure error_code={} config_revision={} \
                             profile_id={} slice_index={}",
                            error.code(),
                            self.config_revision,
                            profile_id,
                            slice_index
                        );
                    }
                }
                context.request_repaint_of(settings_viewport_id());
            }
            SettingsEvent::Export { path, configuration } => {
                self.export_settings_draft(context, path.as_path(), &configuration);
            }
            SettingsEvent::RefreshDiagnostics => self.refresh_diagnostic_preview(context),
            SettingsEvent::ExportDiagnostics { path, request } => {
                match self.diagnostic_bundles.export(path.as_path(), &request) {
                    Ok(summary) => {
                        self.settings.accept_diagnostic_exported(summary);
                        log::info!(
                            target: "diagnostics",
                            "component=diagnostics event_code=diagnostic_bundle_exported \
                             result_code=ok config_revision={} log_count={} \
                             process_path_count={} archive_entry_count={} \
                             configured_process_paths_included={}",
                            self.config_revision,
                            summary.logs,
                            summary.process_paths,
                            summary.archive_entries,
                            request.include_process_paths
                        );
                    }
                    Err(error) => {
                        self.settings.diagnostic_export_failed(error);
                        log::error!(
                            target: "diagnostics",
                            "component=diagnostics event_code=diagnostic_bundle_export_failed \
                             result_code=failure error_code={} config_revision={} \
                             configured_process_paths_included={}",
                            error.code(),
                            self.config_revision,
                            request.include_process_paths
                        );
                    }
                }
                context.request_repaint_of(settings_viewport_id());
            }
            SettingsEvent::RefreshStartAtLogin => self.refresh_start_at_login_status(context),
            SettingsEvent::ChangeStartAtLogin(action) => {
                self.change_start_at_login(context, action);
            }
            SettingsEvent::RefreshPlugins => self.begin_plugin_snapshot(context, true),
            SettingsEvent::PluginAction { action, plugin_id } => {
                self.handle_plugin_settings_action(context, action, plugin_id);
            }
            SettingsEvent::OpenPluginsFolder => self.open_plugins_folder(context),
            SettingsEvent::Closed => self.reconcile_icon_storage(None),
        }
    }

    fn handle_plugin_settings_action(
        &mut self,
        context: &eframe::egui::Context,
        action: PluginSettingsAction,
        plugin_id: String,
    ) {
        let (operation, enable_after_approval) = match action {
            PluginSettingsAction::Enable => (PluginWorkerOperation::Enable, false),
            PluginSettingsAction::ApproveAndEnable => {
                (PluginWorkerOperation::ApprovePermissions, true)
            }
            PluginSettingsAction::Reload => (PluginWorkerOperation::Reload, false),
            PluginSettingsAction::Disable => (PluginWorkerOperation::Disable, false),
        };
        self.begin_plugin_operation(context, operation, plugin_id, enable_after_approval);
    }

    #[cfg(debug_assertions)]
    fn handle_debug_plugin_operation(
        &mut self,
        context: &eframe::egui::Context,
        operation: platform::DebugPluginOperation,
    ) {
        let Some(plugin_id) = debug_plugin_operation_target(&self.configuration) else {
            log::warn!(
                target: "plugins",
                "component=plugins event_code=debug_plugin_operation_rejected \
                 result_code=failure error_code=no_configured_continuous_plugin"
            );
            return;
        };
        let operation = match operation {
            platform::DebugPluginOperation::Reload => PluginWorkerOperation::Reload,
            platform::DebugPluginOperation::Disable => PluginWorkerOperation::Disable,
        };
        self.begin_plugin_operation(context, operation, plugin_id, false);
    }

    fn refresh_start_at_login_status(&self, context: &eframe::egui::Context) {
        let status = self.start_at_login.inspect();
        match status {
            Ok(status) => {
                self.settings.update_start_at_login_status(Ok(status));
                log::info!(
                    target: "startup",
                    "component=start_at_login event_code=start_at_login_status_read \
                     result_code=ok config_revision={} status={}",
                    self.config_revision,
                    status.log_name()
                );
            }
            Err(error) => {
                self.settings.update_start_at_login_status(Err(error));
                log::warn!(
                    target: "startup",
                    "component=start_at_login event_code=start_at_login_status_read \
                     result_code=failure error_code={} native_error_code={} \
                     config_revision={}",
                    error.code(),
                    error.native_error_code(),
                    self.config_revision
                );
            }
        }
        context.request_repaint_of(settings_viewport_id());
    }

    fn change_start_at_login(&self, context: &eframe::egui::Context, action: StartAtLoginAction) {
        match self.start_at_login.apply(action) {
            Ok(status) => {
                self.settings.accept_start_at_login_changed(action, status);
                log::info!(
                    target: "startup",
                    "component=start_at_login event_code=start_at_login_changed \
                     result_code=ok config_revision={} action={} status={}",
                    self.config_revision,
                    action.log_name(),
                    status.log_name()
                );
            }
            Err(error) => {
                self.settings.start_at_login_failed(action, error);
                log::error!(
                    target: "startup",
                    "component=start_at_login event_code=start_at_login_changed \
                     result_code=failure error_code={} native_error_code={} \
                     config_revision={} action={}",
                    error.code(),
                    error.native_error_code(),
                    self.config_revision,
                    action.log_name()
                );
            }
        }
        context.request_repaint_of(settings_viewport_id());
    }

    fn refresh_diagnostic_preview(&self, context: &eframe::egui::Context) {
        let input_status = match self.tray_icon.health() {
            TrayHealth::Healthy => DiagnosticInputStatus::Responsive,
            TrayHealth::Recovering => DiagnosticInputStatus::Recovering,
            TrayHealth::Faulted => DiagnosticInputStatus::Faulted,
        };
        match self.diagnostic_bundles.capture(
            &self.configuration,
            self.overlay_pool.monitors(),
            input_status,
        ) {
            Ok(snapshot) => {
                let log_count = snapshot.logs().len();
                let display_count = snapshot.displays().len();
                let process_path_count = snapshot.process_paths().len();
                self.settings.accept_diagnostic_preview(snapshot);
                log::info!(
                    target: "diagnostics",
                    "component=diagnostics event_code=diagnostic_preview_ready result_code=ok \
                     config_revision={} log_count={} display_count={} process_path_count={} \
                     configured_process_paths_included=false",
                    self.config_revision,
                    log_count,
                    display_count,
                    process_path_count
                );
            }
            Err(error) => {
                self.settings.diagnostic_preview_failed(error);
                log::error!(
                    target: "diagnostics",
                    "component=diagnostics event_code=diagnostic_preview_failed \
                     result_code=failure error_code={} config_revision={}",
                    error.code(),
                    self.config_revision
                );
            }
        }
        context.request_repaint_of(settings_viewport_id());
    }

    fn reconcile_icon_storage(&self, draft: Option<&Configuration>) {
        let protection = self.config_store.icon_protection_snapshot(&self.configuration, draft);
        self.icons.reconcile_storage(
            &protection.references,
            protection.quarantine_allowed,
            self.config_revision,
        );
    }

    fn import_settings_draft(&mut self, context: &eframe::egui::Context, path: &Path) {
        match self.config_store.import_draft(&self.configuration, path) {
            Ok(imported) => {
                let migration_from =
                    imported.migration.map_or(0, |migration| migration.from_schema);
                let action_count = imported.configuration.actions.len();
                self.reconcile_icon_storage(Some(&imported.configuration));
                self.icons.refresh_configuration(context, &imported.configuration);
                self.settings.accept_imported(
                    imported.configuration,
                    imported.migration,
                    imported.icons,
                );
                log::info!(
                    target: "settings",
                    "component=settings event_code=settings_imported_to_draft \
                     result_code=ok config_revision={} action_count={} migration_from={} \
                     icon_referenced_count={} icon_copied_count={} icon_reused_count={} \
                     icon_missing_count={} icon_bundle_present={}",
                    self.config_revision,
                    action_count,
                    migration_from,
                    imported.icons.referenced,
                    imported.icons.copied,
                    imported.icons.reused,
                    imported.icons.missing,
                    imported.icons.bundle_present
                );
            }
            Err(error) => {
                self.settings.import_failed(error);
                log::error!(
                    target: "settings",
                    "component=settings event_code=settings_import_failed \
                     result_code=failure error_code={} config_revision={}",
                    error.code(),
                    self.config_revision
                );
            }
        }
        context.request_repaint_of(settings_viewport_id());
    }

    fn export_settings_draft(
        &mut self,
        context: &eframe::egui::Context,
        path: &Path,
        configuration: &Configuration,
    ) {
        match self.config_store.export_draft(&self.configuration, path, configuration) {
            Ok(icons) => {
                self.settings.accept_exported(icons);
                log::info!(
                    target: "settings",
                    "component=settings event_code=settings_draft_exported \
                     result_code=ok config_revision={} action_count={} \
                     icon_referenced_count={} icon_copied_count={} icon_reused_count={}",
                    self.config_revision,
                    configuration.actions.len(),
                    icons.referenced,
                    icons.copied,
                    icons.reused
                );
            }
            Err(error) => {
                self.settings.export_failed(error);
                log::error!(
                    target: "settings",
                    "component=settings event_code=settings_export_failed \
                     result_code=failure error_code={} config_revision={}",
                    error.code(),
                    self.config_revision
                );
            }
        }
        context.request_repaint_of(settings_viewport_id());
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
                self.configuration.appearance,
                overlay_size_dip(
                    self.debug_continuous_meter,
                    self.continuous_full_scale_radius_dip,
                ),
                &self.icons,
                self.config_revision,
                Arc::clone(&self.timing_markers),
            );
            context.request_repaint();
            return;
        }

        let input_configuration = InputConfiguration {
            geometry_mode: self.geometry_mode,
            monitors: current_monitors,
            activation_binding: self.configuration.activation_binding,
            overlay_half_extent_dip: overlay_half_extent_dip(
                self.debug_continuous_meter,
                self.continuous_full_scale_radius_dip,
            ),
        };
        let interaction = match InteractionRuntime::start(
            context.clone(),
            input_configuration,
            Arc::clone(&self.timing_markers),
            routes,
            Arc::clone(&self.profile_resolver),
            Arc::clone(&self.action_plan_compiler),
            self.action_executor.handle(),
            self.plugin_worker.lifecycle_client(),
            self.onboarding.practice(),
            self.continuous_study.sink(),
            self.debug_menu_override.clone(),
            self.debug_continuous_meter.is_enabled(),
            GestureConfig {
                continuous_arm_radius_dip: continuous_arm_radius_for_size(
                    self.configuration.appearance.size_preset,
                ),
                continuous_full_scale_radius_dip: f64::from(self.continuous_full_scale_radius_dip),
                ..GestureConfig::default()
            },
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

    fn poll_notifications(&mut self) {
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
        while let Some(notification) = crate::tray::try_plugin_notification() {
            if let Err(error) = self.tray_icon.show_plugin_notification(&notification) {
                log::error!(
                    target: "plugins",
                    "component=capabilities event_code=plugin_notification_failed \
                     result_code=failure plugin_id={} error={error}",
                    notification.plugin_id()
                );
            }
        }
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
                TrayCommand::OpenSettings => self.open_settings(context),
                TrayCommand::RestartOnboarding => self.restart_onboarding(context),
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
        self.poll_notifications();

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

        #[cfg(debug_assertions)]
        if let Some(operation) = platform::take_debug_plugin_operation() {
            self.handle_debug_plugin_operation(context, operation);
        }

        if let Some(event) = self.settings.try_event() {
            self.handle_settings_event(context, event);
        }
        self.poll_plugin_worker(context);

        self.onboarding.poll_practice_events(context);
        if let Some(event) = self.onboarding.try_event() {
            self.handle_onboarding_event(context, event);
        }

        self.continuous_study.update(context);

        self.start_interaction_if_ready(context);
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.overlay_pool.show(ui.ctx());
        self.settings.show(ui.ctx());
        self.onboarding.show(ui.ctx());
        self.continuous_study.show(ui.ctx());
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use std::collections::BTreeMap;

    use super::debug_plugin_operation_target;
    use gradial_core::{ActionDefinition, ActionKind, Configuration};
    use uuid::Uuid;

    #[test]
    fn debug_plugin_operation_targets_the_first_configured_continuous_plugin() {
        let configuration = Configuration {
            actions: vec![
                ActionDefinition {
                    id: Uuid::from_u128(1),
                    name: "Delay".to_owned(),
                    kind: ActionKind::Delay { milliseconds: 10 },
                },
                ActionDefinition {
                    id: Uuid::from_u128(2),
                    name: "First plugin".to_owned(),
                    kind: ActionKind::PluginContinuous {
                        plugin_id: "org.gradial.first".to_owned(),
                        plugin_action_id: "measure".to_owned(),
                        parameters: BTreeMap::default(),
                    },
                },
                ActionDefinition {
                    id: Uuid::from_u128(3),
                    name: "Second plugin".to_owned(),
                    kind: ActionKind::PluginContinuous {
                        plugin_id: "org.gradial.second".to_owned(),
                        plugin_action_id: "measure".to_owned(),
                        parameters: BTreeMap::default(),
                    },
                },
            ],
            ..Configuration::default()
        };

        assert_eq!(
            debug_plugin_operation_target(&configuration).as_deref(),
            Some("org.gradial.first")
        );
    }
}
