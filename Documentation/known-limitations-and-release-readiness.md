# Known limitations and release readiness

Date: 2026-08-15

This document is the current release snapshot for the Windows MVP in the
[v0.2 implementation plan](../gradial-v0.2-implementation-plan.md) plus the
continuous-input and local Lua extension in the
[v0.3 implementation plan](../gradial-v0.3-implementation-plan.md). It
distinguishes finished implementation from release qualification. A working
feature, a passing short campaign, and a passed release gate are not
interchangeable.

## Status at a glance

| Area | Current status |
| --- | --- |
| Planned v0.2 core and built-in action surface | Implemented; final release qualification is incomplete |
| Planned v0.3 continuous interaction and local Lua surface | Implemented |
| v0.3 Milestone 4 implementation/documentation checklist | Complete with this document |
| Checked-in automated source verification | Passing at this checkpoint |
| Qualifying 10,000-session Lua continuous soak | Passed |
| Plugin privacy campaign | Passed |
| Display/DPI/recovery/shutdown automation | Passed on the available single 96-DPI display; unavailable physical rows remain open |
| Base Release performance campaign | Valid run completed: seven measured gates passed; 156.934 MiB failed the historical 80 MiB target but passes the public-beta ceiling below 192 MiB; displayed-frame gates remain unevaluated |
| Completed shortcut and continuous callback timing instrumentation | Base shortcut gate passed in a 1,000-invocation Release run; qualifying continuous-plugin campaign remains open |
| Full latency, resource, reliability, hardware, and participant qualification | Incomplete; deferred for stable-release qualification |
| Installer and Authenticode signing | Per-user installer and portable ZIP implemented and package-smoke tested; unsigned because no certificate is available |
| Public beta readiness | **Packaging gate passed locally; ready to publish as a prerelease** |
| Stable release readiness | **Not yet release-qualified** |

There is no known failure in the checked-in automated suites or public-beta
packaging gate at this checkpoint. The historical Release campaign failed its
then-current 80 MiB retained-working-set target. That result remains recorded
as a failure against the old threshold; the measured 156.934 MiB passes the
separately documented public-beta ceiling below 192 MiB. Neither policy turns
unavailable display/hardware evidence into a pass. The present build is a
public-beta candidate, not a stable-release candidate.

## What is implemented

The planned product surface is present:

- a prewarmed, per-monitor Windows radial overlay with four/eight-slice menus,
  M4/M5 activation, live profile resolution, edge assist, renderer animation,
  hook recovery, tray health, and target-integrity validation;
- the bounded built-in action coordinator, keyboard shortcuts, Unicode text,
  HTTP/HTTPS URLs, files/folders, direct `.exe` launch, invocation-window
  actions, delays, and linear sequences;
- onboarding, the settings editor, application profiles, appearance and custom
  icons, atomic configuration recovery and transfer, start at login, and
  privacy-bounded diagnostics;
- the hybrid continuous gesture: ordinary release inside the wedge, normalized
  distance outside it, unlocked live direction, inward restoration to discrete
  behavior, center selection clearing, and cancellation only when release
  occurs with no selection;
- schema-six continuous assignments, a bounded Lua 5.4 runtime, explicit trust
  and permission review, plugin lifecycle management, and context, shortcut,
  notification, and loopback-HTTP capabilities; and
- four example plugins, the author guide, manifest reference, Continuous API v1
  reference, trust/capability guide, and repeatable performance, soak,
  hardening, privacy, and participant-study harnesses.

The release-only 10,000-session continuous campaign passed its lifecycle,
bounded-queue, renderer/input health, clean-shutdown, log-privacy, and
post-warm-up memory-growth contracts. The 12-scenario continuous hardening run
passed on the hardware that was present, and the six-part plugin privacy
campaign passed. These results qualify only the contracts stated in their
individual evidence documents.

## Intentional v0.3 product limits

### Platform and interaction

- Gradial targets Windows 11 on x86-64. Windows 10, Windows on ARM, Linux, and
  macOS are not supported release targets.
- Activation is one configured M4 or M5 chord with optional Ctrl, Alt, and/or
  Shift. When the exact configured chord matches, its side-button down and up
  are consumed; an empty quick press cancels and is not replayed to the
  application. Left, right, and middle buttons, touch, pen, and gamepad
  activation are not supported.
- Menus contain four or eight slices. Twelve slices, nested menus, and command
  palettes are deliberately absent.
- Only one invocation and one built-in action plan can be active at a time. A
  repeated activation while the menu is open is consumed and ignored, and a
  new action is rejected while the executor is busy.
- Continuous input is one unsigned normalized scalar per direct wedge
  extension. There are no simultaneous parameters, two-dimensional values,
  pressure, velocity, acceleration, inertia, or haptics. Opposite meanings such
  as seek forward/back use different wedges or plugin-defined mappings.
- The continuous travel endpoint defaults to 300 DIP and can be set from 160 to
  400 DIP. Gradial may compress the visible track to remain on screen, but the
  configured input distance remains authoritative.
- Continuous mode does not latch and direction does not lock. Moving back
  inside the visible wedge restores its ordinary discrete action; changing
  angle changes the selected wedge; entering the center clears selection but
  does not close the invocation. Releasing with no selection cancels.
- Gradial measures and delivers the scalar. It does not know a media timestamp,
  system volume, brush size, or another application's current value, and it
  does not display feedback returned by that application.
- Plugin cancellation is a lifecycle notification, not rollback. A plugin or
  local service must decide how to handle effects it already issued before a
  Cancel.

### Built-in actions and target boundaries

- The built-in action set is intentionally narrow. There are no core
  mouse-click, drag, pointer-movement, scroll, clipboard, conditional, loop,
  retry, branching, parallel, or graph actions.
- Executable launch accepts an `.exe` and an argument array. It does not invoke
  a shell or accept `.cmd`, `.bat`, `.ps1`, PowerShell, Command Prompt, or an
  arbitrary command string, and Gradial does not wait for the launched process
  to exit.
- URL opening is limited to HTTP/HTTPS and file/folder opening uses the normal
  Windows association. Gradial does not provide a general non-loopback network
  action.
- Actions use the window frozen at activation and are rejected when that target
  is stale, replaced, no longer foreground where required, or at a higher
  integrity level. A normal Gradial process cannot type or send shortcuts into
  an administrator-elevated target and does not elevate itself to do so.
- Exclusive-fullscreen applications, the secure desktop/UAC prompt, the lock
  screen, anti-cheat-protected processes, and Remote Desktop are unsupported.
  Borderless fullscreen remains best effort.

### Local Lua plugins

- v0.3 plugins are local Lua source packages for continuous actions. One-shot
  plugin actions, plugin actions inside sequences, sequences inside continuous
  callbacks, background plugins, free-running timers, global event
  subscriptions, context providers, and plugin-to-plugin calls are absent.
- Lua containment is a defense-in-depth boundary, not a promise that hostile
  downloaded code is safe. Plugins run in the Gradial process after explicit
  approval. Users must inspect or otherwise trust the package source.
- Installation, Rescan, permission review, Reload, Disable, and removal are
  manual. There is no marketplace, publisher identity, signature verification,
  automatic download, automatic update, file watcher, or hot reload.
- Only admitted UTF-8 Lua source and restricted same-package modules run. Lua C
  modules, bytecode, FFI, native DLL plugins, arbitrary Win32 access, and WASM
  are not supported.
- A plugin receives only manifest-declared parameters and approved frozen
  application/title context. There is no PID, window handle, cursor coordinate,
  configuration, secret store, credential store, or live application-state
  channel.
- The only external-effect capabilities are the bounded existing shortcut
  backend, native notifications, and exact manifest-allowlisted loopback HTTP.
  There is no arbitrary filesystem, process, shell, registry, environment,
  clipboard, pointer, mouse, or non-loopback network capability.
- Loopback HTTP accepts direct literal `127.0.0.1` or `[::1]` cleartext origins
  with an explicit port and approved GET/POST methods. It performs no DNS,
  proxy, redirect, credential, HTTPS, or response-body delivery to Lua. A
  player or creative application that cannot be controlled by shortcuts needs
  an application-specific plugin and, where necessary, a local companion
  service.
- Plugin parameters use the generic Boolean, integer, number, string, and choice
  controls. Plugins cannot add custom settings widgets, renderer content,
  dynamic labels, overlay drawing, or application-value feedback.
- Plugin packages and machine-local trust decisions are not included in
  portable configuration exports. A valid imported plugin reference is kept
  but remains unavailable until the exact compatible package is installed and
  approved on that machine; its wedge continues to work as discrete-only.

### Distribution and local state

- Start at login is per Windows user, defaults off, records no arguments, and
  is not portable with menu configuration.
- Gradial has no account, cloud sync, telemetry, automatic update, or managed
  plugin distribution service.
- The public beta has a per-user, no-elevation installer and a portable ZIP.
  Start at login remains opt-in, updates are manual, and uninstall preserves
  `%LOCALAPPDATA%\Gradial` so settings and local plugins survive reinstall.
- The executable and installer are unsigned because no Windows code-signing
  certificate is available. Windows may show an unfamiliar-publisher or
  SmartScreen warning. Signing remains the final stable-distribution step.

## Unsupported versus not yet verified

Unsupported cases above are deliberate product boundaries. The following are
different: they are required or useful qualification cases for supported
behavior, but the current evidence set has not exercised them fully.

| Case | Evidence status |
| --- | --- |
| 120 Hz-or-faster physical display | Unavailable in the current interaction evidence |
| 125%, 150%, 175%, and 200% physical DPI | Unavailable in the latest hardening run; only 100%/96 DPI was present |
| Mixed 100%/150% and 125%/200% monitor pairs | Unavailable |
| Secondary monitor left of/above primary with negative coordinates | Unavailable as a physical configuration; pure coordinate cases are automated |
| Physical monitor attach/detach and primary-display change | Not run; synthetic display notification and rebuild paths pass |
| Physical DPI change | Not run; DPI logic is automated |
| Real sleep/resume, session lock/unlock, and Explorer restart | Not run; synthetic resume/recovery paths pass where applicable |
| Complete standard, elevated, closing/replaced, and borderless-fullscreen target matrix | Not release-qualified |
| Presented-frame latency on every required display configuration | Base Release capture ran, but PresentMon produced no Gradial-process presentation rows; gates remain `not_evaluated` |

Missing hardware is always reported as `unavailable`, and an external event
that was not performed is reported as `not_run`. Neither is inferred from a
simulation.

## Qualification evidence and open gates

| Evidence | Current result | What it does not prove |
| --- | --- | --- |
| Complete checked-in Rust/Lua tests and project checks | Passing at this checkpoint | Hardware, displayed pixels, long-duration behavior, and human usability |
| Base Release performance campaign | Valid; dispatch, completed shortcut, hook, cold start, idle CPU/GPU, and overflow gates passed; 156.934 MiB failed the historical 80 MiB target and passes the beta ceiling below 192 MiB | Displayed pixels, unavailable hardware rows, or stable-release resource policy |
| Public-beta package smoke | Passed clean install, real launch to ready state, in-place reinstall, user-data preservation, and uninstall using isolated local state | Upgrade from a distinct older published build, code signing, or clean-machine hardware diversity |
| Five-state plugin performance harness | Short scenario contracts pass; end-to-end callback and displayed-frame gates are implemented | The five-minute warmup, 1,000-session, 30-minute-idle elevated Release protocol |
| Isolated completed-shortcut timing | Release pass: 1,000 samples, p95 2.436 ms, p99 3.131 ms, and all 4,000 receiver events | Other built-in actions and unavailable target/display configurations |
| Continuous plugin soak | 10,000 sessions passed; p95 working-set growth was 3.270 MiB against a 10 MiB limit | The separate v0.2 24-hour soak, full displayed-frame campaign, or participant study |
| Continuous hardening matrix | 12 of 12 scenarios passed on one 96-DPI display | Unavailable physical DPI/display rows and real OS state transitions |
| Plugin privacy campaign | All six checks passed | Privacy outside the documented log/default-bundle and explicit process-path opt-in boundaries |
| Guided continuous study tooling | Protocol, private reports, and evaluator are implemented | The required eight-participant cohort; every usability gate remains unevaluated |

The base and plugin timing reporters measure completed shortcut submission and
the full continuous outer-edge/publication/release-to-callback boundaries. The
base Release campaign qualified shortcut submission, dispatch, hook duration,
cold start, idle CPU/GPU, and queue integrity. Its 156.934 MiB maximum is a
historical failure against 80 MiB and an explicit public-beta pass below 192
MiB. Displayed usable-frame and highlight gates remain unevaluated because the
valid PresentMon stream contained no Gradial-process presents. The full
continuous-plugin campaign remains open for stable qualification.

The following stable-release work remains; it does not block the public beta
under the [public beta release policy](public-beta-release-policy.md):

1. Decide the stable memory budget from beta evidence and rerun the complete
   base Release protocol. Establish a valid composition/display measurement for
   the transparent overlay without substituting application checkpoints.
2. Complete the plugin Release performance campaign with full-duration
   warmup/idle periods, at least 1,000 sessions per required scenario, and the
   implemented end-to-end continuous callback timing.
3. Run the physical display and Windows-state matrix: high refresh, every
   required DPI, mixed scaling, negative coordinates, attach/detach, primary
   change, DPI change, sleep/resume, lock/unlock, Explorer restart, and the
   supported target-lifetime/integrity cases.
4. Complete the remaining reliability campaigns: the 10,000-invocation focus
   and input-suppression run, the 10,000-case synthesized-key cleanup campaign,
   forced-termination configuration-save matrix, and the v0.2 24-hour soak.
5. Conduct the v0.2 direction/onboarding study and the v0.3 continuous study
   with at least eight participants, including the matched endpoint comparison.
   Revisit the provisional 300-DIP default only from that evidence.
6. Fix or optimize only any gate that fails, rerun the affected complete
   protocol, and retain its machine-readable evidence.
7. Validate upgrade from a genuinely older published installer on a clean
   Windows user profile and repeat the package smoke on additional machines.
8. As the final stable-distribution step, obtain a Windows code-signing certificate,
   Authenticode-sign and timestamp the executable and installer, verify their
   signatures and hashes, and repeat clean-machine install, upgrade, launch,
   uninstall, and diagnostics checks.

Any stuck key, focus theft, leaked activation event, partial configuration
write, permission bypass, disabled-plugin execution, unbounded queue,
post-terminal Update, unclean shutdown, privacy defect, or failed public-beta
packaging requirement blocks a beta. The remaining unmeasured stable gates
continue to block a stable release rather than being reported as passed.

## Deferred beyond v0.3

The following ideas belong to a later plan rather than unfinished v0.3 work:

- WASM and stronger-isolation downloadable plugins;
- a marketplace, publisher provenance, package signing, and automatic updates;
- one-shot or background plugin actions and richer composition;
- multiple continuous dimensions and application-to-Gradial value feedback;
- plugin-defined UI/renderer content and custom parameter widgets;
- broader filesystem, network, process, shell, clipboard, mouse, and pointer
  capabilities;
- nested/twelve-slice menus, general graph automation, skins, cloud sync, and
  non-Windows platforms.

Adding one of these features requires its own use case, isolation boundary,
permissions, resource limits, failure semantics, and test plan. It is not a
shortcut around an open v0.2 or v0.3 release gate.

## Evidence index

- [Base performance harness](milestone-3-performance-harness-results.md)
- [Public beta release policy](public-beta-release-policy.md)
- [Base Release performance result](milestone-4-base-release-performance-results.md)
- [Release timing instrumentation](milestone-4-release-timing-instrumentation-results.md)
- [Interaction matrix](milestone-1-interaction-matrix-results.md)
- [Continuous participant protocol](milestone-0-continuous-study-results.md)
- [Plugin performance baseline](milestone-4-plugin-performance-baseline-results.md)
- [10,000-session continuous soak](milestone-4-plugin-continuous-soak-results.md)
- [Continuous hardening matrix](milestone-4-continuous-hardening-matrix-results.md)
- [Plugin privacy evidence](milestone-4-plugin-privacy-results.md)
- [Plugin author guide](plugin-author-guide.md)
- [Plugin manifest v1 reference](plugin-manifest-reference.md)
- [Continuous API v1 reference](continuous-api-v1-reference.md)
- [Plugin trust and capability guide](plugin-trust-and-capabilities-guide.md)

Update this snapshot when a qualifying campaign is completed, a supported
boundary changes, or distribution work advances. Preserve the distinction
between `passed`, `failed`, `unavailable`, `not_run`, and `not_evaluated`.
