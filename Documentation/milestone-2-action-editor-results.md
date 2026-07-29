# Milestone 2 action-catalog editor results

Date: 2026-07-29

This record covers the bounded action-editor foundation: catalog management,
keyboard-shortcut recording, Unicode-text editing, reference-safe deletion,
and persistence through the existing settings draft. It builds on the
global-menu editor and does not yet expose dedicated payload editors for the
other schema-two action types.

## Catalog and draft behavior

The settings window now has **Menu** and **Actions** tabs. The action catalog
supports:

- creating a keyboard shortcut with an immediately valid F13 default;
- creating a Unicode-text action;
- renaming every action type;
- duplicating every action with a new UUID and unique readable name;
- deleting only actions that have no configuration references.

Every edit remains inside the private full-configuration draft. **Discard
edits**, **Discard changes**, and a failed save do not alter the running
configuration. A successful save performs the same complete validation,
action-plan compilation, atomic replacement, revision advance, and live reload
as the menu editor.

Deletion scans the global menu, every application-profile menu, and every
sequence step before changing the catalog. If any reference exists, deletion
is refused and the editor reports its menu and sequence reference counts. This
avoids silently clearing a menu or rewriting a sequence behind the user's
back.

## Keyboard-shortcut recorder

The recorder captures one supported physical non-modifier key and the exact
left/right Ctrl, Alt, Shift, and Windows modifiers held with it. Its result is
stored as the existing validated Windows scan code plus extended-key flag and
is displayed before save. Escape and **Cancel** leave the prior shortcut
unchanged. Unsupported keys report an error while keeping the recorder open.

If a modifier was already held before recording started, egui may expose only
the collapsed modifier state. In that case the recorder deliberately chooses
the left variant. Physical modifier transitions seen after recording begins
retain their exact left/right identity.

The editor recognizes letters, top-row numbers, punctuation, Enter, Tab,
Backspace, Space, navigation keys, F1 through F24, Browser Back, the
international backslash key, and numpad plus. The runtime continues to accept
manually configured validated scan codes outside that editor set.

## Unicode text and privacy

Text actions use a multiline Unicode field and show their current UTF-8 byte
count. A draft above the existing 8 KiB limit receives a field-specific
validation message and cannot be saved. Text contents are not written to the
application log or the recorder's diagnostic events.

## Automated evidence

Unit coverage verifies:

- exact physical left/right modifier capture and scan-code mapping;
- safe fallback when a modifier predates recording;
- Escape cancellation and unsupported-key behavior;
- shortcut display formatting;
- unique names and UUIDs for create and duplicate;
- reference detection across menus and sequences;
- refusal to delete referenced actions;
- adjacent selection after unreferenced deletion;
- field-specific empty-name and oversized-text validation.

The isolated native settings harness starts with a revision 1 eight-slice
configuration and one Delay action. It creates keyboard and Unicode-text
actions, duplicates and deletes an unreferenced temporary action, sends native
left-Ctrl+F14 key messages into the recorder, assigns that shortcut to North,
and exercises the real dirty-close confirmation. After saving, it requires:

- revision 2 with exactly three actions;
- an exact left-Control modifier, F14 scan code 101, and non-extended key flag;
- the exact Unicode text `Gradial editor text 🙂`;
- the shortcut assigned to North and the remaining cardinal Delay assignments
  preserved;
- revision 1 retained in `config.json.bak`;
- a clean settings close and a fresh M4 invocation using the reloaded menu.

The harness uses an isolated temporary `%LOCALAPPDATA%` and never reads or
writes the user's Gradial configuration.

Final verification passed:

- 153 debug workspace tests and 151 release workspace tests;
- Clippy with warnings denied for every target and feature in debug and release;
- debug and release builds;
- formatting and PowerShell syntax checks;
- the isolated native settings-editor workflow with settled screenshots;
- one live release M4 Delay invocation after the saved-menu reload path.

## Current boundary

URL, file/folder, executable, invocation-window, Delay, and sequence actions
can be renamed, duplicated, assigned, and safely deleted, but their payloads
are not yet editable in the UI. Application-profile rules and menus,
activation binding, appearance, icons, import/export, onboarding, and the
current-window profile tester remain future bounded slices.
