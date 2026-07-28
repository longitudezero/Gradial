# Gradial

Gradial is a Windows 11 radial launcher built around a hold, flick, and release
gesture. The current code is the application-foundation milestone: it creates a
GPU-rendered, transparent, non-activating overlay and establishes the reusable
core, local diagnostics, tests, and developer checks.

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

The preview window is deliberately click-through and does not take focus. Stop
the debug process with `Ctrl+C` in the terminal or the stop button in VS Code.

Run all local checks with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check.ps1
```

See [`Documentation/development.md`](Documentation/development.md) for the
workspace layout and current boundaries.

