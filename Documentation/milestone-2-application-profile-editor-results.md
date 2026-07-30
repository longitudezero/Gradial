# Milestone 2 application-profile editor results

Date: 2026-07-30

This record covers the first bounded application-profile editor slice:
profile lifecycle, exact executable selection, explicit priority, independent
four/eight-slice menus, production preview, validation, persistence, and live
reload.

## Profile creation and lifecycle

The settings window now has a **Profiles** tab alongside **Menu** and
**Actions**. Global remains the fallback and is edited in the existing Menu
tab; the profile catalog contains only application-specific entries.

**New profile** opens the native Windows application chooser. Cancelling the
chooser adds nothing to the draft. A selected `.exe` creates:

- a new immutable UUID;
- a readable unique name derived from the executable filename;
- the chosen complete executable path;
- default priority 50;
- no window-class or title predicate;
- an independent copy of the current Global menu.

The selected executable can be replaced through the same native chooser.
**Duplicate selected** copies the complete rule and menu under a new UUID and
unique name. **Delete selected** removes only the draft profile and selects the
next available entry. Discarding the draft or keeping a guarded dirty window
open retains the existing saved configuration.

Existing configurations may already contain optional window-class or
title-regex predicates. This bounded editor preserves those values during
rename, menu edits, executable changes, and duplication, and visibly explains
that their controls are deferred. It never silently weakens an existing rule.

## Exact matching and priority

The editor exposes the required executable path as an exact application match.
Creation and replacement use the executable-filtered native chooser rather
than shell parsing or environment-variable expansion. The rule feeds the
existing case-insensitive, normalized full-path resolver without introducing
a second matcher.

Priority is edited directly within the existing 0 through 100 bound. Runtime
resolution remains deterministic: priority precedes predicate specificity and
the immutable UUID tie-break. The current-window match explanation remains a
later slice.

## Independent profile menus

Every profile owns a complete four- or eight-direction menu. It can:

- switch geometry independently from Global and other profiles;
- edit every visible label;
- assign or clear any catalog action, including a reusable sequence;
- select a direction through the profile's live radial preview.

New profiles copy Global only once at creation. Later Global edits do not
propagate. Each profile also owns a separate draft-only diagonal stash:
switching that profile from eight to four directions keeps its diagonal
definitions, and switching back restores them while retaining edits to the
four cardinal slices. Saving a four-slice profile intentionally discards that
unsaved-only stash, matching the existing Global behavior.

The preview uses `FoundationOverlayRenderer` and `ResolvedMenu`, so profile
labels, geometry, ordering, and selection are rendered through the production
path.

## Validation and persistence

Before save, field-specific messages cover:

- empty application-profile names;
- a missing required executable;
- priority outside 0 through 100;
- empty labels in a profile's own menu;
- invalid preserved window-class or title-regex predicates.

Complete configuration validation, profile-resolver compilation, and
action-plan compilation remain authoritative. A valid save uses the existing
atomic replacement and backup flow, advances `config_revision`, and rebuilds
the retained resolver. Fresh M4 invocations can therefore use the new profile
without restarting Gradial, while an invocation already open keeps its frozen
profile and menu.

## Automated and native evidence

Focused unit coverage verifies:

- creation copies Global while assigning a fresh UUID, readable unique name,
  exact path, and default priority;
- duplication preserves the complete rule and menu while changing identity;
- deletion chooses a deterministic remaining profile;
- profile four/eight conversion restores its own diagonals and never changes
  Global;
- profile name, executable, priority, and menu-label failures map to
  editor-specific validation messages.

The isolated native settings harness creates a Notepad profile at priority 72,
duplicates and deletes it, converts its menu to four and back to eight
directions, changes its East slice, and assigns a nested sequence. It then
changes Global to four directions. The saved revision must retain exactly one
eight-slice application profile, the exact executable path and priority, the
restored NE diagonal, and the independent East assignment. The previous
profile-free revision must remain in `config.json.bak`, and a fresh M4
invocation must use the reloaded Global menu. The fixture never reads or
writes the user's Gradial configuration.

Final verification passed:

- 166 debug workspace tests and 164 release workspace tests;
- Clippy with warnings denied for every target and feature in debug and release;
- debug and release builds;
- formatting and PowerShell syntax checks;
- the expanded native profile/action editor, save, backup, and live-reload
  workflow;
- one live release M4 Delay invocation.

## Current boundary

The editor now covers exact-executable application profiles, priority, and
complete independent menus. Optional window-class and title-regex controls,
the current-window profile tester and tie-break explanation, activation
binding, appearance, icons, import/export, and onboarding remain future
bounded slices.
