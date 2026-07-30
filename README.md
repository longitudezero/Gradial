# Gradial

Gradial is a Windows 11 radial launcher built around a hold, flick, and release
gesture. The current development build includes the first complete interaction
spine and several bounded Milestone 1 slices: a prewarmed, non-activating GPU
overlay for every connected display; dedicated low-level input thread; bounded
event transport; tested four/eight-slice gesture state machine; automatic edge
assist; display/DPI rebuild handling; input-hook recovery; a native health tray
icon; cached overlay geometry; bounded animations; and focus/timing
instrumentation. A versioned interaction-matrix harness now covers repeatable
direction, corner/edge, rapid-use, focus, display-routing, topology, and
recovery scenarios. The first Milestone 2 foundation adds schema-versioned
configuration, atomic backup recovery, complete invocation target identity,
deterministic application-profile resolution, and profile-driven four/eight
slice menus whose configured labels are rendered directly. The next bounded
Milestone 2 slice adds schema-two built-in action definitions, validated menu
references and reusable sequences, safe schema-one migration, and immutable
release-time action-plan compilation. The first executor slice adds a
single-worker coordinator and cancellable Delay execution with bounded
timeouts and shutdown. The next executor slice adds stale/elevated target
validation and scan-code keyboard shortcuts with strict synthesized-key
cleanup. Unicode text entry now shares that target boundary, emits literal
UTF-16 input without using the clipboard, revalidates throughout long sends,
and cleans up an in-flight code unit on cancellation or failure. The next
action slice opens HTTP/HTTPS URLs and existing absolute files or folders
through Windows shell associations without command-shell interpolation or
payload logging. Direct executable actions now launch existing absolute
`.exe` files with exact argument-array preservation, an optional validated
working directory, no shell, and no wait for child exit. Invocation-window
actions now send one asynchronous minimize, maximize, restore, or polite close
request only to the window frozen at M4-down after revalidating its identity
and integrity. Action failures and cancellations now enter a session-only,
20-item recent-activity tray panel. Failures also request a native,
non-focus-stealing Windows notification; successful actions remain silent.
The first settings-editor slice adds a normal single-instance window for the
global radial menu, with four/eight-direction editing, existing-action
assignment, a production-renderer preview, guarded unsaved changes, atomic
saves, and immediate runtime reload. The action-catalog editor now creates and
edits every leaf action, supports rename and duplication across every action
type, opens native Windows path choosers, and prevents deletion while an action
is still referenced. Reusable sequences can now be created and edited as
ordered, safely nested action lists with live expansion limits.

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

The resolved profile now supplies the visible menu's slice count and labels.
The default configuration is an eight-direction menu. Debug builds retain a
test-only override for exercising a four-slice N/E/S/W menu end to end:

```powershell
cargo run --package gradial-app -- --slices=4
```

`--slices 4` and `--slices 8` are also accepted by debug builds. Release builds
reject this override and always use the resolved profile configuration.

The overlay opens from 96% scale and 85% opacity over 60 ms, changes selection
color over 35 ms, and fades out over 50 ms. Gesture recognition and release do
not wait for those animations. A temporary development override removes visual
interpolation while preserving the same lifecycle timing:

```powershell
cargo run --package gradial-app -- --reduced-motion
```

**M4 is dedicated to Gradial while the app runs.** Its normal browser
back/navigation behavior is suppressed. An assigned slice is compiled into an
immutable, bounded plan at release. Delay, keyboard-shortcut, Unicode-text,
URL-open, path-open, direct executable, and invocation-window steps execute on
one worker; another dispatch while it is busy fails immediately instead of
queuing. Before input,
Gradial verifies that the captured window still has the same process/start
time, remains foreground, and is not at higher integrity. Text is sent as
literal Unicode without clipboard access or environment expansion, and the
target is checked between input events. Open actions use the Windows
association API directly; missing paths fail without dispatch and URLs/paths
are omitted from logs.
Executable actions accept only absolute existing `.exe` files, keep arguments
as an array until the Windows process-creation boundary, validate an optional
absolute working directory, inherit the normal process environment, and omit
all paths and argument values from logs. They never invoke a command shell and
do not wait for the child process to exit.
Window actions revalidate the frozen HWND, PID, process start time, foreground
identity, and integrity immediately before posting one asynchronous command.
Close sends a normal close request and never terminates the target process.
Unassigned slices cancel as `EmptySlice`; reusable sequences stop on the first
failure or cancellation.

The display under the M4-down cursor selects the prewarmed overlay window.
Display, work-area, or DPI changes cancel an active gesture and rebuild the
window pool before M4 is accepted again. Every overlay is click-through and
never intentionally takes focus.

Gradial keeps a notification-area icon visible while it runs. Its icon and
first menu line show `Healthy`, `Recovering input`, or `Input faulted`. A missed
input heartbeat triggers automatic recovery after 750 ms. Queue overflow and
startup failures remain faulted with M4 passed through until **Retry input** is
chosen. The same menu provides **Open settings**, **Cancel current action**
while a plan is running, a **Recent activity** submenu with **Clear activity**,
and **Exit Gradial**. Activity is never written to disk. It contains only fixed
public messages derived from stable error codes, not action text, URLs, paths,
arguments, or target titles. Input is also re-registered after resume, session
unlock, or an Explorer restart.

First startup now creates `%LOCALAPPDATA%\Gradial\config.json` with schema 2,
revision 1, a global eight-slice profile, and an empty action catalog. Existing
schema-one files are migrated to schema two in memory without being rewritten;
the next explicit successful save will persist schema two. Configuration
replacement is validated and atomic, preserves the previous valid primary as
`config.json.bak`, and recovers from that backup without overwriting a broken
primary. Application-profile rules are resolved at every activation. The
selected profile's complete menu—including optional action UUIDs—is frozen for
the invocation, so title or configuration changes cannot switch geometry,
labels, or the selected action while a gesture is open. Delay, keyboard
shortcut, Unicode text, HTTP/HTTPS URL, existing absolute file/folder open, and
direct `.exe` launch are enabled together with close, minimize, maximize, and
restore for the invocation window.

Choose **Open settings** from the notification-area menu to edit the global
menu. The window can switch between four and eight directions, edit every
visible label, assign or clear catalog actions, and select directions directly
in the live radial preview. The **Actions** tab creates and edits keyboard
shortcuts, Unicode text, websites, files/folders, applications, Delay, and
invocation-window commands. Application arguments are separate literal rows,
and native Windows choosers select files, folders, applications, and working
folders. The catalog can rename or duplicate every action and safely delete
unreferenced actions. Sequences provide ordered add, remove, move-up, and
move-down controls; only additions that avoid cycles and stay within nested
depth, executed-step, and Delay limits are offered. Closing a dirty draft
requires **Discard changes** or **Keep editing**. A successful save validates
and compiles the complete proposed configuration before atomic replacement,
advances its revision, and reloads new M4 invocations without restarting
Gradial. Application profiles, activation binding, import/export, icons, and
onboarding remain future slices.

Run all local checks with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check.ps1
```

After building the debug executable, the isolated settings-window smoke check
can also capture the editor and close-confirmation surfaces:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-settings-editor.ps1 -CaptureScreenshot
```

Run the standard live interaction matrix after building the debug executable:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-interaction-matrix.ps1 -Suite standard
```

It writes ignored, versioned JSON and CSV results under
`Gradial APP\captures\interaction-matrix`.

See [`Documentation/development.md`](Documentation/development.md) for the
threading model, safety boundaries, timing capture, workspace layout, and
current limitations. The latest bounded interaction evidence is recorded in
[`Documentation/milestone-0-smoke-results.md`](Documentation/milestone-0-smoke-results.md),
and the per-display pool evidence is in
[`Documentation/milestone-1-overlay-pool-results.md`](Documentation/milestone-1-overlay-pool-results.md).
Four/eight geometry results are recorded in
[`Documentation/milestone-1-slice-geometry-results.md`](Documentation/milestone-1-slice-geometry-results.md).
Input recovery and tray results are recorded in
[`Documentation/milestone-1-input-recovery-results.md`](Documentation/milestone-1-input-recovery-results.md).
Renderer cache and animation results are recorded in
[`Documentation/milestone-1-renderer-animation-results.md`](Documentation/milestone-1-renderer-animation-results.md).
The repeatable interaction-matrix contract and current coverage are recorded in
[`Documentation/milestone-1-interaction-matrix-results.md`](Documentation/milestone-1-interaction-matrix-results.md).
Configuration, snapshot, persistence, and resolver results are recorded in
[`Documentation/milestone-2-configuration-profile-results.md`](Documentation/milestone-2-configuration-profile-results.md).
Profile-driven gesture and renderer results are recorded in
[`Documentation/milestone-2-profile-menu-results.md`](Documentation/milestone-2-profile-menu-results.md).
Schema-two action validation and plan-compilation results are recorded in
[`Documentation/milestone-2-action-model-results.md`](Documentation/milestone-2-action-model-results.md).
Single-plan coordination and Delay execution results are recorded in
[`Documentation/milestone-2-delay-executor-results.md`](Documentation/milestone-2-delay-executor-results.md).
Target validation and keyboard-shortcut results are recorded in
[`Documentation/milestone-2-keyboard-shortcut-results.md`](Documentation/milestone-2-keyboard-shortcut-results.md).
Unicode text-entry and privacy results are recorded in
[`Documentation/milestone-2-unicode-text-results.md`](Documentation/milestone-2-unicode-text-results.md).
URL/file/folder association results are recorded in
[`Documentation/milestone-2-shell-open-results.md`](Documentation/milestone-2-shell-open-results.md).
Direct executable-launch results are recorded in
[`Documentation/milestone-2-process-launch-results.md`](Documentation/milestone-2-process-launch-results.md).
Invocation-window action results are recorded in
[`Documentation/milestone-2-window-action-results.md`](Documentation/milestone-2-window-action-results.md).
Action activity and failure-notification results are recorded in
[`Documentation/milestone-2-action-feedback-results.md`](Documentation/milestone-2-action-feedback-results.md).
Global-menu settings-editor and live-reload results are recorded in
[`Documentation/milestone-2-settings-menu-editor-results.md`](Documentation/milestone-2-settings-menu-editor-results.md).
Action-catalog and shortcut-recorder results are recorded in
[`Documentation/milestone-2-action-editor-results.md`](Documentation/milestone-2-action-editor-results.md).
Remaining leaf-action editor and native-picker results are recorded in
[`Documentation/milestone-2-leaf-action-editor-results.md`](Documentation/milestone-2-leaf-action-editor-results.md).
Reusable-sequence editor results are recorded in
[`Documentation/milestone-2-sequence-editor-results.md`](Documentation/milestone-2-sequence-editor-results.md).
