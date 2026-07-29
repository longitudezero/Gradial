# Gradial development guide

## Current milestone scope

The foundation, Milestone 0 interaction spike, and current bounded Milestone 1
slices establish:

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

It intentionally does not save configuration, validate targets, or execute
actions. A release logs the chosen slice but cannot launch anything.

## Workspace layout

```text
Gradial APP\
    Cargo.toml
    crates\
        gradial-core\    Pure geometry, gesture state, and application paths
        gradial-app\     Windows input, executable, and overlay renderer
    scripts\
        check.ps1          Complete local code verification
        check-focus.ps1    Validate the latest completed focus trace
        capture-timing.ps1 Capture ETW markers and presented-frame timings
        smoke-interaction.ps1 Exercise a real hidden app with synthetic M4 input
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
    icons\
    logs\
```

The application does not create `config.json` yet. Configuration persistence
will be introduced with atomic replacement and backup recovery as required by
the v0.2 plan.

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

Each invocation captures a UUID, QPC timestamp, foreground window handle and
process ID, cursor origin, effective monitor DPI, resolved geometry behavior,
and visual ring origin. These immutable fields are the beginning of the full
v0.2 invocation snapshot; target identity and profile resolution belong to a
later milestone.

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

Eight slices are the default. `--slices=4` selects a cardinal N/E/S/W ring, and
`--slices=8` selects N/NE/E/SE/S/SW/W/NW. Both counts use the same radii and
five-degree boundary hysteresis; only the angular width, labels, selection
indices, and highlight arc change. This is a development override rather than
persistent configuration:

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

Each child owns a persistent renderer. During pool construction it caches that
slice count's boundary segments, label offsets, and 24-segment highlight arcs.
Rendering reuses those structures; display/DPI pool replacement constructs one
new cache for each replacement child. Selection animation uses fixed-size
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
moves the cursor, starts the hidden debug executable, sends one synthetic M4
gesture, verifies foreground identity and edge cursor retention, restores the
cursor, and closes only the process it started:

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

## Checks

`scripts\check.ps1` runs, in order:

1. PowerShell syntax checks for every script in `scripts`.
2. `cargo fmt --all --check`
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
4. `cargo test --workspace --all-features`
5. `cargo build --workspace --all-features`

The script stops on the first failure.

The prototype helpers make the plan's measurements repeatable, but they do not
replace the required hardware matrix, 1,000-invocation latency runs, 320 corner
trials, or 10,000-invocation focus campaign. Those release-gate datasets remain
future validation work.
