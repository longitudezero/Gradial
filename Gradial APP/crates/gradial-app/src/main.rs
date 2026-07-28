#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod application;
mod diagnostics;
mod focus;
mod input;
mod interaction;
mod options;
mod overlay;
mod platform;
mod tray;

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use gradial_core::AppPaths;

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
    let timing_markers = Arc::new(
        diagnostics::TimingMarkers::register()
            .context("could not initialize ETW-compatible timing markers")?,
    );

    let options = overlay::native_options();
    let app_timing_markers = Arc::clone(&timing_markers);
    let run_result = eframe::run_native(
        "Gradial",
        options,
        Box::new(move |creation_context| {
            let app = application::GradialApp::new(
                creation_context,
                startup_options,
                app_timing_markers,
            )?;
            Ok(Box::new(app))
        }),
    );

    let result = match run_result {
        Ok(()) => {
            log::info!(
                target: "startup",
                "component=app event_code=shutdown_complete result_code=ok config_revision=0"
            );
            Ok(())
        }
        Err(error) => {
            log::error!(
                target: "renderer",
                "component=renderer event_code=renderer_unavailable result_code=failure config_revision=0"
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
