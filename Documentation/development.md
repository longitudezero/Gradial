# Gradial development guide

## Current milestone scope

The foundation, Milestone 0 interaction spike, completed bounded Milestone 1
and Milestone 2 slices, and the first three bounded Milestone 3 slices establish:

- A Rust 2024 workspace pinned to the installed Rust 1.97.1 toolchain.
- A reusable `gradial-core` crate.
- A Windows `gradial` executable rendered by egui through wgpu.
- One prewarmed transparent, borderless, topmost, click-through overlay per
  active display.
- Per-Monitor V2 DPI awareness through the embedded application manifest.
- `%LOCALAPPDATA%\Gradial` directories for configuration, icons, and bounded
  local logs.
- Formatting, Clippy, unit-test, and build checks.
- A dedicated Win32 low-level mouse/keyboard hook thread.
- A saved M4/M5 activation chord with optional exact Ctrl/Alt/Shift
  requirements, plus Escape as cancellation.
- A fixed-capacity, single-producer/single-consumer input queue.
- An explicit, pure gesture state machine with four/eight slices and
  hysteresis.
- Hidden-at-idle overlay lifecycle, physical-pixel positioning, and selection
  highlighting.
- Automatic fixed-origin/edge-assist resolution from signed work-area geometry.
- A raw-input message window and virtual gesture pointer for edge assist.
- Focus checkpoints at activation, after the first rendered-frame checkpoint,
  and after close.
- An ETW provider whose invocation markers can be correlated with PresentMon.
- Display/DPI/work-area change detection that suspends new gestures, cancels an
  active gesture, replaces the overlay pool, and restarts input only after every
  new viewport is warm.
- An always-visible native tray icon with Healthy, Recovering, and Faulted input
  states plus Retry Input and Exit commands.
- Automatic full input-runtime replacement after three missed 250 ms
  heartbeats, and input re-registration after resume, session unlock, or
  Explorer restart.
- Persistent per-monitor geometry caches, immediate selection feedback, and
  bounded 60/50 ms open/close animation state.
- A validated schema-six configuration with safe in-memory schema-one through
  schema-five migration plus atomic primary/backup
  replacement.
- Complete immutable target/display/modifier identity for every invocation.
- A compiled, deterministic application-profile resolver with explanation
  data.
- One immutable resolved menu per invocation, shared by gesture recognition
  and rendering.
- Prewarmed four/eight geometry caches on every display and rendering of
  configured profile labels.
- Validated built-in action definitions and optional menu-slice action UUIDs.
- Cycle-, depth-, step-, and delay-bounded sequence validation.
- Immutable release-time execution-plan compilation tied to the invocation's
  frozen configuration revision.
- One bounded action worker with immediate busy rejection, cancellable Delay
  execution, timeouts, and a two-second shutdown wait.
- Frozen-target freshness/integrity validation and scan-code keyboard
  shortcuts with synthesized-key cleanup.
- Literal UTF-16 Unicode text entry with per-event target revalidation and
  in-flight code-unit cleanup.
- HTTP/HTTPS URL and existing absolute file/folder opening through Windows
  shell associations, with literal payloads and redacted diagnostics.
- Direct launch of existing absolute `.exe` files through `CreateProcessW`,
  with exact argument-array preservation, validated working directories,
  inherited environment, immediate handle cleanup, and redacted diagnostics.
- Close, minimize, maximize, and restore for the frozen invocation window,
  with full target revalidation and one asynchronous message dispatch.
- A session-only, 20-item tray activity panel for action failures and
  cancellations, with clear-on-command behavior.
- Native Windows failure notifications that do not activate Gradial; successful
  actions and cancellations do not notify.
- A single-instance native settings window for global-menu four/eight geometry,
  labels, existing-action assignments, and a production-renderer preview.
- An action catalog that creates and edits keyboard shortcuts and Unicode text,
  records physical shortcut data, and supports rename, duplication, and
  reference-safe deletion.
- Leaf-action editors for websites, files/folders, direct applications, Delay,
  and invocation-window commands, with native Windows path choosers and
  literal application-argument rows.
- An ordered reusable-sequence editor with nested-sequence support,
  validator-backed candidate filtering, and live depth, expanded-step, and
  Delay metrics.
- An application-profile catalog with exact executable selection, explicit
  priority, complete-menu duplication, lifecycle controls, and independent
  four/eight-slice editing through the production preview.
- Optional exact window-class and title-regex rule controls plus a private
  countdown tester that uses the production resolver to explain matches,
  winner, and tie-break.
- A draft-only activation editor for M4/M5 and generic Ctrl/Alt/Shift, with a
  one-shot hold/flick/release tester that cannot dispatch an action.
- Draft-safe close confirmation, complete pre-save validation, atomic
  replacement, and immediate live configuration reload.
- Native JSON import into the private settings draft and atomic export of the
  current valid draft, including unsaved edits and an optional adjacent
  portable-icon directory.
- Required, versioned first-run onboarding that chooses the exact activation
  chord, requires the dedicated-button acknowledgment, and practices selection
  and cancellation through an action-free production interaction.
- Draft-safe accent, label-theme, and size-preset appearance editing through
  the production renderer, with immediate post-save application.
- Per-slice managed PNG/SVG icons with bounded import validation, load-time
  texture caching, production-preview rendering, and non-fatal placeholders.
- Reference-aware managed-icon restoration and recoverable quarantine across
  active, backup, and open-draft settings.
- A frozen diagnostic-bundle preview with sanitized bounded logs,
  app/input/display metadata, explicit configured-process-path inclusion, and
  atomic local ZIP export.
- A default-off, per-user Windows start-at-login control with exact
  registration inspection, stale-entry repair, and no administrator
  requirement.
- A versioned performance-report harness with QPC hook/dispatch timing,
  PresentMon displayed-frame correlation, phased resource sampling, and
  explicit evidence-status reporting.
- The first v0.3 continuous-input prototype: hybrid discrete/continuous wedges,
  an outer-edge boundary (107 DIP for Standard), a recommended 300-DIP endpoint
  with a 160-400 DIP development range, live direction and radius modes, a
  production-overlay scalar track, an edge-assist pointer proxy, and a
  debug-only meter that cannot compile or dispatch configured actions.
- A dedicated continuous interaction matrix that checks four/eight directions,
  normalized target values, focus, edge assist, inert execution, and
  machine-readable coverage without fabricating the pending human study.
- A guided participant-study window with a fixed 37-trial protocol, automatic
  center/edge/corner validation, anonymous atomic JSON reports, and a cohort
  summarizer that applies the Section 11.3 gates only with at least eight
  complete reports.
- A runtime-neutral continuous protocol core with version-one
  Begin/Update/Commit/Cancel requests, owned permission-filtered snapshots,
  10–60 Hz latest-value coalescing, sequence-gap visibility, and deterministic
  pending-Begin terminal ordering. It has no Lua or operating-system effects.
- A staged asynchronous continuous coordinator that shares the existing
  single-action lease, preserves it across direction handoffs, uses separate
  bounded control and latest-value paths, applies the planned callback limits,
  and has a scripted fake backend for lifecycle and shutdown verification.
- A live bridge from the debug inert meter to that coordinator, including
  exact visual-scalar publication, unlocked direction handoff, inactive lease
  retention after inward reversal, direct discrete-worker lease transfer, and
  captured-target validation.
- Canonical local Lua package discovery, atomic machine-local trust state,
  explicit approval/enable/reload/disable controls, one bounded plugin worker,
  and one restricted Lua state per admitted plugin.
- A production continuous-plugin route that freezes configured parameters at
  invocation-down, admits only enabled manifest actions, uses the manifest
  update cadence, revalidates the captured target, and dispatches bounded Lua
  Begin/Update/Commit/Cancel callbacks off the interaction thread.
- Manifest-backed continuous action authoring with typed, bounded parameter
  controls and a separate per-wedge continuous checkbox for Global and every
  application profile.

Configuration editing currently covers activation, the global menu, the
complete discrete and continuous action catalog including sequences, and
exact-executable application profiles with their independent menus and
complete matching rules. Required
onboarding, v1 appearance, and per-slice custom icons are complete. A release logs the
chosen slice, compiles its assigned plan, and submits it to the coordinator.
Every schema-two action family can execute through its bounded backend.

## Workspace layout

```text
Gradial APP\
    Cargo.toml
    crates\
        gradial-core\    Pure geometry, gesture state, and application paths
        gradial-lua\     Frozen source images and restricted Lua 5.4 hosting
        gradial-app\     Windows input, persistence, executable, and renderer
    scripts\
        check.ps1          Complete local code verification
        check-focus.ps1    Validate the latest completed focus trace
        capture-timing.ps1 Capture ETW markers and presented-frame timings
        run-performance-report.ps1 Run development or release performance protocol
        run-continuous-matrix.ps1 Run the continuous direction/distance matrix
        summarize-continuous-study.ps1 Evaluate anonymous participant reports
        smoke-interaction.ps1 Exercise a real hidden app with synthetic M4 input
        smoke-delay-executor.ps1 Run an isolated Delay action fixture
        smoke-input-executor.ps1 Shared isolated input-action fixture
        smoke-shortcut-executor.ps1 Verify scan-code shortcut delivery
        smoke-text-executor.ps1 Verify exact private Unicode delivery
        input-smoke-target.ps1 Capture key and Unicode messages safely
        smoke-open-executor.ps1 Verify an isolated Windows file association
        open-smoke-receiver.ps1 Capture the exact associated file path
        smoke-process-executor.ps1 Verify direct process launch in isolation
        smoke-window-executor.ps1 Verify all invocation-window commands
        window-smoke-target.ps1 Observe isolated window messages and states
        smoke-action-feedback.ps1 Verify failure activity, notification, focus, and privacy
        smoke-settings-editor.ps1 Verify native draft, diagnostic ZIP, save, and live reload
        smoke-start-at-login.ps1 Verify isolated Windows startup registration and repair
        smoke-onboarding.ps1 Verify required first run and safe production-menu practice
        run-interaction-matrix.ps1 Run the versioned validation suites
```

`gradial-core` must remain free of UI and Windows windowing dependencies. Pure
gesture geometry, configuration models, profile resolution, and action-plan
validation belong there as they are introduced.

`gradial-lua` owns no filesystem or Windows APIs. It accepts an immutable
source image from discovery and provides the explicit, bounded Lua load path.
Only the dedicated application plugin worker calls that path after explicit
local approval and enablement.

`gradial-app` owns Windows integration and rendering. Its renderer is behind the
small `OverlayRenderer` interface so the production interaction code does not
depend on egui painting details.

## Local files

On first run, Gradial creates:

```text
%LOCALAPPDATA%\Gradial\
    config.json
    onboarding-v1.json
    plugins-state.json
    icons\
        quarantine\
    logs\
    plugins\
```

`config.json` starts at schema 6 and revision 1 with the default unmodified M4
binding, original blue/light/standard appearance, stable global profile,
eight-slice menu, empty action catalog, and a 300-DIP continuous full-scale
distance. Each menu slice has separate optional discrete and continuous action
UUIDs plus an optional managed icon reference.
A successful later replacement
preserves the previous valid primary as `config.json.bak`; `config.json.tmp`
exists only during a save.

`onboarding-v1.json` is separate from user configuration. Only a valid
schema-one marker with `completed: true` suppresses onboarding at startup;
missing, invalid, unsupported, or incomplete marker content safely requires
the flow again. `onboarding-v1.json.tmp` is used only while atomically writing
successful completion.

`plugins-state.json` is a separate machine-local schema-one trust document.
Its `.tmp` and `.bak` companions provide write-through atomic replacement and
recovery without mixing enablement or permission approval into portable menu
configuration.

Existing schema-one files are accepted only when they contain no schema-two
action fields. Gradial adds the action, binding, and appearance defaults in
memory. Schema-two files gain binding and appearance; schema-three files gain
appearance. Schema-one through schema-four all gain no icon assignments.
Schema-five retains its icon assignments while gaining
`continuous_action_id: None` on every slice and the 300-DIP full-scale value.
Every path validates the complete schema-six model and leaves the original
bytes untouched.
Migration is persisted only through a later explicit, successful save. This
prevents startup from silently rewriting either a valid legacy file or a file
containing ambiguous fields.

Settings import uses the same migration and complete model/profile/action-plan
validation path, then resets the imported revision to the current saved
revision and replaces only the editor's proposed draft. The source bytes,
running configuration, and persistent primary remain untouched. A dirty editor
requires an explicit **Import file** choice before its proposed draft is
replaced. For each unique icon reference, import looks for
`icons/<asset-id>.<format>` beside the selected JSON. A missing directory or
file is non-fatal and produces a placeholder warning. Any present asset must
pass the same size, dimension, PNG/SVG, and SVG-content checks as a directly
chosen icon. Valid sources are atomically copied into managed storage;
byte-identical existing IDs are reused and differing same-ID bytes reject the
whole draft import. Files created earlier in a failed import are removed.

Settings export validates the current proposed draft, serializes schema-six
JSON beside a uniquely named temporary file in the selected directory, flushes
it, and atomically moves it over the selected destination. It may include
unsaved edits but never changes the active revision. Gradial's three active
configuration paths and both onboarding-marker paths are protected export
destinations. Import and export preserve literal action
paths, arguments, labels, and text; diagnostics expose only fixed result
categories rather than selected paths or configuration values. Before the JSON
replacement, export validates every unique referenced managed icon and copies
it into the adjacent `icons` directory through a flushed temporary file.
Existing byte-identical assets are reused; conflicting same-ID files reject the
export without touching the selected JSON. Files and a directory created by a
failed attempt are removed, while unrelated existing files are never deleted.

Managed-icon reconciliation builds one protected set from the active
configuration, a valid backup, and the complete open draft when present. A
missing backup is an empty contribution; an unreadable or invalid backup
disables quarantine entirely. Before renderer preload, valid protected assets
found under `icons\quarantine` are restored to the managed root. Only regular
files whose names exactly match canonical lowercase
`<non-nil-UUID>.png|svg` are candidates for quarantine. Directories,
reparse/symlink entries, temporary files, noncanonical names, and all other
files remain untouched. Existing quarantine destinations are never
overwritten, and no quarantined file is permanently deleted.

Configuration is parsed and completely validated before input starts. Profile
regular expressions are compiled into the retained resolver at load rather
than during invocation. An invalid primary loads the backup without rewriting
either file. If neither file is usable, Gradial runs the built-in default in
memory, retains the broken files, and writes a `config_recovery_warning`. The
settings editor surfaces that warning until a successful explicit save.

Logs are local and rotate at 1 MiB. At most five log files are retained.
Gradial uses its own filter so a host-level `RUST_LOG` setting cannot silence
lifecycle and safety events. Gradial-authored records use component and event
codes; dependency warnings may also appear. Action records contain UUIDs,
step counts, and bounded timing metadata only. They do not contain
window titles, typed text, executable arguments, clipboard contents, URLs, or
selected files.

The Settings **Diagnostics** page flushes the asynchronous logger and freezes
only exact regular files named `gradial_rCURRENT.log` or
`gradial_rNNNNN.log`. Unknown files, directories, links/reparse entries, and
more than five or unexpectedly oversized owned logs fail or remain outside the
snapshot rather than broadening ownership. The frozen snapshot is what the
preview and later export use; new log records, display changes, and
configuration changes do not silently alter it. **Refresh preview** creates a
new snapshot.

Every ZIP contains `manifest.json` plus the previewed sanitized log copies.
The manifest records only the bundle format, creation time, Gradial version,
schema/revision, Windows version/build and architecture, hook-thread heartbeat status, display
bounds/work areas/DPI, log entry names/sizes, and the privacy decisions. The
page deliberately says **Hook thread responsive** rather than claiming that
Windows has retained the hook. Sanitization removes the current user/local
app-data roots and every configured application path from copied logs.

**Include configured application paths** starts off for every preview. When
selected, the page displays the exact case-insensitively deduplicated
application-profile and Run-application executable paths and adds only
`configured-process-paths.json`. It never adds arguments or working folders.
Configuration JSON, typed text, URLs, file/folder action paths, labels, window
classes/titles, clipboard data, icons, and a unique installation identifier
are always absent. ZIP creation is local, uses no network behavior, writes a
flushed adjacent temporary file, and atomically replaces the chosen `.zip`.
Every path inside `%LOCALAPPDATA%\Gradial` is a protected destination.

## Start at login

Start at login is Windows account state, not portable Gradial configuration.
The Settings **General** page inspects only:

```text
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
    Gradial
```

An absent value is **Off**. **On** requires a `REG_SZ` value that exactly
matches the current absolute executable path enclosed in quotes, with no
arguments. Any other type, path, quoting, or appended text is **Needs repair**;
Gradial does not silently accept or execute it.

Gradial never creates the value automatically. Enable and repair set only the
`Gradial` value, then immediately read it back before reporting success. Turn
off deletes only that value and verifies its absence. Gradial never deletes
the shared Run key, enumerates another application's values, or requests
elevation. Import, export, configuration save, onboarding, and app startup
never enable the option.

The current executable command and stored stale value are excluded from log
records and error messages. Logs contain only the requested operation, the
resulting state, a stable error code, and a native numeric error code when
Windows provides one.

## Required first-run onboarding

The normal decorated **Welcome to Gradial** window opens automatically unless
the versioned completion marker is valid. Its first page chooses M4 or M5 and
optional generic Ctrl, Alt, and Shift requirements. It states that both edges
of that exact chord are consumed while Gradial runs, including a quick
centered cancellation, and requires an explicit acknowledgment before
practice can begin.

Beginning practice saves a changed chord through the same complete validation,
atomic replacement, and live input-runtime restart used by Settings. An
unchanged chord does not create a needless configuration revision. A save or
runtime-reload failure leaves onboarding open with fixed recovery guidance.
Settings cannot open while onboarding is active, so the binding under
instruction cannot diverge from the live hook.

Practice still enters through the production low-level hook, invocation
snapshot, gesture controller, edge-assist geometry, cached renderer, and
per-display overlay pool. The resolver runs normally, then an immutable
eight-slice sample menu without action UUIDs replaces the resolved menu for
that invocation. The invocation is explicitly tagged as practice before
release, so action-plan compilation and executor submission return without
dispatch even if every underlying production slice has an action.

Onboarding records three bounded bridge events: menu opened, sample selected,
and centered/Escape cancellation. Completion requires a cancellation after a
successful selection. Other cancellation causes do not satisfy the teaching
step. Only after all steps complete does Gradial atomically write the marker
and close the window. Its close button cannot bypass an incomplete required
flow; **Exit Gradial** remains available from the tray.

The tray's **Restart onboarding** command reopens the chooser with the live
binding. Restart is an immediate replay and deliberately retains an existing
completion marker, so exiting partway through an optional replay does not
turn the next startup into a required flow.

## Input and interaction architecture

Gradial uses four execution contexts:

| Context | Responsibility | May block or allocate? |
| --- | --- | --- |
| Win32 input thread | Classify the saved M4/M5 chord, tester input, raw deltas, movement, Escape, and cancellation | No, inside callbacks |
| Interaction thread | Snapshot invocation state and advance the pure gesture controller | Yes, outside callbacks |
| eframe UI thread | Maintain the hidden controller and paint independent per-display viewports | Yes |
| Action worker | Execute one admitted immutable plan and observe cancellation/timeouts | Yes |

The input thread owns the low-level hooks, heartbeat timer, and a message-only
window registered for raw mouse input. Both event sources execute serially on
that one thread and feed one producer. The hook callback copies fixed-size
events into an `rtrb` SPSC ring buffer with a capacity of 4,096. It never logs,
waits, paints, takes a contended lock, enumerates displays, or calls action
code. Display work areas are immutable for the lifetime of one hook runtime.
When topology changes, an atomic gate first makes new activation attempts pass
through, then the interaction thread closes any active invocation, the old
hook runtime stops, and a replacement starts with the newly enumerated displays. Overflow
sets an atomic fault flag. The interaction thread then closes any active
invocation, closes the complete input runtime, and enters a fail-safe fault
state. The configured chord passes through while faulted; recovery requires
**Retry input** from the tray menu.

At activation-down, the interaction thread maps the signed physical cursor coordinate
to the same monitor ordering used by the overlay pool. It updates only that
viewport's small shared presentation state and requests that child viewport's
independent repaint. Open and close also post private native messages directly
to the selected window, so they do not wait for the hidden controller's
throttled repaint cycle. None of this work occurs in a low-level hook callback.

The hook thread owns a 250 ms message-loop heartbeat. The interaction thread
checks it whether or not a gesture is active. Once it is more than 750 ms old
(three missed intervals), new activation attempts are immediately passed through, any
active invocation is cancelled, and the entire hook/raw-input/queue/consumer
runtime is replaced. If replacement cannot start, Gradial remains faulted until
manual retry. On graceful shutdown, Gradial logs the maximum observed
hook-callback duration.

The tray health state is `Recovering input` during startup, input replacement,
and display rebuild; `Healthy` only after a new hook thread reports responsive;
and `Input faulted` after queue overflow or a failed start. Its menu contains
the non-interactive current state, **Open settings**, **Restart onboarding**,
**Retry input**, **Cancel current action**, recent activity, and
**Exit Gradial**.
Explorer's `TaskbarCreated` message restores the icon and re-registers input.
Power-resume and current-session unlock notifications also re-register the
complete runtime. These signals do not silently clear a queue-overflow fault.

The configured exact chord's button-down and matching release are suppressed.
A button or Ctrl/Alt/Shift state that does not exactly match passes through.
In fixed-origin mode, pointer movement passes through normally and absolute cursor
positions drive the gesture. In edge-assist mode, low-level movement is
suppressed so the system cursor remains at activation-down; raw mouse deltas accumulate
into a virtual gesture pointer instead. During an invocation, Escape and
another mouse-button click cancel the gesture; their matching releases are
suppressed to avoid leaking half of a click. Events stamped with Gradial's
private injection marker are passed through and never interpreted as
activation.

## Gesture contract

The pure controller uses physical cursor deltas but scales its thresholds by
the invocation monitor's effective DPI:

- center cancellation radius: 24 DIP;
- initial selection radius: 36 DIP;
- boundary hysteresis: 5 degrees;
- four or eight slices, with north at index 0 and indices increasing
  clockwise;
- release in the center/no-selection zone cancels;
- Escape closes once, then waits for the activation-button release before
  returning to idle.

Each invocation captures a UUID and source QPC timestamp plus immutable target,
display, input, and configuration identity. The target fields are foreground
window handle/PID, process creation time, optional full executable path, window
class, and window title. Display fields are signed cursor coordinate, monitor
device name, monitor rectangle, work area, effective DPI, resolved geometry,
and visual origin. Generic Ctrl/Alt/Shift state and the triggering M4/M5
identity are captured in the low-level button-down event. They must exactly
match the saved binding before the event is consumed. The configuration
revision is frozen before profile selection.

Only the fixed-size modifier bits are added by the hook callback. Process,
window, monitor, configuration, and profile work remains on the interaction
thread. Executable paths, classes, and titles are held for matching but never
written to routine logs.

### Continuous prototype

The debug-only `--continuous-meter` gives every resolved direction an inert
continuous extension for that invocation. It does not modify schema-six
configuration or saved action definitions. The pure controller:

- selects by the existing angle and 36 DIP entry threshold;
- keeps the selected wedge live at every radius with boundary hysteresis;
- uses the visible ring's outer edge as the mode boundary: 90.95 DIP for
  Compact, 107 DIP for Standard, and 123.05 DIP for Large;
- maps that boundary through the recommended 300-DIP endpoint linearly to an
  unsigned `0.0..=1.0` scalar;
- clamps greater distance to `1.0`;
- commits the final scalar when released outside the boundary;
- restores normal discrete selection when movement returns inside the
  boundary; and
- clears selection when movement returns to the center.

The render thread receives the live slice, scalar, continuous-active state, and a bounded
DIP pointer offset. It draws a radial track, percentage, and—during edge
assist—a pointer proxy through `FoundationOverlayRenderer`; it performs no
callback or configured action work. The interaction outcome is
`ContinuousCommitted` outside the ring or `Selected` inside it. The debug meter
logs either result and closes without entering action-plan compilation. This
makes both hybrid branches no-side-effect sample sinks, including when the
selected configured slice normally has an action.

The normalized value uses the invocation DPI. Edge assist accumulates raw
physical deltas into the same virtual pointer and divides by that DPI before
normalization, so equivalent DIP travel produces the same value on every
monitor. The pointer proxy is clamped to the configured full-scale boundary and
contains no raw screen coordinate.

`--continuous-full-scale-dip=160..400` overrides the inert meter's whole-DIP
endpoint and cannot be used without `--continuous-meter` or with either guided
study. Without that development override, the endpoint comes from the active
schema-six configuration; new and migrated configurations use 300 DIP. The
value is frozen for the invocation. The overlay and automatic-geometry extent
use the configured endpoint plus a 20-DIP margin. When the resulting track
cannot fit at one-to-one scale in the work area, only the visible track and
proxy are compressed; normalization continues to use the exact configured
distance.

The controller evaluates every queued fixed-origin point and edge-assist delta,
including intermediate outer-edge crossings and direction changes. Direction
and discrete/continuous-mode changes bypass visual throttling and are published
immediately. Same-wedge value-only updates retain only the newest sample and
are published at no more than 240 Hz. A selection clear supersedes a pending
visual update, and the newest pending update is flushed before release,
cancellation, or another terminal effect. Direction/mode transitions remain at
info level, while ordinary value samples are debug-only. Shutdown diagnostics
record dispatched, prioritized, and coalesced counts.

The schema-six slice model keeps discrete and continuous assignment separate.
**Enable continuous control** defaults off for every wedge. An
unchecked wedge remains discrete at every radius, displays no percentage, and
never opens a plugin session; release still dispatches its discrete action if
one is assigned. Checking it reveals a required continuous-plugin action
selector. The Menu and Profiles editors offer this checkbox whenever the draft
contains at least one continuous action. The normal click action remains
unchanged when continuous control is enabled or disabled.

### Schema-six continuous configuration foundation

`ActionKind::PluginContinuous` stores a plugin ID, plugin action ID, and a
sorted map of generic `PluginValue` parameters. It is a continuous-only leaf:
the validator rejects it from the existing discrete `action_id`, from every
sequence depth, and from discrete execution-plan compilation. Conversely,
`MenuSlice::continuous_action_id` accepts only this action kind. A slice may
therefore pair a normal discrete action with an independent continuous
extension without combining their behavior.

Plugin IDs, plugin action IDs, and parameter keys use at most 96 bytes of
lowercase ASCII letters, digits, `.`, `_`, and `-`. Generic values are tagged
boolean, signed 64-bit integer, finite number, UTF-8 string, or manifest
choice. The complete serialized parameter map is limited to 16 KiB both at
configuration validation and at the runtime-neutral snapshot boundary.
When the referenced compatible manifest is available, Settings checks its
declared keys, required values, exact types, choices, numeric/string ranges,
defaults, and the complete serialized limit before save. The worker repeats
the same validation before Lua Begin. Missing manifests remain portable and do
not prevent unrelated edits, imports, or exports.

A structurally valid action whose local plugin is missing or disabled remains
loadable and exportable. Its UUID is frozen with the resolved menu, but the
current runtime advertises no available plugin actions, so its wedge receives
no continuous capability mask. It stays discrete beyond the outer edge and
release continues to use the independent discrete assignment. Loading or
importing this reference never scans for, downloads, installs, enables, or
executes plugin code.

The schema-six root stores `continuous_full_scale_radius_dip` in the inclusive
160–400 range. Schema-five migration supplies 300 DIP and `None` for every
continuous wedge assignment while preserving icons and all earlier data.
Migration remains in memory until an explicit successful save. Older schema
numbers cannot contain schema-six fields, even as `null`, so version provenance
continues to fail closed.

### Runtime-neutral continuous protocol core

`gradial-core::continuous` defines API version 1 without depending on Lua or a
Windows backend. A `ContinuousActionSnapshot` owns the validated plugin/action
IDs, generic parameter values, permission-filtered context, and requested
update rate. The permitted context can contain only an executable path/window
class pair and an optional window title; it has no PID, HWND, process handle,
screen coordinate, or mutable invocation reference.

`ContinuousSession::begin` creates a fresh segment and its only `Begin`
request. Its initial sample is sequence zero. Every observed scalar allocates a
strictly increasing sequence number, but the session retains only the latest
undelivered ordinary sample. Consequently, a slow adapter sees sequence gaps
instead of an unbounded queue. Delivered `delta` is always relative to the
previously delivered value, and elapsed time begins when the adapter accepts
`Begin`. Backward observation timestamps fail closed.

The session accepts only finite inclusive `0.0..=1.0` values and 10–60 Hz
cadences. `Commit` always allocates and preserves an exact final sample even if
an ordinary update is pending. `Cancel` discards ordinary pending work. Either
terminal decision is exactly once, and no update can be taken afterward.
When release or cancellation wins a race with a pending `Begin`, the decision
waits: acceptance returns the terminal request immediately after `Begin`,
while rejection closes the session without a terminal plugin callback.

The protocol types do not connect themselves to the gesture thread,
configuration, or a plugin worker. The staged coordinator below owns the
session and proves its asynchronous handoff and failure behavior without
pretending that a real plugin runtime exists.

### Continuous coordinator and fake backend

`gradial-app::continuous_executor` now supplies that next boundary. Starting a
continuous invocation first acquires the same atomic coordinator claim used by
built-in discrete execution. Therefore an active discrete action rejects a
continuous start with `action_busy`, and an active continuous invocation
rejects discrete dispatch. One lease belongs to the complete held invocation;
the old segment's `direction_changed` cancel finishes before the replacement
`Begin`, without clearing the busy claim between them.

The continuous worker owns the runtime-neutral `ContinuousSession` and backend
callbacks. Ordinary scalar publication uses a single locked latest-value slot,
not the control queue. Replacements carry an observation count into the core,
so delivered sequence numbers still reveal skipped samples. Handoff, commit,
and cancel use a separate bounded eight-item control queue, and completion uses
one reserved slot. A terminal request cannot be displaced by high-rate value
publication.

Callback deadlines match the v0.3 plan: 100 ms for `Begin`, 16 ms for `Update`,
100 ms for `Commit`, and 50 ms for best-effort `Cancel`. The backend receives
the absolute deadline plus the coordinator cancellation/shutdown token. A
rejected `Begin` produces no terminal callback. A stalled or faulted active
callback stops the segment, drops pending ordinary work, and releases the
lease. Release or cancellation queued while `Begin` is pending follows the
protocol core's accepted-Begin-then-terminal ordering.

The action executor's existing shutdown path now also waits within its
two-second boundary for a continuous lease to observe shutdown, send a
best-effort cancel, and release. Manual **Cancel current action** reaches either
discrete or continuous work through the same coordinator flag.

The fake backend runs on the real asynchronous worker and scripts acceptance,
rejection, gates, faults, and deadline stalls while recording every request.
It is test infrastructure, not a user plugin or production effect backend.

### Live continuous gesture bridge

The debug inert meter and its guided studies now exercise the coordinator on
the real interaction thread. This is deliberately narrower than configured
continuous actions: every wedge uses a host-created inert snapshot, and the
backend validates lifecycle data but performs no operating-system or plugin
effect. Normal profile invocations still provide an empty continuous-slice
mask and never start this worker.

Each coalesced `ContinuousChanged` effect supplies both the overlay and worker
from the same `ContinuousVisualState.selection.value`; the backend therefore
receives the exact normalized scalar requested for display. The overlay can
refresh at its existing 240 Hz cap while the session publishes at no more than
60 Hz through the one-value slot. No renderer sample is reconstructed from a
different cursor path.

The live bridge follows the unlocked hybrid interaction exactly:

- first crossing beyond the visible wedge starts `Begin`;
- value changes in the same wedge replace the latest ordinary sample;
- changing wedges sends `Cancel(direction_changed)` before a fresh `Begin`;
- moving inward sends `Cancel(returned_to_discrete)` but retains the invocation
  lease in an inactive state;
- moving outward again, in any eligible wedge, starts a fresh segment without
  reacquiring the lease;
- releasing outside commits the exact final displayed scalar;
- releasing inside remains an inert discrete selection in the debug meter.

The inactive lease prevents another action from entering between inward
reversal and release. A tested transfer path can consume that lease directly
when a future configured hybrid wedge releases its ordinary discrete action;
it enqueues the compiled plan without a second busy claim, so the invocation
cannot reject itself with `action_busy`. The debug meter never compiles or
uses that plan and remains side-effect free.

The validating inert backend applies the full existing target identity and
integrity check before `Begin`, then the lightweight HWND/PID/foreground
freshness check for `Update` and `Commit`. A stale active target stops with
`Cancel(target_stale)`. `Cancel` remains deliverable even after the target is
gone. Queue, cancellation, fault, and shutdown behavior continues to use the
same bounded worker rules described above.

Closing or clearing the interaction-owned invocation never joins the plugin
worker. The owner submits its terminal control and drops the join handle; the
worker retains the lease until its callback completes or reaches its deadline.
If cleanup finds the bounded control queue full, it also raises the shared
cancellation flag before detaching. A failed or timed-out handoff cancel ends
the invocation and releases the lease without starting the replacement
segment.

### Continuous usability study

Debug builds expose `--continuous-study`. The option implies the inert
continuous meter, bypasses first-run onboarding for that process, requires
automatic geometry, and rejects `--slices` and `--open-settings`. Settings and
onboarding also stay blocked while the study window is open. Saved profiles
and actions remain untouched.

One anonymous session contains:

- four center-screen practice trials at 25%, 50%, 75%, and 100%;
- 24 measured scalar trials covering those four values for both four- and
  eight-slice menus at center, edge, and corner placements, interleaved by
  target to limit simple ordering bias;
- four inside-ring direction-change trials for accidental continuous-entry
  measurement;
- four outside-ring direction-change trials that must finish in the requested
  live wedge; and
- one center-clear cancellation trial after the instruction was shown
  once on the introduction page.

All noncomparison scalar trials use the provisional 300-DIP recommended
endpoint. The dedicated four-candidate comparison remains unchanged.

The next trial appears only after an accepted terminal outcome. A trial begun
in the wrong placement category or a scalar trial released before commit is
retained as a rejected attempt and retried. Placement classification uses a
32-DIP boundary around the work area, but reports contain only the category.

The report schema is version 3 and records a random session ID, requested
and observed categories, direction correctness, normalized values, scalar
error, cancellation reason, invocation duration, and monitor DPI. It does not
record names, raw coordinates, foreground application identity, paths, window
details, or monitor device names. Reports are flushed and atomically published
as `continuous-study-<session-id>.json` under
`%LOCALAPPDATA%\Gradial\continuous-studies`.

After at least eight complete participant runs, execute:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/summarize-continuous-study.ps1
```

The summarizer rejects incomplete, duplicate, unsupported, or privacy-incompatible
reports. It recomputes the cohort metrics from accepted attempts and writes a
JSON summary plus gate CSV. With fewer than eight participants the sample-size
gate fails and the usability gates remain `not_evaluated`.

### Continuous distance comparison

Debug builds also expose `--continuous-distance-study`. It has the same
no-side-effect and option-isolation rules as the main study, but is a compact
personal comparison rather than a cohort usability report. Its four
center-screen trials all request E at 25% and change only the frozen 100%
endpoint, in the order 160, 200, 250, and 300 DIP. The just-completed
measurement and absolute error remain visible between trials. After all four,
the participant selects the endpoint that felt best.

The active endpoint is frozen into the interaction controller and renderer at
invocation-down, so a trial cannot change scale mid-gesture. The comparison
uses a 640-DIP overlay and a 320-DIP geometry extent to keep the longest track
visible. It atomically writes schema-one
`continuous-distance-study-<session-id>.json` beside the ordinary study
reports only after a preference is chosen. The report uses the same privacy
exclusions, but it is deliberately ignored by
`summarize-continuous-study.ps1` and does not modify production configuration.

## Read-only plugin discovery

Startup creates and scans `%LOCALAPPDATA%\Gradial\plugins` after local logging
is available and before application services start. Discovery does not load
Lua, approve permissions, enable a package, mutate plugin files, or connect a
manifest action to the continuous coordinator.

Each package must be one directory immediately below the plugin root. Its
directory name must exactly equal the validated manifest ID. The manifest is
bounded to 64 KiB and uses a closed TOML schema: unknown fields, permissions,
action kinds, or parameter shapes fail that package. Manifest version 1 is
required; a different plugin API version remains structurally valid and is
reported as incompatible. Semantic plugin versions, unique IDs and
permissions, 10–60 Hz continuous update rates, and boolean/integer/number/
string/choice parameter constraints are validated without source execution.

Filesystem inspection canonicalizes the owned plugin root and each immediate
package. Absolute/traversing entry paths, alternate-stream syntax, paths that
leave the package, symlinks, junctions, other reparse points, native modules,
and Lua bytecode are rejected. Every `.lua` source must be UTF-8 and at most
256 KiB; a package may contain at most 256 source files and 1 MiB of Lua source
in total. Discovery also bounds the managed root to 1,024 entries, one package
to 4,096 filesystem entries, and package nesting to 32 directories. Files are
opened through bounded reads so a concurrent size change cannot turn metadata
validation into an unbounded allocation. Non-source documentation/assets are
inspected as inert files and are never candidates for the Lua module loader.

Discovery returns a deterministic package-name-sorted report. Status is
isolated per directory (`Compatible`, `IncompatibleApi`, or `Invalid`) with a
stable non-content error code; a bad package cannot abort or hide later
packages. Routine logs include only sanitized package/plugin identity, version,
status, source counts, byte counts, and error codes—never TOML or Lua source.
Compatible discovery captures accepted Lua bytes in an immutable source image.
Changing a package file afterward cannot change that image or make the Lua
module loader reopen it. An enable/reload operation must still repeat complete
validation and create a new image before creating a Lua state, rather than
treating an earlier startup report as authorization.

## Atomic local plugin trust state

Startup loads a bounded schema-one `plugins-state.json` and reconciles it
against fresh read-only discovery. This file is machine-local and separate
from portable `config.json`. It stores only a revision, per-plugin
enabled/faulted flags, the
canonical approved permission names, and the last successfully loaded manifest
identity plus a package SHA-256 fingerprint. It contains no Lua or TOML source,
plugin path, action parameter, credential, window context, or capability
payload.

The package fingerprint covers the exact bounded manifest and every accepted
Lua path and byte in deterministic order. It is used only for change
detection. A changed permission set is a trust change: reconciliation disables
an enabled record immediately and reports `permission_review_required` until
the new canonical set is explicitly approved. Other manifest or source edits
preserve approval but report `reload_required`; a rescan never loads them.
Missing, invalid, or incompatible packages are unavailable and an enabled
persisted record is disabled fail-closed.

Runtime admission must use the reconciled effective status for the current
discovered package. The persisted `enabled` flag is historical state, not
standalone authorization: `reload_required`, `permission_review_required`,
faulted, unavailable, and incompatible records must never construct or retain
a Lua state after the worker observes them through startup discovery or a
requested operation's fresh scan. Gradial does not watch package files between
operations; editing a currently loaded local script takes effect only after
manual reload.

Permission approval and successful enablement are separate transitions.
Approval alone always leaves the record disabled. The successful-load
transition is callable only after repeated package and API validation, bounded
Lua entry evaluation, and lifecycle callback validation. The dedicated plugin
worker is now that production caller and the only owner allowed to write trust
transitions while Gradial is running. Fault state is persisted; disable or a
later validated successful reload clears it.

The state file is capped at 512 KiB and 1,024 plugin records, rejects unknown
JSON fields, future schemas, malformed IDs, unordered/duplicate permissions,
invalid fingerprints, and impossible enabled-without-loaded states. Saves
advance a checked revision, flush a temporary file, and use Windows
write-through replacement while retaining the previous valid primary as
`plugins-state.json.bak`. A broken primary recovers from the valid backup
without overwriting either file. If neither copy is usable, Gradial retains the
evidence and continues with an empty in-memory state, which disables all
plugins. Stale writers and interrupted saves cannot replace newer state.

## Lua sandbox foundation

`gradial-lua` embeds vendored Lua 5.4 through `mlua`. Loading is an explicit
host call that creates one fresh, sendable state from one frozen source image.
Discovery itself never makes that call. Placing a package in the plugin
directory starts it disabled and therefore executes zero Lua code.

The state loads only Lua's basic value/iteration functions plus `math`,
`string`, `table`, and `utf8`. It removes `os`, `io`, `package`, `debug`,
coroutines, global `require`, `load`, `loadfile`, `dofile`, `collectgarbage`,
`print`, `warn`, `rawset`, and `string.dump`. Native modules and bytecode have
already failed source-image construction, and every executed chunk is
explicitly text-only. The `gradial` host table is a protected proxy and exposes
only the private module loader in this slice—no context or OS capability.

Lua authors load another file with:

```lua
local mapping = gradial.require("lib.mapping")
```

The name uses lowercase ASCII letters, digits, `_`, `-`, and `.` separators;
the example resolves only to the frozen `lib/mapping.lua` entry. Absolute
paths, separators, empty components, traversal, missing modules, and dependency
cycles fail with stable errors. Results are cached once per state, recursive
loads use the same state and environment, and no module request reaches the
filesystem.

Each state has a 16 MiB allocator ceiling. Initial entry/module loading has a
100 ms wall deadline and a one-million-instruction budget, checked by a
host-owned global hook every 1,000 instructions. A caught hook error does not
clear the host's terminal budget state. Memory, instruction, deadline, module,
syntax, runtime, and entry-shape failures are reduced to stable codes that do
not retain source text, Lua messages, or tracebacks.

`LoadedLuaPlugin` remains the non-executing foundation. The separate
`LuaLifecycleAdapter` validates that the entry table has function-valued
`begin`, `update`, `commit`, and `cancel` fields before it can be used. This
keeps lower-level sandbox fixtures independent while making callback-shape
validation mandatory at the lifecycle boundary.

The lifecycle argument schema is deliberately small:

- `context` contains the continuous API version, opaque session UUID text,
  action ID, and only the already permission-filtered application/title
  snapshot. Application context contains executable path and window class;
  no handle, PID, coordinates, raw key state, or configuration is exposed.
- `parameters` maps manifest parameter names to Lua booleans, integers,
  numbers, or strings. Choice values are presented as strings.
- each sample contains `sequence`, `value`, `delta`, and `elapsed_ms`.
- cancel reasons use stable snake-case strings such as `escape` and
  `returned_to_discrete`.

Context, nested application context, parameters, and samples are protected
read-only proxies. Their `pairs` iterator does not reveal the mutable backing
table as iterator state. `begin` must return exactly one ordinary Lua table;
the adapter retains that mutable table privately and passes it only to the
same session's later callbacks. `update`, `commit`, and `cancel` must return no
values. A terminal call removes its session before invoking Lua, so it cannot
be repeated even when Lua fails.

The callback ceilings are 100 ms/1,000,000 instructions for `begin`,
16 ms/160,000 instructions for `update`, 100 ms/1,000,000 instructions for
`commit`, and 50 ms/500,000 instructions for `cancel`. The existing global
hook and memory ceiling enforce them. Runtime, resource, or return-shape
failure faults the adapter and drops every retained session; duplicate or
unknown session requests are rejected as host protocol errors without blaming
the plugin.

## Dedicated plugin worker

`gradial-plugin-worker` owns the mutable trust snapshot and every
`LuaLifecycleAdapter`. No Lua state crosses its thread boundary. Its command
channel is bounded to eight pending operations; submissions are nonblocking
and return a one-result ticket intended for later Settings polling. Invalid
plugin IDs, a full queue, shutdown, and worker loss are rejected before work.
The shared shutdown flag bypasses queue pressure, destroys every adapter, and
waits no longer than two seconds.

Startup loads the atomic local state, performs a fresh complete discovery,
persists any permission/unavailability disablement, and constructs adapters
only for packages whose reconciled status is exactly `enabled`. A package that
fails sandbox or callback validation is unloaded and atomically marked faulted.
If that fault write itself fails, the adapter remains absent for the current
process and the fixed persistence failure is logged. A broken plugin cannot
prevent the worker from reaching its command loop or the rest of Gradial from
starting.

Enable and reload are serialized admission transactions:

1. repeat complete discovery and trust reconciliation;
2. select the compatible frozen package by validated ID;
3. require current approval and the operation-appropriate effective status;
4. construct and validate a candidate adapter under normal Lua limits;
5. atomically record the exact package identity/fingerprint; and
6. admit the candidate only after the state save succeeds.

A validation or persistence failure leaves no candidate adapter reachable.
Reload removes an observed stale/faulted adapter before loading replacement
code. Permission changes unload and disable before reapproval. Disable and
fault requests tear down the adapter before attempting their durable state
transition, which keeps the current process fail-closed even if storage is
unavailable. File scans, Lua work, and state writes never run on the input,
interaction, renderer, or action-executor threads.

The worker exposes an immutable inventory snapshot containing display metadata,
requested permissions, declarative continuous-action schemas, effective
status, whether the current permissions were approved, and safe issue codes. A
manual rescan repeats discovery and trust reconciliation without constructing
a Lua state. If package changes revoke an adapter's effective enabled status,
rescan unloads it immediately. Enable also checks current permission approval
before constructing the candidate, so an unapproved package cannot execute
source even when a caller bypasses the UI.

## Plugins Settings page

The **Plugins** page polls worker tickets without blocking the renderer. It
shows each discovered or remembered package's name, ID, version, plugin API,
requested permissions, declared continuous controls and update rates,
effective status, and a stable user-safe problem message. **Rescan plugins**,
**Enable**, **Disable**, and **Reload** are disabled while one worker request is
pending. **Open plugins folder** uses the managed
`%LOCALAPPDATA%\Gradial\plugins` directory and does not expose arbitrary paths
to the shell.

An unapproved disabled plugin and every permission-change review open a modal
that identifies the plugin, lists its permissions in plain language, and warns
that local Lua scripts should be enabled only when trusted. **Approve and
enable** is deliberately two serialized worker operations: persist the exact
permission set first, then perform fresh discovery and candidate validation.
A failure at either boundary leaves the adapter absent. Once permissions are
unchanged, later enablement does not repeat the confirmation; source changes
still require an explicit reload.

Plugin management is machine-local operational state, not portable menu
configuration. Its controls therefore apply immediately and remain independent
of Save, Discard, import, export, and any unsaved Settings draft. Closing
Settings dismisses a pending trust confirmation but does not cancel an already
accepted worker operation.

## Worker lifecycle bridge

The runtime exposes a cloneable internal lifecycle client with three independent
bounded lanes: one pending Begin, one pending Update, and one pending Commit or
Cancel. The terminal lane is therefore available even when the ordinary update
lane is full, and the worker's biased selection checks terminal work first.
The runtime-neutral continuous core remains responsible for coalescing samples
before this boundary; the worker never accumulates mouse-rate update history.

The worker owns one global active-session record containing only its opaque
session UUID and plugin ID. Begin selects an already admitted adapter, requires
an exact declared action, validates its frozen parameters against that action's
manifest schema, and establishes ownership only after Lua accepts the session.
Update and terminal requests must match that owner. Commit and Cancel clear
worker ownership before calling Lua; the lifecycle adapter independently
removes its private Lua table before invoking the terminal callback. Unknown,
duplicate, invalid-parameter, and post-terminal requests fail without reaching
Lua.

Any callback execution, resource-limit, or return-shape failure that faults the
adapter immediately clears worker ownership, unloads the adapter, and attempts
to persist the plugin's faulted state. The reply and release log contain only a
stable error code. A persistence failure cannot make the adapter reachable
again in the current process.

Manual rescan remains code-free during an active session. It may show that a
package now needs reload or permission review, but replacement and trust-state
reconciliation are deferred until Commit or Cancel has completed. Settings
enable, disable, reload, approval, and explicit fault operations return
`session_active` rather than tearing down private Lua state without its terminal
callback. Reload and Disable from Settings now ask the interaction runtime to
cancel its continuous coordinator first, wait until the bounded terminal
callback completes, and only then submit the worker operation. A short bounded
retry covers an already-detached terminal callback racing with the request.

The checked-in `examples/plugins/org.gradial.example.meter` package requests no
permissions and stores its latest scalar only in private session state. Worker
tests load its real manifest and source and complete Begin, Update, and Commit.

## Live continuous plugin route

The compiled action catalog can freeze a `PluginContinuous` definition into a
runtime-neutral binding without consulting local machine state. At invocation
down, the interaction runtime combines each resolved menu slice's binding with
the worker's immutable catalog of currently admitted manifest actions. Only a
matching enabled plugin/action pair becomes continuous-capable; a missing,
disabled, faulted, reloading, or undeclared reference remains an ordinary
discrete wedge at every radius. The action parameters and manifest-owned
10–60 Hz cadence are frozen for that invocation.

The plugin backend uses the existing continuous action lease and callback
deadlines. It performs full target validation before Begin, lightweight target
freshness checks before Update and Commit, and never lets target loss prevent a
best-effort Cancel. Worker rejection and Lua faults map to the coordinator's
stable rejected, timed-out, stale-target, interrupted, or faulted outcomes. A
Begin that times out while queued is explicitly aborted through the reserved
terminal lane; the worker either rejects it before Lua or cancels it after Lua
accepts, so it cannot leave an orphan session.

This slice exposes no host capability. In particular, Lua receives an empty
permission-filtered application/title context even if a manifest requests
those permissions. Settings can now create actions for enabled manifest
controls, edit their generic parameters without running plugin code, and
assign them independently beyond any Global or application-profile wedge. All
operating-system capabilities remain later Milestone 3 work.

## Configuration and profile resolution

The configuration model contains one global profile and zero or more
application profiles. Every profile has an immutable UUID, name, and complete
four- or eight-slice menu. Each slice contains its visible label, an optional
action UUID, and an optional path-safe icon reference consisting only of a UUID
and closed PNG/SVG format. Application rules require an executable path and may add
window-class and title-regex predicates with an explicit priority from 0
through 100.

Executable separators and case are normalized without expanding environment
variables. Class matching is case-insensitive exact; title regex matching uses
Rust's non-backtracking regex engine and is opt-in. Eligible profiles are
ordered by highest priority, then most predicates, then lexicographically
smallest UUID. If the process path is unavailable or no rule matches, the
global profile wins.

The compiled resolver returns the winner, all matching profile IDs, their
priority/specificity, the exact tie-break step, and a reference-counted
immutable copy of that profile's complete menu. The interaction log records
the explanation without target path, class, title, or menu-label content.

The selected menu is frozen with the active invocation. Its slice count drives
gesture recognition and chooses the matching renderer cache; its label,
action-reference, and icon-reference lists remain paired. A title change after activation
therefore cannot switch profiles or produce a gesture/render/action mismatch.

## Settings editor and live reload

The notification-area menu's **Open settings** command opens one normal,
decorated, resizable taskbar window. Repeating the command focuses that same
window and preserves its draft. The editor begins from the running
configuration and modifies a private full-configuration copy; an unfinished
edit cannot affect an open gesture or a later invocation.

The **Activation** tab chooses M4 or M5 and generic Ctrl, Alt, and Shift
requirements. Matching is exact: a required key may be pressed on either
physical side, while an additional generic modifier prevents activation. The
draft displays the same M4/M5 center label used by the production overlay.
Changing these controls marks only the editor copy; the live hook retains the
saved binding until atomic save succeeds and the interaction runtime restarts.

**Start hold–flick–release test** arms a one-shot atomic path in the existing
low-level hook. The next physical M4 or M5 attempt is consumed before
production activation. The hook compares fixed-size button/modifier bits,
tracks signed origin and latest cursor coordinates, suppresses the matching
release, and reports either a chord mismatch, centered cancellation, or one of
eight flick directions. It does not enter the interaction controller or action
executor, and it never logs target-window data. Leaving the tab, discarding,
saving, or closing disarms the tester; a release already paired with a
consumed down remains suppressed.

The **Appearance** tab edits one global schema-six value shared by Global and
all application profiles. The accent is stored as three bounded RGB bytes.
Label theme is a closed light/dark choice, and size is a closed
compact/standard/large choice. The original `(124, 154, 255)` accent, light
labels, and standard size are the default and schema-one-through-three
migration values.

Compact, standard, and large multiply only renderer coordinates and font sizes
by `0.85`, `1.0`, and `1.15`. Input center/entry radii, hysteresis, direction
boundaries, edge assist, and the fixed overlay window remain unchanged. Dark
labels select a contrasting light ring and a bounded light backing behind each
label so long text remains readable over arbitrary foreground content.

The appearance preview passes the draft value to
`FoundationOverlayRenderer`; the Menu and Profiles previews use the same draft
appearance. Discard restores the saved value. Save atomically persists the
complete configuration, updates the existing prewarmed pool's appearance, and
restarts input against the new revision without rebuilding geometry caches.

The **Menu** tab can switch between four and eight clockwise directions, edit
each label, assign or clear any discrete action in the validated catalog, and
choose, replace, or remove one PNG/SVG icon per direction. Each wedge also has
an independent **Enable distance beyond this wedge** checkbox and continuous
action selector. Releasing over the wedge runs its normal action; moving beyond
the outer edge runs the continuous extension; returning to the wedge switches
back to the normal action. Changing the checkbox never replaces the normal
action. The native icon chooser
accepts only those two source types. Import rejects empty or unreadable files,
sources over 256 KiB, dimensions outside 1–512 logical pixels, invalid
PNG/SVG data, and SVG document/embedded-image/text features. Accepted bytes
are flushed into `%LOCALAPPDATA%\Gradial\icons` under a generated UUID name;
the configuration never stores a selected path. Removing or discarding an
assignment never deletes managed files. Reconciliation retains every reference
from the active configuration, backup, and open draft; once no protected
configuration needs an exact managed asset, it moves to the recoverable
quarantine.
Eight-to-four conversion retains the four cardinal slices. Diagonal slices are
stashed while the draft remains open and restored if the user switches back;
expanding a configuration that was already saved with four slices creates the
standard diagonal defaults. The selected direction follows its cardinal
equivalent across conversions.

The clickable live preview uses `FoundationOverlayRenderer` and
`ResolvedMenu`, so it shares production geometry, label placement, slice
ordering, and selection rendering. It does not contain a second gesture or
layout implementation. Directions with icons render a fitted 30-DIP image and
a smaller outward label; iconless directions preserve the original label
layout. A missing or damaged managed source renders the bounded built-in
question-mark placeholder.

The **Profiles** tab lists application-specific profiles separately from the
Global fallback. **New profile** opens the native application chooser and adds
nothing when that chooser is cancelled. A selected `.exe` receives a new UUID,
a readable unique name derived from the filename, priority 50, an exact-path
rule, and an independent copy of the current Global menu. The executable can
later be replaced through the same chooser. Priority is constrained to 0
through 100.

**Duplicate selected** copies the complete profile rule and menu under a new
UUID and unique name. **Delete selected** removes only that private draft
profile; normal discard and guarded-close behavior can still recover the
saved configuration. Optional window-class matching is an exact,
case-insensitive predicate. Optional window-title matching is a
case-sensitive Rust regular expression. Enabling either rule exposes its
field; an empty class, empty pattern, or invalid pattern has an inline error
and disables save. Disabling the checkbox removes that predicate.

**Test profiles** starts a three-second countdown so the user can switch to a
target application. At the deadline, Gradial privately captures the foreground
executable path, window class, and title, compiles the current complete draft
through `ProfileResolver`, and displays every match in resolver order with
priority, specificity, and immutable UUID. It shows the winning profile and
whether the decision came from sole eligibility, highest priority, highest
specificity, the UUID tie-break, unavailable executable identity, or Global
fallback. Rule edits re-resolve the retained target immediately.

Captured target values remain only in settings memory: they are neither added
to the draft nor written to diagnostics. **Clear captured details** purges them
explicitly, and every clean or discarded settings close also clears them.

Each application profile can independently switch between four and eight
directions, edit labels, assign or clear any catalog action, and select a slice
in its own production-renderer preview. A per-profile diagonal stash preserves
eight-slice edits while that profile temporarily uses four slices. Creating,
resizing, duplicating, or deleting a profile never mutates the Global menu or
another profile. Empty profile names, required paths, out-of-range priorities,
empty profile-menu labels, empty optional predicates, and invalid title
patterns receive field-specific save messages.

The **Actions** tab lists the complete catalog. Its **New action** menu creates
every discrete leaf type: keyboard shortcut, Unicode text, website,
file/folder, application, Delay, and invocation-window command, plus reusable
sequences. It also lists every action declared by a currently enabled,
compatible plugin. Choosing one creates a portable continuous action with a
new UUID and manifest defaults. The action editor renders booleans, bounded
integers/numbers, bounded strings, and closed choices directly from the
manifest; optional parameters have an explicit inclusion checkbox. Plugin code
does not run while any parameter is edited. A disabled compatible plugin keeps
its existing actions editable but inactive. Missing or incompatible plugin
actions retain their stored parameter maps without rewriting them.

The catalog can rename or duplicate every action type. Creation and
duplication generate new UUIDs and readable unique names. A sequence can be
created after at least one regular action exists, and begins with one valid
step so it never creates an invalid empty draft node.

The shortcut recorder accepts one supported non-modifier physical key plus the
exact left/right Ctrl, Alt, Shift, and Windows modifiers held for it. It stores
the Windows scan code and extended-key flag shown before save; Escape cancels
recording. If a modifier was already down before recording began and no
left/right key transition is visible, it safely falls back to the left variant.
The first editor covers letters, top-row numbers, punctuation, Enter, Tab,
Backspace, Space, navigation keys, and F1 through F24. Unicode text is edited
directly without clipboard access and displays its UTF-8 byte count against
the existing 8 KiB limit.

Website fields accept only complete HTTP or HTTPS addresses. File/folder
actions allow direct absolute-path editing plus separate native file and folder
choosers. Application actions use an absolute `.exe` path, an optional absolute
working folder, and one UI row per argument. Rows are preserved literally,
including spaces and empty values; the editor never parses a shell command.
The application and working-folder controls use the native Windows common item
dialog from a dedicated STA thread and do not log selected paths. Delay is
constrained to 10 through 10,000 ms. Window commands are limited to close,
minimize, maximize, and restore for the invocation window.

New leaf actions begin with a syntactically valid, visible default so creating
one does not invalidate unrelated draft work: HTTPS example address, the
current user folder, Notepad from the Windows directory, 250 ms Delay, or
minimize as appropriate. The user must still assign the action to a menu before
it can run.

Draft validation translates invalid URLs, non-absolute paths, non-`.exe`
applications, null-containing arguments, and out-of-range Delay values into
field-specific messages. Complete configuration validation and plan
compilation remain authoritative before save.

Sequence actions display their ordered direct steps with **Up**, **Down**, and
**Remove** controls. The final step cannot be removed. **Add step** lists leaf
or nested-sequence candidates only when replacing the edited sequence with the
proposed list keeps the complete configuration valid. This automatically hides
the sequence itself, ancestors that would introduce cycles, and additions that
would make this sequence or any containing sequence exceed four nested
sequence levels, 32 executed leaf steps, or 30 seconds of total Delay.
Repeated safe actions remain allowed.

The sequence editor compiles the proposed list after each change and displays
direct-step count, expanded executed-step count, nested depth, and total Delay.
Invalid sequence shapes, missing references, cycles, excessive depth, expanded
steps, and Delay totals also have field-specific save messages as a defensive
boundary.

Deletion first scans global and application-profile menu slices plus sequence
steps. A referenced action is never removed; the editor reports the number and
kind of references so the user can clear them. An unreferenced action can be
deleted without rewriting any other configuration data.

**Save changes** is enabled only for a dirty draft whose required labels,
configuration references, profile rules, action catalog, available manifest
parameters, and bounded action plans all validate and compile. The application
repeats portable configuration validation before
disk mutation, performs the existing atomic save/backup procedure, and updates
the retained resolver, action compiler, overlay revision, executor revision,
and interaction runtime only after replacement succeeds. A failed save keeps
the draft open and leaves the running configuration unchanged. A successful
save increments `config_revision`; fresh invocations immediately use that
revision and its saved M4/M5 chord.

A normal close with a clean draft closes immediately. A dirty close displays
exactly **Discard changes** and **Keep editing**. The former abandons the
private draft and closes; the latter returns to the same unsaved draft.
**Discard edits** inside the window resets the draft without closing. Startup
recovery state is shown as a persistent warning and clears only after a
successful explicit save.

## Action model and plan compilation

Schema two supports the complete v1 action family:

- keyboard shortcut by scan code, extended-key flag, and explicit left/right
  modifiers;
- UTF-8 Unicode text;
- HTTP or HTTPS URL;
- absolute file or folder path;
- absolute executable path, argument array, and optional absolute working
  directory;
- close, minimize, maximize, or restore the invocation window;
- delay from 10 through 10,000 ms;
- reusable linear sequence of action UUIDs.

The model rejects empty action names, duplicate action UUIDs, malformed
shortcuts, text above 8 KiB UTF-8, disallowed URLs, relative paths, executable
paths without an `.exe` extension, embedded nulls, missing menu/sequence
references, empty or oversized sequences, direct or indirect cycles, nesting
beyond four sequence levels, expansion beyond 32 leaf steps, and a statically
known delay total above 30 seconds.

The validated catalog is cloned into an immutable `ActionPlanCompiler` when
configuration loads. On release, the selected action UUID comes from the
invocation's frozen menu. The compiler requires the same frozen
`config_revision`, recursively replaces reusable sequences with ordered leaf
steps, and attaches the immutable plan to the invocation. Every plan records a
5-second per-step timeout and 30-second overall ceiling for the worker.

The single-worker executor accepts at most one pending or running plan. A
second dispatch returns `action_busy` immediately and is never queued. Every
compiled leaf operation is supported: Delay, SendShortcut, TypeText, OpenUrl,
OpenPath, RunExecutable, and Window.

Delay uses monotonic time and checks cancellation/shutdown in increments no
larger than 10 ms. The worker defensively enforces the compiled 32-step,
5-second per-step, and 30-second overall limits. The tray's **Cancel current
action** command is enabled only while the admission gate is occupied.
Shutdown requests cancellation and waits at most two seconds for the worker.

Before every shortcut step, the worker confirms that the captured HWND still
exists, resolves to the same PID and process start time, remains foreground,
and is at equal or lower integrity than Gradial. Identity/foreground failure
returns `stale_target`; higher integrity returns `elevated_target`; no key
event is emitted in either case.

Shortcut events use configured scan codes and extended-key flags rather than
layout-dependent characters. Current left/right physical modifier state is
read immediately before injection. Gradial presses only missing requested
modifiers, marks every synthetic event so its own hook ignores it, releases the
primary key, and releases synthesized modifiers in reverse order. A cleanup
guard repeats pending key-up work after failure, cancellation, or unwind and
never releases a modifier that was already physically held.

Text input is converted from the validated UTF-8 configuration value to UTF-16
only at dispatch. Every code unit is sent with `KEYEVENTF_UNICODE`, so BMP
characters and surrogate pairs are delivered literally without keyboard-layout
translation, clipboard access, or variable/environment expansion. Gradial
performs the full identity and integrity check before the first unit, then
checks the HWND, PID, and foreground identity before every key-down and key-up.
If the target changes, cancellation arrives, or Windows rejects an event, a
guard releases the one pending Unicode unit before execution stops. The same
boundaries enforce the plan's step and overall deadlines during long text.

OpenUrl accepts only the configuration model's HTTP/HTTPS URLs. OpenPath
rechecks that its absolute file or folder still exists immediately before
dispatch and returns `path_not_found` without calling the shell if it does
not. Both actions send one literal, null-terminated UTF-16 value to
`ShellExecuteExW` with the `open` verb, no parameters, no working directory,
no `cmd.exe`, and no environment-expansion flag. The worker initializes an STA
COM apartment and uses `SEE_MASK_NOASYNC` because it is a background thread
without a message loop; this completes the association handshake before the
call returns without waiting for the opened application's lifetime.
`SEE_MASK_FLAG_NO_UI` prevents shell error dialogs. Success means Windows
accepted the association request. Other rejection returns
`process_launch_failed`.

RunExecutable accepts only a validated absolute `.exe` path. Immediately
before creation, the worker requires that path to resolve to an existing
regular file and requires an optional working directory to resolve to an
existing directory. A confirmed missing executable or working directory
returns `path_not_found`; invalid path kinds, inspection errors, an oversized
Windows command line, or process-creation rejection return
`process_launch_failed`.

Arguments remain separate strings in the immutable plan. At the Windows
boundary, each value, including empty strings, quotes, spaces, and trailing
backslashes, is encoded and escaped into the one mutable UTF-16 command line
required by `CreateProcessW`. The validated executable is also supplied
separately as the application name. Gradial invokes no shell, performs no
environment interpolation, supplies no custom environment, disables handle
inheritance, and therefore gives the child the action worker's normal
environment and integrity. Once Windows creates the child, Gradial closes the
returned process/thread handles immediately and records success without
waiting for application readiness or exit.

Successful plans record `action_executed=true`. Empty slices still record
`EmptySlice`; a defensive compilation mismatch records `ActionPlanInvalid`.
The overlay begins its close animation immediately and never waits for action
completion. Logs record Unicode scalar counts and open-request kinds but never
text, URLs, paths, executable names, working directories, or argument values.

Executor completions and immediate dispatch rejections share one action
feedback boundary. Success is ignored. Cancellation adds one fixed
**Cancelled** entry, while failure adds one fixed message selected solely from
its stable `ActionErrorCode` and queues a native Windows notification. The
configured `safe_message` is deliberately not copied into either surface, so
an accidentally dynamic path, URL, target title, text value, or argument cannot
cross this boundary.

The store is memory-only, keeps the latest 20 failures/cancellations newest
first, and drops the oldest entry when full. Its pending notification queue is
bounded to the same size. The tray exposes the snapshot through **Recent
activity**, and **Clear activity** removes both retained entries and any
notification not yet submitted to Windows. Notifications use only
`Shell_NotifyIconW(NIF_INFO)` with an error glyph, no sound, and quiet-time
respect; the call does not activate or focus Gradial. A Windows notification
submission failure remains in recent activity and is recorded locally without
retry. There is no success notification and no persisted activity database.

Window actions use the exact HWND captured at activation-down. Immediately before
dispatch, the worker checks that the HWND still exists, still belongs to the
captured PID, and still represents the same process start time. It also
requires that window to remain foreground and checks target integrity against
Gradial's normal integrity. Stale identity returns `stale_target`, a higher
integrity target returns `elevated_target`, and inspection or dispatch failure
returns `window_command_failed`.

Close posts one `WM_CLOSE` request. Minimize, maximize, and restore each post
one matching `WM_SYSCOMMAND`. These are asynchronous queue operations: success
means Windows accepted the message, not that the application obeyed or
finished the transition. Gradial never enumerates windows, substitutes a
current HWND, activates a target, retries, waits for acknowledgement, or
terminates a process. Window titles and target identity values are omitted
from action logs; only the fixed command name is recorded.

## Edge behavior

The default `--geometry=auto` mode uses fixed origin whenever the required
extent centered at activation-down fits in the monitor work area. Discrete
invocations retain their 288 DIP square. The continuous meter requests a 640
DIP square for the recommended 300-DIP endpoint and up to 840 DIP for the
400-DIP maximum. The four-candidate distance comparison requests a 640 DIP
square for its 300 DIP track. Otherwise Gradial selects edge assist and clamps
the visual origin far enough inward for the complete overlay to fit. If the
work area itself is smaller than the viewport, the renderer compresses only
the visible continuous range while preserving exact input normalization. Work-area
coordinates remain signed, so monitors above or left of the primary display
are handled without unsigned conversion.

Comparison overrides are available without changing code:

```powershell
cargo run --package gradial-app -- --geometry=fixed-origin
cargo run --package gradial-app -- --geometry=edge-assist
```

The system cursor is never warped and Gradial does not synthesize pointer
movement. Raw input is used only during an active edge-assisted invocation.
The four/eight-direction mappings, forced modes, corner insets at 200% DPI, and
absolute-motion suppression are unit tested.

The resolved profile is authoritative for both four/eight geometry and labels.
Both counts use the same radii and five-degree boundary hysteresis; their
angular width, selection indices, highlight arc, and configured labels differ.

Debug builds retain `--slices=4|8` solely for repeatable interaction tests. The
override substitutes a complete direction-labelled test menu and is recorded
as `menu_source=debug_override`. Release builds reject the option and always
record `menu_source=configuration`:

```powershell
cargo run --package gradial-app -- --slices=4
cargo run --package gradial-app -- --geometry=edge-assist --slices=8
```

`--reduced-motion` removes opening and closing scale/opacity interpolation
without changing recognition or lifecycle timing. It is a temporary startup
override until appearance configuration is persisted:

```powershell
cargo run --package gradial-app -- --slices=4 --reduced-motion
```

## Per-display overlay guarantees

The root viewport is a permanently hidden controller. At startup, Gradial
creates one independent 288 DIP deferred viewport for every active monitor.
Each child is moved onto its assigned monitor while hidden, checked with
`GetDpiForWindow`, painted enough times to make its graphics resources resident,
and only then exposed to the input runtime. Every child is:

- transparent and borderless;
- rendered with wgpu;
- topmost and omitted from the taskbar;
- initially inactive;
- configured with `WS_EX_NOACTIVATE` and `WS_EX_TOOLWINDOW`;
- shown and repositioned with no-activation flags;
- returning `MA_NOACTIVATE` for `WM_MOUSEACTIVATE`;
- returning `HTTRANSPARENT` for `WM_NCHITTEST`.

eframe/winit finalizes some extended styles after constructing a viewport.
Gradial therefore applies and reads back the required styles during prewarming,
then keeps the viewport hidden until activation. On an open effect, a private
message handled by that child's native subclass positions and shows it with
no-activation flags. The matching child repaint is independent of the invisible
root-controller repaint cycle. Showing the native child invalidates it
immediately; selection and continuous-state changes pair egui's viewport
request with the same native invalidation. While opening or closing animation
remains active, each rendered frame requests another native frame.
That bounded self-pump prevents a hidden child from waiting until release to
paint or remaining at its first 85% opacity frame.

Each child owns a persistent renderer. During pool construction it caches both
four- and eight-slice boundary segments, label offsets, and 24-segment
highlight arcs. Rendering selects one cache from the frozen menu and pairs its
offsets with the configured labels; no geometry or profile matching occurs on
activation's UI path. Display/DPI pool replacement constructs one complete
cache pair for each replacement child. Selection levels are derived directly
from the current wedge on every frame, so pointer feedback never waits for an
interpolation state.

Appearance remains a small copied render-frame value. Accent colors replace
only the foundation outline, divider, and selection colors. The label theme
chooses a complete contrast palette, and the size preset multiplies cached
coordinates at paint time. None of these choices invalidates the prewarmed
four/eight cache pair or enters the interaction thread's gesture calculations.

The icon manager enumerates the complete configuration before interaction
starts, enforces the source bounds again, decodes PNG pixels or rasterizes SVG
vectors, and registers egui textures. The same cache is shared by every
prewarmed display renderer and the Settings preview. A Settings import or icon
choice extends that cache while still editing the private draft; saving
recompiles the complete menu but never decodes an icon on activation. A cached
placeholder is retried after portable import or quarantine recovery restores
its managed source. Reconciliation runs before the startup preload and after
successful icon imports, configuration saves, or Settings close/discard
events. Quarantining an unprotected asset also drops its unused texture handle.

`WM_DISPLAYCHANGE`, `WM_DPICHANGED`, and `WM_SETTINGCHANGE` for the work area
invalidate the pool. Monitoring is deliberately disabled while windows are
being placed, because initial PMv2 placement can itself emit DPI messages. A
real change suspends M4, cancels an open invocation with
`DisplayTopologyChange`, hides the routed child, destroys the old runtime/pool,
enumerates signed work areas again, and prewarms a new generation. Input resumes
only after all new child windows report the expected DPI.

## Animation contract

Opening starts with the complete usable ring at 96% scale and 85% opacity, then
uses cubic ease-out to reach its final state at 60 ms. Wedge selection changes
color immediately because it is direct pointer feedback rather than a
lifecycle animation.

Release finalizes the gesture immediately and starts a 50 ms opacity fade. The
native overlay remains click-through and non-activating during that fade, then
receives its private hide request. Action dispatch will therefore not wait for
the close animation when actions are introduced. A new invocation can safely
supersede a closing one; the prior overlay is hidden and its focus checkpoint
is completed before the next wake request. Pool replacement and shutdown use
the same forced-hide fallback instead of waiting for an obsolete animation.

Close also registers its deadline with one pool-wide, bounded coordinator.
This is a lifecycle fallback rather than a second animation driver: the child
viewport normally completes its own close, while the coordinator guarantees
the final native hide if a completion repaint is missed. It checks the
invocation UUID before changing state, and the hide request plus state reset
are serialized under the same viewport lock so an old close cannot hide a
newer invocation. No thread is created per gesture or per monitor.

Standard animation requests a child-viewport repaint no more frequently than
every 8 ms. Reduced motion snaps the visuals but retains the 60 ms opening state
and 50 ms close lifecycle, scheduling only their completion frames. Gesture
selection is advanced on the interaction thread, so a valid flick can release
before the first rendered frame and still select its slice.

## Focus and timing diagnostics

Focus diagnostics compare the foreground window at activation, after the next
child-viewport frame checkpoint, and immediately after hiding the overlay. A
checkpoint passes when the same window remains foreground or when the target
process itself changed its foreground window. A successfully queued window
action marks its invocation as an expected target change, so a deliberate
minimize or close is not mislabeled as overlay focus theft at the final
checkpoint. Gradial never calls an API that activates or restores the
foreground window.

After completing an M4 gesture that was held long enough to render a frame,
validate the latest invocation with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-focus.ps1
```

Gradial registers ETW provider
`{3c55b348-69f8-4d33-8e81-62ba761d4f52}`. Its marker strings contain the
invocation UUID and the source hook's QPC value. Markers cover activation
dequeue, open and native-wake requests, overlay show, first-frame checkpoint,
highlight request and application, release dequeue, close request, and overlay
hide. They are intended to be correlated with actual display events; the UI
checkpoint alone is not claimed as proof that a frame reached the screen.

With Gradial already running, capture both streams for 15 seconds with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/capture-timing.ps1
```

The helper uses the installed PresentMon executable and Windows `logman`. It
writes ignored `.csv` and `.etl` diagnostics under `Gradial APP\captures`.
Run the full display-event capture from an elevated PowerShell. Pass
`-MarkersOnly` when only the Gradial ETW stream is needed.

For a complete machine-readable report, first build the release executable and
run:

```powershell
cargo build --workspace --all-features --release
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-performance-report.ps1 -Mode Development -MarkersOnly
```

Development mode is deliberately short: a two-second warmup, 20 invocations,
eight seconds of idle sampling, and three cold launches. It validates the
harness but marks every release gate `not_evaluated`.

From an elevated, otherwise idle PowerShell, the full protocol is:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-performance-report.ps1 -Mode Release
```

Release mode fixes the warmup at five minutes, performs 1,000 real synthetic
M4 invocations against an isolated 10 ms Delay fixture, retains a 30-minute
idle phase, and measures 100 cold launches. It writes Markdown,
schema-versioned JSON, flat CSV, raw ETW/PresentMon tables, application log,
resource samples, and helper output below `captures\performance`.

Renderer checkpoints are reported only as application diagnostics. The
displayed usable-frame and displayed-highlight gates remain `unavailable`
unless PresentMon produces correlatable displayed events. The shortcut
completion gate remains `not_run` because the Delay fixture does not synthesize
keys. Any foreground change or power-plan change invalidates a release run.
The complete protocol and current-machine development numbers are recorded in
`Documentation\milestone-3-performance-harness-results.md`.

## Manual interaction check

1. Start `cargo run --package gradial-app`. Complete required onboarding if it
   is presented. Afterward no overlay should be visible, and the Gradial
   notification-area icon should report Healthy.
2. Put another application in the foreground and note that it has keyboard
   focus.
3. Hold M4. The menu should appear centered on the cursor without changing
   the foreground application.
4. Move east. The `E` slice should highlight.
5. Release M4. The menu should hide; no application should launch.
6. Hold M4 again and press Escape. The menu should hide, and Escape should not
   reach the foreground application.
7. Put the cursor within five physical pixels of a work-area corner. Hold M4
   and move in an outward direction. The complete ring should be inset, the
   outward slice should highlight, and the system cursor should remain in
   place.
8. Change display scaling, work area, or the connected-display layout. An open
   gesture should cancel, and M4 should become available again after
   `overlay_pool_ready` reports the next generation.
9. Stop Gradial and inspect
   `%LOCALAPPDATA%\Gradial\logs\gradial_rCURRENT.log` for
   `hook_thread_responsive`, `invocation_started`, three
   `focus_checkpoint` records, `slice_selected` or `invocation_cancelled`, and
   `hook_thread_stopped`.
10. Open the tray icon and confirm its status line, **Open settings**,
    **Restart onboarding**, **Retry input**, **Cancel current action**, and
    **Exit Gradial** commands are present.
11. Repeat once with `--reduced-motion`; selection and close behavior should be
    identical without interpolated visuals.

This is a global input development build. Save important work before manual
testing. If input becomes faulted, use **Retry input**. Stop Gradial with its
tray **Exit Gradial** command, its terminal, or the VS Code debug controls.

The repeatable live smoke helper builds on the same contract. It temporarily
moves the cursor, starts the hidden executable, sends one or more synthetic M4
gestures, verifies expected slices, foreground identity, and edge cursor
retention, restores the cursor, and closes only the process it started:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -Slices 4
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry edge-assist
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -MonitorIndex 1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -SimulateDisplayChange
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -SimulateInputReregistration
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -SimulateInputFault heartbeat
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -SimulateInputFault queue-overflow
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -ReducedMotion
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -ContinuousMeter -ContinuousPercent 75
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -ContinuousMeter -ContinuousPercent 50 -ChangeFromDirection W -Direction S
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -ContinuousMeter -ContinuousPercent 50 -Direction W -ReturnInsideBeforeRelease
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -ContinuousMeter -ContinuousPercent 50 -ChangeFromDirection N -Direction E -PointerBurstEvents 1000
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry edge-assist -ContinuousMeter -ContinuousPercent 50 -ActivationPoint top-left -ChangeFromDirection E -Direction E -InvocationCount 20 -GestureDelayMilliseconds 75 -InterInvocationDelayMilliseconds 5 -RequireOpenAnimationComplete
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -QuickFlick
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -ActivationPoint top-left -AllDirections
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -ActivationPoint center -Direction SW -InvocationCount 20 -QuickFlick -InterInvocationDelayMilliseconds 10
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-delay-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-shortcut-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-text-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-open-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-process-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-window-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-settings-editor.ps1 -CaptureScreenshot
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-onboarding.ps1 -CaptureScreenshot
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-continuous-matrix.ps1 -Suite standard
```

Debug `smoke-interaction.ps1` runs append the development-only
`--skip-onboarding` option so a fresh test machine reaches the interaction
under test without changing the user's completion marker. Release builds
reject that option; isolated release action fixtures create their own valid
marker inside their disposable `LOCALAPPDATA` trees.

Add `-CaptureTiming` to the second command to run the documented ETW and
PresentMon capture around the synthetic gesture. In a non-elevated development
shell, add `-TimingMarkersOnly`. Pass `-Configuration release` after building
with `cargo build --release` for performance-oriented checks. `-MonitorIndex`
selects an entry from Windows' current display list. `-Slices 4` or `-Slices 8`
selects and validates the corresponding startup geometry.
`-ChangeFromDirection N -Direction W` adds a deliberate post-arm turn to a
single continuous-meter invocation and verifies the final live wedge.
`-PointerBurstEvents 1000` adds an even, no-coalescing Windows pointer burst to
a single fixed-origin continuous invocation and requires Gradial to report that
obsolete visual updates were collapsed while retaining the final value.
Every non-quick smoke invocation must now render its first profile-menu frame
before release and within 100 ms. `-RequireOpenAnimationComplete` additionally
requires the 60 ms opening lifecycle to finish before each release; use a
gesture delay of at least 60 ms.
`-SimulateDisplayChange` holds M4, posts the same notification used by the
native rebuild path, verifies the active invocation is cancelled, waits until
generation 1 is fully rewarmed, and then sends a fresh gesture.
Every run confirms that Windows can locate Gradial's live notification icon.
`-SimulateInputReregistration` posts a resume notification and verifies that a
new hook runtime becomes responsive. Debug-only `-SimulateInputFault heartbeat`
verifies automatic replacement; `queue-overflow` verifies the manual Faulted
state, M4 pass-through, and recovery through the same Retry Input command path
used by the tray menu.
`-ReducedMotion` confirms that the same cached timing contract is initialized
without interpolation. `-QuickFlick` submits press, movement, and release
without an animation wait, proving that rendering is not a prerequisite for
selection. Every normal run also correlates the selected invocation with its
animated hide and checks the observed close lifecycle against the 50 ms target
with scheduler tolerance.
`smoke-delay-executor.ps1` creates a temporary schema-two configuration with a
50 ms Delay on every slice, runs the release executable through the same
gesture checks, requires one successful executor completion per invocation,
and removes the isolated data directory afterward. It never reads or writes
the user's Gradial configuration.
`smoke-shortcut-executor.ps1` opens a temporary normal-integrity receiver,
foregrounds it, and uses an isolated configuration to dispatch Ctrl+F13. The
receiver requires Ctrl down, F13 down/up, and Ctrl up in order, proving the
production target-validation and `SendInput` path without sending keys to the
user's applications. It then closes the receiver and removes all temporary
data.
`smoke-text-executor.ps1` uses the same isolated receiver and dispatches a
literal string containing environment-like tokens, BMP Unicode, and a
surrogate pair. It requires the exact UTF-16 message sequence, confirms the
tokens were not expanded, and searches Gradial's isolated log for a private
sentinel to prove the payload was not recorded.
`smoke-open-executor.ps1` creates a randomly named per-user test extension and
hidden receiver, dispatches an existing path containing environment-like and
command-shell metacharacters, and requires the exact literal path. It also
checks that the private path sentinel is absent from Gradial's isolated log.
Its `finally` block removes the association, receiver data, and isolated
configuration; it never changes a normal file association or opens one of the
user's applications.
`smoke-process-executor.ps1` compiles a private temporary `.exe`, assigns it
through an isolated configuration, and checks empty, spaced, quoted,
trailing-backslash, shell-character, environment-like, and Unicode arguments.
The receiver also confirms the exact configured working directory and
inherited environment. It remains alive after Gradial reports completion,
proving that the worker closed its handles without waiting for child exit. The
harness checks redacted metadata, stops only the exact receiver path, and
removes its isolated data.
`smoke-window-executor.ps1` starts four separate normal-integrity WinForms
targets and an isolated release configuration for each command. It observes
the exact `WM_CLOSE` or `WM_SYSCOMMAND`, verifies minimized/maximized/restored
state or normal close, and checks that a private target title is absent from
Gradial's log. Cleanup closes only the receiver processes it started and
removes the isolated configurations.
`smoke-action-feedback.ps1` assigns a deliberately missing private executable
through an isolated release configuration. One real M4 invocation must finish
with `path_not_found`, enter recent activity, and produce exactly one accepted
Windows failure-notification request. The shared interaction harness checks
the foreground application after notification submission, while the wrapper
confirms that neither its private path sentinel nor its configured argument
appears in the local log. The isolated configuration is always removed.
`smoke-settings-editor.ps1` starts the debug executable with a private
schema-two eight-slice configuration and opens its real settings viewport. It
seeds one canonical unreferenced PNG in managed storage, requires startup to
move it into quarantine, and compares SHA-256 hashes to prove the recoverable
move preserved its bytes. It opens and safely cancels the real native JSON
import/export and diagnostic-ZIP choosers, migrates
a schema-two fixture into the private draft, and confirms that neither the
source nor active `config.json` changes. It edits the activation draft to exact
Ctrl+M5, arms the safe tester, submits a
native hold/east-flick/release gesture, and requires that no production action
run during the test. It also edits the appearance draft to RGB `(224, 96, 72)`,
dark labels, and large size. It generates a 48×48 transparent PNG, imports it
through the real managed storage/cache path, assigns it to north, and
optionally captures the real production preview with the fitted icon and
contrast backings. It creates and
persists every leaf-action type plus a base and nested sequence, exercises
duplication, unreferenced deletion, sequence addition/removal/reordering, and
records an exact left-Ctrl+F14 combination through native key messages. It
also creates, duplicates, and deletes application profiles, round-trips one
profile through four/eight geometry, and retains an eight-slice menu with its
own nested-sequence assignment while Global later becomes four slices. That
profile enables exact window-class and title-regex predicates. The harness
runs the real three-second tester countdown, injects a deterministic private
target for resolver display, and verifies that its path, class, and title are
absent from the local log. The application fixture includes three exact
argument rows—one containing spaces and one empty—plus a working folder. The
harness opens the real native application chooser, observes its window, and
cancels it without changing the draft. It then selects the nested sequence and
observes four expanded steps, depth two, and 275 ms total Delay before
converting the Global draft to four slices. It exports that complete unsaved
draft, verifies schema six at the unchanged revision with the exact
appearance, icon reference, literal executable path, working folder, and
argument rows, verifies the referenced PNG was copied byte-for-byte into the
adjacent portable `icons` directory, and
again confirms that the active configuration is untouched. After the guarded close it requires
revision 2 on disk with the exact complete profile rule, priority, independent
menu, action payloads, ordered sequence UUIDs, Ctrl+M5 binding, appearance, and
the one shared managed icon source;
the old profile-free schema-two revision remains in `config.json.bak`. It then
previews a frozen diagnostic snapshot, reads a default ZIP to prove configured
application paths remain excluded, and reads an explicitly opted-in ZIP to
prove exactly one deduplicated executable path is included without arguments
or a working folder. It requires no diagnostic temporary debris. Finally, it
requires the renderer appearance update and a fresh Ctrl+M5 invocation to
freeze the saved binding and use the reloaded Global menu. Optional screenshots
cover appearance, icons, activation, the menu,
advanced profile tester, payload, sequence, shortcut-recorder, Windows chooser,
JSON import/export choosers, and confirmation surfaces. The fixture never reads or writes the user's
Gradial configuration.
`smoke-onboarding.ps1` starts the debug executable in a disposable
`LOCALAPPDATA` tree without a completion marker and requires the welcome
window. Its configuration assigns a real Delay action to every production
direction. The harness chooses and acknowledges exact Ctrl+M5, confirms the
atomic revision-2 binding reload, then sends native side-button gestures
through the live hook to select the east sample and cancel through the center.
It requires the onboarding progress events while rejecting every action
dispatch/completion record, proving that practice cannot reach the configured
actions beneath it. It completes the flow, validates the schema-one marker,
restarts Gradial against the same isolated data, and requires onboarding to
remain closed. Optional screenshots capture the binding and completed-practice
pages; cleanup closes only the process it started and removes the temporary
tree.

`smoke-start-at-login.ps1` uses a debug-only, unique
`GradialSmoke-<UUID>` value in the real current-user Run key. It drives the
native Settings window through Off, On, externally stale, repaired, and Off
again; requires one quoted argument-free executable command; checks that
logs contain neither the command nor the isolated value name; and confirms
that the production `Gradial` value is unchanged. Cleanup removes only that
unique test value and its disposable local-data tree, including after
failure. The release build ignores the test-name environment hook.

For matrix validation, first build the selected configuration, then run one of
the versioned suites:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-interaction-matrix.ps1 -Suite quick
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-interaction-matrix.ps1 -Suite standard
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-interaction-matrix.ps1 -Suite extended
```

The quick suite is a short regression subset. Standard adds four-slice center
coverage, all four corners and edge midpoints, per-monitor routing, topology,
and recovery cases. Extended defaults to ten repetitions of all eight
directions from all four corners, yielding 320 automated repetitions, and
raises the rapid-invocation count to 100. Counts can be set explicitly with
`-CornerTrialsPerDirection` and `-RapidInvocationCount`. Automated repetitions
do not measure the plan's human direction-error or selection-time gates.

Each run creates an ignored timestamped directory under
`captures\interaction-matrix` containing `interaction-matrix.json`,
`scenarios.csv`, and `coverage.csv`. The JSON starts with `schema_version: 1`
and includes host/display metadata, exact scenario arguments, captured output,
exit codes, timings, and a coverage table. Missing mixed-DPI, negative-origin,
or high-refresh hardware is reported as `unavailable`; physical events and
long-run campaigns that were not performed are reported as `not_run`.

## Checks

`scripts\check.ps1` runs, in order:

1. PowerShell syntax checks for every script in `scripts`.
2. The deterministic performance-report percentile/QPC analyzer self-test.
3. `cargo fmt --all --check`
4. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
5. `cargo test --workspace --all-features`
6. `cargo build --workspace --all-features`

The script stops on the first failure.

The harness makes the plan's automated interaction checks repeatable and can
produce a 320-repetition corner stress dataset, but it cannot create absent
hardware or replace physical state changes and the required participant
validation. The 1,000-invocation presented-frame latency run,
10,000-invocation focus campaign, and remaining hardware/human datasets stay
separate release gates.
