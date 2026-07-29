# Milestone 2 profile-driven menu results

Date: 2026-07-29

This record covers the second bounded Milestone 2 slice: making the resolved
profile's complete menu authoritative for gesture recognition and overlay
rendering. It does not add actions, configuration editing, icons, or appearance
settings.

## Implemented contract

- Configuration loading compiles every profile menu into an immutable
  `ResolvedMenu` containing a typed four/eight slice count and the complete
  configured label list.
- Repeated resolutions of one profile reuse the same reference-counted menu.
  The menu no longer depends on the mutable deserialization model after
  configuration activation.
- Button-down completes the target snapshot and profile resolution before the
  interaction controller opens.
- The winning menu is attached to the active invocation. Its slice count drives
  selection geometry and the same menu is sent to the routed display overlay.
- Profile, slice count, and labels cannot change while an invocation is open.
- Routine logs record profile UUID, slice/label counts, resolution source, and
  cache choice without recording user-authored labels.

## Renderer cache

Every prewarmed display renderer now owns both supported geometry caches:

- four/eight boundary segments;
- four/eight label offsets;
- four/eight 24-segment selection-highlight arcs;
- one fixed eight-entry selection-transition state.

The renderer selects a cache from the immutable invocation menu and pairs its
label offsets with the configured label strings. It does not rebuild geometry,
resolve a profile, or read configuration on activation.

The first prepared overlay frame records the selected slice count and complete
label count. Existing focus, 60 ms opening, 35 ms selection, and 50 ms closing
contracts remain unchanged.

## Debug override boundary

`--slices=4|8` remains available in debug builds only for the repeatable
interaction harness. It substitutes a complete direction-labelled test menu
and is explicitly recorded as `menu_source=debug_override`.

Release builds reject `--slices` and always use
`menu_source=configuration`. Release interaction tests can therefore request
only a slice count already supplied by the resolved test profile.

## Automated coverage

The debug workspace suite contains 69 passing tests; the release suite contains
67 because debug-option tests are compiled out. New checks prove:

- a resolver freezes a custom four-label menu independently of later model
  mutation;
- repeated resolutions reuse that immutable menu;
- one gesture controller can consume four- and eight-slice resolved menus on
  consecutive invocations without retaining the prior geometry;
- every display renderer constructs both supported caches;
- renderer label layout uses the custom resolved-profile labels;
- incomplete resolved menus cannot be constructed;
- release option parsing rejects the debug slice override.

Formatting, strict Clippy in debug and release configurations, debug tests, and
release tests are part of the final local gate.

## Live Windows result

| Check | Result |
| --- | --- |
| Debug four-slice menu | All N/E/S/W selections passed |
| Debug authority record | `menu_source=debug_override`, four labels |
| Release eight-slice menu | All eight directions passed |
| Release authority record | `menu_source=configuration`, eight labels |
| Prepared renderer frames | Correct slice and complete label counts |
| Close/focus contract | Preserved; final debug/release runs observed 50 ms |

The final quick interaction matrix, run ID `20260729T110516492Z`, passed all 9
scenarios and 32 expected selections across center/corner directions, rapid
render-independent flicks, both connected monitors, topology rebuild, resume,
heartbeat recovery, and queue-overflow retry.

## Current boundary

Profiles now control the visible ring end to end, but every slice remains
action-free. Release logs a valid selected slice with
`action_executed=false`. The next bounded slice can introduce validated action
definitions and immutable execution-plan compilation without revisiting
profile matching or menu rendering.
