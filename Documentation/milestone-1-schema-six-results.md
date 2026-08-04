# Milestone 1 schema-six configuration results

Date: 2026-08-04

## Outcome

Gradial's portable configuration now uses schema 6. The model can retain a
continuous plugin action and assign it independently from a wedge's existing
discrete action without loading or executing plugin code.

The schema-six root stores `continuous_full_scale_radius_dip`. The accepted
range is 160 through 400 DIP and new configurations use 300 DIP. The active
value is frozen by the interaction runtime for each invocation; the existing
debug-only command-line distance continues to take precedence when explicitly
provided.

Each `MenuSlice` now has two independent optional UUIDs:

- `action_id` remains the ordinary release action inside the visible wedge;
- `continuous_action_id` is the extension beyond the wedge's outer edge.

`ActionKind::PluginContinuous` stores the plugin ID, plugin action ID, and a
sorted generic parameter map. Generic values serialize with an explicit
boolean, integer, number, string, or choice tag so string values and manifest
choices cannot become ambiguous during a portable round trip.

## Validation boundary

Configuration activation, save, import, and export now enforce:

- lowercase closed identifiers no longer than 96 bytes;
- closed parameter value types and finite floating-point values;
- a 16 KiB serialized parameter-map ceiling;
- continuous actions only in `continuous_action_id`;
- non-continuous actions only in the ordinary discrete slot;
- no continuous action at any sequence depth; and
- no direct compilation of a continuous action by the discrete executor.

The 16 KiB ceiling is checked again when a runtime-neutral
`ContinuousActionSnapshot` is constructed. Manifest-specific required keys,
choice membership, and number/string ranges remain deferred until a manifest
catalog exists.

## Unavailable references

A valid `PluginContinuous` definition does not require its plugin to exist on
the current machine. Loading such a file is not configuration recovery and
does not discard the reference. Profile compilation freezes its UUID with the
rest of the selected menu.

There is deliberately no plugin catalog in this slice, so the normal
configuration path exposes no continuous-capable wedges. A stored unavailable
extension therefore remains discrete at every radius and cannot begin a
continuous session. Its independent discrete action still releases normally.
Settings preserves the definition, identifies it read-only, excludes it from
discrete action pickers and sequences, and prevents deletion while a
continuous wedge assignment references it.

No plugin directory is scanned. Nothing is downloaded, installed, enabled,
approved, or executed, and no Lua dependency is introduced.

## Migration and transfer

Schema-five files migrate in memory to schema six with:

- `continuous_full_scale_radius_dip: 300`;
- `continuous_action_id: None` for every wedge; and
- no plugin action definitions.

Existing schema-five icons and all prior configuration data are retained.
Startup leaves the original bytes untouched and persists schema six only after
an explicit successful save. The same migration applies to Settings import.
Export now writes schema six and retains plugin references and non-secret
generic parameters.

The provenance checks were extended with the new boundary: schema-one through
schema-five files are rejected if they contain any schema-six root, action, or
wedge field, even when an optional field is explicitly `null`.

## Evidence

Automated coverage proves:

- 160, 300, and 400 DIP are accepted while out-of-range values fail;
- every generic plugin value round-trips through schema-six JSON;
- invalid identifiers, parameter keys, non-finite numbers, and oversized maps
  fail before activation;
- discrete/continuous slot inversion and sequence inclusion fail;
- direct discrete compilation returns a typed continuous-executor error;
- a missing local plugin reference loads without migration or recovery;
- resolved profile menus freeze continuous UUIDs with labels, icons, and
  discrete UUIDs;
- schema-five migration preserves icons, supplies the new defaults, and does
  not rewrite the source; and
- all three schema-six field families fail when smuggled under schema five.

The repository check passed with 206 debug application tests and 74 core
tests, formatting, strict Clippy, script self-tests, and debug builds. The
all-feature release suite passed with 202 application tests and 74 core tests.
The complete Settings smoke check passed schema-six portable export, atomic
save, and live reload. A quick live continuous matrix then passed three
scenarios and all 24 expected inert outcomes.

## Deferred boundary

This slice does not discover manifests, determine installed/enabled status,
validate manifest-specific parameter declarations, expose the continuous
checkbox or distance control in Settings, construct plugin snapshots from
resolved menus, emit the manifest-backed portable dependency summary, or run
Lua. Those are subsequent bounded v0.3 slices.
