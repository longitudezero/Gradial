# Milestone 0 continuous-input prototype results

Date: 2026-07-30

Live-direction revision: 2026-08-03

This record covers the first bounded v0.3 slice and its live-direction
revision: continuous radial geometry, production-overlay feedback, extended
edge assist, and a debug-only no-side-effect meter. It deliberately introduces
no Lua runtime, plugin manifest, plugin configuration, or application-specific
effect.

## Implemented contract

- Radial travel from 48 DIP through 160 DIP maps linearly to an unsigned
  `0.0..=1.0` scalar and clamps outside that range.
- A continuous direction arms at 48 DIP. Angle continues selecting the live
  wedge after arming while radial distance independently changes the scalar.
- Release before arming cancels without a continuous commit.
- Returning inside the existing 24 DIP center radius after arming cancels the
  invocation instead of switching direction.
- The renderer displays a radial track, a position marker, and a center
  percentage using the production overlay path. Edge assist also displays a
  Gradial-owned pointer proxy from a full-scale-clamped DIP offset.
- The prewarmed overlay and edge-assist extent expand from the discrete
  288-DIP square to a 400-DIP square only for the continuous prototype.
  Existing discrete selection geometry remains unchanged.
- Debug-only `--continuous-meter` treats each resolved menu slice as
  continuous for that invocation. A commit is logged as inert evidence and
  cannot compile or dispatch the saved slice action.
- Onboarding practice remains discrete, and release builds reject the
  development option.

The continuous state and normalization live in `gradial-core`; they are not
coupled to Lua. This preserves the runtime-neutral boundary required before a
plugin adapter is introduced.

## Automated checks

The workspace test suite passes with 186 application tests and 58 core tests.
The new coverage verifies:

- normalization at 0%, 50%, and 100%, including clamping and invalid input;
- arming, live post-arm direction changes, final-wedge commit, and
  center-return cancellation;
- cancellation when released before arming;
- equivalent normalized values at different DPI scales;
- signed edge-assist pointer feedback and full-scale visual clamping;
- 48/104/160-DIP renderer marker positions;
- continuous mode's 200-DIP edge-assist extent while discrete mode retains
  its 144-DIP half extent;
- rejection of `--continuous-meter` by release builds; and
- the invariant that a continuous-meter commit never compiles a saved action.

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

## Remaining validation

Milestone 0's automated implementation work is complete, but its usability
exit gate is not yet satisfied. The Section 11.3 study requires at least eight
participants and must measure:

- scalar precision at the planned target values;
- accidental arming during ordinary discrete use;
- reliable direction changes after arming;
- comfortable access to the full-scale radius;
- edge and corner use; and
- understanding of release and center-return cancellation.

Automation records that study as `not_run`; it does not infer human usability.
The guided schema-two participant harness and cohort evaluator are now
available and documented in `milestone-0-continuous-study-results.md`, but no
participant reports have yet been collected.
The current 48-DIP arm and 160-DIP full-scale radii therefore remain candidate
defaults until the study is recorded. Mixed-DPI, negative-coordinate, and
high-refresh hardware were also unavailable in this local run and remain part
of later cross-hardware validation.
