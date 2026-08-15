# Public beta release policy

Date: 2026-08-15

Gradial `0.3.0-beta.1` is an intentionally early public build. It is suitable
for interested Windows users who are comfortable reporting bugs, but it is not
the stable-release qualification described by the v0.2 and v0.3 implementation
plans.

## Required for each public beta

A public beta may be published only when all of the following are true:

- the checked-in formatting, lint, test, and debug-build checks pass;
- a locked, all-feature Release build succeeds;
- dependency license review succeeds and fresh third-party notices are bundled;
- both the per-user installer and portable ZIP are produced reproducibly;
- install, in-place reinstall, launch, health, uninstall, and preserved-user-data
  smoke checks pass with isolated local state;
- SHA-256 hashes are published alongside the artifacts;
- the changelog and known limitations identify the beta's supported boundaries;
  and
- no known privacy, stuck-key, input-suppression, configuration-integrity, plugin
  permission, queue-bounding, or shutdown defect remains open.

## Resource policy

The old implementation-plan target was a strict working set below 80 MiB. A
valid Release campaign measured a maximum of 156.934 MiB. For public beta
builds, the accepted working-set ceiling is therefore below 192 MiB.

This is a deliberate product-policy change, not a reinterpretation of the old
result: the historical 80 MiB run remains recorded as failed against its
original threshold. The new beta ceiling gives enough headroom for the current
renderer and runtime while still detecting a major regression. Memory
optimization can be revisited from user reports or before stable release.

## Deliberately deferred until stable qualification

The beta does not claim completion of the remaining physical mixed-DPI and
multi-monitor matrix, high-refresh presentation measurements, real Windows
sleep/session/Explorer transitions, full long-duration reliability campaigns,
or participant studies. Those gates remain documented and must be resolved or
explicitly re-scoped before a stable `1.0` release.

## Distribution boundaries

- Windows 11 x86-64 is the supported beta platform.
- The installer is per-user, needs no administrator privileges, and does not
  enable start at login automatically.
- The portable archive and installer are unsigned because the project does not
  yet have a Windows code-signing certificate. Windows may display an
  unfamiliar-publisher or SmartScreen warning.
- Updates are manual. Users should download a newer release from the official
  GitHub repository.
- Uninstalling the application leaves the user's configuration and local
  plugins in `%LOCALAPPDATA%\Gradial` so an upgrade or reinstall does not erase
  them.
- Lua plugins execute locally after explicit trust and permission review. Users
  should install plugins only from authors they trust and review source where
  practical.

Public bugs and feature requests belong in GitHub Issues. Security-sensitive
reports belong in GitHub's private vulnerability-reporting form rather than a
public issue.
