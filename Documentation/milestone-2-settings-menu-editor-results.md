# Milestone 2 global-menu settings editor results

Date: 2026-07-29

This record covers the first bounded Milestone 2 editor slice: a native
single-instance settings window for the global radial menu, guarded draft
lifecycle, atomic saving, and live runtime reload. It does not yet cover
action creation, application profiles, activation binding, appearance, icons,
import/export, or onboarding.

## Window and draft boundary

The notification-area menu now includes **Open settings**. It opens a normal,
decorated, resizable taskbar window and focuses the existing instance when
chosen again. Opening an existing editor does not replace its draft.

The draft owns a complete configuration copy and remains separate from the
running configuration until save succeeds. A clean native close exits
immediately. A dirty native close is cancelled and replaced by a modal with
the exact choices **Discard changes** and **Keep editing**. Discard abandons
the private draft; keep returns to the same unsaved values. An in-window
**Discard edits** command resets without closing.

If startup recovered a backup or built-in default because the primary was
invalid, the editor shows a persistent recovery warning. It clears only after
an explicit successful save.

## Global-menu editing

The window supports four- and eight-direction menus. It edits each visible
label and assigns or clears an action already present in the configuration.
Action choices use the configured name and a non-technical action-type label.
An empty catalog has an explicit explanatory state.

Eight-to-four conversion keeps north, east, south, and west together with
their action assignments. The four diagonal slices remain stashed in the
open draft and return if the user switches back to eight. Expanding a
configuration that was already saved as four directions creates the standard
NE, SE, SW, and NW defaults. The currently selected direction maps to the same
cardinal direction across conversions.

The clickable radial preview uses the production `FoundationOverlayRenderer`
with a `ResolvedMenu`. Direction order, ring geometry, labels, and selection
rendering therefore stay shared with the overlay instead of being duplicated
in the editor.

## Validation, persistence, and activation

The editor disables save for a clean or invalid draft. Empty labels receive a
direction-specific message. The full configuration model, profile resolver,
and action-plan compiler must all accept the proposed draft.

The application repeats validation and compilation before filesystem
mutation. It then uses the existing `ConfigStore` atomic replacement path,
which writes and flushes the temporary file, preserves the previous valid
primary as `config.json.bak`, replaces the primary, and advances the revision.
Only the exact prevalidated saved model becomes active.
When the running model came from backup recovery, explicit save repairs the
primary without replacing that known-good backup with the broken primary.

After success, Gradial swaps in the new profile resolver and action compiler,
updates the overlay pool and action executor revision, and replaces the
interaction runtime. New M4 invocations use the saved revision without an
application restart. A save failure keeps the editor draft open and leaves the
running configuration unchanged.

## Automated coverage

Unit tests prove:

- four/eight conversion preserves cardinal labels and action references;
- draft-local diagonal values return when switching back to eight;
- a saved four-slice menu receives deterministic default diagonals;
- selection maps to its equivalent cardinal direction;
- preview hit testing follows the production clockwise order and ignores the
  center;
- empty labels block save with a direction-specific error;
- existing action assignment remains valid;
- dirty close requires an explicit decision while clean close does not;
- reopening the single editor preserves its unsaved draft.

The isolated native smoke harness starts Gradial with a temporary schema-two
revision 1 configuration and one Delay action. It observes the real settings
window, applies a four-slice draft, sends an actual `WM_CLOSE`, captures the
confirmation, keeps editing, and saves. It then requires:

- one visible settings window;
- an on-disk revision 2 four-slice configuration with all edited labels;
- all cardinal action assignments preserved;
- the prior eight-slice revision 1 in `config.json.bak`;
- a clean native close without another prompt;
- a fresh M4 invocation resolved with four labels at revision 2;
- successful execution of the preserved Delay action.

The temporary local-data directory is always removed, and the harness never
reads or writes the user's Gradial configuration. Optional screenshots are
written only to the ignored diagnostics directory.

## Validation evidence

The completed implementation passed:

- 144 debug workspace tests and 142 release workspace tests;
- Clippy for all targets and features in debug and release;
- debug and release builds;
- formatting and PowerShell syntax validation for every development script;
- the isolated native settings-window, close-confirmation, atomic-save, live
  reload, and M4 action check.

## Current boundary

Only the global menu is editable. Existing actions can be assigned, but the
catalog cannot yet be created or changed in the UI. Application-profile menus
and match rules are not exposed. Activation binding, appearance, icons,
import/export, onboarding, and the plan's current-window profile tester remain
future bounded slices.
