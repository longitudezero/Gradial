# Milestone 2 reusable-sequence editor results

Date: 2026-07-30

This record covers the bounded reusable-sequence editor: safe creation,
ordered-step editing, nested sequence selection, cycle prevention, live
expanded metrics, persistence, and runtime-plan compatibility. It completes
editing for every schema-two action type.

## Creation and ordered editing

The catalog's **New action** menu includes **Sequence** after at least one
regular action exists. A new sequence receives a new UUID, a unique readable
name, and one existing leaf step, keeping the draft valid from creation.

Each sequence row displays the referenced action name and type. **Up** and
**Down** exchange exactly one adjacent row, **Remove** deletes exactly one
reference, and the final row cannot be removed. **Add step** appends one chosen
action. Repeated action UUIDs are retained because repetition is meaningful in
an ordered execution plan.

Rename, duplicate, reference-safe delete, draft isolation, guarded close,
atomic save, backup recovery, and live reload continue to apply unchanged.
Deleting an action used by a sequence remains blocked until every step
reference is removed.

## Validator-backed candidate filtering

Nested sequences remain supported through the existing model. The editor
constructs the complete proposed configuration for every possible added
action and invokes the authoritative configuration validator. It lists a
candidate only when that configuration remains valid.

As a result, the list hides:

- the sequence itself;
- any ancestor that would create a direct or indirect cycle;
- additions that would exceed four nested sequence levels;
- additions that would expand this sequence or any containing sequence beyond
  32 executed leaf steps;
- additions that would take total Delay beyond 30,000 ms.

Safe repeated actions and safe nested sequences remain visible. The UI does
not maintain a second approximation of the safety rules.

## Live expanded metrics

After each edit, the sequence is inserted into a cloned draft and compiled
through `ActionPlanCompiler`. The editor shows:

- the number of direct ordered rows;
- the number of flattened leaf operations that will execute;
- maximum nested sequence depth against the four-level limit;
- accumulated Delay against the 30,000 ms limit.

The displayed executed-step and Delay values therefore come from the same
flattened operation plan used at runtime. A defensive depth walk uses the same
sequence graph after complete validation.

Field-specific save messages cover empty or oversized direct lists, missing
steps, cycles, excessive nesting, excessive expanded steps, and excessive
Delay. Complete validation and immutable plan compilation still repeat before
atomic disk replacement.

## Automated and native evidence

Unit coverage verifies:

- valid sequence creation from the action template;
- exact add, remove, move-up, and move-down ordering;
- refusal to remove the final step or move beyond either boundary;
- nested expansion into the expected executed-step count, depth, and Delay;
- filtering of self-reference, indirect cycles, and Delay-limit overflow;
- field-specific empty-sequence and cycle validation.

The isolated native settings harness creates a base sequence containing
shortcut, text, and Delay actions. It creates a nested sequence, temporarily
adds and removes a step, then moves its base sequence below a window command.
The real editor surface must report:

- two direct steps;
- four executed leaf steps;
- nested depth two of four;
- 275 ms of 30,000 ms total Delay.

After the guarded close and save, revision 2 must retain exactly ten actions.
The base sequence must preserve shortcut, text, and Delay UUIDs in that order;
the nested sequence must preserve window-command then base-sequence UUIDs.
The prior revision remains in `config.json.bak`, and a fresh M4 invocation uses
the reloaded menu. The fixture remains isolated from the user's configuration.

Final verification passed:

- 162 debug workspace tests and 160 release workspace tests;
- Clippy with warnings denied for every target and feature in debug and release;
- debug and release builds;
- formatting and PowerShell syntax checks;
- the expanded native sequence-editor, save, backup, and live-reload workflow;
- one live release M4 Delay invocation.

## Current boundary

The global menu and complete action catalog are now editable. Application
profiles and match rules, activation binding, appearance, icons, import/export,
onboarding, and the current-window profile tester remain future bounded
slices.
