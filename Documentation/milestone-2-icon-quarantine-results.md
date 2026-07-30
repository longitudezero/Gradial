# Milestone 2 managed-icon quarantine results

## Scope

This bounded slice prevents abandoned custom-icon imports from accumulating in
active managed storage without permanently deleting recovery data:

- build one protected reference set from active settings, backup settings, and
  the complete open draft;
- restore a protected valid asset from quarantine when its managed source is
  missing;
- move exact unprotected managed icon files into
  `%LOCALAPPDATA%\Gradial\icons\quarantine`;
- leave unknown files, directories, links/reparse entries, and collisions
  untouched;
- defer permanent deletion.

The configuration schema and renderer geometry do not change.

## Protection snapshot

Every reconciliation begins with the active in-memory schema-five
configuration. When Settings is open, its complete proposed draft is included
as a second source, so newly imported or unsaved assignments cannot be
quarantined.

Gradial independently reads and validates `config.json.bak` through the normal
configuration migration and validation path:

- a valid backup contributes all of its unique icon references;
- a missing backup is an empty contribution;
- an unreadable, malformed, unsupported, or invalid backup disables
  quarantine for that reconciliation.

Disabling quarantine is deliberately fail-safe. Gradial may still restore a
known active or draft asset, but it will not classify any managed file as
unused without a complete recovery view.

## Exact candidate boundary

Only a regular file whose name exactly equals canonical lowercase
`<non-nil-UUID>.png` or `<non-nil-UUID>.svg` is recognized. The UUID and closed
format must derive the same complete filename.

The following are never moved:

- unknown extensions or non-UUID names;
- uppercase or otherwise noncanonical spellings;
- nil UUIDs;
- temporary files;
- directories, including canonical-looking directory names;
- symlinks or other reparse-style entries;
- the quarantine directory itself.

This permits users and future Gradial versions to place unrelated metadata in
the icon directory without an older build claiming ownership of it.

## Recoverable moves and conflicts

An unprotected recognized file is renamed into the quarantine directory on the
same volume. Gradial creates that directory only when the first move is
needed. It never replaces an existing quarantine destination; both copies
remain in place and a fixed conflict count is recorded.

Quarantine is not a deletion boundary. No file in quarantine has an age,
retention deadline, or automatic permanent-removal path in this slice.

Before renderer preload, each protected reference whose managed source is
missing is checked in quarantine. A candidate must still be a regular file and
pass the full PNG/SVG size, dimension, and content validation. A valid source
is renamed back to the managed root. A damaged source remains quarantined and
the normal renderer placeholder remains available.

When an unprotected asset is quarantined, its unused texture handle is removed
from the in-process cache. A restored reference is decoded through the normal
preload/refresh path; no filesystem or decoder work enters activation.

## Lifecycle integration

Reconciliation runs:

- at startup, before icon preload and overlay construction;
- after successful portable settings import, with the imported configuration
  included as the open draft;
- after successful direct icon import, with the complete Settings draft;
- after successful atomic configuration save;
- after a clean Settings close or explicit **Discard changes**.

Removing an assignment therefore does not immediately bypass the backup. The
asset remains protected while either `config.json.bak` or the current draft
still references it. It can move only after every protected source releases
the reference.

## Diagnostics and privacy

One fixed `icon_storage_reconciled` event reports only:

- protected, restored, quarantined, conflict, unknown, and error counts;
- the current configuration revision;
- whether backup-safe quarantine was allowed.

No filename, selected path, icon contents, label, action payload, or backup
parse excerpt is logged.

## Automated coverage

The complete debug suite contains 213 tests:

- 161 `gradial-app` tests;
- 52 `gradial-core` tests.

New tests verify:

- exact canonical filename recognition and nil/uppercase/name rejection;
- missing managed-icon storage as a clean empty state;
- active, valid-backup, and open-draft reference union;
- malformed-backup cleanup disablement;
- movement of one unprotected canonical asset;
- retention of protected assets;
- preservation of unknown files and canonical-looking directories;
- non-overwrite behavior when the quarantine destination already exists;
- valid live-asset restoration while quarantine is disabled;
- rejection of damaged restoration candidates;
- cache placeholder retry after managed restoration.

## Native Windows evidence

`smoke-settings-editor.ps1 -CaptureScreenshot` passes in a disposable
`LOCALAPPDATA` tree. Before Gradial starts, the harness places a valid
canonical UUID-named PNG in managed storage without any configuration
reference. It then:

1. requires startup reconciliation to report exactly one quarantined asset;
2. verifies the managed-root source is absent;
3. verifies the exact UUID-named quarantine file exists;
4. compares SHA-256 hashes with the source fixture;
5. continues through managed icon import, portable export, save, live reload,
   and the existing Settings safety checks.

All paths remain inside the disposable tree, and cleanup removes only that
tree and process.

## Remaining boundary

Recoverable quarantine completes conservative managed-icon ownership for
Milestone 2. Permanent deletion, retention duration, and any user-facing
storage-management command remain deferred. They require a separate explicit
policy rather than being inferred from this recoverable move.
