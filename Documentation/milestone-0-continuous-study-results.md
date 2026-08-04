# Milestone 0 guided continuous-study results

Date: 2026-08-04

This record covers the guided Section 11.3 participant harness. It makes the
required human study repeatable and privacy-bounded; it does not claim that the
study has already been performed.

## Implemented protocol

- Debug-only `--continuous-study` opens a dedicated nontechnical study window
  and implies the no-side-effect continuous meter.
- The 37-trial session contains four practice trials, 24 balanced measured
  scalar trials, four inside-ring direction corrections, four outside-ring
  wedge changes, and one cancellation trial.
- Measured scalar trials cover 25%, 50%, 75%, and 100% in both four- and
  eight-slice menus at center, edge, and corner placements. Placement and menu
  conditions are interleaved within each target value.
- The study uses the configured activation chord and the production input,
  geometry, overlay, focus, and close paths. Its menus contain no actions, and
  neither continuous outcomes nor inert inside-ring selections can enter
  discrete action-plan compilation.
- A 32-DIP work-area boundary classifies center, edge, and corner starts. A
  misplaced attempt is retained but must be retried.
- The cancellation instruction is displayed once before practice and states
  that cancellation stops further effects without undoing earlier effects.
- Settings, onboarding, slice overrides, and non-automatic geometry cannot
  interfere with an active study.

## Privacy and export

Each new run creates a random session ID and atomically publishes one
schema-three JSON report under `%LOCALAPPDATA%\Gradial\continuous-studies`.

The report includes requested and observed categories, direction correctness,
normalized values and errors, terminal reasons, duration, and DPI. It excludes
participant names, raw cursor coordinates, application identity, file paths,
window details, and monitor device names. The destination path is shown locally
after completion but is not embedded in the report.

`summarize-continuous-study.ps1` accepts compatible schema-two history and the
new hybrid schema-three reports, while rejecting unsupported, incomplete,
duplicate, or privacy-incompatible input. It recomputes cohort metrics and
emits JSON plus CSV gate evidence. With fewer than eight complete sessions, it
leaves the Section 11.3 usability gates unevaluated rather than inferring a
pass.
The direction gate uses the v0.2 corner release baseline of at most 3% errors.

## Distance-comparison harness

The debug-only `--continuous-distance-study` mode is now implemented as a
separate no-side-effect protocol. It guides one E-at-25% attempt at each 100%
endpointâ€”160, 200, 250, and 300 DIPâ€”while keeping the Standard 107-DIP mode
boundary unchanged. The measured release and absolute error remain visible
after each attempt. Once all four are complete, the participant chooses the
endpoint that felt best.

The selected endpoint is frozen for the complete invocation and drives both
normalization and the rendered track. A larger prewarmed overlay accommodates
the 300-DIP candidate. Choosing a preference atomically publishes a private
schema-one `continuous-distance-study-<session-id>.json` report. It does not
alter saved configuration, create a speed preset, or enter the main cohort
summarizer.

One initial hands-on comparison was completed on 2026-08-04 and preferred
300 DIP. That result establishes the provisional recommended default and the
prototype now supports manual 160-400 DIP checks. It is recorded as design
feedback only, not fabricated into the still-unrun eight-participant cohort.

## Automated evidence

Automated coverage verifies:

- the exact 37-trial shape and balanced target/menu/placement matrix;
- DPI-scaled center, edge, and corner classification, including negative
  virtual-screen coordinates;
- exclusion of practice and rejected attempts from measured summaries;
- scalar-error, accidental-entry, cancellation, and timing summaries;
- outside direction-switch success summaries;
- atomic report publication without temporary debris;
- absence of identity and raw-position fields from serialized reports;
- debug-option isolation and release rejection; and
- the cohort summarizer's passing eight-participant self-test.

## Remaining human evidence

No participant dataset has been fabricated. The current status remains:

| Requirement | Status |
| --- | --- |
| At least eight participants | Not run |
| Eight-direction error within the accepted v0.2 baseline | Not evaluated |
| Median absolute scalar error at most 0.06 | Not evaluated |
| At least 95% of final samples within 0.15 | Not evaluated |
| Accidental continuous entry below 2% | Not evaluated |
| At least 95% of outside direction changes finish in the requested wedge | Not evaluated |
| At least 95% of inward reversals restore discrete mode | Not evaluated |
| At least seven of eight participants clear selection after one instruction | Not evaluated |
| Edge/corner median no more than 20% slower than center | Not evaluated |

The visible outer edge is now the mode boundary (107 DIP at Standard size).
The provisional full-scale recommendation is 300 DIP, with an intended exact
160-400 DIP user setting. The recommendation remains open to revision after
the formal cohort; named Fast, Normal, and Precise presets are no longer part
of the v0.3 design.
