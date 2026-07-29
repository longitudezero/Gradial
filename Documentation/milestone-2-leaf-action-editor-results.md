# Milestone 2 leaf-action editor results

Date: 2026-07-29

This record covers the bounded completion of the leaf-action editor: website,
file/folder, direct application, Delay, and invocation-window payloads plus
native Windows path selection. Keyboard shortcuts and Unicode text were
already editable. Reusable sequence steps remain a separate editor slice.

## Complete leaf-action catalog

The catalog's **New action** menu now creates:

- keyboard shortcuts;
- Unicode text;
- HTTP/HTTPS websites;
- absolute file or folder paths;
- direct `.exe` applications;
- bounded Delay actions;
- close, minimize, maximize, or restore for the invocation window.

New actions receive a new UUID, a readable unique name, and a syntactically
valid visible default. Creation alone never assigns or runs an action.
Rename, duplicate, reference-safe delete, complete draft isolation, guarded
close, atomic save, and live reload continue to apply to the entire catalog.

## Native path selection

File/folder actions offer separate **Choose file** and **Choose folder**
controls. Application actions offer **Choose application** and an optional
**Choose working folder** control. Manual absolute-path editing remains
available.

The choosers use the Windows common item dialog directly through the existing
Windows dependency. Each dialog runs on a dedicated COM STA thread, requires
an existing filesystem result, and does not change the process working
directory. The application chooser filters for `.exe` files. Cancellation
leaves the previous draft value untouched; a chooser failure produces a fixed
safe UI message. Selected paths and dialog errors are not logged.

## Literal application arguments

Application arguments are an ordered list of independent rows. Adding a row
creates one empty argument, removing a row removes only that boundary, and
editing a row never splits on spaces or interprets quotes, environment
variables, redirection, or shell syntax. Empty arguments are intentionally
preserved. The optional working folder is a separate absolute path rather than
an argument.

This UI representation matches the existing direct `CreateProcessW` backend:
Gradial performs Windows quoting only at the final process boundary and never
invokes a command shell.

## Constrained and field-specific editing

- Website actions accept only complete `http://` or `https://` addresses.
- File/folder and application paths must be absolute.
- Applications must use an `.exe` path; scripts remain disallowed.
- Arguments cannot contain a null character.
- Delay is constrained to 10 through 10,000 ms.
- Window commands are selected from close, minimize, maximize, and restore.

Invalid leaf fields receive an action- and field-specific save message.
Complete configuration validation, profile compilation, and immutable
action-plan compilation remain authoritative before any disk replacement.

## Automated and native evidence

Unit coverage verifies:

- valid defaults for every leaf-action creation template;
- literal application argument ordering, spaces, empty values, addition, and
  removal;
- field-specific URL, path, application-extension, argument, and Delay
  validation;
- a distinct nonempty title for every native chooser kind.

The isolated native settings harness begins with one Delay action. Through the
real settings window and its debug automation boundary it builds an eight-item
catalog, then requires revision 2 to contain exactly:

- left-Ctrl+F14 by scan code;
- the Unicode text `Gradial editor text 🙂`;
- `https://example.com/gradial-editor`;
- the absolute folder `C:\Windows`;
- Notepad with a working folder and three literal arguments, including one
  containing spaces and one empty argument;
- a 275 ms Delay;
- a restore command for the invocation window;
- the original Delay action.

The harness also opens the real Windows application chooser, waits for its
native window, captures its settled surface when requested, cancels it, and
requires a cancellation result without a payload log. It then exercises dirty
close, atomic backup, clean close, live configuration reload, and a fresh M4
invocation. The isolated temporary `%LOCALAPPDATA%` ensures the user's
configuration is never read or changed.

Final verification passed:

- 158 debug workspace tests and 156 release workspace tests;
- Clippy with warnings denied for every target and feature in debug and release;
- debug and release builds;
- formatting and PowerShell syntax checks;
- the expanded native settings-editor, picker, save, and live-reload workflow;
- one live release M4 Delay invocation.

## Current boundary

Reusable sequence actions can be renamed, duplicated, assigned, and safely
deleted, but their ordered steps are not yet editable in the UI. Application
profiles and rules, activation binding, appearance, icons, import/export,
onboarding, and the current-window profile tester remain future bounded
slices.
