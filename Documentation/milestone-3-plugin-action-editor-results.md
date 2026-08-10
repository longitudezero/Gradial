# Milestone 3 plugin action editor results

Date: 2026-08-10

## Outcome

Gradial Settings can now create, edit, and assign continuous plugin actions
without running plugin code. This is the first bounded Milestone 3 slice. It
completes generic manifest parameter controls and separate slice-extension
assignment while deliberately adding no host capability.

The worker inventory snapshot carries the declarative action schemas of every
compatible discovered package. The **New action** menu offers only actions from
currently admitted plugins. Choosing one creates a portable
`PluginContinuous` action with a fresh UUID, readable name, exact plugin/action
IDs, and its manifest-defined initial parameters.

Disabled, faulted, missing, incompatible, and permission-review packages are
not offered for new action creation. Existing portable references to them are
retained. They can still be renamed, duplicated, removed when unreferenced,
imported, and exported without causing discovery, enablement, or Lua execution.

## Generic parameter editor

The action editor renders the closed manifest schema directly:

- booleans use a checkbox;
- signed integers and finite numbers use manifest-bounded controls;
- strings show their character count and declared length limit;
- choices use only the manifest's closed list; and
- optional values have a separate inclusion checkbox.

Defaults are applied without Lua. A required parameter without a default gets
a deterministic in-range editor value: false, the numeric minimum, a
minimum-length placeholder string, or the first choice. Unknown keys, missing
required values, type mismatches, range/length/choice violations, and an
oversized serialized map block save whenever the matching compatible manifest
is locally available. A reset operation reconstructs the complete initial map.

Missing manifests do not block unrelated configuration work. This preserves
the schema-six guarantee that plugin references remain portable across
machines. Once the matching manifest is present, its schema becomes
authoritative for editing and save validation.

## Independent wedge assignment

Every Global and application-profile wedge now displays **Enable distance
beyond this wedge**. Enabling it selects one continuous action from the draft's
continuous catalog and writes only `continuous_action_id`. Disabling it clears
only that field. The existing discrete `action_id`, label, and icon are never
rewritten.

The visible explanation matches runtime hybrid behavior: release over the
wedge invokes its normal action, crossing the outer edge invokes its continuous
extension, returning inside switches back to the normal action, and direction
selection remains unlocked throughout the gesture.

## Runtime enforcement

The worker's loaded action catalog now retains the complete manifest action
definition rather than only its update rate. Before Lua Begin, the worker
requires the exact declared action and validates the frozen parameter map with
the same core validator used by Settings. Invalid parameters are rejected as a
closed Begin outcome without calling Lua or faulting an otherwise healthy
adapter.

This runtime check protects configurations loaded outside Settings and closes
the race between local manifest replacement and a previously authored portable
action. The existing configuration-size and continuous-snapshot checks remain
independent defense-in-depth boundaries.

## Evidence

Focused automated coverage proves:

- initial values use defaults and valid deterministic fallbacks;
- undeclared, missing, mistyped, and out-of-range values fail closed;
- only loaded manifest actions are offered for creation;
- disabled and reload-required plugins expose no creation candidate;
- known manifest schemas participate in draft validation;
- a missing plugin remains portable and does not block the draft;
- the wedge checkbox changes only continuous assignment;
- manifest action metadata and update cadence survive the worker snapshot; and
- invalid Begin parameters are rejected before deliberately failing Lua code
  can execute.

The complete repository check covers formatting, warning-denied Clippy,
PowerShell analyzer self-tests, all-feature debug and release suites,
documentation tests, and the debug workspace build. The debug suites pass 257
application, 86 core, and 22 Lua tests. The optimized suites pass 253
application, 86 core, and 22 Lua tests.

## Deferred boundary

Lua still receives an empty permitted context and no operating-system service.
Permission-filtered application/title context, `input.shortcut`, notification,
and `network.loopback` brokers, capability revocation/rate/timeout evidence,
dependency summaries, and external-effect examples remain later Milestone 3
work.
