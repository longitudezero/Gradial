# Gradial development guide

## Current milestone scope

The foundation, Milestone 0 interaction spike, completed bounded Milestone 1
slices, and the first two bounded Milestone 2 slices establish:

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
- M4 as a suppressed development trigger and Escape as cancellation.
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
- Persistent per-monitor geometry caches and bounded 60/35/50 ms
  open/selection/close animation state.
- A validated schema-one configuration with atomic primary/backup replacement.
- Complete immutable target/display/modifier identity for every invocation.
- A compiled, deterministic application-profile resolver with explanation
  data.
- One immutable resolved menu per invocation, shared by gesture recognition
  and rendering.
- Prewarmed four/eight geometry caches on every display and rendering of
  configured profile labels.

It intentionally does not expose configuration editing, perform release-time
action-target validation, or execute actions. A release logs the chosen slice
and resolved profile but cannot launch anything.

## Workspace layout

```text
Gradial APP\
    Cargo.toml
    crates\
        gradial-core\    Pure geometry, gesture state, and application paths
        gradial-app\     Windows input, persistence, executable, and renderer
    scripts\
        check.ps1          Complete local code verification
        check-focus.ps1    Validate the latest completed focus trace
        capture-timing.ps1 Capture ETW markers and presented-frame timings
        smoke-interaction.ps1 Exercise a real hidden app with synthetic M4 input
        run-interaction-matrix.ps1 Run the versioned validation suites
```

`gradial-core` must remain free of UI and Windows windowing dependencies. Pure
gesture geometry, configuration models, profile resolution, and action-plan
validation belong there as they are introduced.

`gradial-app` owns Windows integration and rendering. Its renderer is behind the
small `OverlayRenderer` interface so the production interaction code does not
depend on egui painting details.

## Local files

On first run, Gradial creates:

```text
%LOCALAPPDATA%\Gradial\
    config.json
    icons\
    logs\
```

`config.json` starts at schema 1 and revision 1 with the stable global profile
and an eight-slice label-only menu. A successful later replacement preserves
the previous valid primary as `config.json.bak`; `config.json.tmp` exists only
during a save. The current bounded schema deliberately has no action
definitions or references.

Configuration is parsed and completely validated before input starts. Profile
regular expressions are compiled into the retained resolver at load rather
than during invocation. An invalid primary loads the backup without rewriting
either file. If neither file is usable, Gradial runs the built-in default in
memory, retains the broken files, and writes a `config_recovery_warning`. The
settings editor will surface that same warning visually when it is introduced.

Logs are local and rotate at 1 MiB. At most five log files are retained.
Gradial uses its own filter so a host-level `RUST_LOG` setting cannot silence
lifecycle and safety events. Gradial-authored records use component and event
codes; dependency warnings may also appear. It does not log window titles,
typed text, executable arguments, clipboard contents, or selected files.

## Input and interaction architecture

Gradial uses three execution contexts:

| Context | Responsibility | May block or allocate? |
| --- | --- | --- |
| Win32 input thread | Classify M4, raw deltas, movement, Escape, and cancellation input | No, inside callbacks |
| Interaction thread | Snapshot invocation state and advance the pure gesture controller | Yes, outside callbacks |
| eframe UI thread | Maintain the hidden controller and paint independent per-display viewports | Yes |

The input thread owns the low-level hooks, heartbeat timer, and a message-only
window registered for raw mouse input. Both event sources execute serially on
that one thread and feed one producer. The hook callback copies fixed-size
events into an `rtrb` SPSC ring buffer with a capacity of 4,096. It never logs,
waits, paints, takes a contended lock, enumerates displays, or calls action
code. Display work areas are immutable for the lifetime of one hook runtime.
When topology changes, an atomic gate first makes new M4 presses pass through,
then the interaction thread closes any active invocation, the old hook runtime
stops, and a replacement starts with the newly enumerated displays. Overflow
sets an atomic fault flag. The interaction thread then closes any active
invocation, closes the complete input runtime, and enters a fail-safe fault
state. M4 passes through while faulted; recovery requires **Retry input** from
the tray menu.

At M4-down, the interaction thread maps the signed physical cursor coordinate
to the same monitor ordering used by the overlay pool. It updates only that
viewport's small shared presentation state and requests that child viewport's
independent repaint. Open and close also post private native messages directly
to the selected window, so they do not wait for the hidden controller's
throttled repaint cycle. None of this work occurs in a low-level hook callback.

The hook thread owns a 250 ms message-loop heartbeat. The interaction thread
checks it whether or not a gesture is active. Once it is more than 750 ms old
(three missed intervals), new M4 presses are immediately passed through, any
active invocation is cancelled, and the entire hook/raw-input/queue/consumer
runtime is replaced. If replacement cannot start, Gradial remains faulted until
manual retry. On graceful shutdown, Gradial logs the maximum observed
hook-callback duration.

The tray health state is `Recovering input` during startup, input replacement,
and display rebuild; `Healthy` only after a new hook thread reports responsive;
and `Input faulted` after queue overflow or a failed start. Its menu contains
the non-interactive current state, **Retry input**, and **Exit Gradial**.
Explorer's `TaskbarCreated` message restores the icon and re-registers input.
Power-resume and current-session unlock notifications also re-register the
complete runtime. These signals do not silently clear a queue-overflow fault.

M4 down and its matching release are suppressed while Gradial runs. In
fixed-origin mode, pointer movement passes through normally and absolute cursor
positions drive the gesture. In edge-assist mode, low-level movement is
suppressed so the system cursor remains at M4-down; raw mouse deltas accumulate
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
- Escape closes once, then waits for M4 release before returning to idle.

Each invocation captures a UUID and source QPC timestamp plus immutable target,
display, input, and configuration identity. The target fields are foreground
window handle/PID, process creation time, optional full executable path, window
class, and window title. Display fields are signed cursor coordinate, monitor
device name, monitor rectangle, work area, effective DPI, resolved geometry,
and visual origin. Ctrl/Alt/Shift state is captured in the low-level
button-down event along with the fixed development binding ID. The
configuration revision is frozen before profile selection.

Only the fixed-size modifier bits are added by the hook callback. Process,
window, monitor, configuration, and profile work remains on the interaction
thread. Executable paths, classes, and titles are held for matching but never
written to routine logs.

## Configuration and profile resolution

The current action-free configuration model contains one global profile and
zero or more application profiles. Every profile has an immutable UUID, name,
and complete four- or eight-slice label menu. Application rules require an
executable path and may add window-class and title-regex predicates with an
explicit priority from 0 through 100.

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
gesture recognition and chooses the matching renderer cache; its label list is
passed to the same frame. A title change after activation therefore cannot
switch profiles or produce a gesture/render mismatch.

## Edge behavior

The default `--geometry=auto` mode uses fixed origin whenever a 288 DIP square
centered at M4-down fits in the monitor work area. Otherwise it selects edge
assist and clamps the visual origin far enough inward for the complete overlay
to fit. Work-area coordinates remain signed, so monitors above or left of the
primary display are handled without unsigned conversion.

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

`--reduced-motion` removes scale, opacity, and selection-color interpolation
without changing recognition or close timing. It is a temporary startup
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
root-controller repaint cycle.

Each child owns a persistent renderer. During pool construction it caches both
four- and eight-slice boundary segments, label offsets, and 24-segment
highlight arcs. Rendering selects one cache from the frozen menu and pairs its
offsets with the configured labels; no geometry or profile matching occurs on
activation's UI path. Display/DPI pool replacement constructs one complete
cache pair for each replacement child. Selection animation uses fixed-size
eight-entry state and can retarget from its current color without rebuilding
geometry.

`WM_DISPLAYCHANGE`, `WM_DPICHANGED`, and `WM_SETTINGCHANGE` for the work area
invalidate the pool. Monitoring is deliberately disabled while windows are
being placed, because initial PMv2 placement can itself emit DPI messages. A
real change suspends M4, cancels an open invocation with
`DisplayTopologyChange`, hides the routed child, destroys the old runtime/pool,
enumerates signed work areas again, and prewarms a new generation. Input resumes
only after all new child windows report the expected DPI.

## Animation contract

Opening starts with the complete usable ring at 96% scale and 85% opacity, then
uses cubic ease-out to reach its final state at 60 ms. A selected slice fades
between its old and new color over 35 ms. Selection retargeting samples the
current transition before starting the next one, avoiding a flash or restart
from zero.

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
child-viewport frame checkpoint, and immediately after hiding the overlay. A checkpoint passes
when the same window remains foreground or when the target process itself
changed its foreground window. Gradial never calls an API that activates or
restores the foreground window.

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

## Manual interaction check

1. Start `cargo run --package gradial-app`. No overlay should be visible; the
   Gradial notification-area icon should report Healthy.
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
10. Open the tray icon and confirm its status line, **Retry input**, and
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
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -QuickFlick
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -ActivationPoint top-left -AllDirections
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -ActivationPoint center -Direction SW -InvocationCount 20 -QuickFlick -InterInvocationDelayMilliseconds 10
```

Add `-CaptureTiming` to the second command to run the documented ETW and
PresentMon capture around the synthetic gesture. In a non-elevated development
shell, add `-TimingMarkersOnly`. Pass `-Configuration release` after building
with `cargo build --release` for performance-oriented checks. `-MonitorIndex`
selects an entry from Windows' current display list. `-Slices 4` or `-Slices 8`
selects and validates the corresponding startup geometry.
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
2. `cargo fmt --all --check`
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
4. `cargo test --workspace --all-features`
5. `cargo build --workspace --all-features`

The script stops on the first failure.

The harness makes the plan's automated interaction checks repeatable and can
produce a 320-repetition corner stress dataset, but it cannot create absent
hardware or replace physical state changes and the required participant
validation. The 1,000-invocation presented-frame latency run,
10,000-invocation focus campaign, and remaining hardware/human datasets stay
separate release gates.
