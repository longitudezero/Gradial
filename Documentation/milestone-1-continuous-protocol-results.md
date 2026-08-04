# Milestone 1 runtime-neutral continuous protocol results

Date: 2026-08-04

## Scope

This is the first bounded Milestone 1 slice from the v0.3 plan. It introduces
the protocol vocabulary and a pure host-side lifecycle for one continuous
outside-wedge segment. It does not add Lua, schema six, plugin discovery,
capabilities, action-coordinator integration, or operating-system effects.

## Protocol boundary

The core exports version-one `Begin`, `Update`, `Commit`, and `Cancel` request
types. Request fields are read-only outside the module, so adapters consume a
validated request rather than assembling an invalid sample themselves.

- Plugin and action IDs use the closed lowercase ASCII grammar and a 96-byte
  limit from the v0.3 plan.
- Scalars must be finite and inclusive of `0.0..=1.0`.
- Numeric plugin parameters must be finite. Manifest-specific parameter types,
  ranges, and payload-size limits remain configuration/runtime work.
- Update cadence is validated from 10 through 60 Hz; the default constant is
  30 Hz.
- A non-nil UUID identifies each segment.
- Permission-filtered context owns only explicitly allowed application and
  title data. It cannot carry native handles, process identity, coordinates,
  or a mutable target reference.

## Lifecycle and backpressure

| Host state | Accepted operation | Result |
| --- | --- | --- |
| Begin pending | Observe | Replace the single pending ordinary sample |
| Begin pending | Commit or Cancel | Retain the exact terminal decision |
| Begin pending | Accept | Become active, or emit the retained terminal immediately |
| Begin pending | Reject | Terminate with no terminal callback |
| Active | Observe | Replace the single pending ordinary sample |
| Active | Cadence due | Emit the latest complete `Update` |
| Active | Commit or Cancel | Emit exactly one terminal request and terminate |
| Terminated | Any delivery attempt | Emit nothing; mutations fail closed |

The initial sample is sequence zero. Every observation and final commit gets a
strictly increasing sequence even when intermediate ordinary samples are
replaced. This lets an adapter detect loss without requiring every physical
mouse sample. `delta` is calculated against the previously delivered value,
not against a discarded sample. `Commit` supersedes an ordinary pending update
but preserves the exact final scalar. `Cancel` drops ordinary pending work.

If release or cancellation occurs while `Begin` is pending, acceptance yields
the retained terminal request immediately after the already-issued `Begin`.
Rejection yields no terminal callback because the plugin never accepted a
session. Once a terminal decision exists, later updates and duplicate terminal
requests cannot be generated.

## Verification

Deterministic unit coverage includes:

- identifier, API input, update-rate, parameter-number, UUID, and scalar
  rejection;
- owned permission-filtered `Begin` snapshots;
- latest-value replacement and observable sequence gaps;
- cadence enforcement and elapsed time from accepted `Begin`;
- rejection of observation time before acceptance or earlier than the prior
  sample;
- exact commit superseding a pending update;
- active and pending-Begin cancellation;
- accept/reject races after a pending terminal decision;
- absence of updates and duplicate terminals after termination; and
- 10,000 complete sessions with no retained work.

The full workspace formatting, strict Clippy, debug tests, release tests, and
build checks passed. The recorded totals are 187 debug application tests, 183
release application tests, and 70 core tests in both modes. The core total
includes the deterministic 10,000-session lifecycle loop.

## Deferred boundary

The next slice should place a single continuous-session lease in the action
coordinator and connect gesture effects through a fake asynchronous backend.
That is where direction handoff, target invalidation, backend rejection,
stall/timeout, and shutdown ordering can be verified without introducing Lua.
