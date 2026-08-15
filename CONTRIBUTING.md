# Contributing to Gradial

Thank you for helping improve Gradial. Bug reports, usability feedback,
documentation corrections, plugins, and focused code changes are welcome.

## Before opening an issue

- Search existing issues first.
- Confirm the behavior on the newest beta when practical.
- Read the supported boundaries in
  [Known limitations](Documentation/known-limitations-and-release-readiness.md).
- Do not publish private window titles, typed text, paths, plugin parameters,
  tokens, or other sensitive data.

Use GitHub's private vulnerability reporting for security-sensitive findings.

## Development setup

Gradial currently builds on 64-bit Windows 11 with Rust 1.97.1, the MSVC
toolchain, Visual Studio Build Tools, and a Windows SDK.

```powershell
cd "Gradial APP"
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check.ps1
```

The check validates PowerShell harness syntax, analyzer self-tests, formatting,
Clippy with warnings denied, all workspace tests, and a debug build. Run the
relevant production-route smoke or matrix script for behavior that crosses a
Windows boundary.

## Pull requests

- Keep changes narrowly scoped and explain the user-visible behavior.
- Add or update tests for deterministic logic and failure cases.
- Preserve input-hook, focus, target-integrity, synthesized-key cleanup,
  bounded-queue, privacy, and atomic-save guarantees.
- Update user documentation when behavior or a supported boundary changes.
- Do not weaken an enforced safety limit merely to make a test pass.

By contributing, you agree that your contribution is licensed under the MIT
License covering this repository.
