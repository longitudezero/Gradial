# Milestone 1 four/eight geometry results

Date: 2026-07-28

This record covers the bounded Milestone 1 four/eight geometry slice. Eight
slices remain the default. `--slices=4|8` is a startup-only development
override; persisted per-profile selection remains deferred.

## Automated checks

`scripts/check.ps1` passed after the implementation:

- PowerShell syntax validation.
- Rust formatting.
- Clippy with warnings denied.
- 35 unit tests.
- Documentation tests.
- Debug workspace build.

New tests cover both command-line forms, invalid counts, the four-slice
N/E/S/W renderer contract, 90-degree highlight geometry, out-of-range labels,
and the configured gesture controller selecting east as slice 1. Existing
four/eight center, boundary, hysteresis, and direction tests continue to pass.

## Live Windows checks

| Check | Result |
| --- | --- |
| Four-slice fixed-origin east gesture | Pass; selected slice 1 |
| Four-slice edge-assist east gesture | Pass; selected slice 1 and retained cursor |
| Four-slice display rebuild while M4 was held | Pass; cancelled, rewarmed, and accepted a new gesture |
| Default eight-slice fixed-origin east gesture | Pass; selected slice 2 |
| Focus checkpoints through close | Pass |

The live helper confirms the selected startup count in the log before accepting
the result. It then checks the eastward slice index appropriate to that
geometry.

## Remaining validation

Profile-backed slice selection, real icons/actions, animation limits, the
complete mixed-DPI hardware matrix, and the required high-volume latency/focus
campaigns remain future work.
