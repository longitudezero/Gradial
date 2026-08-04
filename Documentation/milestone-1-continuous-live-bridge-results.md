# Milestone 1 live continuous bridge results

Date: 2026-08-04

## Scope

This bounded v0.3 Milestone 1 slice connects the existing debug inert meter and
continuous usability studies to the asynchronous continuous coordinator. It
proves the live gesture-to-worker boundary without adding Lua, schema-six
configuration, plugin discovery, capabilities, or any new operating-system
effect.

Normal configured menus still use an empty continuous-slice mask. Only the
explicit debug meter and its study modes create inert continuous sessions.

## Live hybrid mapping

The interaction thread derives every worker command from the current unlocked
`ContinuousVisualState`:

| Gesture state change | Worker action |
| --- | --- |
| First eligible wedge crosses outward | Start `Begin` with that wedge and value |
| Value changes in the active wedge | Replace the latest ordinary value |
| Pointer changes to another outward wedge | `Cancel(direction_changed)`, then fresh `Begin` |
| Pointer returns inside the visible wedge edge | `Cancel(returned_to_discrete)`, keep invocation lease inactive |
| Pointer crosses outward again | Fresh `Begin` for the current wedge, using the retained lease |
| M4 releases outside | `Commit` with the final displayed value |
| M4 releases inside | Preserve the inert discrete outcome; no configured action is compiled |
| Gesture cancels or Gradial shuts down | One mapped terminal `Cancel` |

Direction never locks and continuous mode never latches. Returning inward does
not end the held invocation, so the user may move outward again in the same or
another wedge. The inactive state retains the shared coordinator claim until
release, cancellation, failure, or shutdown.

## Exact scalar and bounded traffic

The overlay and worker receive the same
`ContinuousVisualState.selection.value`. The bridge does not recompute a
second percentage from cursor coordinates. Renderer batching remains capped
at approximately 240 Hz for responsive visual transitions, while the protocol
session applies its 60 Hz maximum and one-value coalescing slot. Final commit
still carries the exact release value even when an ordinary update is pending.

Direction, suspension, resume, commit, and cancellation continue to use the
separate eight-item control queue. Ordinary pointer samples cannot displace a
terminal request.

The interaction owner never joins the continuous worker while closing or
clearing a gesture. It submits the terminal request and detaches; the worker
keeps the lease until the bounded callback finishes. A full control queue also
raises shared cancellation before detach. If a handoff cancel fails or times
out, no replacement `Begin` is sent and the lease is released.

## Lease continuity and discrete transfer

Moving inward ends only the current plugin segment. The coordinator remains
busy for the full M4 hold, so unrelated Gradial actions cannot enter while the
gesture is temporarily discrete.

The action coordinator also has a direct lease-transfer path for a future
configured hybrid wedge. After `Cancel(returned_to_discrete)`, the continuous
worker can enqueue the already compiled ordinary action without releasing and
reacquiring the busy flag. The discrete worker becomes the owner of that same
claim and clears it on completion. This removes the self-generated
`action_busy` race. The path is covered with a real Delay execution test but is
not used by the debug meter, which remains inert.

## Target validation

The validating inert backend owns the frozen invocation target independently
of the permission-filtered plugin snapshot.

- `Begin` applies the existing full HWND, PID, process-start-time, foreground,
  and integrity validation.
- `Update` and `Commit` recheck lightweight HWND/PID/foreground freshness.
- A stale active target produces `Cancel(target_stale)` and ends the session.
- `Cancel` is always deliverable even if the target has disappeared.

No PID, HWND, process handle, cursor position, or mutable target reference is
placed in the runtime-neutral plugin context.

## Automated evidence

The focused asynchronous suite covers:

- suspension with `returned_to_discrete` while the coordinator stays busy;
- resume on a different unlocked wedge with a fresh session UUID;
- direct transfer from inactive continuous ownership to a successful discrete
  Delay action;
- stale target rejection before `Begin` acceptance;
- stale target cancellation during an active update;
- non-blocking owner drop while a `Begin` callback remains stalled;
- failed handoff cancellation without a replacement `Begin`;
- all previously recorded accept, reject, coalescing, handoff, timeout, fault,
  cancellation, and shutdown cases.

The pure live-bridge classification test covers start, same-wedge publication,
direction handoff, inward suspension, inactive stability, and outward resume.
The Windows interaction smoke harness now additionally requires backend
`Begin`, `Commit`, `returned_to_discrete`, and `direction_changed` records for
the matching visible gesture cases.

Final verification:

- `scripts/check.ps1`: passed with 202 debug application tests and 70 core
  tests, formatting, strict Clippy, script self-tests, and debug builds.
- `cargo test --release --workspace --all-features`: passed with 198 release
  application tests and 70 core tests.
- The standard continuous matrix passed all 14 scenarios and 99 live inert
  outcomes. Its machine-readable report is
  `captures/continuous-matrix/20260804T183319072Z/continuous-matrix.json`.
- Focused live checks separately passed outward commit, inward reversal, and
  west-to-south unlocked handoff.

## Deferred boundary

Schema six remains the next missing Milestone 1 boundary: persisted continuous
plugin-action definitions, per-wedge extension assignments, schema-five
migration, and unavailable-plugin reference behavior. A real plugin runtime
is still deferred to Milestone 2.
