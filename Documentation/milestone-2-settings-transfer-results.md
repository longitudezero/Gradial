# Milestone 2 settings import/export results

## Scope

This bounded slice adds native JSON transfer to the existing settings editor:

- import schema-one, schema-two, or schema-three settings into the private
  editor draft;
- export the current complete valid draft, including unsaved edits;
- preserve literal action paths, working directories, arguments, labels, and
  text;
- keep the running and persistent configuration unchanged until an explicit
  **Save changes**;
- defer icon copying and path portability.

## Import boundary

**Import settings…** uses a native Windows open dialog restricted to JSON. The
selected file is read through the production configuration parser and in-memory
migration path. Gradial then validates the complete configuration model,
compiles all profile rules, and compiles the action catalog before the editor
can accept it.

An imported external `config_revision` is never trusted. The proposed draft is
rebased onto the currently saved revision so the existing Save path remains
the only revision authority. Import replaces neither `config.json` nor the
running resolver, action compiler, input binding, or overlay state. The source
file remains byte-for-byte unchanged.

If the editor already contains unsaved changes, a second confirmation offers
**Import file** or **Keep current draft**. After a successful import, profile
and action selections, temporary four/eight-slice stashes, gesture/profile
test state, and shortcut recording state are reset against the new draft.

## Export boundary

**Export current draft…** uses a native Windows save dialog with a JSON filter,
default `.json` extension, suggested `gradial-settings.json` name, and the
standard overwrite prompt. The button is disabled while the current draft is
invalid.

Before writing, Gradial repeats complete model, profile, and action-plan
validation and requires the draft revision to match the current saved
revision. Export serializes formatted schema-three JSON with a final newline,
writes a uniquely named adjacent temporary file, flushes it, and atomically
moves it over the selected destination. A failed operation removes only that
exact temporary file and leaves an existing destination untouched.

Export cannot target Gradial's active `config.json`, `config.json.bak`,
`config.json.tmp`, or the corresponding primary/backup/temporary plugin-state
paths, including a differently cased spelling of those Windows paths. The
export operation does not advance the revision, update the running application,
or create a configuration backup.

## Privacy and failure behavior

Transfer events use redacted debug formatting. Application logs contain only
fixed event/error codes, the current revision, migration version, and bounded
counts. They never contain:

- selected import/export paths;
- imported labels or profile names;
- action text, URLs, executable arguments, or working directories;
- parser excerpts from private JSON.

The editor maps missing, unreadable, malformed, unsupported-schema, invalid,
protected-destination, and write failures to fixed user-facing explanations.
The private draft remains open after every failure.

## Automated coverage

The debug verification suite contains 181 tests:

- 132 `gradial-app` tests;
- 49 `gradial-core` tests.

Coverage added by this slice verifies:

- schema-two import migration without source or primary mutation;
- imported revision rebasing;
- literal Windows path, working-directory, and argument preservation;
- fixed error categories for malformed and newer-schema files;
- complete atomic export and temporary-file cleanup;
- unchanged existing destinations after invalid export;
- protection of all active configuration paths with case-insensitive matching;
- replacement of only the proposed settings draft;
- reset of editor navigation and transient state after import;
- redaction of transfer paths and configuration values from debug output;
- titles for both native transfer dialogs.

The complete formatting, Clippy, unit-test, and debug-build check passes.

## Native Windows evidence

`smoke-settings-editor.ps1` passes against a disposable
`LOCALAPPDATA` tree. The harness:

1. opens and cancels the real native import and export dialogs;
2. imports a private schema-two fixture with external revision 999;
3. observes migration into revision-1 editor memory;
4. confirms the schema-two source and active `config.json` are unchanged;
5. performs the existing binding, action, sequence, and application-profile
   draft edits;
6. exports the unsaved schema-three draft at revision 1;
7. verifies the exact Ctrl+M5 binding, action catalog, executable path, working
   directory, and three literal argument rows in the export;
8. again confirms that active `config.json` remains schema two at revision 1;
9. saves normally and observes the existing atomic revision-2 live reload;
10. confirms imported values and both transfer file names are absent from the
    Gradial log.

Optional screenshot mode captures both native transfer pickers. All fixture
paths are inside the disposable test tree and are removed during cleanup.

## Remaining boundary

JSON configuration transfer was complete for this checkpoint. Portable icon
copying was added later and is covered by
`milestone-2-portable-icon-transfer-results.md`; configured action paths remain
literal. Appearance, icons, and onboarding were separate at this checkpoint;
onboarding is covered by the later `milestone-2-onboarding-results.md` slice.
