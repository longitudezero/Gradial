# Gradial

Gradial is a Windows 11 radial launcher built around a hold, flick, and release
gesture. The current development build includes the first complete interaction
spine and several bounded Milestone 1 slices: a prewarmed, non-activating GPU
overlay for every connected display; dedicated low-level input thread; bounded
event transport; tested four/eight-slice gesture state machine; automatic edge
assist; display/DPI rebuild handling; input-hook recovery; a native health tray
icon; cached overlay geometry; bounded animations; and focus/timing
instrumentation. A versioned interaction-matrix harness now covers repeatable
direction, corner/edge, rapid-use, focus, display-routing, topology, and
recovery scenarios. The first Milestone 2 foundation adds schema-versioned
configuration, atomic backup recovery, complete invocation target identity,
and deterministic application-profile resolution.

The implementation baseline is
[`gradial-v0.2-implementation-plan.md`](gradial-v0.2-implementation-plan.md).
The broader v0.1 document is retained for historical context, but v0.2 controls
the MVP.

## Build and run

Requirements:

- Windows 11 x86-64.
- Rust 1.97.1 with the MSVC target.
- Visual Studio Build Tools with the Windows SDK.

From `Gradial APP`:

```powershell
cargo run --package gradial-app
```

The app starts hidden. While this development build is running:

- Hold mouse button 4 (M4) to open the menu at the cursor.
- Move beyond the center zone to highlight a slice.
- Release M4 to log the selected slice and close the menu.
- Press Escape, or press another mouse button, to cancel.

Gradial normally keeps the gesture fixed at M4-down. If the complete ring would
clip the current monitor's work area, it automatically insets the ring and uses
raw mouse deltas as a virtual pointer. The system cursor stays at the invocation
point during that edge-assisted gesture.

The two Milestone 0 prototypes can also be forced for comparison:

```powershell
cargo run --package gradial-app -- --geometry=fixed-origin
cargo run --package gradial-app -- --geometry=edge-assist
```

Eight slices are the default. The current development override also exercises
the four-slice N/E/S/W geometry end to end:

```powershell
cargo run --package gradial-app -- --slices=4
```

`--slices 4` and `--slices 8` are also accepted. This startup-only switch
continues to control visible development geometry while profile-menu rendering
is introduced in the next bounded slice.

The overlay opens from 96% scale and 85% opacity over 60 ms, changes selection
color over 35 ms, and fades out over 50 ms. Gesture recognition and release do
not wait for those animations. A temporary development override removes visual
interpolation while preserving the same lifecycle timing:

```powershell
cargo run --package gradial-app -- --reduced-motion
```

**M4 is dedicated to Gradial while the app runs.** Its normal browser
back/navigation behavior is suppressed. This milestone deliberately does not
execute launcher actions; release only writes `slice_selected` with
`action_executed=false` to the local log.

The display under the M4-down cursor selects the prewarmed overlay window.
Display, work-area, or DPI changes cancel an active gesture and rebuild the
window pool before M4 is accepted again. Every overlay is click-through and
never intentionally takes focus.

Gradial keeps a notification-area icon visible while it runs. Its icon and
first menu line show `Healthy`, `Recovering input`, or `Input faulted`. A missed
input heartbeat triggers automatic recovery after 750 ms. Queue overflow and
startup failures remain faulted with M4 passed through until **Retry input** is
chosen. The same menu provides **Exit Gradial**. Input is also re-registered
after resume, session unlock, or an Explorer restart.

First startup now creates `%LOCALAPPDATA%\Gradial\config.json` with schema 1,
revision 1, and a global eight-slice profile. Configuration replacement is
validated and atomic, preserves the previous valid primary as
`config.json.bak`, and recovers from that backup without overwriting a broken
primary. Application-profile rules are resolved at every activation, but
actions and the settings editor are not present yet.

Run all local checks with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check.ps1
```

Run the standard live interaction matrix after building the debug executable:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-interaction-matrix.ps1 -Suite standard
```

It writes ignored, versioned JSON and CSV results under
`Gradial APP\captures\interaction-matrix`.

See [`Documentation/development.md`](Documentation/development.md) for the
threading model, safety boundaries, timing capture, workspace layout, and
current limitations. The latest bounded interaction evidence is recorded in
[`Documentation/milestone-0-smoke-results.md`](Documentation/milestone-0-smoke-results.md),
and the per-display pool evidence is in
[`Documentation/milestone-1-overlay-pool-results.md`](Documentation/milestone-1-overlay-pool-results.md).
Four/eight geometry results are recorded in
[`Documentation/milestone-1-slice-geometry-results.md`](Documentation/milestone-1-slice-geometry-results.md).
Input recovery and tray results are recorded in
[`Documentation/milestone-1-input-recovery-results.md`](Documentation/milestone-1-input-recovery-results.md).
Renderer cache and animation results are recorded in
[`Documentation/milestone-1-renderer-animation-results.md`](Documentation/milestone-1-renderer-animation-results.md).
The repeatable interaction-matrix contract and current coverage are recorded in
[`Documentation/milestone-1-interaction-matrix-results.md`](Documentation/milestone-1-interaction-matrix-results.md).
Configuration, snapshot, persistence, and resolver results are recorded in
[`Documentation/milestone-2-configuration-profile-results.md`](Documentation/milestone-2-configuration-profile-results.md).
