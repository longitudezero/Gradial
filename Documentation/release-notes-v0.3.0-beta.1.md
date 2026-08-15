# Gradial 0.3.0-beta.1

This is Gradial's first public beta: a fast, configurable radial launcher for
Windows 11.

## Install

- `Gradial-0.3.0-beta.1-Windows-x86_64-Setup.exe` installs for the current user
  without administrator access and adds a Start menu shortcut.
- `Gradial-0.3.0-beta.1-Windows-x86_64-portable.zip` can be extracted anywhere
  you control. Run `gradial.exe` from the extracted folder.
- `SHA256SUMS.txt` contains the SHA-256 hashes for both downloads.

The executable and installer are **not digitally signed**. Windows may display
an Unknown publisher or Microsoft Defender SmartScreen warning. Download this
beta only from the official `longitudezero/Gradial` repository.

## Highlights

- Four- and eight-direction radial menus activated with M4 or M5.
- Application-specific profiles and a visual settings editor.
- Keyboard, text, URL, file/folder, executable, window, delay, and sequence
  actions.
- DPI-aware per-monitor overlays with edge/corner assistance and custom icons.
- Hybrid continuous input for plugin-defined controls such as volume, seeking,
  and brush size.
- Local Lua plugins with explicit trust, permissions, bounded execution, and
  four example packages.
- Privacy-bounded local diagnostics, tray health/recovery, and no telemetry.

## Beta boundaries

Windows 11 x86-64 is the supported platform. Updates and plugin installation
are manual. The remaining physical high-refresh, mixed-DPI, Windows-state,
participant, and long-duration campaigns are deferred to stable-release
qualification.

Read the bundled known-limitations document before relying on Gradial for
important workflows. Please report ordinary bugs in GitHub Issues and use the
private vulnerability-reporting form for security-sensitive findings.
