# Milestone 1 input recovery and tray results

Date: 2026-07-28

This record covers the bounded Milestone 1 input-health slice: native tray
health and commands, heartbeat recovery, fail-safe overflow behavior, and
Windows lifecycle re-registration.

## Implemented contract

- Gradial owns one notification-area icon for its complete lifetime.
- The icon and menu expose Healthy, Recovering input, and Input faulted states.
- The menu provides Retry input and Exit Gradial.
- The 250 ms hook heartbeat is checked continuously. A sample older than
  750 ms causes the input gate to pass new M4 presses through, cancels any open
  invocation, and replaces the complete hook/raw-input/queue/consumer runtime.
- Queue overflow closes the runtime and remains Faulted until Retry input.
  Overflow also closes the callback gate at the point where the failed queue
  write is detected.
- Power resume, current-session unlock, and Explorer restart request complete
  input re-registration. Explorer restart also restores the tray icon.
- A failed initial start or failed replacement leaves Gradial running and
  Faulted instead of retaining a partial input stack.

`hook_thread_responsive` remains a responsiveness observation rather than a
claim that Windows can never remove a hook between heartbeats.

## Automated checks

`scripts/check.ps1` passed after the implementation:

- PowerShell syntax validation.
- Rust formatting.
- Clippy with warnings denied.
- 40 unit tests.
- Documentation tests.
- Debug workspace build.

New unit coverage checks the exact three-interval heartbeat boundary, ignores an
uninitialized heartbeat sample, verifies immediate input suspension at queue
overflow, validates M4 pass-through while suspended, and checks native tray
tooltip bounds and conservative health decoding.

## Live Windows checks

| Check | Result |
| --- | --- |
| Windows can resolve Gradial's live notification icon | Pass |
| Normal fixed-origin gesture after tray startup | Pass |
| Simulated stale heartbeat | Pass; runtime stopped and automatically restarted |
| Simulated queue overflow | Pass; Faulted, M4 passed through, Retry input restarted runtime |
| Power-resume notification | Pass; complete input runtime re-registered |
| Display change during held M4 | Pass; cancelled, rewarmed, and accepted a new gesture |
| Foreground focus through post-recovery gesture | Pass |

Fault simulation is compiled only into debug builds and is reached through
private Gradial window messages. It drives the same runtime and tray command
paths as the production signals; release builds do not expose those messages.

## Remaining validation

Explorer restart and physical suspend/unlock should still be exercised on the
release hardware matrix, along with deliberate hook-thread stalls under a
debugger and a sustained producer-overrun stress case. The required long-run
latency, focus, multi-display, and recovery campaigns remain future release
gates.
