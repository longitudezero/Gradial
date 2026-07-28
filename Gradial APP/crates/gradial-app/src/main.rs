#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod application;
mod diagnostics;
mod overlay;
mod platform;

use anyhow::{Context, Result, anyhow};
use gradial_core::AppPaths;

fn main() -> Result<()> {
    let paths = AppPaths::discover().context("could not resolve Gradial's local data directory")?;
    paths.ensure_directories().context("could not prepare Gradial's local data directories")?;

    let _logger =
        diagnostics::initialize(&paths.logs).context("could not initialize local logging")?;
    log::info!(
        target: "startup",
        "component=app event_code=startup_begin result_code=ok config_revision=0"
    );

    let options = overlay::native_options();
    let run_result = eframe::run_native(
        "Gradial",
        options,
        Box::new(|creation_context| {
            let app = application::GradialApp::new(creation_context)?;
            Ok(Box::new(app))
        }),
    );

    match run_result {
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
    }
}
