# Milestone 1 overlay-pool results

Date: 2026-07-28

This record covers the first bounded Milestone 1 slice: one prewarmed overlay
window per active display, signed monitor routing, and cancellation/rebuild on
display, DPI, or work-area changes. It does not claim the remaining production
interaction-core work is complete.

## Automated checks

`scripts/check.ps1` passed after the implementation:

- PowerShell syntax validation.
- Rust formatting.
- Clippy with warnings denied.
- 29 unit tests.
- Documentation tests.
- Debug workspace build.

New unit coverage verifies monitor routing with negative coordinates and nearest
fallback, hidden-window centering on a negative virtual screen, and pass-through
of new M4 presses while input is suspended for a rebuild.

## Live Windows checks

The development executable was tested on the two displays connected to this
machine. Windows reported both at 96 DPI.

| Check | Result |
| --- | --- |
| Fixed-origin M4 gesture on display 0 | Pass |
| Fixed-origin M4 gesture on display 1 | Pass; invocation routed to `monitor_index=1` |
| Edge-assist M4 gesture | Pass |
| Focus retained through open, first-frame checkpoint, and close | Pass |
| Display notification while M4 was held | Pass; invocation cancelled with `DisplayTopologyChange` |
| Pool replacement after notification | Pass; two slots rewarmed and `generation=1` became ready |
| New M4 gesture after replacement | Pass |

The display-change check posts `WM_DISPLAYCHANGE` to the isolated test process.
It exercises the real native signal, active-invocation cancellation, hook
teardown, monitor re-enumeration, deferred-viewport replacement, DPI check,
input restart, and a subsequent gesture. It does not physically change the
connected hardware or scaling.

## Remaining validation

The v0.2 release gates still require real add/remove, primary-display,
work-area, and mixed-DPI changes; 100%, 125%, 150%, 175%, and 200% scaling; a
secondary display above and left of primary; elevated PresentMon correlation;
and the specified high-volume latency and focus campaigns.
