# Milestone 1 interaction matrix results

Date: 2026-07-29

This record covers the repeatable interaction-validation harness for the
remaining Milestone 1 gesture, focus, display/DPI, rapid-invocation, and input
recovery cases. The harness writes both human-readable progress and versioned
machine-readable JSON/CSV evidence. It reports hardware-dependent cases as
`unavailable` or `not_run`; it never treats an absent display arrangement or
physical event as a pass.

## Harness contract

- `smoke-interaction.ps1` can exercise all supported directions in one warm
  Gradial process from the center, each work-area edge midpoint, or each
  work-area corner.
- Four-slice runs validate N/E/S/W. Eight-slice runs validate
  N/NE/E/SE/S/SW/W/NW.
- Repeated and quick-flick invocations have explicit delay and count controls.
- Every selected invocation must match its expected slice and resolved geometry
  behavior.
- Every invocation must retain foreground focus at activation and after close.
- Edge-assisted invocations must retain the system cursor within two physical
  pixels of the activation point.
- Normal animated runs must finish the close lifecycle within the documented
  scheduler-tolerant bound.
- Display-change, resume, heartbeat-recovery, and queue-overflow recovery
  scenarios reuse the production paths already exposed by the smoke helper.
- `run-interaction-matrix.ps1` records host/display metadata, scenario results,
  coverage status, exit codes, arguments, and captured output under
  `Gradial APP\captures\interaction-matrix\<run-id>`.

The JSON format begins at `schema_version: 1`. `scenarios.csv` is suitable for
per-scenario analysis, while `coverage.csv` keeps passed, unavailable, and
not-run release-gate requirements distinct.

## Standard matrix result

The debug standard suite completed with run ID `20260729T095546079Z`:

| Measure | Result |
| --- | --- |
| Scenarios | 17 passed, 0 failed |
| Expected selected invocations | 107 |
| Center geometry | All four- and eight-slice directions passed |
| Corner geometry | 32 trials: all eight directions from all four corners |
| Edge geometry | All eight directions from all four edge midpoints |
| Rapid invocation | 25 quick flicks, 10 ms apart |
| Focus checkpoints | Activation and after-close checks passed for all 107 invocations |
| Display routing | Both connected 96-DPI monitors passed |
| Topology | Synthetic display cancellation, pool rebuild, and rewarm passed |
| Recovery | Resume, heartbeat replacement, and queue-overflow manual retry passed |

The run took 49.157 seconds on Windows 11 build 26100 with an Intel
Core i9-13900KF and NVIDIA GeForce RTX 4090. The connected layout contained a
2560x1440 primary display and a 1920x1080 secondary display, both at 96
effective DPI. The video-controller observation was 60 Hz.

A final quick-suite regression run against the completed implementation, ID
`20260729T100329531Z`, passed all 9 scenarios and 32 expected selected
invocations.

## Defect found by the matrix

The first repeated multi-direction run exposed a real close race: if a child
viewport missed its final repaint, an overlay could remain in its closing
state and a later invocation could overwrite it before the hide completed.

Close now requests a native child repaint and also registers its 50 ms deadline
with one pool-wide, bounded close coordinator. The coordinator verifies the
invocation identity before hiding and resetting a viewport. Hide requests and
state reset are serialized under the same viewport lock, preventing an old
close from hiding a newer invocation. Repeated 20-invocation stress runs passed
after the fix, with observed close completion at 50-51 ms.

## Hardware and release-gate gaps

The local machine could not supply mixed 100%/150% or 125%/200% DPI pairs, a
negative-coordinate monitor, or a display observed at 120 Hz or faster. These
requirements are recorded as `unavailable`, not passed.

The following remain explicit release-gate work:

- the extended 320-repetition automated corner-direction stress run;
- physical DPI, monitor plug/unplug, primary-display, sleep/resume,
  session-lock, and Explorer-restart exercises;
- the 1,000-invocation presented-frame latency capture;
- the 10,000-invocation focus campaign;
- the plan-required human direction-error and selection-time study, including
  at least 320 participant corner trials.

The extended suite defaults to ten repetitions of every direction at every
corner, producing 320 automated repetitions. That result validates
repeatability of the automated input path; it does not stand in for the plan's
320-trial participant dataset or human usability gates.
