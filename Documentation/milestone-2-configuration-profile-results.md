# Milestone 2 configuration and profile foundation results

Date: 2026-07-29

This record covers the first bounded Milestone 2 slice: schema-versioned,
action-free configuration; atomic primary/backup persistence; complete
invocation target snapshots; and deterministic profile resolution. It does not
add action definitions, action execution, profile-driven rendering, or the
settings editor.

## Implemented contract

- First startup creates `%LOCALAPPDATA%\Gradial\config.json` with
  `schema_version: 1`, revision 1, a stable global-profile UUID, and a complete
  default eight-slice menu.
- The in-memory model validates its schema, unique profile UUIDs, profile
  names, four/eight menu length, required executable paths, priority range,
  non-empty optional predicates, and title regex syntax.
- A proposed save is completely validated and serialized before disk changes.
  Its temporary file is written beside the primary and flushed.
- Existing configurations are exchanged through Win32 `ReplaceFileW`, which
  atomically installs the new primary and preserves the prior valid primary as
  `config.json.bak`. First initialization uses a same-volume, write-through
  move.
- `config_revision` is prepared in the serialized candidate but becomes the
  running revision only when replacement succeeds.
- An invalid primary loads a valid backup without changing either file. If
  neither is usable, Gradial runs its built-in default in memory and retains
  the broken files for later recovery.
- Missing, invalid, unknown, and newer schema files have distinct safe load
  paths. The current no-editor build records the persistent recovery warning in
  the local log.

The schema intentionally contains only global/application profiles and their
complete menu labels. Action IDs and definitions are excluded rather than
accepted as unvalidated placeholders; they will enter the schema with the
bounded executor slice.

## Invocation snapshot

The immutable activation snapshot now includes:

- foreground window handle and process ID;
- process creation time and optional full executable path;
- window class and title;
- signed cursor coordinate;
- monitor device name, monitor bounds, work area, and effective DPI;
- physical Ctrl/Alt/Shift state captured at the low-level button-down event;
- activation binding ID;
- configuration revision and gesture geometry.

Process, window, and display queries run after the hook callback on the
interaction thread. Profile resolution happens only after the snapshot is
complete. Logs record availability flags and IDs, but do not record executable
paths, window classes, or window titles.

## Profile resolver

Rules are validated and their Rust regular expressions are retained in the
compiled resolver when configuration loads. Invocation-time matching then
follows the plan exactly:

1. case-insensitive exact normalized executable path;
2. optional case-insensitive exact window class;
3. optional Rust title regex;
4. highest priority;
5. most populated predicates;
6. lexicographically smallest immutable profile UUID;
7. global profile when no application profile is eligible.

If the executable path cannot be read, application profiles are not considered.
Each result includes the winning profile, configured slice count, all matching
profile IDs with priority/specificity, and the exact tie-break decision. The
live diagnostic record includes only non-sensitive resolution data.

## Automated results

The workspace unit suite contains 67 passing tests. New coverage verifies:

- valid default configuration and every current field constraint;
- duplicate UUID, invalid regex, priority, and menu-shape rejection;
- path separator/case normalization and class matching;
- title-regex opt-in behavior;
- priority, specificity, and UUID tie-breaking;
- global fallback when no rule matches or the executable path is unavailable;
- first-run configuration initialization;
- revision advancement only after successful replacement;
- previous-primary backup preservation;
- interruption after temporary-file flush leaving the primary unchanged;
- broken-primary backup recovery without overwriting the broken file;
- safe handling of newer schema versions;
- invalid candidates leaving disk and the running revision unchanged.

Formatting, strict Clippy with warnings denied, and release compilation are
part of the final local gate.

## Live Windows result

An eight-direction fixed-origin run passed with revision 1:

| Check | Result |
| --- | --- |
| Default configuration initialization | Pass; schema 1, revision 1 |
| Complete target snapshot | Pass for all eight invocations |
| Process start/path, window class/title, monitor device | Available in the test target |
| Modifier/binding freeze | Ctrl/Alt/Shift false; Mouse Button 4 |
| Profile resolution | Stable global UUID selected for all eight invocations |
| Slice selection and close | All eight directions passed; observed close 50 ms |
| Foreground focus | Preserved |

The final quick interaction matrix, run ID `20260729T104016950Z`, also passed
all 9 scenarios and 32 expected selected invocations across center/corner,
rapid use, both connected monitors, topology rebuild, resume, heartbeat
recovery, and queue-overflow retry. Every smoke scenario now requires validated
schema loading, complete snapshot records, and profile-resolution records.

## Current boundary

The resolver is active and its winner is attached to the interaction lifecycle
diagnostics. The development `--slices=4|8` startup option still controls
gesture and renderer geometry. Profile menu labels/slice count do not alter the
visible ring yet. That separation keeps this persistence/resolution slice
bounded and prevents configuration from driving only half of the renderer.

The next profile-facing slice must make the resolved complete menu immutable
for the invocation and teach each prewarmed viewport to cache/render both
four- and eight-slice profile menus. Actions and the editor remain separate.
