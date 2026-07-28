# Gradial

Gradial is a Windows 11 radial launcher built around a hold, flick, and release
gesture. The current development build includes the first complete interaction
spine and the first bounded Milestone 1 slice: a prewarmed, non-activating GPU
overlay for every connected display; dedicated low-level input thread; bounded
event transport; tested four/eight-slice gesture state machine; automatic edge
assist; display/DPI rebuild handling; and focus/timing instrumentation.

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

`--slices 4` and `--slices 8` are also accepted. This startup-only switch is
temporary; persisted per-profile selection remains part of the profiles
milestone.

**M4 is dedicated to Gradial while the app runs.** Its normal browser
back/navigation behavior is suppressed. This milestone deliberately does not
execute launcher actions; release only writes `slice_selected` with
`action_executed=false` to the local log.

The display under the M4-down cursor selects the prewarmed overlay window.
Display, work-area, or DPI changes cancel an active gesture and rebuild the
window pool before M4 is accepted again. Every overlay is click-through and
never intentionally takes focus. Stop the debug process with `Ctrl+C` in the
terminal or the stop button in VS Code.

Run all local checks with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check.ps1
```

See [`Documentation/development.md`](Documentation/development.md) for the
threading model, safety boundaries, timing capture, workspace layout, and
current limitations. The latest bounded interaction evidence is recorded in
[`Documentation/milestone-0-smoke-results.md`](Documentation/milestone-0-smoke-results.md),
and the per-display pool evidence is in
[`Documentation/milestone-1-overlay-pool-results.md`](Documentation/milestone-1-overlay-pool-results.md).
Four/eight geometry results are recorded in
[`Documentation/milestone-1-slice-geometry-results.md`](Documentation/milestone-1-slice-geometry-results.md).
