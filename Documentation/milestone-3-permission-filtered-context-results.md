# Milestone 3 permission-filtered context results

Date: 2026-08-10

## Outcome

Gradial now supplies each continuous Lua session with only the frozen target
fields approved for that plugin. This completes the permission-filtered context
deliverable without adding any host capability or operating-system effect.

At invocation-down, the interaction runtime resolves the menu and consults the
worker's immutable catalog of currently admitted actions. One catalog value
contains the manifest cadence and the approved context flags together. This
prevents action availability, update rate, and context authorization from
coming from independently changing lookups.

## Field boundary

`context.application` requires the approved `context.application` permission.
When approved and available, it contains exactly:

- the frozen foreground executable path; and
- the frozen window class.

If the executable path could not be captured, the complete application object
is omitted. Gradial does not send a partial application identity.

`context.window_title` is independent and requires the approved
`context.window_title` permission. An empty captured title remains an approved
empty string; it is different from an omitted unapproved title.

The permitted context cannot contain the foreground HWND, PID, process handle,
process start time, cursor position, monitor identity or geometry, physical
modifier state, activation binding, configuration revision/object, or another
plugin's state. Lua receives omitted optional fields as `nil`. Supplied tables
remain protected read-only host values.

## Freezing and revocation

The filtered context is built from the owned target snapshot captured when the
activation button is pressed. Window-title, foreground, process, and display
changes after that point cannot mutate the action's context.

Only successfully loaded adapters publish action admission. Disablement,
faulting, missing packages, reload-required state, and permission review expose
no action record. When Gradial observes a permission change, the worker unloads
the adapter and removes its action plus context admission together before
reapproval. During an already active session, the existing bounded deferred
reconciliation rule preserves exact terminal delivery; data already supplied
to Lua cannot be revoked retroactively, and the adapter is removed immediately
after the terminal callback.

Gradial does not monitor plugin files continuously. Local edits become visible
through startup, manual rescan, or a requested plugin operation, matching the
existing authoring and trust model.

## Evidence

Focused automated coverage proves:

- application permission exposes the exact frozen path and class but no title;
- title permission exposes the exact frozen title but no application identity;
- requesting neither permission produces an empty permitted context;
- a missing executable path omits application context even when approved;
- a valid empty title remains distinguishable from an unapproved title;
- Lua receives omitted application/title fields as `nil`;
- only effectively enabled adapters publish action admission;
- cadence and both context permission flags originate from the same catalog
  record; and
- an observed permission change removes that catalog record before reapproval.

The complete repository check covers formatting, warning-denied Clippy,
PowerShell analyzer self-tests, all-feature debug and release suites,
documentation tests, and the debug workspace build. The debug suites pass 259
application, 86 core, and 23 Lua tests. The optimized suites pass 255
application, 86 core, and 23 Lua tests.

## Deferred boundary

The context object is data-only. It cannot synthesize input, display a
notification, open a socket, launch a process, access files, or otherwise ask
Windows to do anything. `input.shortcut`, `notification`, and
`network.loopback` remain separate permission-checked capability brokers for
later Milestone 3 slices.

Subsequent status (2026-08-10): `input.shortcut` is now implemented as that
separate broker. `notification` and `network.loopback` remain deferred. See
`milestone-3-shortcut-capability-results.md` for the new boundary and evidence.
