# Milestone 2 plugin lifecycle bridge results

Date: 2026-08-10

## Outcome

Gradial's dedicated plugin worker now accepts the runtime-neutral continuous
Begin, Update, Commit, and Cancel requests and invokes the matching worker-owned
Lua adapter. The bridge is isolated from live radial interaction for this
slice and exposes no host capability.

One global active-session record binds an opaque session ID to one admitted
plugin. A successful Begin establishes ownership. Later requests must match
that exact session; a second Begin, an unknown session, or work after terminal
completion is rejected before Lua is called.

## Bounded delivery

The lifecycle client uses three separate bounded lanes:

- one pending Begin;
- one pending ordinary Update; and
- one pending terminal Commit or Cancel.

The worker selects terminal work before Begin, Update, and management work.
Consequently an ordinary update occupying its lane cannot displace a terminal
request. The existing runtime-neutral coordinator remains responsible for its
single latest-value slot and exact final sample, so the worker does not create
a second unbounded stream.

## Fault and teardown behavior

Callback execution, resource-limit, or return-shape failure faults the Lua
adapter, clears worker session ownership, removes the adapter, and attempts to
persist faulted state. Only a stable error category crosses the worker boundary
or reaches logs. A failed fault-state write still leaves the adapter absent for
the current process.

Commit and Cancel clear worker ownership before invoking Lua. The lifecycle
adapter independently removes its private session table before a terminal
callback, so even a failing terminal callback cannot be repeated or retain the
session.

## Settings and package changes during a session

State-changing Settings operations return `session_active` while a lifecycle
session exists. They never silently destroy its Lua table. A code-free rescan
may report changed source or permissions immediately, but reconciliation and
adapter teardown are deferred until Commit or Cancel. The deferred scan then
persists permission changes and unloads any adapter whose effective status is
no longer enabled.

The later live coordinator slice will turn reload and disable requests into a
continuous Cancel followed by the requested operation. This isolated worker
slice deliberately does not invent that cross-component policy.

## No-side-effect example and evidence

`Gradial APP/examples/plugins/org.gradial.example.meter` is a complete manifest
and Lua package with no requested permissions. It keeps only the latest scalar
and an update count in private session state. It performs no file, process,
input, network, notification, or window action.

Focused worker tests cover:

- loading the checked-in example and completing Begin, Update, and Commit;
- rejecting post-terminal updates;
- enforcing one worker-owned session;
- rejecting Settings teardown during an active session;
- reporting a changed package immediately and unloading it only after Cancel;
- faulting and unloading after an Update callback error; and
- admitting terminal work while the ordinary update lane is full.

The complete repository check passes the PowerShell analyzer self-tests,
formatting, warning-denied linting, all-feature tests, documentation tests, and
the debug workspace build. The debug suites pass 249 application, 81 core, and
22 Lua tests. Independent optimized suites pass 245 application, 81 core, and
22 Lua tests.

## Deferred boundary

No production radial invocation submits lifecycle work yet. Manifest action
catalog exposure, generic parameter editing, live coordinator routing, and all
host capabilities remain separate slices.

This historical boundary was completed by
`milestone-2-plugin-runtime-route-results.md`; generic parameter editing and
host capabilities remain deferred.
