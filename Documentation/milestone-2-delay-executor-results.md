# Milestone 2 bounded Delay executor results

Date: 2026-07-29

This record covers the fourth bounded Milestone 2 slice: a single-worker action
coordinator, Delay execution, immediate busy rejection, cancellation,
step/overall timeouts, and bounded shutdown. Keyboard, text, URL, file,
executable, and window backends remain disabled.

## Coordinator contract

- One dedicated worker owns action execution outside the input, interaction,
  and UI threads.
- Release clones the selected immutable plan into a capacity-one handoff.
- An atomic admission gate allows exactly one pending or running plan.
- A second release while busy fails immediately with `action_busy`; it is not
  queued and cannot affect the running plan.
- Display rebuild and input-hook recovery replace only the interaction runtime.
  They do not duplicate, cancel, or take ownership of an accepted action.
- Completion records contain invocation/action UUIDs, revision, step count,
  elapsed time, and a structured `ActionResult`. They contain no payload text,
  URL, path, or argument values.

## Delay execution and safety limits

The worker preflights the complete flattened plan before running its first
step. A plan containing any non-Delay operation fails with
`unsupported_action`, so mixed sequences cannot run a Delay prefix before
reaching a disabled backend.

Delay waits use a monotonic clock and sleep in increments no larger than 10 ms.
Every increment checks cancellation and shutdown. The executor defensively
enforces:

- at most 32 steps;
- the plan's 5-second per-step timeout;
- the plan's 30-second overall duration;
- first failure or cancellation stops the sequence.

The current fixed 5-second step timeout means a configured Delay longer than
five seconds returns `step_timeout`. The schema continues to accept the v1
10–10,000 ms range so a later settings/executor slice can make the timeout
policy configurable without another schema migration.

## Cancellation and shutdown

The notification-area menu now includes **Cancel current action**. It is
enabled only while a plan is pending or running. Choosing it sets the same
cancellation token checked by every Delay increment.

Shutdown sets both shutdown and cancellation, wakes the worker, and waits at
most two seconds. A responsive worker completes and joins normally. If that
bound is exceeded, Gradial records `shutdown_timeout` and does not block
application shutdown indefinitely.

## Result and diagnostic behavior

- Accepted dispatch: `action_dispatched`.
- Successful plan: `action_execution_completed`, `result_code=ok`,
  `action_executed=true`.
- Tray/shutdown cancellation: `result_code=cancelled`.
- Busy, timeout, unsupported, or unavailable execution:
  `result_code=failure` with a stable safe error code.
- Empty slices continue to cancel before the executor.

Success produces no popup. This slice records results locally and exposes
active cancellation through the tray; the richer activity panel remains part
of the settings/tray UI milestone.

## Automated coverage

New tests prove:

- a Delay plan completes successfully;
- a second concurrent dispatch is rejected as `action_busy`;
- cancellation interrupts a running Delay well before its original deadline;
- per-step and overall timeouts return distinct errors;
- mixed unsupported plans fail preflight before their Delay prefix runs;
- shutdown cancels an active Delay and returns within its bound;
- release compiles and dispatches the action frozen with the invocation;
- all other action families remain non-executing.

The live smoke harness additionally requires the production 5 s / 30 s / 32
step / 10 ms cancellation / 2 s shutdown limits and a clean executor shutdown
for every real-app scenario.

## Validation evidence

The completed implementation passed:

- 90 debug workspace tests and 88 release workspace tests;
- Clippy for all targets and features in debug and release;
- debug and release builds;
- an isolated release smoke fixture with two successful 50 ms Delay
  invocations, without touching the user's configuration;
- action-free live smoke checks across all four debug slices and all eight
  release slices;
- the nine-scenario quick interaction matrix, with 32 selected invocations and
  no failures.

The matrix artifacts for this run are under
`Gradial APP/captures/interaction-matrix/20260729T125631285Z`. The normal live
checks observed the 50 ms close target. The Delay fixture observed 52 ms, which
is inside the smoke harness's scheduler tolerance.

## Current boundary

Only Delay can succeed. Target freshness/integrity validation and Win32
backends for shortcut, Unicode text, URL/path opening, executable launch, and
invocation-window commands are not present. The tray exposes cancellation but
does not yet retain activity history or show failure toasts.
