# Milestone 2 portable icon transfer results

## Scope

This bounded slice makes schema-five settings with custom icons portable
without changing their path-safe configuration references:

- export copies every unique referenced managed icon into an adjacent
  `icons` directory;
- import validates and copies assets from that optional directory;
- byte-identical assets with an existing UUID are reused;
- different bytes under an existing UUID are rejected rather than overwritten;
- missing portable assets remain a non-fatal placeholder warning;
- files created by a failed transfer are rolled back.

Configured executable, file, folder, URL, and working-directory values remain
literal. Managed-asset reclamation remains outside this slice.

## Portable layout

An icon-bearing export has this layout:

```text
selected-folder\
    gradial-settings.json
    icons\
        <asset-uuid>.png
        <asset-uuid>.svg
```

The JSON still stores only:

```json
{
  "asset_id": "a5fda722-80ba-46ee-8983-6ff7bd0c6a66",
  "format": "png"
}
```

The closed format and UUID derive the only filename Gradial will read. No path
from the exporting computer enters the configuration.

## Export transaction

Export first performs the existing complete configuration, profile-resolver,
and action-plan validation. It then enumerates unique icon references in
deterministic filename order.

Each managed source must still be a regular non-empty PNG or SVG no larger
than 256 KiB, with dimensions from 1 through 512 logical pixels and all SVG
content restrictions intact. A missing, unreadable, or damaged managed source
prevents an incomplete portable export.

For each valid source:

1. an absent destination is written to a uniquely named temporary file in the
   adjacent `icons` directory;
2. the file is flushed before rename;
3. an existing destination is validated and reused only when its bytes are
   identical;
4. a different file under the same derived UUID name produces an
   `icon_conflict` error and is never overwritten.

Only after every asset is ready does Gradial atomically replace the selected
JSON. If asset preparation or JSON replacement fails, files created by that
attempt are removed. A newly created empty `icons` directory is also removed.
Existing unrelated files are never deleted.

## Import transaction

Import parses, migrates, and completely validates the selected JSON before
looking at assets. The adjacent `icons` directory is optional:

- no directory means every referenced asset is reported missing;
- an absent referenced file increments the same warning count;
- the configuration still enters the private Settings draft and affected
  directions render placeholders;
- a present directory entry must be a regular file and pass all production
  icon validation.

Valid portable files are copied atomically into
`%LOCALAPPDATA%\Gradial\icons`. An existing managed file with identical bytes
is reused. A same-ID managed file with different or invalid contents rejects
the import before the proposed draft is replaced.

If a later asset fails, managed files created earlier in that import are
removed. Existing managed assets are never modified. As with direct icon
selection, a successful import may leave newly copied but unreferenced assets
if the user later discards the draft; safe garbage collection requires active
and backup reference analysis and remains deferred.

## Cache and Settings behavior

After a successful portable import, the shared renderer cache preloads the
imported draft. Placeholder cache entries are retried, so restoring a
previously missing file can immediately replace the placeholder in Settings.
The saved production menu remains unchanged until **Save changes**.

Settings feedback reports:

- how many portable icons are ready;
- how many referenced icons were missing and will use placeholders;
- field-specific read, validation, collision, and write failures;
- how many custom icons accompanied an export.

Transfer logs contain only fixed error codes, revision numbers, action counts,
asset counts, and a bundle-present flag. The renderer may additionally record
the path-safe asset UUID and closed format while choosing a placeholder.
Selected paths, icon bytes, labels, and action payloads remain absent.

## Automated coverage

The complete debug suite contains 208 tests:

- 156 `gradial-app` tests;
- 52 `gradial-core` tests.

New coverage verifies:

- duplicate references produce one portable file;
- a repeated export reuses byte-identical assets;
- an export collision leaves existing JSON untouched;
- valid portable imports copy into managed storage;
- repeated imports reuse byte-identical managed assets;
- JSON-only imports report missing icons without failing;
- damaged portable assets fail validation;
- differing same-ID managed assets fail with a conflict;
- failed multi-asset imports remove files created earlier in the transaction;
- a restored managed source replaces a cached placeholder.

## Native Windows evidence

`smoke-settings-editor.ps1 -CaptureScreenshot` passes in a disposable
`LOCALAPPDATA` tree. In addition to the existing Settings coverage, it:

1. imports a real transparent PNG into managed storage;
2. exports the complete unsaved schema-five draft into a separate folder;
3. requires one referenced and one newly copied portable icon in the fixed
   transfer log metadata;
4. verifies the adjacent UUID-named PNG exists;
5. compares SHA-256 hashes to prove the portable and managed bytes match;
6. confirms the export does not mutate active configuration before Save;
7. confirms transfer paths remain absent from Gradial-authored logs.

## Remaining boundary

Portable schema-five settings and icons are complete. The remaining icon
storage task is conservative garbage collection across the active
configuration, backup, and open draft. It must never reclaim an asset still
needed for recovery, so it remains a separate bounded slice.
