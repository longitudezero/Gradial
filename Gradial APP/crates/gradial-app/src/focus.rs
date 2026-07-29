use std::sync::Mutex;

use gradial_core::InvocationSnapshot;
use uuid::Uuid;

use crate::platform;

static EXPECTED_TARGET_CHANGE: Mutex<Option<Uuid>> = Mutex::new(None);

pub(crate) fn allow_target_change(invocation_id: Uuid) {
    *EXPECTED_TARGET_CHANGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(invocation_id);
}

pub(crate) fn revoke_target_change(invocation_id: Uuid) {
    let mut expected =
        EXPECTED_TARGET_CHANGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if *expected == Some(invocation_id) {
        expected.take();
    }
}

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
    let expected_target_change = if phase == "after_close" {
        let mut expected =
            EXPECTED_TARGET_CHANGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if *expected == Some(snapshot.invocation_id) {
            expected.take();
            true
        } else {
            false
        }
    } else {
        false
    };
    let accepted = same_window || same_process || expected_target_change;

    if accepted {
        log::info!(
            target: "focus",
            "component=focus event_code=focus_checkpoint result_code=ok config_revision={} \
             invocation_id={} phase={phase} accepted=true \
             same_window={same_window} same_process={same_process} \
             expected_target_change={expected_target_change}",
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
