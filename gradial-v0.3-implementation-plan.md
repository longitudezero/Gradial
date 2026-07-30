# Gradial v0.3 — Continuous Input and Lua Plugin Plan

Status: proposed post-v1 implementation baseline

Target: Windows 11, x86-64

Implementation language: Rust with embedded Lua 5.4

`v0.3` is the version of this planning baseline, not a promise that these
features ship before an application version numbered `1.0`.

This document defines the first extension to the Windows MVP described by
`gradial-v0.2-implementation-plan.md`. It does not reopen or weaken the v0.2
interaction, focus, input-suppression, configuration-recovery, privacy, or
performance contracts. Unless this document explicitly changes a behavior, the
v0.2 requirement remains authoritative.

The broader plugin ideas in `plan v0.1.md` remain long-term direction. This
document replaces that file's plugin recommendations only for the bounded v0.3
work described here.

Requirements marked **MUST** are release-blocking. Requirements marked
**SHOULD** may be changed only with a documented reason and a replacement test.

## 1. Product outcome

Gradial v0.3 adds radial distance as a second input dimension:

> Hold → choose a direction → vary distance → release.

Direction still chooses the action. Distance supplies that action with a
continuous normalized scalar. Gradial measures, displays, bounds, and delivers
the scalar; it does not contain application-specific concepts such as media
seek position, volume, brush size, timeline zoom, or color intensity.

Application-specific behavior belongs to a plugin. A media-player plugin may
map the scalar to seeking, a painting plugin may map it to brush size, and a
different plugin may divide the scalar into discrete bands. Gradial does not
gain separate hardcoded implementations for those applications.

The first plugin runtime is Lua because the initial product goal is local,
approachable scripting. A user should be able to inspect and edit a text file,
reload it, and create a useful plugin without installing a compiler toolchain.

This release does **not** claim that arbitrary downloaded Lua is safe. v0.3
plugins are explicitly presented as local scripts written by the user or
obtained from a source the user trusts. The runtime still contains mistakes,
limits resource use, and exposes only declared Gradial capabilities.

## 2. Release boundary

### Included

- One continuous scalar per plugin-backed continuous action.
- A normalized `0.0..=1.0` value derived from radial distance.
- Begin, update, commit, and cancel lifecycle messages.
- Direction locking after a continuous action is armed.
- Latest-value coalescing with a configurable 10–60 Hz update rate.
- Production-overlay feedback for continuous distance.
- Edge-assist support for the full continuous range without cursor warping.
- A runtime-neutral, versioned continuous-action protocol.
- Lua 5.4 source plugins embedded through a Rust host.
- One isolated Lua state per enabled plugin.
- Declarative plugin manifests and action definitions.
- Explicit plugin enablement and permission review.
- Bounded, permissioned access to:
  - Frozen target-application context.
  - The existing safe keyboard-shortcut backend.
  - Non-focus-stealing notifications.
  - Manifest-allowlisted loopback HTTP endpoints.
- Generic bounded parameters declared in the plugin manifest.
- A Plugins settings page, manual rescan, and manual reload.
- Configuration schema-six migration and plugin-action editing.
- A no-side-effect continuous-meter example plugin.
- A shortcut-based example showing both smooth and banded mappings.
- Developer documentation and machine-readable validation evidence.

### Explicitly deferred

- WASM execution.
- A plugin marketplace, discovery service, automatic download, or automatic
  update.
- A promise that hostile third-party plugins are safely contained.
- Native DLL plugins, Lua C modules, FFI, or arbitrary Win32 access.
- Arbitrary filesystem, non-loopback network, shell, PowerShell, environment,
  registry, process-launch, clipboard, pointer-movement, mouse-click, drag, or
  scroll capabilities.
- Plugin-defined UI, overlay drawing, custom settings widgets, or dynamic
  renderer content.
- Background plugins, timers outside an active invocation, global event
  subscriptions, context providers, and plugin-to-plugin calls.
- One-shot plugin actions, plugin actions inside sequences, and sequences
  inside continuous actions.
- Multiple simultaneous continuous parameters, two-dimensional values,
  pressure, velocity as a separate input, acceleration, inertia, or haptics.
- Application-value feedback into Gradial, such as displaying the current VLC
  timestamp or Krita brush size.
- A separate core action-band feature. A Lua plugin may quantize the continuous
  value into bands when that behavior is useful.
- Secrets or credential storage in plugin parameters.
- Linux application integrations. The protocol remains portable, but the v0.3
  product and tests remain Windows-only.

Native plugins remain unsupported. WASM is the intended future runtime for
downloadable or marketplace-distributed plugins.

## 3. Continuous interaction contract

### 3.1 Discrete and continuous slices

Every menu slice remains exactly one of:

1. Empty.
2. Assigned to a v0.2 discrete action.
3. Assigned to a v0.3 continuous plugin action.

A discrete slice behaves exactly as it did in v0.2. Merely installing or
enabling plugins MUST NOT change normal hold/flick/release behavior, geometry,
latency, cancellation, or action execution.

A continuous action is valid only as a direct menu-slice assignment. It cannot
be referenced by a sequence because its lifetime is tied to the held activation
control rather than the bounded discrete executor.

### 3.2 Distance mapping

All persisted dimensions use DIP and are converted using the invocation
monitor's DPI.

| Parameter | v0.3 default |
| --- | ---: |
| Existing center cancel radius | 24 DIP |
| Existing direction-selection radius | 36 DIP |
| Continuous arm radius | 48 DIP |
| Continuous full-scale radius | 160 DIP |
| Default update rate | 30 Hz |
| Allowed update-rate range | 10–60 Hz |

Before the pointer reaches the continuous arm radius, angle may change the
selected slice using the existing v0.2 geometry and hysteresis.

For radial distance `d`, arm radius `a`, and full-scale radius `f`, Gradial
computes:

```text
value = clamp((d - a) / (f - a), 0.0, 1.0)
```

- The mapping is linear in v0.3.
- The value is finite and inclusive of `0.0` and `1.0`.
- Distance beyond the full-scale radius remains `1.0`.
- The scalar is unsigned. Opposite meanings such as seek forward/back or
  increase/decrease are separate plugin actions in v0.3.
- Application units and curves belong to the plugin.
- A plugin may implement action bands by comparing the value with its own
  thresholds.
- Raw cursor coordinates and raw Win32 handles are never included in a
  continuous sample.

The Milestone 0 usability prototype may adjust the 48 DIP and 160 DIP defaults.
Changing them requires recorded results for precision, accidental arming, edge
use, and cancellation; it does not change the normalized protocol.

### 3.3 Arming and direction lock

A continuous session arms when all of the following are true:

1. A continuous slice is selected.
2. Distance reaches at least the arm radius.
3. The target snapshot remains valid.
4. The referenced plugin and action are enabled and healthy.
5. The plugin accepts `Begin`.

Crossing the arm radius freezes the candidate direction before `Begin` is
submitted. If the plugin rejects or times out, the invocation cancels instead
of changing direction. Once `Begin` is accepted:

- The selected slice is locked for that invocation.
- Angular movement no longer switches to a neighboring action.
- Radial movement continues to update the scalar.
- Choosing a different direction requires cancelling and invoking Gradial
  again.
- The continuous session owns the action coordinator until it ends.
- A second discrete or continuous dispatch fails immediately with
  `action_busy`.

Direction locking prevents live side effects from unexpectedly switching
between plugins or actions during one held gesture.

Releasing on a continuous slice before the arm radius is reached cancels with
no plugin callback and no application effect.

### 3.4 Release and cancellation

Releasing the activation control after a successful `Begin` sends `Commit`
with the final sample. `Commit` is delivered even if the same value was already
sent in the latest `Update`.

The v0.2 cancellation causes remain valid. In addition, an active continuous
session cancels when:

- The target identity or foreground relationship becomes stale.
- The plugin faults, exceeds a limit, unloads, or loses a required capability.
- The capability broker becomes unavailable.
- Plugin reload is requested while the session is active.

Returning inside the 24 DIP center radius after `Begin` cancels the continuous
session. It does not select another slice.

`Cancel` means “stop producing effects and release session resources.” It does
**not** promise to undo effects already applied by a plugin. The settings UI,
author guide, and API documentation MUST use the terms **stop** or **cancel**,
not **rollback**, for this behavior.

After any terminal event:

- No later update may be delivered for that session.
- Gradial makes one best-effort terminal callback.
- Input suppression and overlay closing follow the v0.2 state machine.
- A missing or failed terminal callback cannot keep Gradial open or busy
  indefinitely.

### 3.5 Visual feedback

The overlay renderer receives only Gradial-owned immutable render data:

- Locked direction.
- Normalized scalar.
- Armed/active/faulted visual state.

The production overlay SHOULD show a radial progress track from arm radius to
full-scale radius. It may display `0–100%`, but it does not display a
plugin-supplied dynamic label or application value in v0.3.

Plugin code MUST NOT run during layout, painting, texture upload, animation, or
frame presentation. A stalled plugin may stop application effects, but it
cannot stall pointer highlighting or closing animation.

### 3.6 Edge assist and DPI

Continuous distance uses the same physical-pointer or raw-delta virtual pointer
selected by the v0.2 invocation geometry.

- Edge assist accumulates signed raw deltas beyond the visible work-area edge.
- The system cursor is not warped.
- The foreground application receives no synthetic pointer movement.
- The normalized value is the same for equivalent DIP movement at every tested
  monitor scale.
- A display, work-area, or DPI topology change cancels the session before the
  overlay pool is rebuilt.
- Negative virtual-screen coordinates remain valid.

The overlay's prewarmed extent and scissor behavior may expand to display the
160 DIP track, but window or graphics-device creation remains prohibited during
activation.

## 4. Runtime-neutral plugin protocol

The core protocol is defined independently of Lua so a future WASM adapter can
implement the same behavior without changing gesture, configuration, or
capability semantics.

Conceptually:

```rust
struct ContinuousSample {
    sequence: u64,
    value: f64,       // finite, 0.0..=1.0
    delta: f64,       // value - previously delivered value
    elapsed_ms: u64,  // monotonic time since accepted Begin
}

enum ContinuousRequest {
    Begin {
        session_id: Uuid,
        plugin_id: PluginId,
        action_id: PluginActionId,
        context: PermittedContext,
        parameters: PluginParameters,
        initial: ContinuousSample,
    },
    Update {
        session_id: Uuid,
        sample: ContinuousSample,
    },
    Commit {
        session_id: Uuid,
        final_sample: ContinuousSample,
    },
    Cancel {
        session_id: Uuid,
        reason: ContinuousCancelReason,
    },
}
```

Protocol rules:

- `api_version` is separate from the Gradial application version and begins at
  `1`.
- Every accepted session receives exactly one `Begin`, zero or more `Update`
  requests, and at most one terminal `Commit` or `Cancel`.
- Sequence numbers strictly increase.
- Requests for different sessions are never interleaved in v0.3.
- The value transport is loss-tolerant; each `Update` contains complete current
  state rather than an irreplaceable event delta.
- The final sample is never discarded by coalescing.
- If release or cancellation occurs while `Begin` is pending, Gradial retains
  the latest sample and terminal decision. An accepted `Begin` is followed
  immediately by the terminal request; a rejected `Begin` produces no terminal
  plugin callback.
- Unknown required fields, unsupported major API versions, non-finite numbers,
  out-of-range values, and malformed identifiers fail closed.
- The protocol does not expose Rust types, Lua registry handles, Win32 handles,
  or an in-process ABI.

### 4.1 Coalescing and backpressure

The interaction thread publishes the latest complete sample into a
single-session latest-value slot. It does not enqueue every mouse event.

- At most one ordinary update waits for plugin delivery.
- A newer ordinary sample replaces an older undelivered ordinary sample.
- `Begin`, `Commit`, and `Cancel` are never replaced or dropped.
- The plugin can detect skipped samples from the sequence number.
- Plugin slowness cannot create an unbounded queue.
- The update cadence comes from the action manifest, clamped to 10–60 Hz.
- A value change smaller than an implementation-defined floating-point epsilon
  may avoid an update, but the final sample remains exact.

## 5. Configuration model

v0.3 advances the portable configuration to schema 6.

The action model gains one leaf variant:

```rust
ActionKind::PluginContinuous {
    plugin_id: String,
    plugin_action_id: String,
    parameters: BTreeMap<String, PluginValue>,
}
```

`PluginValue` supports only:

- Boolean.
- Bounded signed integer.
- Finite bounded number.
- Bounded UTF-8 string.
- One value from a manifest-declared choice list.

The manifest defines parameter keys, types, limits, defaults, labels, and
whether a value is required. Parameters are frozen with the selected menu and
configuration revision at activation.

### 5.1 Validation

- Plugin IDs and action IDs use lowercase ASCII letters, digits, `.`, `_`, and
  `-`, with a maximum of 96 bytes.
- Duplicate plugin or action IDs are invalid.
- Parameter keys not declared by the referenced action are invalid.
- Missing required parameters and type/range mismatches are invalid.
- Continuous actions are rejected from sequences at save and import time.
- A continuous action may be assigned only to a menu slice.
- A syntactically valid reference to a missing or disabled local plugin remains
  loadable but is marked unavailable and cannot execute.

Allowing an unavailable plugin reference preserves portable configuration
without treating absent local plugin files as configuration corruption. The
settings editor shows the affected actions and profiles. Activation of such a
slice cancels with `plugin_unavailable`.

### 5.2 Migration, import, and export

- Schema-five configuration migrates to schema six in memory with no plugin
  actions.
- Migration follows the existing explicit-save rule; startup does not rewrite
  a valid legacy file.
- Import and atomic replacement retain the v0.2 recovery contract.
- Export includes plugin action references and non-secret parameter values.
- Export does not copy plugin source or enable plugins on another machine.
- Import never downloads, installs, enables, or approves a plugin.
- The exported configuration lists required plugin IDs and API versions in a
  derived summary so missing dependencies can be explained before save.

Plugin installation and approval state is machine-local and stored atomically
outside portable `config.json`:

```text
%LOCALAPPDATA%\Gradial\
    config.json
    plugins-state.json
    plugins\
```

`plugins-state.json` contains enabled state, approved permissions, last loaded
manifest identity, and fault status. It contains no plugin parameters or
credentials.

## 6. Lua plugin package

### 6.1 On-disk layout

Each plugin is a directory immediately below the managed plugin root:

```text
%LOCALAPPDATA%\Gradial\plugins\
    example.media-controls\
        plugin.toml
        main.lua
        lib\
            mapping.lua
```

- Gradial never executes a plugin merely because files appeared in this
  directory.
- A newly discovered plugin starts disabled.
- Directory traversal, absolute module paths, alternate data streams, symlinks,
  junctions, reparse points, and files outside the canonical plugin root are
  rejected.
- Only UTF-8 Lua source is accepted.
- Precompiled Lua bytecode and native modules are rejected.
- The complete plugin source set is bounded to 1 MiB, with no source file
  larger than 256 KiB.
- A Gradial-provided module loader may load `.lua` files below that plugin's
  own canonical root. Standard `package`, `require`, `loadfile`, and `dofile`
  are not exposed.
- The immediate directory name must exactly equal the manifest plugin ID.

### 6.2 Manifest

The manifest is parsed and validated without executing Lua:

```toml
manifest_version = 1
id = "example.media-controls"
name = "Example media controls"
version = "0.1.0"
api_version = 1
entry = "main.lua"
permissions = ["context.application", "input.shortcut"]

[[actions]]
id = "seek-forward"
name = "Seek forward"
kind = "continuous"
description = "Maps radial distance to repeated seek shortcuts."
update_hz = 30

[[actions.parameters]]
key = "maximum_steps"
label = "Maximum steps"
type = "integer"
default = 12
minimum = 1
maximum = 120
```

The manifest, not executable Lua, is authoritative for identity, visible
actions, parameters, update rates, permissions, and compatibility. This lets
Gradial show and validate a plugin before trusting its code to run.

Changing requested permissions automatically disables the plugin until the
user reviews them again. Editing source without changing permissions remains a
manual reload operation and does not require a new approval; this preserves a
fast authoring workflow for local scripts.

### 6.3 Lua callback adapter

A Lua entry module returns a table implementing:

```lua
local plugin = {}

function plugin.begin(context, parameters, initial_sample)
    return {}
end

function plugin.update(session, sample)
end

function plugin.commit(session, final_sample)
end

function plugin.cancel(session, reason)
end

return plugin
```

- `begin` may return a session-local Lua table.
- Gradial passes that table back only to the matching session callbacks.
- Missing required callbacks fail plugin loading.
- Callback arguments are read-only host values.
- A callback may use only approved functions in the `gradial` host module.
- `begin` initializes session state and cannot request an external effect.
- Effect-producing host functions reject calls made during module loading,
  `begin`, or outside the currently active update/commit/cancel callback.
- Lua errors become stable plugin failures; they never unwind into Gradial.
- Callback return values are bounded and validated before use.

This Lua shape is an adapter, not the permanent cross-runtime ABI. A future
WASM component may represent session state differently while preserving the
protocol in Section 4.

### 6.4 Available Lua environment

The initial environment includes a reviewed subset of:

- Basic value and iteration functions.
- `math`.
- `string`, excluding bytecode-producing facilities.
- `table`.
- `utf8`.
- The permission-filtered `gradial` host module.
- The plugin-rooted source module loader.

It excludes:

- `os`.
- `io`.
- `package`.
- `debug`.
- FFI and native module loading.
- `load`, `loadfile`, `dofile`, and precompiled chunks.
- Access to environment variables or the process command line.
- Direct filesystem, socket, process, registry, clipboard, window, input, or
  renderer APIs.

Lua's standard library is not itself the security boundary. Gradial constructs
the environment, limits the VM, and brokers every external effect. The
implementation should use the safe embedding controls provided by
[`mlua`](https://docs.rs/mlua/latest/mlua/struct.Lua.html), including explicit
library selection, memory limits, and host-owned execution hooks. Library
behavior is checked against the
[Lua 5.4 reference](https://www.lua.org/manual/5.4/manual.html).

## 7. Trust, permissions, and capabilities

### 7.1 Trust statement

The Plugins page MUST display this before first enablement:

> Lua plugins are local scripts. Enable only plugins you wrote or trust.
> Gradial limits their access and resource use, but v0.3 does not promise safe
> execution of hostile downloaded code.

This is not a generic “run anyway” warning shown for every invocation. It is a
clear trust decision made when enabling a plugin or approving new permissions.

The purpose of v0.3 containment is:

1. Prevent common mistakes from destabilizing Gradial.
2. Avoid granting capabilities a plugin did not request.
3. Make external effects understandable to the user.
4. Preserve an API that can later be backed by stronger WASM isolation.

### 7.2 Context permissions

By default a plugin receives opaque session identity, its action ID, approved
parameters, and continuous samples only.

Optional context permissions are:

| Permission | Exposed data |
| --- | --- |
| `context.application` | Frozen executable path and window class |
| `context.window_title` | Frozen window title |

The plugin never receives an HWND, PID with process rights, process handle,
cursor position, monitor path, raw key state, configuration object, or another
plugin's data.

Window titles and executable paths remain private in logs and diagnostic
bundles under the v0.2 rules. A plugin that is permitted to receive a value
does not grant Gradial permission to log it.

### 7.3 Service capabilities

The first capability set is intentionally small but versionable. Adding a
future capability does not require replacing the Lua runtime or continuous
protocol.

#### `input.shortcut`

- Uses the existing scan-code shortcut backend and synthesized-key cleanup.
- Targets only the frozen invocation window.
- Revalidates HWND, PID, process start time, foreground identity, and integrity
  immediately before input.
- Rejects text, mouse input, arbitrary input arrays, and key-down without
  matching key-up.
- Allows at most 30 accepted shortcut requests per second per active session.
- Stops immediately on target loss, cancellation, or plugin fault.

#### `notification`

- Requests a non-focus-stealing Gradial notification.
- Accepts bounded plain text with no markup, URLs, or action buttons.
- Allows at most one notification per session and five per plugin per minute.
- The UI identifies the requesting plugin.

#### `network.loopback`

- Connects only to manifest-allowlisted literal loopback origins using
  `127.0.0.1` or `[::1]`.
- Performs no DNS lookup, proxy discovery, redirect following, Windows
  credential use, or non-loopback fallback.
- Allows only manifest-declared HTTP methods and ports.
- Bounds request and response bodies to 64 KiB.
- Uses a maximum 250 ms operation deadline and a bounded broker queue.
- Allows at most 30 accepted requests per second per active session.
- Enqueues requests without blocking the Lua callback. Lua receives only
  accepted/rejected status; response content is not delivered in v0.3.
- Never logs headers, bodies, query values, or responses.
- Supports no stored secret in v0.3.

Loopback access is included because some desktop applications expose a local
control API. It remains explicit and origin-specific because local services can
still perform powerful operations.

Every service call is checked at the call boundary, not only when the plugin is
loaded. A missing, revoked, or out-of-scope capability fails the current
session with `plugin_permission_denied`.

Host calls validate and enqueue bounded broker work, then return
accepted/rejected status without performing blocking OS work on the Lua worker.
After a terminal callback returns, its request epoch closes; already accepted
terminal work must complete or reach its capability deadline before the action
coordinator releases the continuous-session lease.

### 7.4 Resource and execution limits

Initial release limits:

| Resource | Limit |
| --- | ---: |
| Lua memory per enabled plugin | 16 MiB |
| Plugin source set | 1 MiB |
| One Lua source file | 256 KiB |
| `begin` callback | 100 ms |
| `update` callback CPU/wall budget | 16 ms |
| `commit` callback | 100 ms |
| Best-effort `cancel` callback | 50 ms |
| Host-returned string | 8 KiB |
| Plugin parameters per action instance | 16 KiB serialized |

The implementation MUST combine memory accounting, a host-owned instruction
hook, and wall-clock cancellation. A plugin cannot disable or replace those
controls.

If a callback exceeds a limit:

1. Stop the callback.
2. Cancel the continuous session.
3. Drop pending capability work where safe.
4. Mark that plugin faulted until manual reload or disable/enable.
5. Release the action coordinator.
6. Leave Gradial input, overlay, settings, and tray operation healthy.

An uncooperative plugin must never require terminating Gradial.

## 8. Threading and ownership

The v0.2 input callback remains unchanged and MUST NOT know that a selected
action belongs to a plugin.

| Owner | New responsibility | Prohibited work |
| --- | --- | --- |
| Input thread | Existing fixed-size pointer events only | Lua, manifests, capabilities |
| Interaction thread | Scalar calculation, lock state, latest sample publication | Lua execution, blocking I/O |
| UI/render thread | Draw immutable scalar feedback | Lua callbacks, plugin file reads |
| Action coordinator | Grant one continuous-session lease | Running Lua or network requests |
| Plugin worker | Own Lua states and serialized callbacks | Hooks, rendering, configuration writes |
| Capability broker | Validate and perform approved host effects | Input-hook decisions, plugin code |
| Config/diagnostics | Scan manifests, atomic local state, bounded logs | Gesture decisions |

- All channels are bounded.
- Lua states are created, used, and destroyed only by the plugin worker.
- One plugin callback runs at a time in v0.3.
- Capability operations carry the continuous session ID and are rejected after
  its request epoch closes.
- Broker queues reserve capacity for terminal work so ordinary update traffic
  cannot prevent a final commit or stop request.
- Reload waits until no session for that plugin is active; requesting reload
  during a session cancels it first.
- Shutdown sends best-effort cancellation, waits at most the existing two-second
  executor shutdown boundary, and does not wait forever for Lua.

## 9. Settings and authoring experience

### 9.1 Plugins page

The settings window gains a **Plugins** page that shows:

- Plugin name, ID, version, API version, and local path.
- Disabled, enabled, incompatible, unavailable, or faulted status.
- Requested permissions in plain language.
- Last load error using a safe stable message.
- **Enable**, **Disable**, **Reload**, **Rescan**, and **Open plugins folder**.

Rescan reads manifests but executes no plugin code. Enablement performs:

1. Complete path and manifest validation.
2. Compatibility validation.
3. Permission review.
4. Lua environment construction.
5. Entry-module evaluation under the normal limits.
6. Callback validation.
7. Atomic update of local plugin state.

A failed step leaves the plugin disabled and does not change menu
configuration.

### 9.2 Action editor

The action catalog gains **Plugin continuous action**.

- The user chooses an enabled compatible plugin action.
- Gradial renders generic controls from the manifest parameter schema.
- Plugin code does not run while editing parameters.
- The editor shows requested capabilities before assignment.
- Save uses complete schema-six validation.
- A missing plugin produces a visible unavailable warning rather than deleting
  the action or slice assignment.
- Continuous actions are not offered in the sequence editor.

The production radial preview visualizes direction, arming, and normalized
distance but never calls the plugin or performs its effects.

### 9.3 Local development loop

The authoring guide provides:

1. Copy the minimal example directory.
2. Choose a unique plugin and action ID.
3. Edit `plugin.toml` and `main.lua`.
4. Rescan and enable once.
5. Edit Lua and choose **Reload** for later iterations.
6. Use a built-in dry-run meter to inspect normalized samples without
   application effects.

Gradial SHOULD report Lua errors with file, line, callback, and a bounded
message in Settings. Release logs retain only plugin ID, action ID, error code,
and timing; source lines, parameter values, context fields, and host payloads
remain excluded.

## 10. Failure, diagnostics, and privacy

Add stable failure codes:

- `plugin_unavailable`
- `plugin_incompatible`
- `plugin_manifest_invalid`
- `plugin_load_failed`
- `plugin_runtime_error`
- `plugin_timeout`
- `plugin_memory_limit`
- `plugin_permission_denied`
- `plugin_capability_failed`
- `plugin_rate_limited`
- `continuous_target_lost`
- `continuous_protocol_error`

The tray activity panel states which local plugin failed by its manifest name
and gives the next useful step, normally **Open settings** or **Reload plugin**.
It does not include Lua source, stack contents, application paths, window
titles, HTTP data, shortcut values, or plugin parameters.

Diagnostic bundle preview may include:

- Plugin IDs and versions.
- Enabled/disabled/faulted status.
- Declared and approved permission names.
- Stable load/runtime error codes.
- Callback counts and bounded timing aggregates.

It never includes plugin source, plugin parameters, Lua globals, target context,
network payloads, or credentials. Configured application paths retain the
existing explicit opt-in rule.

## 11. Measurable release requirements

All applicable v0.2 release gates remain active. The plugin runtime does not
receive a separate latency exemption.

### 11.1 Continuous delivery gates

| Metric | Release gate |
| --- | ---: |
| Arm threshold to `Begin` callback start | p95 < 20 ms; p99 < 35 ms |
| Latest sample publication to `Update` callback start | p95 ≤ configured update period + 5 ms |
| Activation release to terminal callback start | p95 < 20 ms; p99 < 35 ms |
| Pending ordinary updates per session | ≤ 1 |
| Terminal callbacks lost to ordinary coalescing | 0 |
| Updates delivered after terminal event | 0 |
| Scalar disagreement between renderer and plugin sample | 0 beyond floating-point tolerance |

Measurements use at least 1,000 sessions per tested update rate and record
sample count, coalesced count, callback duration, terminal reason, DPI, display
geometry, and plugin identity.

### 11.2 Isolation and resource gates

- With no plugin enabled, plugin support adds less than 5 MiB to the 30-minute
  idle working set and no measurable periodic CPU wake-up beyond the existing
  measurement noise.
- One enabled idle example plugin adds no more than 8 MiB working set and
  performs no callbacks while Gradial is idle.
- A 10,000-session continuous soak grows working set by no more than 10 MiB
  after the first 1,000 sessions.
- A timed-out, memory-exhausted, or erroring plugin produces zero hook-heartbeat
  misses, focus changes, stuck keys, input-queue overflows, or renderer stalls.
- A plugin generating requests above every capability rate limit remains
  bounded and is cancelled or rejected deterministically.
- Invalid manifests and path traversal fixtures never cause code outside the
  canonical plugin root to be read or executed.
- Disabled plugins execute zero Lua code.

### 11.3 Usability gates

Before freezing the distance defaults, test at least eight participants using
four- and eight-slice menus at screen center, edges, and corners.

Participants select displayed target values at 25%, 50%, 75%, and 100%.
Accept the defaults when:

- Existing eight-direction error remains within the accepted v0.2 baseline.
- Median absolute scalar error after practice is at most 0.06.
- At least 95% of final samples are within 0.15 of the requested target.
- Accidental arming while changing direction is below 2%.
- At least seven of eight participants can cancel an armed action after being
  told once that cancellation stops but does not undo prior effects.
- Edge/corner median completion time is no more than 20% slower than
  center-screen completion.

If these gates fail, adjust arm radius, full-scale radius, progress rendering,
or onboarding and repeat the prototype. Do not compensate by exposing
application-specific values in the core.

## 12. Test matrix

### 12.1 Pure and automated tests

- Distance normalization at arm radius, full-scale radius, between them, and
  beyond both bounds.
- NaN, infinity, very large deltas, negative coordinates, and every supported
  DPI.
- Four/eight slice centers, boundaries, hysteresis, direction locking, and
  return-to-center cancellation.
- Monotonic scalar behavior for monotonic outward movement.
- Deterministic update coalescing and sequence gaps.
- Begin rejection, callback error, timeout, cancellation, and exact terminal
  ordering.
- Release racing with update publication.
- Target loss racing with commit.
- Plugin reload, disable, configuration save, display change, session lock, and
  shutdown during every lifecycle phase.
- Manifest IDs, duplicate actions, API mismatch, permission changes, parameter
  limits, and unknown fields.
- Schema-five migration, schema-six round trip, unavailable references, import,
  export, primary recovery, and backup recovery.
- Safe-library probes for every excluded Lua facility.
- Instruction, memory, string, source-size, callback-time, and rate limits.
- Canonical-path, traversal, reparse-point, and module-root fixtures.
- Permission checks at load and every host-call boundary.
- Existing shortcut cleanup under plugin error and cancellation.
- Loopback IPv4/IPv6 acceptance plus DNS, redirect, proxy, non-loopback, body
  size, rate, and timeout rejection.
- Diagnostic and release-log privacy scans using sentinel secrets.

### 12.2 Windows integration

- 60 Hz and 120 Hz-or-faster displays.
- 100%, 125%, 150%, 175%, and 200% scaling.
- Mixed-scale pairs and negative virtual-screen coordinates.
- Every edge and corner under edge assist.
- Standard, stale, closed, replaced, background, and elevated targets.
- Sleep/resume, lock/unlock, Explorer restart, display attach/detach, and DPI
  change during an active session.
- Plugins disabled, enabled, faulted, missing, edited, and permission-changed.
- A loopback test server that records bounded requests without requiring an
  external application.
- A target helper that records exact synthesized shortcuts and verifies cleanup.

### 12.3 Manual authoring

Starting from the documented example, a user familiar with basic scripting
should be able to:

1. Rename the plugin.
2. Add one parameter.
3. Convert the scalar into either a smooth value or four bands.
4. Reload it.
5. Assign it to a menu.
6. Observe it in the dry-run meter.

The median completion target is under 20 minutes without compiling Gradial or
installing a Lua toolchain. Record points of confusion and revise the manifest,
error messages, and author guide before release.

## 13. Implementation milestones

### Milestone 0 — Continuous geometry prototype

Deliver:

- Pure distance normalization.
- Continuous arming and direction-lock state.
- Production-renderer progress feedback.
- Edge-assist extension to the candidate full-scale radius.
- A built-in no-side-effect sample sink.
- Repeatable direction/distance interaction matrix.
- The usability study in Section 11.3.

No Lua or plugin configuration is introduced in this milestone.

Exit only when arm radius, full-scale radius, visual feedback, cancellation,
and edge behavior have recorded results.

### Milestone 1 — Runtime-neutral continuous core

Deliver:

- Versioned Begin/Update/Commit/Cancel types.
- Latest-value coalescing and exact terminal ordering.
- Continuous-session lease in the action coordinator.
- Frozen target/context/parameter snapshot.
- Overlay and interaction integration.
- Schema-six action model and schema-five migration.
- Unavailable-plugin reference behavior.
- Fake backend and deterministic lifecycle/race tests.

Exit only when a fake backend can complete, reject, stall, fault, and cancel
10,000 sessions without input, focus, queue, renderer, or memory regressions.

### Milestone 2 — Local Lua runtime

Deliver:

- Manifest parser and canonical plugin discovery.
- Lua 5.4 embedding through `mlua`.
- Reviewed library environment and plugin-rooted source loader.
- One Lua state per enabled plugin.
- Memory, instruction, callback, source, and return-value limits.
- Permission approval and atomic local plugin state.
- Plugins settings page, rescan, enable/disable, and reload.
- Lua adapter for the runtime-neutral lifecycle.
- No-side-effect continuous-meter example.

Exit only when all sandbox probes, malformed-package fixtures, timeout cases,
and manual authoring steps pass without executing code from a disabled plugin.

### Milestone 3 — Capabilities and action editing

Deliver:

- Permission-filtered context.
- `input.shortcut`, `notification`, and `network.loopback` brokers.
- Per-call target and permission validation.
- Capability rate, payload, origin, and timeout limits.
- Generic manifest parameter controls.
- Plugin continuous action creation and assignment.
- Shortcut-based smooth/banded example.
- Loopback integration harness.
- Import/export dependency summary.

Exit only when every capability has positive, denied, revoked, stale-target,
rate-limit, timeout, cancellation, and privacy evidence.

### Milestone 4 — Hardening and documentation

Deliver:

- Performance report for disabled, idle-enabled, active, slow, and faulted
  plugin cases.
- 10,000-session continuous soak.
- Display/DPI, recovery, and shutdown matrices.
- Diagnostic-bundle and log privacy evidence.
- Plugin author guide.
- Plugin manifest reference.
- Continuous API v1 reference.
- Trust, permissions, and capability guide.
- Known limitations matching actual behavior.

Release is blocked by a failed v0.2 MUST requirement, callback on an
input/render path, unbounded queue, post-terminal update, stuck synthesized key,
focus theft, configuration corruption, permission bypass, code execution from a
disabled plugin, or unmeasured continuous latency gate.

## 14. Future WASM admission

WASM becomes the preferred next runtime when Gradial intends to support
frictionless installation of community plugins, a marketplace, or plugins from
authors the user does not personally trust.

Before admission:

- The runtime-neutral protocol in Section 4 must be stable in production.
- Every host capability must have enforceable permission and resource limits.
- The WASM implementation must use explicit imports rather than ambient OS
  access.
- Memory, fuel, epoch/deadline, and blocking host-call limits must be tested.
- A typed interface definition, such as
  [WIT](https://component-model.bytecodealliance.org/design/wit.html), must
  version the protocol.
- Package provenance, signing, update ownership, and permission-change behavior
  must be designed for distribution.
- The UI must distinguish local Lua trust from isolated WASM distribution
  without implying that either format makes a broadly granted capability safe.

Lua and WASM may eventually coexist:

- Lua for approachable local scripting.
- WASM for stronger-isolation, distributable, multi-language plugins.

Both must share action identity, configuration references, parameters,
permissions, lifecycle semantics, diagnostics, and host capabilities. Native
DLL loading remains out of scope.

The future implementation should follow the
[Wasmtime security model](https://docs.wasmtime.dev/security.html) or an
equivalent runtime with explicit capability imports and enforceable resource
limits.

## 15. Architectural rule

The v0.1 principle remains:

> The core knows about the operating system. Plugins know about applications.

For v0.3, that principle is made concrete:

- Gradial owns input capture, geometry, normalized values, target integrity,
  rendering, capability enforcement, timing, cancellation, and recovery.
- Plugins own the mapping from a normalized scalar to application meaning.
- Gradial never adds VLC seek, Krita brush, media timeline, or similar
  application concepts to the core.
- A plugin cannot enter the input hook or renderer merely because an
  integration would be easier that way.

This boundary preserves creative freedom at the plugin layer without turning
plugin convenience into instability in Gradial's critical interaction path.
