# Milestone 2 plugin discovery results

Date: 2026-08-04

## Outcome

Gradial now has the first bounded Milestone 2 foundation: strict declarative
plugin manifests and read-only local package discovery. Startup creates and
scans `%LOCALAPPDATA%\Gradial\plugins`, records a deterministic inventory, and
does not load or execute any Lua source.

Every immediate package is classified independently as compatible,
API-incompatible, or invalid. A malformed package therefore remains visible as
invalid without blocking healthy packages before or after it in directory
order. API incompatibility is distinct from corruption: a structurally valid
manifest written for a future plugin API can be explained accurately later in
Settings.

## Declarative manifest

`plugin.toml` version 1 is authoritative for:

- lowercase plugin identity and exact package-directory ownership;
- nonempty display name and semantic plugin version;
- plugin API version and a safe relative `.lua` entry path;
- a closed set of requested permissions;
- unique continuous action IDs, names, descriptions, and 10–60 Hz update
  rates; and
- boolean, bounded integer, finite bounded number, bounded UTF-8 string, and
  declared-choice parameters, including labels, defaults, and required state.

The parser rejects unknown fields and enum values rather than silently
ignoring misspellings. It also rejects duplicate permissions, action IDs, and
parameter keys; invalid defaults/ranges; excessive collection sizes; unsafe
entry syntax; non-UTF-8 TOML; and manifests above 64 KiB. Parameter-specific
validation is now available without trusting executable code, but portable
configuration references remain unavailable until a later catalog/editor
slice connects them.

## Package boundary

Discovery applies the v0.3 package rules before returning a compatible entry:

- the managed plugin root, package, manifest, directories, and source files
  are inspected without following symlinks, junctions, or other reparse points;
- canonical package/source paths must remain inside the expected root;
- traversal, absolute paths, alternate-data-stream syntax, and an entry source
  absent from the inventory are rejected;
- native modules and precompiled Lua modules make that package invalid;
- every `.lua` file must be UTF-8 and no larger than 256 KiB;
- the complete Lua source set is limited to 1 MiB and 256 files;
- the managed root is limited to 1,024 entries, one package to 4,096 entries,
  and directory nesting to 32 levels; and
- bounded file reads recheck actual bytes instead of trusting potentially stale
  metadata lengths.

Other files can exist as inert documentation or assets, but discovery never
exposes them as source modules. The future custom Lua loader must use a newly
validated source inventory and may expose only accepted `.lua` files.

## Privacy and isolation

Routine startup logs contain the compatible plugin ID and version or a
sanitized package name, stable result/error code, and aggregate source size.
They do not contain manifest text, Lua source, parameter defaults, source
paths, window context, or capability data.

Root-level failure produces one bounded report rather than crashing. Package
failures are isolated. Test fixtures prove that malformed TOML, an ID/directory
mismatch, future API, non-UTF-8 source, oversized source, and a native module
receive the intended status while a neighboring valid package remains
discoverable.

## Evidence

The repository check passed PowerShell harness self-tests, formatting, strict
Clippy, all-feature tests, documentation tests, and debug builds. The debug
suite passed 216 application tests and 81 core tests. The separate all-feature
release suite passed 212 application tests and 81 core tests.

Of those, seventeen focused tests cover the manifest and discovery layer. They
exercise the plan's example manifest, closed fields/permissions, unsafe entry
paths, duplicate action/parameter IDs, invalid parameter defaults, bounded
manifest input, future API classification, malformed-package isolation,
directory/ID mismatch, UTF-8 enforcement, native-module rejection, individual
and aggregate source bounds, source-count/root-entry/nesting limits,
unavailable roots, and Windows directory-reparse rejection.

## Superseded deferred boundary

The frozen source loader, Lua sandbox/lifecycle adapter, and atomic
`plugins-state.json` trust model originally deferred by this report are now
implemented in later isolated slices. Capability brokers, the Settings plugin
page, a plugin worker, rescan/reload controls, and live continuous catalog
connection remain deferred. Newly discovered source still has no production
execution path, and enable/reload must repeat complete discovery before a Lua
state can be admitted.
