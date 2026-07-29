use gradial_core::InvocationSnapshot;

use crate::platform;

/// Records the three Milestone 0 focus checkpoints without asking Windows to
/// activate or restore any window.
pub(crate) fn activation(snapshot: &InvocationSnapshot) {
    log::info!(
        target: "focus",
        "component=focus event_code=focus_checkpoint result_code=ok config_revision={} \
         invocation_id={} phase=activation accepted=true same_window=true same_process=true",
        snapshot.context.config_revision,
        snapshot.invocation_id
    );
}

pub(crate) fn checkpoint(snapshot: &InvocationSnapshot, phase: &str) {
    let current = platform::foreground_window_identity();
    let same_window = current.window == snapshot.context.foreground_window;
    let same_process = snapshot.context.foreground_pid != 0
        && current.process_id == snapshot.context.foreground_pid;
    let accepted = same_window || same_process;

    if accepted {
        log::info!(
            target: "focus",
            "component=focus event_code=focus_checkpoint result_code=ok config_revision={} \
             invocation_id={} phase={phase} accepted=true \
             same_window={same_window} same_process={same_process}",
            snapshot.context.config_revision,
            snapshot.invocation_id,
        );
    } else {
        log::error!(
            target: "focus",
            "component=focus event_code=focus_changed result_code=failure config_revision={} \
             invocation_id={} phase={phase} accepted=false \
             same_window=false same_process=false",
            snapshot.context.config_revision,
            snapshot.invocation_id,
        );
    }
}
