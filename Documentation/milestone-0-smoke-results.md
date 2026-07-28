# Milestone 0 smoke results

These results record implementation-level evidence for the interaction spike.
They are not the release-gate dataset defined in Sections 2 and 9 of the v0.2
plan.

## 2026-07-28 local run

Verified:

- 26 automated Rust tests passed.
- Strict Clippy passed with warnings denied.
- Debug fixed-origin live smoke selected east, preserved foreground identity,
  and closed cleanly.
- Debug automatic mode resolved a primary-work-area corner to edge assist and
  selected east through the raw-delta path.
- Debug and release edge-assist live smokes consumed raw input, selected east,
  kept the system cursor within the 2 physical-pixel limit, preserved foreground
  identity at all three checkpoints, and closed cleanly.
- The ETW marker stream decoded through `tracerpt` with one invocation UUID
  spanning activation through overlay hide.
- Maximum release-build hook callback time logged below the one-microsecond
  integer reporting resolution for the single smoke invocation.

The optimized edge-assist marker trace reported:

| Marker interval | Observed |
| --- | ---: |
| Activation dequeued to overlay shown | 0.676 ms |
| Activation dequeued to next-frame checkpoint | 1.125 ms |
| Highlight requested to applied | 0.078 ms |
| Release dequeued to close requested | 0.042 ms |
| Close requested to overlay hidden | 0.681 ms |

These are ETW application-marker intervals from one short run. They demonstrate
that the hidden-window 100 ms repaint throttle is no longer on the open path,
but they do not measure when pixels reached the display.

Still required before treating the v0.2 release gates as passed:

- Elevated PresentMon correlation with displayed frames.
- Warm-process runs of at least 1,000 invocations per hardware/display
  configuration.
- The full monitor, DPI, refresh-rate, and hardware matrix.
- At least 320 corner trials for direction error and release-time comparison.
- The 10,000-invocation focus campaign and 24-hour reliability soak.
