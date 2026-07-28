# Gradial development guide

## Current milestone scope

The foundation and M4 interaction-spine milestones establish:

- A Rust 2024 workspace pinned to the installed Rust 1.97.1 toolchain.
- A reusable `gradial-core` crate.
- A Windows `gradial` executable rendered by egui through wgpu.
- A transparent, borderless, topmost, click-through overlay.
- Per-Monitor V2 DPI awareness through the embedded application manifest.
- `%LOCALAPPDATA%\Gradial` directories for configuration, icons, and bounded
  local logs.
- Formatting, Clippy, unit-test, and build checks.
- A dedicated Win32 low-level mouse/keyboard hook thread.
- M4 as a suppressed development trigger and Escape as cancellation.
- A fixed-capacity, single-producer/single-consumer input queue.
- An explicit, pure gesture state machine with eight slices and hysteresis.
- Hidden-at-idle overlay lifecycle, physical-pixel positioning, and selection
  highlighting.

It intentionally does not create a tray icon, save configuration, validate
targets, or execute actions. A release logs the chosen slice but cannot launch
anything.

## Workspace layout

```text
Gradial APP\
    Cargo.toml
    crates\
        gradial-core\    Pure geometry, gesture state, and application paths
        gradial-app\     Windows input, executable, and overlay renderer
    scripts\
        check.ps1        Complete local verification
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
| Win32 hook thread | Classify M4, movement, Escape, and cancellation input | No, inside callbacks |
| Interaction thread | Snapshot invocation state and advance the pure gesture controller | Yes, outside callbacks |
| eframe UI thread | Move, show, paint, and hide the overlay | Yes |

The hook callback copies fixed-size events into an `rtrb` SPSC ring buffer with
a capacity of 4,096. It never logs, waits, paints, takes a contended lock, or
calls action code. Overflow sets an atomic fault flag. The interaction thread
then closes any active invocation and enters a fail-safe fault state; restart
the development build to recover.

The interaction-to-UI channel is bounded to 64 commands. Selection repaint
commands can be dropped with an explicit log if the UI falls behind. Open and
close lifecycle commands receive one bounded 50 ms drain opportunity so menu
visibility remains balanced without ever delaying the hook callback.

The hook thread owns a 250 ms message-loop heartbeat. An active invocation is
faulted when the heartbeat is more than 750 ms old. On graceful shutdown,
Gradial logs the maximum observed hook-callback duration.

M4 down and its matching release are suppressed while Gradial runs. Pointer
movement passes through normally. During an invocation, Escape and another
mouse-button click cancel the gesture; their matching releases are suppressed
to avoid leaking half of a click. Events stamped with Gradial's private
injection marker are passed through and never interpreted as activation.

## Gesture contract

The pure controller uses physical cursor deltas but scales its thresholds by
the invocation monitor's effective DPI:

- center cancellation radius: 24 DIP;
- initial selection radius: 36 DIP;
- boundary hysteresis: 5 degrees;
- eight slices, with north at index 0 and indices increasing clockwise;
- release in the center/no-selection zone cancels;
- Escape closes once, then waits for M4 release before returning to idle.

Each invocation captures a UUID, QPC timestamp, foreground window handle and
process ID, cursor origin, and effective monitor DPI. These immutable fields
are the beginning of the full v0.2 invocation snapshot; target identity and
profile resolution belong to a later milestone.

## Overlay guarantees in this milestone

The root viewport starts hidden and is:

- transparent and borderless;
- rendered with wgpu;
- topmost and omitted from the taskbar;
- initially inactive;
- configured with `WS_EX_NOACTIVATE` and `WS_EX_TOOLWINDOW`;
- shown and repositioned with no-activation flags;
- returning `MA_NOACTIVATE` for `WM_MOUSEACTIVATE`;
- returning `HTTRANSPARENT` for `WM_NCHITTEST`.

eframe/winit finalizes some extended styles after constructing the application.
Gradial therefore applies and reads back the required styles during construction
and the first two logic passes. Logic passes run when the hidden window receives
a cross-thread repaint request. It does no style polling after the window
reaches its steady state.

This is one development preview window, not the final per-monitor pool. The
production core will prewarm one overlay per connected monitor and keep them
hidden until activation.

## Manual interaction check

1. Start `cargo run --package gradial-app`. No menu should be visible.
2. Put another application in the foreground and note that it has keyboard
   focus.
3. Hold M4. The menu should appear centered on the cursor without changing
   the foreground application.
4. Move east. The `E` slice should highlight.
5. Release M4. The menu should hide; no application should launch.
6. Hold M4 again and press Escape. The menu should hide, and Escape should not
   reach the foreground application.
7. Stop Gradial and inspect
   `%LOCALAPPDATA%\Gradial\logs\gradial_rCURRENT.log` for
   `hook_thread_responsive`, `invocation_started`, `slice_selected` or
   `invocation_cancelled`, and `hook_thread_stopped`.

This is a global input development build. Save important work before manual
testing. If the overlay or hook becomes unresponsive, stop Gradial from its
terminal or the VS Code debug controls.

## Checks

`scripts\check.ps1` runs, in order:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --workspace --all-features`
4. `cargo build --workspace --all-features`

The script stops on the first failure.
