# Gradial

Gradial is a Windows 11 radial launcher built around a hold, flick, and release
gesture. The current development build includes the first complete interaction
spine: a non-activating GPU overlay, dedicated low-level input thread, bounded
event transport, and a tested eight-slice gesture state machine.

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
- Move beyond the center zone to highlight one of eight slices.
- Release M4 to log the selected slice and close the menu.
- Press Escape, or press another mouse button, to cancel.

**M4 is dedicated to Gradial while the app runs.** Its normal browser
back/navigation behavior is suppressed. This milestone deliberately does not
execute launcher actions; release only writes `slice_selected` with
`action_executed=false` to the local log.

The overlay is click-through and never intentionally takes focus. Stop the
debug process with `Ctrl+C` in the terminal or the stop button in VS Code.

Run all local checks with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check.ps1
```

See [`Documentation/development.md`](Documentation/development.md) for the
threading model, safety boundaries, workspace layout, and current limitations.
