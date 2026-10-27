<p align="center">
  <img src="assets/gradial-logo.png" width="128" alt="Gradial logo">
</p>

# Abandoned project. If you want to pick it up and continue it, feel free to do so. 

# Gradial

Gradial is a fast, configurable radial launcher for Windows 11. Hold a mouse
side button, move toward an action, and release. Menus can change automatically
for different applications, and optional continuous actions turn distance into
a smooth `0–100%` value for local Lua plugins.

> [!WARNING]
> Gradial 0.3 is public beta software. Builds are currently unsigned, so
> Windows SmartScreen may show an unknown-publisher warning. Review the known
> limitations before installing, and keep an export of any configuration you
> do not want to recreate.

## Download

Download the latest installer or portable ZIP from
[GitHub Releases](https://github.com/longitudezero/Gradial/releases).

- **Installer:** installs for the current Windows user without administrator
  access and adds Gradial to the Start menu.
- **Portable ZIP:** extract it to a folder you control and run `gradial.exe`.

Gradial supports 64-bit Windows 11. It does not support Windows 10, Windows on
ARM, macOS, or Linux in this beta.

## What it does

- Four- or eight-direction radial menus activated with M4 or M5.
- Application-specific profiles with deterministic matching.
- Keyboard shortcuts, Unicode text, URLs, files and folders, direct executable
  launch, window actions, delays, and bounded sequences.
- Edge and corner assistance without moving the visible system pointer.
- Per-monitor, DPI-aware overlays with live visual customization and icons.
- Hybrid continuous wedges for volume, seeking, brush size, and other
  plugin-defined controls.
- Local Lua plugins with explicit trust, permissions, time limits, memory
  limits, and bounded host capabilities.
- Local diagnostics with a privacy preview and no telemetry or cloud account.

## First run

Gradial starts with a short onboarding flow:

1. Choose M4 or M5 and any required modifier keys.
2. Acknowledge that the chosen chord is dedicated to Gradial while it runs.
3. Practice selecting and cancelling the menu.
4. Open **Settings** from the notification-area icon to create actions and
   assign them to menu directions.

The default configuration is stored under `%LOCALAPPDATA%\Gradial`. Installer
upgrades and uninstalling the application do not delete that user data.

## Plugins

Gradial 0.3 plugins are local Lua source packages for continuous actions. They
are intentionally narrow: plugins receive normalized input and can request
only explicitly approved shortcuts, notifications, or allowlisted loopback
HTTP calls.

Lua containment is defense in depth, not a promise that hostile downloaded
code is safe. Inspect or otherwise trust a plugin before enabling it.

- [Plugin author guide](Documentation/plugin-author-guide.md)
- [Plugin manifest reference](Documentation/plugin-manifest-reference.md)
- [Continuous API v1](Documentation/continuous-api-v1-reference.md)
- [Trust and capabilities](Documentation/plugin-trust-and-capabilities-guide.md)

Four example plugins are included in
[`Gradial APP/examples/plugins`](Gradial%20APP/examples/plugins).

## Privacy and safety

Gradial has no telemetry, analytics, account, automatic update, or crash
upload. Logs remain local and exclude typed text, executable arguments, window
titles, plugin parameters, and capability payloads. A diagnostic bundle is
created only when requested and is shown for review before export.

The beta intentionally cannot send shortcuts or text to a higher-integrity
administrator application. Exclusive-fullscreen and anti-cheat-protected
applications are unsupported.

See [Known limitations and release readiness](Documentation/known-limitations-and-release-readiness.md)
for the complete boundary and deferred stable-release qualification work. The
[public beta policy](Documentation/public-beta-release-policy.md) explains the
smaller gate applied to prerelease builds.

## Feedback

- [Report a bug](https://github.com/longitudezero/Gradial/issues/new?template=bug_report.yml)
- [Suggest an improvement](https://github.com/longitudezero/Gradial/issues/new?template=feature_request.yml)
- For security-sensitive reports, use the repository's private
  **Report a vulnerability** form instead of a public issue.

Please attach a Gradial diagnostic bundle when it is safe and relevant. Preview
the bundle first and remove anything you do not want to publish.

## Build from source

Requirements:

- Windows 11 x86-64.
- Rust 1.97.1 with the MSVC toolchain.
- Visual Studio Build Tools and a Windows SDK.

```powershell
cd "Gradial APP"
cargo build --workspace --all-features --release --locked
```

The executable is written to `Gradial APP\target\release\gradial.exe`. Run the
complete local verification with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check.ps1
```

The detailed implementation and test chronicle is retained in
[DEVELOPMENT.md](DEVELOPMENT.md).

## License

Gradial is free and open-source software licensed under the [MIT License](LICENSE).
