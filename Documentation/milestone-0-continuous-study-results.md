# Milestone 0 guided continuous-study results

Date: 2026-08-03

This record covers the guided Section 11.3 participant harness. It makes the
required human study repeatable and privacy-bounded; it does not claim that the
study has already been performed.

## Implemented protocol

- Debug-only `--continuous-study` opens a dedicated nontechnical study window
  and implies the no-side-effect continuous meter.
- The 37-trial session contains four practice trials, 24 balanced measured
  scalar trials, four pre-arm direction corrections, four armed wedge changes,
  and one cancellation trial.
- Measured scalar trials cover 25%, 50%, 75%, and 100% in both four- and
  eight-slice menus at center, edge, and corner placements. Placement and menu
  conditions are interleaved within each target value.
- The study uses the configured activation chord and the production input,
  geometry, overlay, focus, and close paths. Its menus contain no actions, and
  continuous outcomes cannot enter discrete action-plan compilation.
- A 32-DIP work-area boundary classifies center, edge, and corner starts. A
  misplaced attempt is retained but must be retried.
- The cancellation instruction is displayed once before practice and states
  that cancellation stops further effects without undoing earlier effects.
- Settings, onboarding, slice overrides, and non-automatic geometry cannot
  interfere with an active study.

## Privacy and export

Each run creates a random session ID and atomically publishes one schema-two
JSON report under `%LOCALAPPDATA%\Gradial\continuous-studies`.

The report includes requested and observed categories, direction correctness,
normalized values and errors, terminal reasons, duration, and DPI. It excludes
participant names, raw cursor coordinates, application identity, file paths,
window details, and monitor device names. The destination path is shown locally
after completion but is not embedded in the report.

`summarize-continuous-study.ps1` rejects unsupported, incomplete, duplicate,
or privacy-incompatible reports. It recomputes cohort metrics and emits JSON
plus CSV gate evidence. With fewer than eight complete sessions, it leaves the
Section 11.3 usability gates unevaluated rather than inferring a pass.
The direction gate uses the v0.2 corner release baseline of at most 3% errors.

## Automated evidence

Automated coverage verifies:

- the exact 37-trial shape and balanced target/menu/placement matrix;
- DPI-scaled center, edge, and corner classification, including negative
  virtual-screen coordinates;
- exclusion of practice and rejected attempts from measured summaries;
- scalar-error, accidental-arm, cancellation, and timing summaries;
- armed direction-switch success summaries;
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
| Accidental arming below 2% | Not evaluated |
| At least 95% of armed direction changes finish in the requested wedge | Not evaluated |
| At least seven of eight participants cancel after one instruction | Not evaluated |
| Edge/corner median no more than 20% slower than center | Not evaluated |

The candidate 48-DIP arm radius and 160-DIP full-scale radius remain unfrozen
until eight complete reports are collected and the cohort summarizer passes.
