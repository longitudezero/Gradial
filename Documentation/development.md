# Gradial development guide

## Foundation scope

This milestone establishes:

- A Rust 2024 workspace pinned to the installed Rust 1.97.1 toolchain.
- A reusable `gradial-core` crate.
- A Windows `gradial` executable rendered by egui through wgpu.
- A transparent, borderless, topmost, click-through overlay.
- Per-Monitor V2 DPI awareness through the embedded application manifest.
- `%LOCALAPPDATA%\Gradial` directories for configuration, icons, and bounded
  local logs.
- Formatting, Clippy, unit-test, and build checks.

It intentionally does not install global input hooks, create a tray icon, save
configuration, or execute actions. Those features require their own testable
milestones and must not be coupled to this preview window.

## Workspace layout

```text
Gradial APP\
    Cargo.toml
    crates\
        gradial-core\    Pure geometry and application paths
        gradial-app\     Windows executable and overlay renderer
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

Logs are local and rotate at 1 MiB. At most five log files are retained. The
foundation logs component and event codes only; it does not log window titles,
typed text, executable arguments, clipboard contents, or selected files.

## Overlay guarantees in this milestone

The root viewport is:

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
and the first two UI passes. It does no style polling after the window reaches
its steady state.

This is one development preview window, not the final per-monitor pool. The
production core will prewarm one overlay per connected monitor and keep them
hidden until activation.

## Checks

`scripts\check.ps1` runs, in order:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --workspace --all-features`
4. `cargo build --workspace --all-features`

The script stops on the first failure.
