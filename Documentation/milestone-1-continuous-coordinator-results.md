# Milestone 1 continuous coordinator results

Date: 2026-08-04

## Scope

This bounded v0.3 Milestone 1 slice connects the runtime-neutral continuous
session to the existing single-action coordinator and a fake asynchronous
backend. It proves worker, lease, handoff, timeout, cancellation, and shutdown
semantics without adding Lua, schema six, plugin discovery, capabilities, or a
user-visible continuous action.

The executor is compiled as staged production code but is not started by the
application yet. Only its scripted fake backend exercises it in this slice.

## Shared action lease

Built-in discrete actions and continuous invocations now acquire the same
coordinator claim.

- A discrete action running first causes a continuous start to fail with
  `action_busy`.
- A continuous invocation running first causes discrete dispatch to fail with
  `action_busy`.
- Completing, cancelling, rejecting, timing out, faulting, or shutting down a
  continuous invocation releases the claim.
- Direction handoff retains the invocation lease while the old segment sends
  `Cancel(direction_changed)` and the new segment sends its fresh `Begin`.
- Manual cancellation and application shutdown use the existing shared atomic
  stop state.

The action-executor shutdown wait now includes a continuous lease within the
existing two-second upper bound. It signals shutdown first, lets the
continuous worker attempt its bounded terminal callback, and only force-clears
the coordinator after the bound expires.

## Bounded worker transport

The continuous backend runs on a dedicated worker and never on the input,
interaction, or render thread.

| Path | Bound | Purpose |
| --- | ---: | --- |
| Latest scalar slot | 1 | Replace undelivered ordinary values |
| Handoff/terminal control queue | 8 | Keep control work separate from value traffic |
| Completion queue | 1 | Return the invocation's single completion |

Every publication replaced in the latest slot increases an observation count.
The worker passes that count to `ContinuousSession::observe_coalesced`, so the
plugin-visible sequence still jumps over discarded samples. A rapid
three-publication test delivers only the last scalar as sequence three.

The callback deadlines are the planned v0.3 limits:

- `Begin`: 100 ms.
- `Update`: 16 ms.
- `Commit`: 100 ms.
- Best-effort `Cancel`: 50 ms.

A backend callback receives the host deadline and the shared cancellation and
shutdown state. Returning after the deadline is also classified as a timeout,
even if the backend reports success.

## Lifecycle results

The scripted backend records the exact asynchronous request order and can
accept, reject, wait on a gate, stall to a deadline, or fault.

- Accepted `Begin` permits coalesced updates and exactly one terminal request.
- Release queued while `Begin` waits is delivered as `Begin`, then `Commit`,
  using the same session ID.
- Rejection after a queued release produces only `Begin`; no terminal callback
  is sent to a session the backend never accepted.
- A stalled `Begin` reaches its 100 ms deadline, produces no terminal callback,
  and releases the lease.
- A stalled or faulted `Update` sends one best-effort
  `Cancel(plugin_faulted)`, reports the original update failure, and releases
  the lease.
- Direction handoff records old `Begin`, old
  `Cancel(direction_changed)`, new `Begin`, and new terminal request, with a
  fresh segment UUID and one uninterrupted coordinator lease.
- **Cancel current action** sends `Cancel(user_requested)`.
- Shutdown sends `Cancel(shutdown)` and completes within the existing shutdown
  wait.

The nine asynchronous tests were also repeated ten times serially to check for
timing races before the full workspace gates.

## Verification

- `scripts/check.ps1`: passed, including 196 debug application tests and 70
  core tests, formatting, strict Clippy, script self-tests, and debug builds.
- `cargo test --release --workspace --all-features`: passed with 192 release
  application tests and 70 core tests.
- The nine focused asynchronous coordinator tests passed in ten consecutive
  serial runs.

## Follow-on boundary

The live gesture boundary described here was completed in the next bounded
slice. See [Milestone 1 live continuous bridge results](milestone-1-continuous-live-bridge-results.md).
Lua and schema six remain later work.
