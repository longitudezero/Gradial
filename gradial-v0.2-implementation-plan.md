# Gradial v0.2 — Windows MVP Product and Engineering Plan

Status: proposed implementation baseline  
Target: Windows 11, x86-64  
Implementation language: Rust

This document replaces the broad v0.1 architecture and action notes for the first shippable version. Requirements marked **MUST** are release-blocking. Requirements marked **SHOULD** may be changed only with a documented reason and a replacement test.

## 1. Release boundary

Gradial v1 is a local Windows radial launcher built around one interaction:

> Hold the activation control, move in a direction, release to execute.

The release proves the interaction, not a general automation or plugin platform.

### Included

- One configurable activation binding, defaulting to Mouse Button 4.
- Mouse Button 4 and Mouse Button 5 as supported activation controls.
- Optional `Ctrl`, `Alt`, or `Shift` modifier requirements on the activation control.
- Four-slice and eight-slice menus.
- One global profile and any number of application profiles.
- A radial-menu editor with live preview.
- Built-in actions:
  - Send keyboard shortcut.
  - Type Unicode text.
  - Open URL.
  - Open file or folder.
  - Run executable with an argument array.
  - Close, minimize, maximize, or restore the invocation window.
  - Delay.
  - Linear sequence.
- Tray operation, start-at-login, configuration import/export, and local diagnostics.
- Per-Monitor V2 DPI behavior from the first prototype.

### Explicitly deferred

- Twelve-slice menus.
- Nested menus and command palettes.
- Mouse-click, drag, pointer-movement, and scroll actions.
- Clipboard reads or writes.
- Conditions, variables, loops, retries, branches, parallel execution, and a graph runtime.
- PowerShell, Command Prompt, batch files, arbitrary shell strings, environment editing, and network requests.
- Lua, WASM, native plugins, context providers, plugin events, and a marketplace.
- Arbitrary skins, custom fonts, animated textures, and downloadable themes.
- Cloud sync, telemetry, accounts, automatic updates, Linux, ARM64, touch, pen, and exclusive-fullscreen support.

`Action` is the user-facing and internal v1 term. The implementation must not introduce a `Node` or graph abstraction until a graph use case is approved.

## 2. Interaction contract

### 2.1 Activation

- The default binding is Mouse Button 4 with no modifier.
- Activation is edge-triggered on button-down; there is no hold-delay timer.
- Both the activation button-down and matching button-up are consumed and never delivered to the foreground application.
- A quick press and release with no selected slice cancels. Gradial does not replay the original mouse event.
- Left, right, and middle mouse buttons cannot be activation controls in v1. This avoids suppress-and-replay behavior for buttons with common application meanings.
- A second activation button-down while a menu is open is consumed and ignored.
- Input tagged as injected by Gradial is ignored by Gradial's activation logic to prevent recursion.

The first-run flow MUST tell the user that the chosen side button becomes dedicated to Gradial while the app is running.

### 2.2 Invocation snapshot

Button-down creates an immutable `InvocationSnapshot` before profile selection or rendering begins:

```rust
struct InvocationSnapshot {
    invocation_id: Uuid,
    qpc_timestamp: i64,
    foreground_hwnd: HWND,
    foreground_pid: u32,
    process_start_time: u64,
    executable_path: Option<PathBuf>,
    window_class: String,
    window_title: String,
    cursor_physical_px: Point<i32>,
    monitor_device_name: String,
    monitor_rect_physical_px: Rect<i32>,
    work_area_physical_px: Rect<i32>,
    monitor_dpi: u32,
    physical_modifiers: ModifierState,
    activation_binding: BindingId,
    config_revision: u64,
}
```

- Profile resolution uses this snapshot.
- Window actions always target `foreground_hwnd` from this snapshot.
- Input actions dispatch only if the captured window still exists and remains the foreground window at release.
- Reuse of an `HWND` is guarded by matching the captured PID and process start time.
- A title change while the menu is open does not change the selected profile.
- A target that closes, changes PID, or loses foreground status causes release to fail with `stale_target`; no action is sent.

### 2.3 Gesture geometry

All persisted dimensions use device-independent pixels (DIP). At invocation they are converted using the DPI of the invocation monitor.

| Parameter | v1 default |
| --- | ---: |
| Center cancel radius | 24 DIP |
| Selection-entry radius | 36 DIP |
| Ring inner radius | 40 DIP |
| Ring outer radius | 112 DIP |
| Label/icon radius | 76 DIP |
| Boundary hysteresis | 5 degrees |
| Open animation | 60 ms |
| Close animation | 50 ms |
| Selection-color transition | Immediate |

The original 35 ms selection-color prototype was removed after interaction
testing showed that consistent immediate wedge feedback felt more accurate in
both fixed-origin and edge-assist geometry.

- The gesture origin is the physical cursor position captured on button-down.
- No slice is selected until the pointer reaches 36 DIP from the origin.
- After selection, the slice remains selected until the pointer returns within 24 DIP or crosses at least 5 degrees beyond an adjacent slice boundary.
- Pointer distance beyond the outer ring does not clear the selection; angle remains authoritative.
- Returning within 24 DIP clears the selection.
- Releasing with no selected slice or on an empty slice cancels.
- Four-slice menus are centered on north, east, south, and west.
- Eight-slice menus start at north and continue clockwise at 45-degree intervals.
- Pointer movement received before the first rendered frame still participates in selection.
- A valid flick released before the menu becomes visible still executes. Rendering is not a prerequisite for gesture recognition.
- Modifier changes after button-down do not change the profile or menu.

The angle function MUST be a pure function covered by table-driven tests at every slice center, exact boundary, boundary ±0.1 degrees, and the negative virtual-screen coordinates.

### 2.4 Cancellation

The following cancel an open menu:

- `Escape`.
- Any non-activation mouse button-down.
- Return to the center followed by activation release.
- Target-window invalidation.
- Display topology change.
- Session lock.
- Hook-thread or renderer failure.

When cancellation begins:

1. The triggering cancel event is consumed.
2. The activation button-up is consumed even if it arrives later.
3. No action is queued.
4. The overlay begins its close animation.
5. The state returns to `Idle` only after input suppression bookkeeping is cleared.

### 2.5 State machine

| State | Entry | Accepted events | Exit |
| --- | --- | --- | --- |
| `Idle` | App and hooks are healthy | Activation down | `Opening` |
| `Opening` | Snapshot and profile resolved; first frame requested | Movement, activation up, cancel | `Tracking`, `Executing`, or `Cancelling` |
| `Tracking` | Menu is visible or movement passed 36 DIP | Movement, activation up, cancel | `Executing` or `Cancelling` |
| `Executing` | Release selected a non-empty slice and target validation passed | Executor acknowledgement | `Closing` |
| `Cancelling` | A cancel rule fired | Matching suppressed button-ups | `Closing` |
| `Closing` | Input decision is final | New activation is ignored until suppression bookkeeping is safe | `Idle` |
| `Faulted` | Required input or rendering component failed | Tray recovery or process restart | `Idle` after reinitialization |

No action may be dispatched from `Closing`, `Cancelling`, or `Faulted`.

### 2.6 Edge-of-screen decision

The v0.1 documents do not define how users select an outward direction when the pointer begins at a screen edge. This must be resolved in the interaction prototype before production architecture is frozen.

Prototype these two behaviors:

1. **Fixed origin:** keep the gesture origin at button-down and allow the ring to clip.
2. **Edge assist:** use raw mouse deltas to drive a virtual gesture pointer while leaving the system cursor at its original location; render the complete ring inset from the work-area edge.

Do not ship a cursor-warp implementation that visibly moves the application cursor.

The chosen behavior must satisfy all of the following:

- Every one of eight directions can be selected from positions within 5 physical pixels of each corner.
- Gradial changes the system cursor position by no more than 2 physical pixels after close.
- The foreground application receives no synthetic pointer movement.
- Direction error rate is at most 3% over at least 320 corner trials.
- Median release time is no more than 20% slower than the same users' center-screen result.

If edge assist cannot meet these gates, v1 must document fixed-origin clipping as a known limitation and disable inaccessible empty-space labels rather than pretending all directions are available.

## 3. Focus, overlay, and DPI behavior

### 3.1 Overlay window

Create and prewarm one transparent, borderless overlay window per connected monitor at application start. Do not create a window or graphics device during activation.

Each overlay MUST:

- Use `WS_EX_NOACTIVATE`, `WS_EX_TOOLWINDOW`, and topmost positioning.
- Be shown with `SW_SHOWNOACTIVATE`.
- Use `SWP_NOACTIVATE` for position changes.
- Return `MA_NOACTIVATE` for `WM_MOUSEACTIVATE`.
- Return `HTTRANSPARENT` for hit testing.
- Stay out of Alt-Tab and the taskbar.
- Never call `SetForegroundWindow`, `SetActiveWindow`, or focus APIs.

Windows documents `WS_EX_NOACTIVATE` as preventing foreground activation by the system, with `WM_MOUSEACTIVATE` still requiring deliberate handling. See [CreateWindowExW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-createwindowexw).

The automated focus test records `GetForegroundWindow()` at activation, after the first presented frame, and after close. A mismatch is release-blocking unless the target process itself changed focus.

### 3.2 Rendering

- Use `egui` for the settings UI.
- Use a GPU-backed overlay renderer behind a narrow `OverlayRenderer` interface. An `egui`/`wgpu` overlay is acceptable only if it passes the latency gates.
- The first presented frame contains complete ring geometry, labels, and at least 85% final opacity. Animation must not reveal an empty or unusable frame.
- Open animation: scale `0.96 → 1.00` and opacity `0.85 → 1.00` over 60 ms with ease-out.
- Close animation: opacity `1.00 → 0.00` over 50 ms. Action dispatch does not wait for this animation.
- `Reduced motion` removes scale and interpolation but does not change timing or gesture behavior.
- Icons are decoded and uploaded to the GPU when configuration loads, not on activation.
- Custom icons are limited to PNG or SVG, 512 × 512 logical pixels, and 256 KiB per source file. Invalid assets fall back to a built-in placeholder.
- v1 exposes an accent color, light/dark label choice, and size preset. It does not load a skin directory or custom font.

### 3.3 DPI and monitor coordinates

The application manifest declares Per-Monitor V2 awareness before any window is created. Windows does not bitmap-scale PMv2 windows and makes the application responsible for `WM_DPICHANGED`; see [Microsoft's high-DPI desktop guidance](https://learn.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows).

- Hook and Win32 window coordinates remain signed physical pixels.
- Layout values remain DIP until the renderer prepares an invocation frame.
- The invocation monitor is selected from the button-down cursor position.
- `WM_DPICHANGED` invalidates scale-dependent text, icon, and geometry caches for that window.
- `WM_DISPLAYCHANGE`, device arrival/removal, and primary-monitor changes rebuild the monitor-window set while no menu is open; if received while open, the invocation cancels.
- Negative virtual-screen coordinates are valid and must never be cast to unsigned values.
- v1 tests 100%, 125%, 150%, 175%, and 200% scaling, including mixed-scale monitor pairs.

## 4. Input architecture

### 4.1 Hook thread

Install `WH_MOUSE_LL` and `WH_KEYBOARD_LL` on one dedicated thread with a Win32 message loop.

The hook callback may only:

1. Read the event.
2. Timestamp it with `QueryPerformanceCounter`.
3. Check the small in-memory suppression state.
4. Copy a fixed-size event into a bounded single-producer queue.
5. Return the suppress/pass decision.

It MUST NOT allocate, lock a contended mutex, resolve processes, match profiles, read configuration, log to disk, call plugins, render, or execute actions.

Microsoft warns that a timed-out low-level hook can be silently removed and recommends a dedicated hook thread that immediately hands work to another thread; see [LowLevelMouseProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc).

Required safeguards:

- Queue capacity: 4,096 events.
- Movement coalescing is permitted only after the hook callback; button and key transitions are never coalesced.
- Queue overflow cancels any open invocation, increments `input_queue_overflow`, and changes tray status to faulted.
- Hook callback duration is measured in a lock-free in-memory histogram.
- The hook thread emits a heartbeat every 250 ms from its message loop.
- Missing three heartbeats restarts the hook thread and cancels an open menu.
- Session unlock, resume from sleep, and Explorer restart trigger hook re-registration.
- Because Windows exposes no positive signal for silent hook removal, the diagnostics page must say “hook thread responsive” rather than claiming that the hook is certainly installed.

### 4.2 Event and work separation

| Thread | Owns | Prohibited work |
| --- | --- | --- |
| Input | Hooks, suppression state, event timestamps | Rendering, profile resolution, filesystem access, actions |
| Interaction | Gesture state machine, snapshot creation, profile selection | Win32 hook callbacks, blocking I/O |
| UI/render | Overlay windows, GPU resources, settings UI | Input injection, process launch |
| Executor | Validated action plans and cancellation | Hook management, configuration mutation |
| Config/diagnostics | Atomic saves, backups, bounded log writes | Gesture decisions |

All inter-thread channels are bounded. A full channel returns an explicit error; no producer waits indefinitely.

## 5. Profile resolution

### 5.1 Profile rule

An application profile contains:

```rust
struct ProfileRule {
    executable_path: PathBuf,       // required, case-insensitive exact match
    window_class: Option<String>,   // case-insensitive exact match
    title_regex: Option<String>,    // Rust regex; no backtracking engine
    priority: u8,                   // 0..=100
}
```

- Process name alone is not a v1 matcher.
- Executable paths come from `QueryFullProcessImageNameW` and are normalized for separator and case comparison without following user-authored environment variables.
- If the process path cannot be read, only the global profile is eligible.
- Title matching is opt-in because titles are volatile and may contain private data.
- Invalid regular expressions prevent the profile from being saved.

### 5.2 Deterministic precedence

Resolve eligible profiles in this order:

1. Highest `priority`.
2. Most populated predicates among executable path, window class, and title.
3. Lexicographically smallest immutable profile UUID.
4. Global profile if no application profile matches.

The editor shows a “Test against current window” panel containing:

- Captured executable path and class.
- Matching profiles.
- The winner.
- The exact tie-break step that selected it.

Profiles do not inherit from one another in v1. Each profile contains a complete four- or eight-slice menu. “Duplicate profile” is provided to avoid manual reconstruction.

## 6. Actions and execution

### 6.1 Action result

Every action returns exactly one of:

```rust
enum ActionResult {
    Success,
    Cancelled,
    Failure {
        code: ActionErrorCode,
        safe_message: String,
    },
}
```

Success produces no notification. Cancellation and failure appear in the tray activity panel; failures also show a non-focus-stealing toast unless the user disables failure toasts.

### 6.2 Shared execution rules

- The selected action is compiled into an immutable execution plan using the invocation's `config_revision`.
- Only one plan executes at a time.
- A second dispatch while the executor is busy fails immediately with `action_busy`; it is not queued.
- Default per-step timeout: 5 seconds.
- Maximum sequence duration: 30 seconds.
- Maximum executed steps: 32.
- Maximum nested sequence depth: 4.
- Configuration validation rejects direct and indirect sequence cycles.
- A sequence stops on the first `Failure` or `Cancelled` result.
- Delays are cancellable in increments no larger than 10 ms.
- The tray activity panel provides “Cancel current action.”
- Shutdown requests cancellation and waits at most 2 seconds before terminating the executor.

### 6.3 Target validation

Before a keyboard or text action:

1. Confirm the captured `HWND` still exists.
2. Confirm PID and process start time still match.
3. Confirm it is still the foreground window.
4. Compare the target process integrity level with Gradial's.

Gradial runs at normal user integrity and v1 does not offer an administrator mode. If the target is at a higher integrity level, input actions fail before injection with `elevated_target`. Windows restricts `SendInput` to equal or lower integrity targets and does not identify UIPI as the reason for failure; see [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput).

### 6.4 Send keyboard shortcut

Configuration stores:

- Zero or more modifiers from left/right `Ctrl`, `Alt`, `Shift`, and `Win`.
- Exactly one non-modifier key.
- Key identity as a Windows scan code plus the extended-key flag.

Execution:

1. Read current physical modifier state.
2. Press only requested modifiers that are not already physically down.
3. Press and release the non-modifier key.
4. Release synthesized modifiers in reverse order.

Every synthetic event includes Gradial's `dwExtraInfo` marker. A cleanup guard records only keys Gradial pressed and issues their key-up events on failure, cancellation, or unwind. It never releases a modifier that was physically down before the action.

F13–F24 are allowed, which is sufficient for basic AutoHotkey interoperability. There is no dedicated AutoHotkey backend, listener detection, acknowledgement, or IPC in v1.

### 6.5 Type Unicode text

- Use Unicode `SendInput` events; do not modify the clipboard.
- Store text as UTF-8 in configuration and convert at dispatch.
- No variable or environment expansion.
- Maximum value: 8 KiB UTF-8.
- Logs record character count only, never the text.
- If the target changes during a multi-event send, stop and run synthesized-key cleanup.

### 6.6 Open URL, file, or folder

- URL schemes allowed in v1: `https` and `http`.
- File and folder values are absolute paths after editor validation.
- Use the Windows shell association API; do not construct a command line through `cmd.exe`.
- Missing paths fail with `path_not_found`.
- Launch acknowledgement means Windows accepted the open request; Gradial does not wait for the opened application.

### 6.7 Run executable

Configuration stores:

```rust
struct RunExecutable {
    executable: PathBuf,
    arguments: Vec<OsString>,
    working_directory: Option<PathBuf>,
}
```

- `executable` must be an absolute path chosen through the file picker.
- Arguments remain an array until the Windows process-creation boundary.
- Do not accept or persist an arbitrary shell command string.
- No environment-variable interpolation or custom environment map in v1.
- The child inherits Gradial's normal integrity level.
- Process creation timeout is 5 seconds.
- Success means process creation succeeded; Gradial does not wait for process exit.
- Arguments are omitted from release logs.

### 6.8 Window actions

Supported commands are close, minimize, maximize, and restore.

- They target the captured `HWND`, never whichever window is current at execution time.
- PID/start-time validation runs immediately before the operation.
- Close requests a normal close; Gradial does not terminate the process.
- “Toggle fullscreen” is not included because it is generally application-specific.
- Centering, always-on-top, virtual desktops, FancyZones, and monitor movement are deferred.

### 6.9 Delay and sequence

- Delay accepts 10–10,000 ms.
- A sequence contains 1–32 action references.
- Reusable sequences are allowed, subject to the depth, cycle, step, and duration limits above.
- There are no conditions, return values, variables, retries, branches, or parallel children.

## 7. Configuration and editor

### 7.1 Files

Use:

```text
%LOCALAPPDATA%\Gradial\
    config.json
    config.json.bak
    icons\
    logs\
```

`config.json` contains `schema_version`, `config_revision`, activation binding, appearance settings, actions, profiles, and menu slices. IDs are UUIDs and references are validated before the new configuration becomes active.

Save procedure:

1. Serialize and validate the complete model in memory.
2. Write `config.json.tmp` in the same directory.
3. Flush the file.
4. Preserve the previous valid file as `config.json.bak`.
5. Atomically replace `config.json`.
6. Increment `config_revision` only after replacement succeeds.

The running configuration changes only after a successful save. On parse or schema failure at startup, load the backup, retain the broken file for recovery, and show a persistent settings warning. Never overwrite a broken file automatically.

Import validates into a temporary model and shows errors before replacing anything. Export writes a self-contained JSON file plus an optional adjacent `icons` directory; executable and file paths are not made portable.

### 7.2 Editor requirements

The settings window has four functional areas:

1. **Binding:** choose Mouse Button 4 or 5, optional modifiers, and run a hold/flick/release test.
2. **Profiles:** create, duplicate, order by explicit priority, edit match rules, and test against the current foreground window.
3. **Menu:** choose four or eight slices; select a slice in a live radial preview; assign, replace, clear, or edit its action and icon.
4. **Actions:** record a shortcut, edit text, choose paths, edit one executable argument per row, or build a bounded linear sequence.

The preview uses the production geometry and layout code. It does not install a second gesture engine.

Save is disabled while any of the following exist:

- Empty required path.
- Invalid URL scheme.
- Invalid title regex.
- Missing action reference.
- Sequence cycle, excess depth, or excess step count.
- Duplicate profile UUID or action UUID.
- Unsupported slice count.
- Icon outside type or size limits.

The first-run flow is required, not optional. It ends only after the user:

1. Chooses and successfully presses an activation button.
2. Opens the sample menu.
3. Selects one sample action.
4. Cancels once with the center or `Escape`.
5. Confirms that the activation button is dedicated while Gradial runs.

## 8. Errors, diagnostics, and privacy

### 8.1 User-visible errors

At minimum, define stable codes for:

- `stale_target`
- `elevated_target`
- `input_injection_failed`
- `action_busy`
- `action_timeout`
- `path_not_found`
- `process_launch_failed`
- `window_command_failed`
- `config_invalid`
- `input_queue_overflow`
- `hook_unresponsive`
- `renderer_unavailable`

Messages state what failed and the next useful step. For example, `elevated_target` explains that a normal Gradial process cannot type into an administrator-elevated application.

### 8.2 Local logs

- Release logging is local only.
- Five files of at most 1 MiB each; oldest file rotates out.
- Each record contains UTC timestamp, invocation ID, component, event code, duration, result code, and config revision.
- Do not log typed text, executable arguments, window titles, clipboard data, or selected files.
- Process paths are omitted by default. A user-created diagnostic bundle may include them only after a preview and explicit confirmation.
- No analytics, crash upload, network call, or unique installation identifier exists in v1.

## 9. Measurable release requirements

### 9.1 Measurement protocol

- Performance builds only; debugger detached.
- Warm process means Gradial has been running for at least 5 minutes, configuration is loaded, overlay windows exist, and GPU resources are resident.
- Each latency percentile uses at least 1,000 invocations per hardware/display configuration.
- Hook timestamps use `QueryPerformanceCounter`.
- Presented-frame timing uses ETW/PresentMon-style display events, not the time the draw call was submitted.
- Report p50, p95, p99, maximum, sample count, CPU, GPU, Windows build, DPI, refresh rate, and hardware.
- A test run is invalid if another foreground application steals focus or the machine changes power mode.

### 9.2 Latency and resource gates

| Metric | Release gate |
| --- | ---: |
| Button-down to first displayed usable frame, warm process | p95 < 25 ms; p99 < 40 ms |
| Pointer event to displayed highlight change | p95 ≤ one refresh interval + 4 ms; p99 ≤ two refresh intervals |
| Activation release to executor dispatch | p95 < 10 ms; p99 < 20 ms |
| Activation release to completed `SendInput` submission for a shortcut | p95 < 15 ms; p99 < 25 ms |
| Hook callback duration | p99 < 250 µs; maximum < 1 ms |
| Cold launch to healthy tray state | p95 < 750 ms |
| Idle CPU after warm-up, 10-minute average | < 0.2% of total machine CPU |
| Idle GPU after warm-up, 10-minute average | < 0.5% |
| Working set after 30 minutes idle plus 1,000 invocations | < 192 MiB |
| Input queue overflow during standard test suite | 0 |

“Usable frame” means the correct menu geometry, labels, and highlight state are present at no less than 85% final opacity. A transparent placeholder frame does not count.

### 9.3 Reliability gates

- Zero Gradial-caused foreground-window changes in 10,000 invocations.
- Zero stuck synthesized keys after 10,000 successful, cancelled, timed-out, and failed action cases.
- Zero activation button events observed by the target helper application while Gradial is active.
- Configuration survives forced termination during every save step; startup loads either the complete old file or complete new file, never a partial file.
- A target-window replacement between activation and release never receives the action.
- 24-hour soak test completes with no hook-thread stall, unbounded log growth, working-set growth greater than 10 MiB after the first hour, or GPU-device leak.

## 10. Test matrix

### 10.1 Automated

- Gesture unit tests:
  - Four- and eight-slice centers and boundaries.
  - Radial and angular hysteresis.
  - Clear-on-center behavior.
  - Negative coordinates and large virtual desktops.
- Profile resolver tests:
  - Priority, specificity, UUID tie-break, missing executable path, and invalid regex.
- Action validation tests:
  - Sequence cycles, depth, step count, timeout, and cancellation.
  - Modifier cleanup with every failure inserted between key-down and key-up events.
- Configuration tests:
  - Atomic replacement interruption.
  - Backup recovery.
  - Unknown or newer schema version.
  - Broken references and invalid icon metadata.
- Win32 integration helper:
  - Records focus, received mouse-button events, keyboard events, window commands, and process integrity.
  - Can close or replace its own window between activation and release.

### 10.2 Hardware and display

Test at minimum:

- One 60 Hz monitor at 100%.
- One 120 Hz or faster monitor.
- Two monitors with 100%/150% scaling.
- Two monitors with 125%/200% scaling.
- Secondary monitor left of and above the primary, producing negative coordinates.
- Primary-monitor change while Gradial is running.
- Plug, unplug, sleep, resume, session lock/unlock, and Explorer restart.
- Pointer activation at center, every edge midpoint, and every corner.
- Standard user target, elevated target, and target closing during the gesture.
- Borderless-fullscreen application as best effort.

Exclusive-fullscreen games, secure desktop/UAC prompts, Windows lock screen, anti-cheat-protected processes, and Remote Desktop are not supported in v1. Gradial must fail or remain inactive without hanging or elevating itself.

### 10.3 Usability prototype

Before building the editor, run a throwaway eight-slice shortcut-only prototype:

- At least 8 participants.
- 40 center-screen and 40 edge/corner trials per participant.
- Record wrong direction, cancellation, activation-to-release time, focus change, and accidental activation.
- Compare four and eight slices; twelve slices are measured only to validate deferral.

Production defaults are accepted when:

- Eight-slice direction error is at most 5% in the first 20 trials and at most 2% in the last 20.
- Median selection time improves between the first and last halves.
- Accidental activation is below 1 per 100 non-invocation side-button presses in the scripted test.
- At least 7 of 8 participants can intentionally cancel without instruction after onboarding.

If these gates fail, change geometry, hysteresis, or onboarding and repeat the prototype before implementing plugins, sequences beyond the defined linear model, or visual customization.

## 11. Implementation milestones

### Milestone 0 — Interaction and latency spike

Deliver:

- Dedicated input thread.
- One prewarmed overlay.
- Eight fixed shortcut slices.
- Invocation snapshot.
- Gesture state machine and cancellation.
- Focus test helper.
- ETW-compatible timing markers.
- Fixed-origin and edge-assist prototypes.

Exit only when button-down, highlight, release, focus, suppression, and edge behavior have measured results against Sections 2 and 9.

### Milestone 1 — Production interaction core

Deliver:

- Four/eight geometry.
- Per-monitor overlay windows and PMv2 handling.
- Hook recovery and faulted tray state.
- Final edge behavior.
- Renderer cache and animation limits.
- Automated gesture, focus, DPI, and target-lifetime tests.

Exit only when no settings window or action implementation is needed to reproduce the interaction metrics.

### Milestone 2 — Profiles, actions, and editor

Deliver:

- Deterministic profile resolver and diagnostic explanation.
- The action types in Section 6.
- Bounded executor and cleanup guards.
- Full radial editor and production preview.
- Atomic configuration, backup, import, and export.
- Required onboarding.

Exit only when every editor-created configuration is either valid and executable or rejected with a field-specific error.

### Milestone 3 — Hardening and release candidate

Deliver:

- Release logging and diagnostic bundle preview.
- Start-at-login and signed per-user installer.
- Performance report for every required display configuration.
- 24-hour soak result.
- Elevated-target, crash-save, display-change, sleep/resume, and target-replacement results.
- Known-limitations document matching the actual behavior.

The release candidate is blocked by any failed MUST requirement, unresolved stuck-key case, focus theft, partial configuration write, or unmeasured latency gate.

## 12. Post-v1 admission criteria

A deferred feature enters planning only with a concrete use case and isolation boundary:

- **Mouse actions:** require a defined cursor-restoration and target-coordinate model.
- **Conditions or variables:** require typed values, snapshot/live semantics, failure behavior, and recursion limits.
- **Plugins:** require enforced context-field permissions, capability isolation, timeouts, reentrancy rules, API versioning, and zero plugin code on input/render paths.
- **Skins:** require asset-size, texture-dimension, font, animation-duration, and fallback limits.
- **AutoHotkey IPC:** requires delivery acknowledgement, backend health, loop prevention, collision handling, and version negotiation.
- **Twelve slices:** requires measured error and selection-time results comparable to the accepted eight-slice baseline.

No deferred feature may weaken the v1 focus, input-suppression, latency, cleanup, or configuration-recovery gates.
