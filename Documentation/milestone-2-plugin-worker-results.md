# Milestone 2 plugin worker results

Date: 2026-08-10

## Outcome

Gradial now has one dedicated worker thread that exclusively owns every Lua
state and the mutable machine-local plugin trust snapshot. Normal startup may
load a previously enabled plugin, but only after fresh complete discovery and
an exact effective-status check. Merely copying a package into the managed
directory still executes no code.

The worker is connected to application lifetime and the Plugins Settings page,
but not to the continuous coordinator. It exposes no OS capability and cannot
currently run a lifecycle callback after load-time validation.

## Admission transaction

Approval, enablement, and reload remain separate. Enable and reload perform:

1. fresh bounded package discovery and trust reconciliation;
2. compatible frozen-source selection by a validated plugin ID;
3. current permission/status validation;
4. sandboxed entry evaluation and all-four-callback validation;
5. atomic successful-load recording for that exact fingerprint; and
6. adapter admission only after persistence succeeds.

The package files are not reopened by Lua. A file edit after an earlier scan
cannot replace the frozen image used by the transaction. If callback
validation succeeds but the state save fails, the candidate adapter is
destroyed and the prior disabled state remains authoritative.

Startup uses the same effective trust rule. It loads only exact `enabled`
records. Disabled, permission-changed, source-changed, incompatible, invalid,
missing, and faulted packages do not construct a state. A trusted package that
fails startup validation is unloaded and atomically marked faulted. If that
write fails, it remains unloaded for the current process and Gradial records a
stable persistence failure rather than claiming durable success.

## Reload, fault, and teardown

A fresh scan unloads an adapter whose effective status is no longer enabled.
Permission changes are persisted as disabled before reapproval can proceed.
Source edits retain approval but require explicit reload. Reload removes the
observed old/faulted adapter, validates a replacement, and clears fault state
only after the new fingerprint is saved successfully.

Explicit fault and disable requests destroy the current adapter before their
state transition. This makes the running process fail-closed if persistence
then fails. The durable error is reported so the caller does not claim that
the on-disk change succeeded.

## Thread and queue boundary

The worker command queue holds eight pending operations. Submission never
blocks the caller and returns a one-result ticket for future UI polling.
Malformed plugin IDs, queue saturation, shutdown, and disconnection fail
before worker execution. Commands are serialized, so state writes and adapter
replacement cannot race each other.

A shared shutdown flag bypasses a full command queue. The current bounded Lua
load may finish, queued ordinary work is abandoned, all adapters are dropped,
and application shutdown waits at most two seconds. No discovery, Lua work, or
plugin-state write occurs on the input, interaction, render, or action-executor
threads.

## Evidence

Focused fixtures cover:

- startup admission of only effectively enabled packages;
- rejection before source evaluation when enable is requested without current
  permission approval;
- startup faulting when a persisted enabled package cannot validate;
- separate approval and successful enable transactions;
- fresh-source callback validation before enablement is recorded;
- candidate destruction when atomic persistence fails after validation;
- permission-change unload and automatic disablement;
- valid source edits becoming active only after manual reload;
- failed reload faulting, unloading, and later explicit recovery;
- durable explicit fault, reload, and disable teardown;
- invalid submission IDs and bounded-queue rejection;
- shutdown under queued work and a bounded nonterminating Lua load; and
- plugin-root failure admitting no persisted enabled package.

The later Plugins Settings slice also covers code-free explicit rescan and
automatic unloading when changed source invalidates an admitted package.

The complete repository check passed PowerShell analyzer self-tests,
formatting, warning-denied linting, all-feature tests, documentation tests, and
the debug workspace build. After the lifecycle bridge slice, the debug suites
pass 249 application, 81 core, and 22 Lua tests. The independent optimized
suites pass 245 application, 81 core, and 22 Lua tests.

## Superseded deferred boundary

The Plugins Settings page now provides the production permission-review,
enable, disable, reload, rescan, and managed-folder controls described in
`milestone-2-plugin-settings-results.md`. The isolated Begin/Update/Commit/Cancel
command boundary is implemented in
`milestone-2-plugin-lifecycle-bridge-results.md`. There is still no live radial
caller, continuous action catalog connection, or capability broker. Production
adapters remain deliberately idle after validation; those boundaries remain
separate future slices.
