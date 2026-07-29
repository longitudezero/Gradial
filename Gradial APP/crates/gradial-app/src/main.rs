#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod action_executor;
mod action_target;
mod application;
mod configuration;
mod diagnostics;
mod focus;
mod input;
mod interaction;
mod options;
mod overlay;
mod platform;
mod shortcut;
mod tray;
mod unicode_text;

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use gradial_core::{ActionPlanCompiler, AppPaths, ProfileResolver};

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|argument| matches!(argument.as_str(), "--help" | "-h")) {
        println!("{}", options::HELP);
        return Ok(());
    }
    let startup_options =
        options::StartupOptions::parse(arguments).context("could not parse Gradial options")?;

    let paths = AppPaths::discover().context("could not resolve Gradial's local data directory")?;
    paths.ensure_directories().context("could not prepare Gradial's local data directories")?;

    let logger =
        diagnostics::initialize(&paths.logs).context("could not initialize local logging")?;
    log::info!(
        target: "startup",
        "component=app event_code=startup_begin result_code=ok config_revision=0"
    );
    let loaded_configuration = configuration::ConfigStore::new(paths.clone())
        .load_or_initialize()
        .context("could not load Gradial configuration")?;
    let config_revision = loaded_configuration.configuration.config_revision;
    let profile_resolver = Arc::new(
        ProfileResolver::compile(&loaded_configuration.configuration)
            .context("could not compile Gradial profiles")?,
    );
    let action_plan_compiler = Arc::new(
        ActionPlanCompiler::from_configuration(&loaded_configuration.configuration)
            .context("could not compile Gradial action catalog")?,
    );
    let migrated_from_schema = loaded_configuration
        .migration
        .map_or_else(|| "none".to_owned(), |migration| migration.from_schema.to_string());
    log::info!(
        target: "config",
        "component=config event_code=config_loaded result_code=ok \
         config_revision={config_revision} schema_version={} source={} \
         migrated_from_schema={} action_count={}",
        loaded_configuration.configuration.schema_version,
        loaded_configuration.source.as_str(),
        migrated_from_schema,
        loaded_configuration.configuration.actions.len()
    );
    if let Some(warning) = &loaded_configuration.warning {
        log::error!(
            target: "config",
            "component=config event_code=config_recovery_warning result_code=failure \
             config_revision={config_revision} primary_error={} backup_error={}",
            warning.primary_error,
            warning.backup_error.as_deref().unwrap_or("none")
        );
    }
    let timing_markers = Arc::new(
        diagnostics::TimingMarkers::register()
            .context("could not initialize ETW-compatible timing markers")?,
    );

    let options = overlay::native_options();
    let app_timing_markers = Arc::clone(&timing_markers);
    let app_profile_resolver = Arc::clone(&profile_resolver);
    let app_action_plan_compiler = Arc::clone(&action_plan_compiler);
    let run_result = eframe::run_native(
        "Gradial",
        options,
        Box::new(move |creation_context| {
            let app = application::GradialApp::new(
                creation_context,
                startup_options,
                app_timing_markers,
                app_profile_resolver,
                app_action_plan_compiler,
                config_revision,
            )?;
            Ok(Box::new(app))
        }),
    );

    let result = match run_result {
        Ok(()) => {
            log::info!(
                target: "startup",
                "component=app event_code=shutdown_complete result_code=ok \
                 config_revision={config_revision}"
            );
            Ok(())
        }
        Err(error) => {
            log::error!(
                target: "renderer",
                "component=renderer event_code=renderer_unavailable result_code=failure \
                 config_revision={config_revision}"
            );
            Err(anyhow!("Gradial's window loop failed: {error}"))
        }
    };

    // The file writer is asynchronous. Explicit shutdown drains its queue so
    // the final interaction and hook metrics survive a short development run.
    drop(timing_markers);
    logger.shutdown();
    result
}
