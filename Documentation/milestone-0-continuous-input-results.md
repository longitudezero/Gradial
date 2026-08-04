# Milestone 0 continuous-input prototype results

Date: 2026-07-30

Live-direction revision: 2026-08-03

Hybrid-radius revision: 2026-08-04

Immediate-feedback revision: 2026-08-04

This record covers the first bounded v0.3 slice plus its live-direction and
hybrid-radius revisions: continuous radial geometry, production-overlay
feedback, extended edge assist, and a debug-only no-side-effect meter. It
deliberately introduces no Lua runtime, plugin manifest, plugin configuration,
or application-specific effect.

## Implemented contract

- Each continuous-capable wedge is hybrid: release inside its visible button
  ring selects the normal discrete action, while movement beyond the outer edge
  exposes the continuous scalar.
- The outer-edge boundary is 90.95 DIP for Compact, 107 DIP for Standard, and
  123.05 DIP for Large. Radial travel from that boundary through the current
  160-DIP endpoint maps linearly to an unsigned `0.0..=1.0` scalar and clamps
  outside that range.
- Angle continuously selects the live wedge at every radius. Returning inside
  the outer edge immediately removes continuous mode; moving outside restores
  it without retaining a prior direction or mode.
- Selection highlighting is immediate in every menu. Direction and mode
  changes bypass visual throttling; same-wedge value-only updates retain the
  latest sample and publish at no more than 240 Hz.
- Release uses the live state: outside commits the current wedge and scalar;
  inside selects the current discrete wedge; center release has no selection.
- The renderer displays a radial track, a position marker, and a center
  percentage using the production overlay path. Edge assist also displays a
  Gradial-owned pointer proxy from a full-scale-clamped DIP offset.
- The prewarmed overlay and edge-assist extent expand from the discrete
  288-DIP square to a 400-DIP square only for the continuous prototype.
  Existing discrete selection geometry remains unchanged.
- Debug-only `--continuous-meter` gives each resolved menu slice a continuous
  extension for that invocation. Both continuous commits and inside-ring
  discrete selections are logged as inert evidence and cannot compile or
  dispatch the saved slice action.
- Onboarding practice remains discrete, and release builds reject the
  development option.

The continuous state and normalization live in `gradial-core`; they are not
coupled to Lua. This preserves the runtime-neutral boundary required before a
plugin adapter is introduced.

## Automated checks

The workspace test suite passes with 184 debug application tests, 181 release
application tests, and 60 core tests in each configuration.
The new coverage verifies:

- normalization at 0%, 50%, and 100%, including clamping and invalid input;
- live outside direction changes and final-wedge continuous commits;
- outward-to-inward reversal with discrete release in the same wedge;
- current-radius mode selection without direction or mode latching;
- equivalent normalized values at different DPI scales;
- signed edge-assist pointer feedback and full-scale visual clamping;
- 107/133.5/160-DIP renderer marker positions for Standard size;
- continuous mode's 200-DIP edge-assist extent while discrete mode retains
  its 144-DIP half extent;
- rejection of `--continuous-meter` by release builds; and
- the invariant that neither branch of the inert hybrid meter compiles a saved
  action.

The PowerShell interaction scripts also pass parser validation.

## Live Windows matrix

The original direction-lock baseline completed with run ID
`20260730T191727072Z`:

| Measure | Result |
| --- | --- |
| Scenarios | 10 passed, 0 failed |
| Inert continuous commits | 76 |
| Eight-slice center values | 0%, 25%, 50%, 75%, and 100% in all eight directions |
| Four-slice center value | 50% in N/E/S/W |
| Corner edge assist | 32 trials: all eight directions from all four corners at 75% |
| Cursor retention at corners | 0 physical pixels observed |
| Scalar accuracy | Every commit within 0.02 of its requested normalized target |
| Foreground focus | Activation and after-close checks passed for all invocations |
| Saved-action isolation | No discrete selection, action compilation, or action dispatch observed |
| Close lifecycle | 50–51 ms observed |

That run took 37.393 seconds on one 1920x1080, 96-DPI display. It writes
machine-readable JSON plus scenario and coverage CSV files under
`Gradial APP\captures\continuous-matrix\<run-id>`. Those generated captures are
ignored by Git.

Two failures seen while developing the matrix were confined to cursor
quantization in the test harness. Integer diagonal coordinates could fall just
inside the exact 48-DIP boundary, and a protective pixel applied at every
target could exceed the strict 0.02 tolerance. The final harness uses an
outward guard only for the 0% cardinal boundary, outward diagonal rounding,
and exact fixed-origin cursor correction. The final quick and standard runs
both pass.

The live-direction revision then completed the standard suite with run ID
`20260803T194459363Z`:

| Measure | Result |
| --- | --- |
| Scenarios | 11 passed, 0 failed |
| Inert continuous commits | 77 |
| Armed direction change | Armed N at 50%, changed to W at the same radius, committed W |
| Four/eight directions and target values | Passed |
| Every corner under edge assist | Passed |
| Saved-action isolation and focus checks | Passed |

The revised run took 40.978 seconds on the same 1920x1080, 96-DPI display and
supersedes the direction-lock baseline for current interaction behavior.

The fixed-origin feedback-coalescing revision completed the standard suite
with run ID `20260804T111919973Z`:

| Measure | Result |
| --- | --- |
| Scenarios | 12 passed, 0 failed |
| Inert continuous commits | 78 |
| Fixed-origin stress | 1,000-event burst; 890 obsolete visual updates coalesced; final value within 0.02 of the 50% target |
| Armed direction change | Passed |
| Every corner under edge assist | Passed |
| Saved-action isolation and focus checks | Passed |

The run took 41.581 seconds on a two-display Windows setup. Its dedicated
stress scenario submitted 1,000 no-coalescing Windows pointer inputs, retained
the requested east wedge and value, and completed the 50 ms close lifecycle in
52 ms. This run supersedes the prior live-direction matrix for current
interaction behavior.

After manual edge/corner testing exposed intermittent sleeping native
viewports, the native-repaint revision completed the standard suite with run ID
`20260804T120608279Z`:

| Measure | Result |
| --- | --- |
| Scenarios | 13 passed, 0 failed |
| Inert continuous commits | 98 |
| Rapid edge-assist repaint | 20 consecutive top-left invocations; maximum first frame 6 ms |
| Opening lifecycle | All 20 rapid invocations reached full opacity before release |
| Fixed-origin stress | Maximum first frame 1 ms; 858 obsolete visual updates coalesced |
| Cursor and focus | Edge cursor retained at 0 px; every focus checkpoint passed |

The run took 48.966 seconds on the same two-display Windows setup. Every normal
smoke invocation now requires its first menu frame before release and within
100 ms. The rapid scenario also requires the 60 ms opening animation to finish
before release. This run supersedes the preceding coalescing matrix for current
interaction and rendering behavior.

The hybrid-radius revision completed the standard suite with run ID
`20260804T125841271Z`:

| Measure | Result |
| --- | --- |
| Scenarios | 14 passed, 0 failed |
| Inert outcomes | 99 total: 98 continuous commits and one restored discrete selection |
| Live radius reversal | Continuous W at 50%, returned over W, released as discrete W |
| Live direction change | Continuous W at 50%, turned to S at the same radius, committed S |
| Target values | 0%, 25%, 50%, 75%, and 100% passed in every eight-slice direction |
| Four-slice geometry | 50% passed in N/E/S/W |
| Edge/corner behavior | All directions passed from all four corners; cursor retained at 0 px |
| Rendering and stress | Maximum rapid first frame 6 ms; 872 obsolete burst updates coalesced |

The run took 51.259 seconds on the same two-display Windows setup. The focused
hybrid smoke also passed under edge assist at the top-left corner with 0 px of
physical cursor movement. This run supersedes the native-repaint matrix for
current interaction behavior.

The immediate-feedback revision then passed focused Windows checks:

| Measure | Result |
| --- | --- |
| Ordinary fixed-origin menus | All eight directions passed; maximum first frame 1 ms |
| Fixed-origin stress | 1,000-event burst; 970 obsolete updates coalesced and only 39 visual samples dispatched |
| Priority path | Direction and mode transitions bypassed the value-update interval |
| Hybrid reversal | Continuous W returned inside and released as discrete W |
| Edge assist | N-to-W live direction change passed with 0 px retained-cursor movement |
| Lifecycle | 50â€“51 ms close observed |

The full debug checks, strict linting, and release test suites passed after the
revision. Ordinary value samples are now debug-only log records; direction,
mode, and terminal transitions remain available at info level.

## Remaining validation

Milestone 0's automated implementation work is complete, but its usability
exit gate is not yet satisfied. The Section 11.3 study requires at least eight
participants and must measure:

- scalar precision at the planned target values;
- accidental arming during ordinary discrete use;
- reliable direction changes after arming;
- comfortable access to the full-scale radius;
- edge and corner use; and
- understanding of inside-release, outside-release, and center cancellation.

Automation records that study as `not_run`; it does not infer human usability.
The guided schema-three participant harness and backward-compatible cohort
evaluator are now available and documented in
`milestone-0-continuous-study-results.md`, but no participant reports have yet
been collected.
The visible outer edge is now fixed as the mode boundary, while the full-scale
distance remains a candidate. The next comparison will measure 160, 200, 250,
and 300 DIP endpoints before selecting Fast, Normal, and Precise defaults.
Mixed-DPI, negative-coordinate, and high-refresh hardware were also unavailable
in this local run and remain part of later cross-hardware validation.
