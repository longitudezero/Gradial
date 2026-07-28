use gradial_core::InvocationSnapshot;
use uuid::Uuid;

use crate::platform;

#[derive(Clone, Copy, Debug)]
struct FocusExpectation {
    invocation_id: Uuid,
    foreground_window: usize,
    foreground_pid: u32,
}

/// Records the three Milestone 0 focus checkpoints without asking Windows to
/// activate or restore any window.
#[derive(Debug, Default)]
pub(crate) struct FocusProbe {
    expected: Option<FocusExpectation>,
}

impl FocusProbe {
    pub(crate) fn begin(&mut self, snapshot: InvocationSnapshot) {
        self.expected = Some(FocusExpectation {
            invocation_id: snapshot.invocation_id,
            foreground_window: snapshot.foreground_window,
            foreground_pid: snapshot.foreground_pid,
        });
        log::info!(
            target: "focus",
            "component=focus event_code=focus_checkpoint result_code=ok config_revision=0 \
             invocation_id={} phase=activation accepted=true same_window=true same_process=true",
            snapshot.invocation_id
        );
    }

    pub(crate) fn after_first_frame(&self, invocation_id: Uuid) {
        self.checkpoint(invocation_id, "after_first_frame");
    }

    pub(crate) fn after_close(&mut self, invocation_id: Uuid) {
        self.checkpoint(invocation_id, "after_close");
        if self.expected.is_some_and(|expected| expected.invocation_id == invocation_id) {
            self.expected = None;
        }
    }

    fn checkpoint(&self, invocation_id: Uuid, phase: &str) {
        let Some(expected) =
            self.expected.filter(|expected| expected.invocation_id == invocation_id)
        else {
            log::error!(
                target: "focus",
                "component=focus event_code=focus_checkpoint_missing result_code=failure \
                 config_revision=0 invocation_id={invocation_id} phase={phase}"
            );
            return;
        };
        let current = platform::foreground_window_identity();
        let same_window = current.window == expected.foreground_window;
        let same_process =
            expected.foreground_pid != 0 && current.process_id == expected.foreground_pid;
        let accepted = same_window || same_process;

        if accepted {
            log::info!(
                target: "focus",
                "component=focus event_code=focus_checkpoint result_code=ok config_revision=0 \
                 invocation_id={invocation_id} phase={phase} accepted=true \
                 same_window={same_window} same_process={same_process}"
            );
        } else {
            log::error!(
                target: "focus",
                "component=focus event_code=focus_changed result_code=failure config_revision=0 \
                 invocation_id={invocation_id} phase={phase} accepted=false \
                 same_window=false same_process=false"
            );
        }
    }
}
