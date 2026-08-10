# Milestone 2 plugin trust-state results

Date: 2026-08-10

## Outcome

Gradial now has a versioned, atomic, machine-local trust-state boundary for
plugins. Startup loads and reconciles `%LOCALAPPDATA%\Gradial\plugins-state.json`
after read-only discovery, but still never constructs a Lua state. There is no
enable button, Settings page, capability, plugin worker, or live coordinator
connection in this slice.

## Stored state

Schema one stores only:

- a checked state revision;
- enabled and faulted flags;
- canonical approved permission names; and
- the last successfully loaded plugin/manifest/API version and package SHA-256
  fingerprint.

It stores no source text, manifest text, package path, action parameter,
credential, window context, target identity, or capability payload. Portable
`config.json` import/export neither includes nor changes this local state, and
the three plugin-state paths are now protected configuration-export
destinations.

## Trust and reload transitions

Permission approval and enablement are separate. Approving the currently
declared permission set leaves a plugin disabled. Recording a successful load
requires a current compatible package and matching approval; its API contract
requires the caller to have completed fresh discovery, bounded Lua loading,
and callback validation first. No production caller exists yet.

The deterministic package fingerprint covers the exact bounded manifest plus
every accepted Lua path and byte. Reconciliation therefore distinguishes:

- permission changes, which immediately disable an enabled record and require
  review of the new set;
- other manifest or source edits, which retain approval and produce
  `reload_required`; and
- missing, invalid, or API-incompatible packages, which cannot remain enabled.

Fault state is retained for future worker recovery. Explicit disable or a
later validated successful load clears the fault.

The future worker must admit only a record whose reconciled effective status
is `enabled` for the package found by its fresh scan. The stored enabled flag
alone is never sufficient authorization to construct or retain a Lua state.

## Atomic persistence and recovery

The state document is capped at 512 KiB and 1,024 plugin records. Strict reads
reject unknown fields, unsupported schemas, malformed IDs, duplicate or
noncanonical permissions, invalid package hashes, impossible lifecycle state,
and oversized input before use.

Every save validates the current and proposed documents, advances the revision,
flushes `plugins-state.json.tmp`, and uses a write-through Windows atomic
replacement. The prior valid primary becomes `plugins-state.json.bak`. A stale
writer is rejected. An interrupted write cannot replace the current primary.
A broken primary recovers from the backup without rewriting evidence. When
both copies are unusable, Gradial preserves them and proceeds with an empty
in-memory state, so all plugins are disabled while the rest of the app remains
available.

## Evidence

Focused fixtures cover:

- separate approval and successful-load transitions;
- canonical permission ordering;
- automatic disablement after permission changes;
- source and non-permission manifest edits requiring reload without approval;
- incompatible, missing, and invalid package handling;
- persistent fault state and its explicit recovery transitions;
- first-run atomic initialization and backup preservation;
- interruption, stale-writer, broken-primary, future-schema, oversized, and
  impossible-state recovery;
- state-file privacy sentinels; and
- discovery/reconciliation of deliberately invalid Lua text without executing
  it.

The complete repository check passed with formatting, warning-denied linting,
PowerShell analyzer self-tests, documentation tests, and the debug workspace
build. The debug suites passed 229 application, 81 core, and 22 Lua tests. The
optimized suites passed 225 application, 81 core, and 22 Lua tests.

## Superseded deferred boundary

This slice originally stopped at read-only startup reconciliation. The
dedicated owner and production admission caller are now implemented in
`milestone-2-plugin-worker-results.md`. The Plugins page, capability effects,
and live continuous coordinator connection remain deferred.
