# Milestone 3 diagnostic-bundle results

## Scope

This first bounded Milestone 3 slice completes the planned release-log and
diagnostic-bundle preview boundary:

- retain the existing local-only rotation of at most five 1 MiB Gradial logs;
- prepare a frozen preview before any bundle is written;
- show every archive entry and the exact optional configured application
  paths;
- export sanitized logs plus bounded app, input, and display metadata;
- require explicit path inclusion for every new preview;
- write one local ZIP atomically;
- perform no upload or other network operation.

Start-at-login, the installer, performance reports, soak testing, and the
remaining release matrices are outside this slice.

## Frozen preview

The **Diagnostics** Settings page does not edit or dirty the configuration.
**Prepare preview** flushes the asynchronous file logger and freezes:

- the current schema and configuration revision;
- Gradial version and Windows version/build/architecture;
- the current input health expressed as **Hook thread responsive**,
  **recovering**, or **faulted**;
- every display's signed physical bounds, work area, and effective DPI;
- sanitized bytes from each exact owned log file;
- the deduplicated set of configured application paths.

The app never claims that a responsive hook thread proves Windows has retained
the low-level hook. A preview remains unchanged if logs, health, displays, or
settings later change. **Refresh preview** is the only way to replace it, so
the archive cannot silently differ from what the user reviewed.

## Closed archive contents

Every ZIP contains:

- `manifest.json`;
- `logs/gradial_rCURRENT.log` when present;
- zero through four `logs/gradial_rNNNNN.log` archives.

Only exact regular files with those owned names can enter the snapshot.
Unknown names, directories, links/reparse entries, and other files remain
outside it. A recognized file larger than the bounded post-rotation allowance,
an unreadable file, invalid UTF-8, or more than five recognized logs fails
preview preparation instead of producing an ambiguous bundle.

The manifest records the bundle format, snapshot time, Gradial version,
schema/revision, Windows version/build/architecture, hook-thread heartbeat status, display
geometry/DPI, log archive names and sizes, and privacy flags. It contains no
configuration payload or installation identifier.

## Privacy and explicit process paths

Copied logs receive a second bundle-time sanitization pass. It replaces:

- the configured application-profile executable paths;
- Run-application executable paths;
- the current `LOCALAPPDATA` root;
- the current user-profile root.

Configured paths are compared and deduplicated case-insensitively. They are
excluded by default even after a previous opted-in export. Selecting
**Include configured application paths** displays every exact path in the
preview and adds one `configured-process-paths.json` entry. That document
contains only the deduplicated executable paths and a fixed notice.

The following never enter either bundle mode:

- `config.json` or its backup;
- profile names, rules other than executable path, menu labels, or icons;
- typed text;
- URLs;
- file/folder action paths;
- executable arguments or working folders;
- window titles or clipboard data.

The diagnostic request and Settings event use redacted debug output, and
success/failure logs contain only counts, booleans, revisions, and stable error
codes.

## Atomic export

The native Windows save chooser:

- filters for `.zip`;
- suggests `gradial-diagnostics.zip`;
- uses the normal overwrite prompt;
- never reveals the selected destination to Gradial's log.

Export accepts only an absolute `.zip` path whose parent exists. The whole
`%LOCALAPPDATA%\Gradial` tree is protected, including differently cased or
junction-resolved spellings. Gradial writes a uniquely named adjacent
temporary file, finishes the ZIP central directory, flushes the file, and
uses one write-through Windows replacement. Failure removes only that exact
temporary file and leaves an existing destination unchanged.

The feature has no HTTP client, shell dispatch, analytics, automatic sharing,
or upload action.

## Automated coverage

The complete debug suite contains 221 tests:

- 169 `gradial-app` tests;
- 52 `gradial-core` tests.

New tests verify:

- exact owned-log filename recognition;
- case-insensitive path sanitization;
- owned-log-only frozen capture and process-path deduplication;
- display, configuration-revision, and input-status capture;
- default archive exclusion and manifest flags;
- explicit path-document inclusion without arguments or working folders;
- atomic replacement with no temporary debris;
- rejection of relative, non-ZIP, and Gradial-owned destinations;
- redacted diagnostic request debug output;
- private preview and path-opt-in clearing on Settings close;
- the native diagnostic save-dialog title/filter path.

Clippy with warnings denied and the complete workspace build also pass.

## Native Windows evidence

`smoke-settings-editor.ps1 -CaptureScreenshot` passes in a disposable
`LOCALAPPDATA` tree. It:

1. opens the real **Gradial: export diagnostic bundle** chooser and safely
   cancels it;
2. saves a configuration in which one executable is referenced by both an
   application profile and a Run-application action;
3. prepares the real Diagnostics preview and requires one deduplicated path;
4. exports the default ZIP and verifies the path document is absent;
5. reads `manifest.json` and the sanitized log from that ZIP and rejects the
   configured executable path;
6. exports an explicitly opted-in ZIP and verifies the path document contains
   exactly that one executable;
7. rejects the configured arguments and working folder from the optional
   document;
8. verifies both manifest privacy flags and the absence of temporary debris.

The harness reads and writes only its disposable tree and the two ZIPs it
creates there.

## Remaining boundary

Release logging and local diagnostic preview/export are complete for this
milestone checkpoint. A future diagnostic expansion must preserve the closed
entry list and require a new explicit privacy decision; it cannot infer
permission from an earlier export. Start-at-login and the signed per-user
installer remain the next planned product slice.
