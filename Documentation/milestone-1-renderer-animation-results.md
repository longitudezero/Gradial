# Milestone 1 renderer cache and animation results

Date: 2026-07-29

This record covers the bounded Milestone 1 renderer slice: persistent
per-monitor geometry caches, bounded open/selection/close transitions, forced
hide safety, and the temporary reduced-motion development override.

## Implemented contract

- Each prewarmed monitor viewport owns one persistent renderer cache for its
  four- or eight-slice geometry.
- Boundaries, label offsets, and every slice's 24-segment highlight arc are
  generated when the overlay pool is built rather than during activation.
- The first opening frame contains the complete ring at 96% scale and 85%
  opacity. Cubic ease-out reaches the final state at 60 ms.
- Selection color interpolates over 35 ms and retargets from the currently
  displayed value.
- Release completes gesture recognition immediately. The click-through overlay
  fades to zero over 50 ms and is then hidden.
- Pool destruction, display rebuild, and an overlapping new invocation retain
  an immediate native hide fallback.
- `--reduced-motion` removes interpolation while preserving the 60 ms opening
  state and 50 ms close lifecycle.
- Standard animation schedules frames at an 8 ms minimum interval. Reduced
  motion schedules only lifecycle completion frames.

## Automated checks

The complete local gate passes with 50 unit tests. New coverage verifies:

- exact 0/60 ms opening values and the 85% first-frame floor;
- exact 0/50 ms closing values;
- the 35 ms selection endpoint;
- reduced-motion visual snapping without shortened lifecycle timing;
- four/eight cache entry counts and 24 segments per highlight arc;
- mid-transition selection retargeting;
- combined parsing of `--reduced-motion`, geometry, and slice-count options.

Formatting and strict Clippy pass with warnings denied. The debug and release
workspace configurations compile.

## Live Windows checks

| Check | Result |
| --- | --- |
| Standard fixed-origin gesture | Pass; observed close lifecycle 54 ms |
| Reduced-motion fixed-origin gesture | Pass; observed close lifecycle 53 ms |
| Quick flick with no animation wait | Pass; east slice selected before rendering was required |
| Display change during held M4 | Pass; active pool forced hidden, rebuilt, and rewarmed |
| Foreground focus after animated close | Pass |
| Renderer cache/timing contract in local diagnostics | Pass |

Observed close values include Windows scheduling and asynchronous local logging;
the renderer's interpolation itself clamps to its pure 50 ms endpoint.

## Remaining validation

PresentMon correlation must still confirm that the first actually presented
frame meets the 85% opacity requirement across the complete display matrix.
The long-run latency/focus campaign, animation behavior under GPU pressure, and
appearance settings backed by persistent configuration remain future release
gates.
