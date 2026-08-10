# Milestone 2 Lua lifecycle adapter results

Date: 2026-08-10

## Outcome

Gradial now has a strict adapter between its runtime-neutral continuous
protocol and the restricted Lua 5.4 state. It can validate and exercise
`begin`, `update`, `commit`, and `cancel` in isolated host tests without
enabling a plugin, exposing an OS capability, or connecting Lua to the live
continuous coordinator.

`LoadedLuaPlugin` remains the lower-level sandbox foundation.
`LuaLifecycleAdapter` is the lifecycle boundary and refuses an entry table
whose required callback is absent or is not a function.

## Callback contract

The adapter implements this exact return contract:

```lua
function plugin.begin(context, parameters, initial_sample)
    return {}
end

function plugin.update(session, sample) end
function plugin.commit(session, final_sample) end
function plugin.cancel(session, reason) end
```

`begin` must return exactly one table. The other callbacks must return no
values. A wrong shape becomes a stable, source-free plugin error.

The table returned from `begin` is retained in the Lua registry under the
opaque continuous session ID. It is never returned to another session. A
terminal request removes the registry entry before calling Lua, guaranteeing
that commit/cancel cannot be delivered twice through this adapter. Unknown and
duplicate session requests fail without faulting otherwise healthy plugin
code.

## Immutable host values

The adapter translates owned `gradial-core` snapshots into inert values:

- context: API version, opaque session ID, action ID, and the already
  permission-filtered application and window-title fields;
- parameters: boolean, integer, number, string, and string-valued choices;
- samples: sequence, scalar value, delta, and elapsed milliseconds; and
- cancellation: a stable snake-case reason.

All host tables, including nested application context, are protected read-only
proxies. Normal assignment fails, the metatable is hidden, and the custom
`pairs` iterator does not expose its backing table. The plugin's own session
table remains mutable by design.

## Per-phase containment

Every callback reuses the host-owned global instruction hook and allocator
limit from the sandbox foundation. Default ceilings are:

| Phase | Wall deadline | Instruction ceiling |
| --- | ---: | ---: |
| `begin` | 100 ms | 1,000,000 |
| `update` | 16 ms | 160,000 |
| `commit` | 100 ms | 1,000,000 |
| `cancel` | 50 ms | 500,000 |

Custom limits may lower a phase for deterministic tests but cannot raise these
ceilings. A hook interval remains 1,000 instructions. Callback runtime,
memory, deadline, instruction, or return-shape failure faults the adapter and
removes all session tables. Later requests receive the stable
`plugin_faulted` result.

## Isolation evidence

Focused fixtures prove:

- all four function-valued callbacks are mandatory;
- context, nested context, parameters, and samples cannot be modified;
- parameter iteration cannot reveal the backing table;
- the callback receives exact protocol values and stable cancel reasons;
- two active session tables retain independent state;
- commit and cancel remove their state before invocation;
- duplicate/unknown sessions are host-state errors rather than plugin faults;
- non-table begin state and nonempty later returns fault and clear sessions;
- each phase reaches its own instruction ceiling;
- callback wall expiry remains distinct from instruction exhaustion; and
- the adapter is sendable and its debug output contains no Lua content.

## Repository validation

The complete PowerShell check passed harness self-tests, formatting, strict
Clippy, all-feature tests, documentation tests, and a debug workspace build.
The debug suite passed 217 application tests, 81 core tests, and 22 Lua-host
tests. The independent all-feature release suite passed 213 application tests,
81 core tests, and the same 22 Lua-host tests.

## Superseded deferred boundary

The dedicated plugin worker now owns production `LuaLifecycleAdapter`
construction and validated enable/reload admission; see
`milestone-2-plugin-worker-results.md`. Settings UI, coordinator callback
routing, capabilities, and OS effects remain later boundaries.
