# Milestone 2 action-model foundation results

Date: 2026-07-29

This record covers the third bounded Milestone 2 slice: schema-two built-in
actions, safe schema-one migration, complete action-reference validation, and
immutable release-time plan compilation. It deliberately does not execute a
plan or call an operating-system action API.

## Implemented contract

- Configuration schema two adds a top-level action catalog and one optional
  action UUID per menu slice.
- The catalog represents every v1 action family: shortcut, Unicode text, URL,
  file/folder, executable with an argument array and optional working
  directory, invocation-window command, delay, and reusable linear sequence.
- Configuration activation rejects duplicate action UUIDs, empty names,
  malformed action fields, missing menu or sequence references, direct and
  indirect cycles, more than four nested sequence levels, more than 32
  expanded leaf steps, and more than 30 seconds of statically known delays.
- Text is limited by UTF-8 byte length to 8 KiB. URLs permit only HTTP and
  HTTPS with a nonempty authority. Persisted paths must be absolute. Delays
  permit 10 through 10,000 ms.
- Action payload variants reject unknown JSON fields. Executable arguments
  remain an array; arbitrary shell command strings are not represented.

## Schema-one migration

A valid schema-one file is decoded through schema-two defaults, advanced to
schema two in memory, and then completely validated. Its action catalog and all
slice references are empty. The loader records `migrated_from_schema=1` but
does not write the migrated value during startup.

Schema-one files that already contain `actions` or `action_id` are rejected as
ambiguous instead of silently reinterpreted. The normal backup/default recovery
path applies, and the original primary remains untouched. A future explicit
successful save may persist the active schema-two value through the existing
atomic replacement path.

## Immutable release plan

The active action catalog is cloned into a platform-independent compiler after
configuration validation. When a gesture releases on a selected slice:

1. the action UUID is read from the invocation's frozen resolved menu;
2. the compiler requires the invocation's frozen `config_revision`;
3. reusable sequences are flattened in declared order;
4. owned action payloads are copied into an immutable execution plan;
5. the plan records the 5-second per-step timeout, 30-second duration ceiling,
   root action UUID, source UUID for every step, and maximum validated size.

No plan borrows from the deserialized configuration. Later model mutation
cannot change an already compiled catalog or plan.

## Safety and diagnostics boundary

There is no executor in this slice. Neither release nor compilation performs
keyboard injection, text entry, shell association, process creation,
window-management calls, or delay waits.

An assigned slice records `action_plan_compiled` with UUIDs, revision, step
count, and timing ceilings only. An unassigned slice records cancellation with
`reason=EmptySlice`. Both preserve the existing close/focus lifecycle and
record `action_executed=false`. Typed text, URLs, paths, and executable
arguments are never written to routine logs.

## Automated coverage

New unit coverage proves:

- every leaf action compiles to exactly one owned step without execution;
- nested sequences flatten in declared order;
- schema-one migration changes only memory and retains exact source bytes;
- schema-one files cannot smuggle schema-two action fields;
- broken menu and sequence references are rejected;
- direct/indirect cycles, depth, expanded-step, and delay-duration limits are
  enforced;
- malformed shortcut, text, URL, path, argument, and delay fields report
  field-specific validation failures;
- unknown action payload fields are rejected;
- compiler/catalog data remains unchanged after the source model mutates;
- revision mismatch prevents plan compilation;
- release attaches the frozen slice action using the invocation revision.

The live interaction harness additionally requires one non-executing action
outcome for every selected gesture and fails if any record contains
`action_executed=true`.

The final debug workspace suite contains 85 passing tests; the release suite
contains 83 because debug-option tests are compiled out. PowerShell syntax,
formatting, strict Clippy, debug/release tests, and debug/release builds all
passed.

## Live Windows result

| Check | Result |
| --- | --- |
| Existing schema-one primary | Loaded as schema two with `migrated_from_schema=1` and zero actions |
| Debug four-slice menu | All N/E/S/W selections passed |
| Release eight-slice menu | All eight directions passed |
| Empty configured slices | One `EmptySlice` action outcome per selected gesture |
| Execution boundary | Every outcome recorded `action_executed=false`; none recorded true |
| Close/focus contract | Preserved; debug and release runs observed 50 ms close |

The final quick interaction matrix, run ID `20260729T121740253Z`, passed all 9
scenarios and 32 expected selections across center/corner directions, rapid
render-independent flicks, both connected monitors, topology rebuild, resume,
heartbeat recovery, and queue-overflow retry.

## Current boundary

Action selection is now deterministic and safely compiled, but no action can
run. Target freshness/integrity validation, the single-plan executor,
cancellation/timeout behavior, result reporting, tray activity, and every
Win32 action backend remain future bounded slices.
