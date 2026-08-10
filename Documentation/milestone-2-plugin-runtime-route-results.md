# Milestone 2 live plugin runtime route results

Date: 2026-08-10

## Outcome

Configured schema-six continuous actions can now drive an explicitly enabled
local Lua plugin through the production radial interaction. This completes the
remaining Milestone 2 runtime connection without adding an operating-system
capability or a plugin-action editor.

At invocation-down, Gradial freezes the resolved menu's continuous action
bindings and consults an immutable catalog containing only actions from
currently admitted plugin adapters. The catalog supplies the manifest-owned
update cadence. A configured reference becomes continuous-capable only when
its exact plugin ID and manifest action ID are present. Missing, disabled,
faulted, changed, incompatible, or undeclared references remain ordinary
discrete wedges beyond the outer edge.

The portable configuration remains authoritative for the plugin ID, action ID,
parameter values, menu assignment, and configuration revision. Local plugin
state remains authoritative for admission and manifest cadence. Both sources
are combined into an owned `ContinuousActionSnapshot` before the gesture can
begin a segment.

## Callback route and deadlines

The existing continuous coordinator still owns coalescing, segment handoff,
terminal ordering, and the single action lease. Its plugin backend submits the
resulting Begin, Update, Commit, and Cancel requests through the worker's
separate bounded lanes and waits only within the existing stage deadlines:

- Begin: 100 ms.
- Update: 16 ms.
- Commit: 100 ms.
- Cancel: 50 ms.

The backend maps worker admission failures, queue failures, Lua faults, and Lua
deadline failures into the coordinator's stable rejected, faulted, interrupted,
or timed-out outcomes. Lua execution remains entirely on the plugin worker;
neither the hook callback, interaction thread, renderer, nor action coordinator
runs plugin code.

A Begin can time out while it is queued behind worker work. The backend now
uses the reserved terminal lane to abort that exact session ID. If the abort is
observed first, the later Begin is rejected before Lua. If Lua has already
accepted Begin, the worker performs the matching bounded Cancel. This prevents
a late Begin from leaving an orphan worker-owned session after the coordinator
has already failed it.

## Target and privacy boundary

Begin performs complete captured-target validation. Update and Commit repeat
the lightweight foreground identity/freshness validation. Cancel never depends
on a still-valid target, so target loss cannot prevent best-effort session
cleanup.

Milestone 3 permission filtering and capabilities are deliberately absent.
Lua receives no application path, window class, or title in this slice, even
when its manifest requests those context permissions. It also receives no
filesystem, process, input, notification, network, window, registry, clipboard,
or renderer service.

## Reload and disable ordering

Reload and Disable from the Plugins page no longer race an active callback.
The application asks the interaction runtime to make the current continuous
coordinator terminal with `reload_requested` or `plugin_disabled`, moves that
coordinator into a private completion wait, and submits the management
operation only after the terminal callback has completed or reached its bound.
The held radial invocation cannot resume another continuous segment after this
management cancellation, but its Gradial-owned visual/discrete interaction can
finish normally.

A bounded 200 ms retry window covers a coordinator that had already detached
from interaction state while its terminal callback was finishing. If the
worker still reports an active session after that window, the operation fails
visibly instead of unloading Lua state without its terminal callback.

## Evidence

Focused automated coverage proves:

- only loaded adapters publish manifest actions into the runtime catalog;
- disabled plugins publish no callable action;
- the manifest update cadence and configured parameter values are frozen into
  the invocation snapshot;
- the latest scalar published while Begin is still pending is retained and
  delivered after Begin acceptance with a zero-floor elapsed timestamp;
- an unavailable catalog entry produces no continuous-capable slice;
- an undeclared action ID is rejected before Lua Begin;
- queued Begin abortion prevents later Lua admission;
- the terminal lane remains usable while the Update lane is full;
- callback failures fault and unload the adapter; and
- lifecycle/queue errors map to the coordinator's closed outcome contract.

The complete repository check covers formatting, warning-denied Clippy,
PowerShell analyzer self-tests, all-feature debug and release suites,
documentation tests, and the debug workspace build. The debug suites pass 253
application, 83 core, and 22 Lua tests. The optimized suites pass 249
application, 83 core, and 22 Lua tests.

## Historical deferred boundary

At the time of this Milestone 2 result, the Settings action catalog could not
create or edit a plugin continuous action, and its menu editor could not assign
the separate continuous extension slot. Manifest-specific parameter
requirements, types, ranges, defaults, and choices were not yet validated
against portable action parameters. Valid schema-six references imported or
written outside the editor could run.

Permission-filtered context, `input.shortcut`, notifications, loopback network,
generic parameter controls, dependency summaries, and all examples that
produce external effects remain Milestone 3 work.

This historical editor boundary was completed by
`milestone-3-plugin-action-editor-results.md`. Permission-filtered context,
host capabilities, dependency summaries, and external-effect examples remain
deferred.
