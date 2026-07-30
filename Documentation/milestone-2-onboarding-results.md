# Milestone 2 required onboarding results

## Scope

This bounded slice completes the required v1 first-run flow:

- choose M4 or M5 with optional exact Ctrl, Alt, and Shift requirements;
- explicitly acknowledge that the chosen chord is dedicated to Gradial while
  it runs;
- open the sample radial menu through the production interaction;
- select one inert sample slice;
- cancel once through the center or Escape;
- persist versioned completion independently of user configuration;
- replay the flow later through **Restart onboarding**.

The flow is required when its completion marker is missing, invalid,
unsupported, or incomplete. Closing its window cannot bypass it; the tray
still provides the normal application exit.

## Completion and configuration persistence

`%LOCALAPPDATA%\Gradial\onboarding-v1.json` contains only the schema version and
completion boolean. The marker is written as formatted JSON with a final
newline through `onboarding-v1.json.tmp`, flushed, and atomically moved into
place. It is written only after every practice requirement succeeds.

The selected activation chord remains part of the versioned `config.json`
(schema four after the appearance slice).
Beginning practice persists a changed chord through the existing complete
configuration validation, compilation, atomic backup/replacement, and live
input-runtime restart. An unchanged chord does not advance the configuration
revision. Persistence or restart errors keep the onboarding window and its
progress available with fixed user-facing guidance.

The onboarding marker is deliberately separate from configuration import and
export. Both marker paths are protected settings-export destinations. Replaying
onboarding from the tray retains a valid completion marker, so exiting an
optional replay does not require onboarding again at the next startup.

## Safe production-interaction practice

Practice uses the real saved input chord and the production:

- low-level input runtime and suppression behavior;
- immutable invocation snapshot;
- profile resolver boundary;
- gesture controller and center/Escape cancellation;
- fixed-origin or edge-assist geometry;
- per-display overlay pool and cached renderer.

After normal profile resolution, practice replaces the invocation menu with an
immutable eight-direction sample containing labels but no action UUIDs. The
invocation also carries an explicit practice bit. Release returns before
action lookup, plan compilation, or executor submission, providing a second
guard if the underlying resolved profile assigns an action to every direction.

A bounded bridge reports menu-open, sample-selection, and cancellation progress
to the welcome window. Cancellation counts only after selection, ensuring the
user intentionally performs both interaction outcomes. Other cancellation
reasons do not complete the teaching step.

## UI and tray behavior

The normal **Welcome to Gradial** taskbar window explains exact chord matching
and makes the dedicated-button consequence explicit, including quick centered
cancellation. The acknowledgment is required before **Start safe practice**.
The practice page shows live progress and enables completion only after the
required open, selection, and later center/Escape cancellation.

**Open settings** focuses onboarding instead of opening a competing binding
editor while the flow is active. **Restart onboarding** reopens the first page
with the currently saved chord. Onboarding write, settings-save, and
input-reload failures do not discard progress or silently mark completion.

## Automated coverage

The debug verification suite contains 186 tests:

- 137 `gradial-app` tests;
- 49 `gradial-core` tests.

The unit and integration suite covers:

- missing, malformed, unsupported, and incomplete markers;
- atomic completion and replacement of an incomplete marker;
- exact temporary-file cleanup after a successful write;
- the complete eight-slice sample menu with no action references;
- selection-before-cancellation ordering;
- protected export handling for both onboarding paths;
- every tray command code, including **Restart onboarding**.

The complete PowerShell syntax, formatting, Clippy, unit-test, and debug-build
check passes.

## Native Windows evidence

`smoke-onboarding.ps1 -CaptureScreenshot` passes against a disposable
`LOCALAPPDATA` tree. The harness:

1. starts Gradial without an onboarding marker and requires the welcome window;
2. assigns a real Delay action to every production-menu direction;
3. chooses and acknowledges exact Ctrl+M5;
4. observes the atomic schema-four revision-2 binding save and live runtime
   reload;
5. sends native Ctrl+M5 input through the production hook to open the sample
   menu and select east;
6. opens it again and performs a centered cancellation;
7. requires all onboarding progress while rejecting every action-dispatch and
   action-completion record;
8. completes onboarding and validates the schema-one completion marker;
9. restarts Gradial against the same isolated data and verifies onboarding
   remains closed.

Screenshots cover the chord acknowledgment and completed-practice pages. The
fixture never reads or writes the user's Gradial data and removes its exact
temporary tree after closing only the processes it started.

## Remaining boundary

Required onboarding is complete for this slice. The plan's participant
learnability gate, including whether at least seven of eight participants can
cancel without further instruction, remains a human release study rather than
an automated claim.
