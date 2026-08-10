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
request only to the window frozen at activation-down after revalidating its identity
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
ordered, safely nested action lists with live expansion limits. The application
profile editor creates, duplicates, and deletes exact-executable profiles,
sets deterministic priority, and gives each profile an independent
four/eight-direction menu. Optional exact window-class and title-regex rules
are editable, and a private countdown tester explains every matching profile,
winner, and deterministic tie-break.
The activation editor now configures M4 or M5 with an exact optional
Ctrl/Alt/Shift requirement and includes a safe one-shot hold/flick/release
tester. Required first-run onboarding now guides the user through choosing and
acknowledging that dedicated chord, opening the production radial interaction,
selecting an inert sample slice, and cancelling once before completion. The
appearance editor adds an accent color, contrasting light/dark label themes,
and compact/standard/large visual sizing through the same production preview.
Each Global or application-profile direction can now import, replace, or
remove a bounded PNG/SVG icon, rendered by that same production path. Settings
exports carry referenced icons in an adjacent `icons` directory for portable
round trips. The first Milestone 3 slices add a dedicated Diagnostics page
that previews a frozen local ZIP before export, a per-user start-at-login
control that detects and repairs stale Windows registration, and a versioned
performance-report harness with QPC hook/dispatch timing, PresentMon
correlation, resource sampling, and explicit unavailable/not-run evidence.
Configured application paths remain excluded from diagnostics unless the user
explicitly includes them. The first bounded v0.3 slice adds pure,
DPI-independent continuous distance, live direction with reversible
inside/outside behavior, a production-overlay progress track, an edge-assist
pointer proxy, and a debug-only inert meter. The meter exercises the real
input, edge-assist, interaction, rendering, focus, and close paths while
remaining unable to compile or dispatch a saved action.

The implementation baseline is
[`gradial-v0.2-implementation-plan.md`](gradial-v0.2-implementation-plan.md).
The broader v0.1 document is retained for historical context, but v0.2 controls
the MVP. [`gradial-v0.3-implementation-plan.md`](gradial-v0.3-implementation-plan.md)
controls the continuous-input and plugin extension work.

## Build and run

Requirements:

- Windows 11 x86-64.
- Rust 1.97.1 with the MSVC target.
- Visual Studio Build Tools with the Windows SDK.

From `Gradial APP`:

```powershell
cargo run --package gradial-app
```

The app starts hidden. With the default binding:

- Hold mouse button 4 (M4) to open the menu at the cursor.
- Move beyond the center zone to highlight a slice.
- Release M4 to log the selected slice and close the menu.
- Press Escape, or press another mouse button, to cancel.

Gradial normally keeps the gesture fixed at activation-down. If the complete ring would
clip the current monitor's work area, it automatically insets the ring and uses
raw mouse deltas as a virtual pointer. The system cursor stays at the invocation
point during that edge-assisted gesture; continuous input displays a bounded
pointer proxy inside the overlay.

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

The debug-only continuous meter makes every direction an inert hybrid wedge.
Inside the visible button ring, release follows the normal discrete-selection
path; crossing its outer edge switches the live wedge to a `0–100%` continuous
meter. For the standard menu size that boundary is 107 DIP and the current
recommended endpoint is 300 DIP. Compact and Large boundaries follow their
visible outer edges. The wedge follows live angle at every radius, while radial
distance independently chooses discrete or continuous mode and the scalar. The
meter uses the wider edge-assist extent and never compiles or dispatches the
saved slice action:

```powershell
cargo run --package gradial-app -- --continuous-meter --skip-onboarding
```

Debug builds can try any whole-DIP endpoint from 160 through 400 without
changing saved configuration:

```powershell
cargo run --package gradial-app -- --continuous-meter --continuous-full-scale-dip=400 --skip-onboarding
```

Returning across the outer edge removes the meter immediately and restores the
discrete wedge. Releasing there selects that button; moving outside again
restores continuous mode. Returning to the center clears the selection.
The center does not end the held gesture; moving outward can select again, and
only releasing there cancels. Direction never locks, continuous mode never
latches, and the value always follows current straight-line distance rather
than accumulated travel.

The planned v0.3 Settings editor enables continuous control per wedge with a
checkbox that defaults off. Unchecked wedges remain ordinary discrete actions
at every distance, which suits commands such as Copy or Open. A separate
global **Continuous travel** control will default to 300 DIP and allow exact
values from 160 through 400.

The v0.3 Milestone 1 core defines a runtime-neutral, version-one
Begin/Update/Commit/Cancel lifecycle with a single latest-value slot, 10–60 Hz
cadence, strictly increasing samples, exact terminal ordering, and
permission-filtered owned context. It performs no Lua or operating-system
work.

A staged asynchronous executor now proves the next internal boundary. It uses
the same single-action lease as ordinary shortcuts, keeps that lease across
direction handoffs, separates terminal controls from its one-value update
slot, enforces the planned callback deadlines, and is tested with a fake
accept/reject/stall/fault backend. The live gesture bridge now starts this
worker only for the debug inert meter and its studies. It sends the exact
displayed scalar, suspends without losing the held lease when the pointer
returns inside, resumes on any unlocked wedge, and validates the captured
target before `Begin` and while active. Normal configured menus still start no
continuous worker until schema-six action assignments are implemented.

High-polling mice do not enqueue a render/log operation for every physical
sample. Gradial still evaluates every sample for live direction and radius,
but each input-queue drain publishes only its newest continuous
visual state. Release and cancellation remain ordered after the final sample.
Each per-monitor native overlay is explicitly woken after show and state
changes, and active animations keep that native repaint path alive until their
bounded lifecycle completes.

The debug-only guided study runs the Section 11.3 participant protocol with
configured actions disabled:

```powershell
cargo run --package gradial-app -- --continuous-study
```

Each participant completes practice, balanced scalar trials, four inside-ring
direction corrections, four outside-ring wedge changes, and one center-clear
check.
Gradial asks for no name and writes one
anonymous JSON report under
`%LOCALAPPDATA%\Gradial\continuous-studies`. After collecting at least eight
reports, evaluate the cohort gates with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/summarize-continuous-study.ps1
```

A separate debug-only comparison keeps direction E and the target 25% fixed
while testing 160, 200, 250, and 300 DIP as the 100% endpoint:

```powershell
cargo run --package gradial-app -- --continuous-distance-study
```

It shows the measured release after every trial, then asks which distance felt
best. The comparison is inert, does not alter profiles or actions, and writes a
dedicated `continuous-distance-study-<session-id>.json` report beside the
guided-study reports. These comparison files are intentionally separate from
the cohort summarizer, and choosing a favorite does not yet create a
production speed preset.

The overlay opens from 96% scale and 85% opacity over 60 ms, highlights the
current wedge immediately, and fades out over 50 ms. Gesture recognition and
release do not wait for the lifecycle animations. A temporary development
override removes opening and closing interpolation while preserving the same
lifecycle timing:

```powershell
cargo run --package gradial-app -- --reduced-motion
```

**The saved activation chord is dedicated to Gradial while the app runs.**
With the default unmodified M4 chord, normal browser back/navigation behavior
is suppressed. If modifiers are required, the side button without that exact
Ctrl/Alt/Shift combination continues to pass through. An assigned slice is compiled into an
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

The display under the activation-down cursor selects the prewarmed overlay window.
Display, work-area, or DPI changes cancel an active gesture and rebuild the
window pool before the configured chord is accepted again. Every overlay is click-through and
never intentionally takes focus.

Gradial keeps a notification-area icon visible while it runs. Its icon and
first menu line show `Healthy`, `Recovering input`, or `Input faulted`. A missed
input heartbeat triggers automatic recovery after 750 ms. Queue overflow and
startup failures remain faulted with the activation chord passed through until **Retry input** is
chosen. The same menu provides **Open settings**, **Cancel current action**
while a plan is running, a **Recent activity** submenu with **Clear activity**,
**Restart onboarding**, and **Exit Gradial**. Activity is never written to disk. It contains only fixed
public messages derived from stable error codes, not action text, URLs, paths,
arguments, or target titles. Input is also re-registered after resume, session
unlock, or an Explorer restart.

First startup opens required onboarding and creates
`%LOCALAPPDATA%\Gradial\config.json` with schema 6, revision 1, the default M4
binding, original blue/light/standard appearance, a global eight-slice profile,
an empty action catalog, and a 300-DIP continuous full-scale distance. Onboarding
does not finish until the user acknowledges that the exact activation chord is
dedicated while Gradial runs, opens the real radial interaction, selects one
inert sample slice, and cancels once through the center or Escape. Practice
uses the production hook, controller, geometry, and overlay but cannot compile
or dispatch a configured action. Successful completion is saved separately in
`onboarding-v1.json`; the tray's **Restart onboarding** command replays the
flow without making a previously completed installation incomplete.
Existing schema-one through schema-five files are migrated to schema six in
memory without being rewritten; the next explicit successful save persists
schema six. Schema-five migration adds an unchecked continuous assignment to
every wedge and the 300-DIP default while retaining existing icons. Configuration
replacement is validated and atomic, preserves the previous valid primary as
`config.json.bak`, and recovers from that backup without overwriting a broken
primary. Application-profile rules are resolved at every activation. The
selected profile's complete menu—including separate optional discrete and
continuous action UUIDs plus managed icon references—is frozen for
the invocation, so title or configuration changes cannot switch geometry,
labels, or the selected action while a gesture is open. Delay, keyboard
shortcut, Unicode text, HTTP/HTTPS URL, existing absolute file/folder open, and
direct `.exe` launch are enabled together with close, minimize, maximize, and
restore for the invocation window.

Choose **Open settings** from the notification-area menu to edit the global
menu. The **Activation** tab selects M4 or M5 and optional generic Ctrl, Alt,
and Shift requirements. Matching is exact. Its one-shot tester consumes the
next side-button attempt, reports the recognized eight-way flick or centered
cancel, and never opens the production menu, launches an action, or navigates
another application. Edits remain a private draft until **Save changes**.
The **Appearance** tab provides an RGB accent picker, light or dark labels, and
compact, standard, or large rendering. Dark labels automatically receive a
light surface and backing for contrast. The live preview updates from the
draft, while production keeps the saved appearance until the complete draft
is atomically saved.
The window can switch between four and eight directions, edit every
visible label, assign or clear catalog actions, import/replace/remove a custom
icon, and select directions directly in the live radial preview. Icon imports
accept PNG or SVG sources no larger than 256 KiB or 512×512 logical pixels,
then copy them under `%LOCALAPPDATA%\Gradial\icons`. Missing or damaged managed
files render a built-in placeholder instead of blocking the radial menu.
Gradial protects every icon referenced by the active settings, backup, or open
draft. Exact unreferenced UUID-named assets move into a recoverable
`icons\quarantine` directory; unknown files are never touched.
The **Profiles** tab creates an application profile
by choosing an `.exe`, copies the current Global menu as an independent
starting point, and supports rename, priority, duplication, deletion, complete
four/eight-direction editing, action assignment, and a production preview.
Optional window-class matching is exact and case-insensitive; optional window
title matching uses a case-sensitive regular expression. **Test profiles**
provides three seconds to switch applications, captures that foreground
window, and displays its path, class, title, all matching profiles, the winner,
and the precise priority/specificity/UUID decision. Captured details remain
only in editor memory and are cleared when settings closes.
The **Actions** tab creates and edits keyboard shortcuts, Unicode text,
websites, files/folders, applications, Delay, and invocation-window commands.
Application arguments are separate literal rows, and native Windows choosers
select files, folders, applications, and working folders. The catalog can
rename or duplicate every action and safely delete unreferenced actions.
Sequences provide ordered add, remove, move-up, and move-down controls; only
additions that avoid cycles and stay within nested depth, executed-step, and
Delay limits are offered. Closing a dirty draft requires **Discard changes**
or **Keep editing**. **Import settings…** reads schema-one through schema-six
JSON into the private editor draft, fully validates it, and asks
before replacing existing unsaved edits. It does not change the running app or
`config.json` until **Save changes**. **Export current draft…** writes the
complete valid draft—including unsaved edits—to a schema-six JSON file
through an atomic adjacent temporary file. When the draft references custom
icons, export validates each managed source and copies each unique asset into
an adjacent `icons` directory. Import validates that optional directory and
copies its assets into managed storage. Byte-identical UUID assets are reused;
same-ID files with different bytes are rejected instead of overwritten.
Missing adjacent assets do not block a JSON-only import: Settings reports the
count and the affected directions use placeholders. Imported/exported action
paths, arguments, labels, and text stay literal and are not rewritten for
portability.
Schema six can preserve a `plugin_continuous` action and a wedge's separate
`continuous_action_id` even when the referenced plugin is not installed. Such
an extension is deliberately unavailable in this build: the wedge remains an
ordinary discrete wedge at every radius, and release still uses only its
discrete action. Settings retains these portable definitions, shows their
identity read-only in the action catalog, and excludes them from discrete
assignment and sequences.

The dedicated plugin worker performs read-only discovery below
`%LOCALAPPDATA%\Gradial\plugins`. A strict `plugin.toml` declares identity,
version, API, entry source, permissions, continuous actions, and bounded
parameters. Each immediate package directory is independently classified as
compatible, API-incompatible, or invalid; one malformed package cannot hide a
healthy one. Discovery canonicalizes every package/source path, rejects
reparse points and native or bytecode modules, accepts only bounded UTF-8 Lua
source, freezes accepted bytes, and never executes them during discovery.

Gradial also initializes a separate, machine-local
`%LOCALAPPDATA%\Gradial\plugins-state.json`. It records only revisioned
enabled/faulted state, approved permission names, and the identity/fingerprint
of the last successfully loaded package. Permission changes disable an enabled
record until reviewed; ordinary manifest or Lua edits retain approval but are
marked as requiring manual reload. Atomic `.tmp`/`.bak` recovery fails closed
to an empty disabled state and never stores source, paths, parameters, or
credentials. The plugin worker owns these transitions and the Lua states they
authorize; the UI only submits bounded commands and reads immutable snapshots.
The **Plugins** Settings page is now the user-facing caller: it presents the
discovered inventory and requested permissions, asks for explicit trust before first
enablement or after permission changes, and submits rescan, enable, disable,
and reload work asynchronously. Those machine-local changes apply immediately
and never modify or save the open menu-settings draft. **Open plugins folder**
opens the managed package root.

The separate `gradial-lua` foundation embeds vendored Lua 5.4. A fresh state
exposes only reviewed basic functions,
`math`, `string`, `table`, `utf8`, and the rooted/cached
`gradial.require("lib.mapping")` source loader. It has no filesystem-backed
loader; its only current operating-system surface is the explicit protected
shortcut and notification capability set. Memory, instruction, and wall-time
limits remain enforced.
Its strict lifecycle adapter validates and bounds `begin`, `update`, `commit`,
and `cancel` with private session state and immutable host values. At startup,
the bounded worker constructs one adapter only for a freshly rediscovered
package whose effective trust status is exactly enabled. Newly discovered,
disabled, changed, incompatible, invalid, unavailable, and faulted packages
execute zero Lua. Enable and reload validate Lua first, atomically record the
exact successful package second, and admit the adapter last. The worker accepts
bounded Begin, Update, Commit, and Cancel
requests through separate one-slot lanes; Commit and Cancel share a reserved
terminal lane and are selected before ordinary updates. It owns one active
plugin session, faults and unloads an adapter after a callback failure, and
defers package-change teardown observed during a session until its terminal
callback. The production continuous route freezes an enabled manifest action,
its validated parameters, update cadence, permission-filtered context, and
target identity at invocation-down. Rescan is code-free; unapproved source is
rejected before Lua evaluation even if enablement is requested outside the
Settings UI.

Plugins approved for `input.shortcut` can request one readable, complete key
chord through the protected `gradial.input.shortcut(...)` API during Begin,
Update, or Commit. A bounded broker handles Windows input outside Lua and the
interaction runtime, permits at most 30 accepted requests per active session
per second, rechecks the frozen target and permission, and always uses the
existing stuck-key cleanup path. Cancellation closes the request epoch before
Lua's Cancel callback, and Commit waits within its existing deadline for
accepted work to finish. Shortcut contents, target details, and plugin values
remain absent from logs.

Plugins approved for `notification` can request one bounded, single-line
message through `gradial.notification(...)`. Gradial rejects control characters
and URL-shaped content, never interprets markup, offers no action button, and
shows the plugin ID in a silent native information notification. Each gesture
may accept one message; each plugin may accept five in a rolling minute.
Pending delivery is cancelled with the gesture or permission epoch, and the
native request remains outside Lua and interaction threads.

A permission-free no-side-effect package is included at
[`Gradial APP/examples/plugins/org.gradial.example.meter`](Gradial%20APP/examples/plugins/org.gradial.example.meter).
It records only private session-local scalar state and is used by the worker
lifecycle tests. The permission-checked
[`Gradial APP/examples/plugins/org.gradial.example.shortcut-bands`](Gradial%20APP/examples/plugins/org.gradial.example.shortcut-bands)
example demonstrates bounded radial bands with at most one whole shortcut
request per callback. The
[`Gradial APP/examples/plugins/org.gradial.example.notification`](Gradial%20APP/examples/plugins/org.gradial.example.notification)
example requests one plain notification only when its gesture commits.

The **General** tab keeps **Start at login** outside portable menu
configuration. Gradial never enables it automatically, and it uses only the
current Windows account, so it does not require administrator access. Gradial reports
whether the entry is off, on, or needs repair because it no longer points
exactly to the running application. Repair replaces only Gradial's named
entry. Turning the option off removes only that entry; neither operation
touches another application's startup value. Importing or exporting settings
never changes this preference.
The **Diagnostics** tab prepares a frozen preview listing every ZIP entry,
current hook-thread status, and physical display/DPI record. The default
bundle contains `manifest.json` and sanitized copies of Gradial's bounded
logs. **Include configured application paths** is off for every new preview;
turning it on displays the exact deduplicated paths and adds only
`configured-process-paths.json`. Settings, typed text, URLs, executable
arguments, working folders, file/folder paths, window titles, clipboard data,
and custom icons are always excluded. Export writes an atomic local `.zip`;
Gradial never uploads it.
A successful save validates and compiles the complete
proposed configuration before atomic replacement, advances its revision, and
reloads new invocations without restarting Gradial.

Run all local checks with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check.ps1
```

After building the debug executable, the isolated settings-window smoke check
can also capture the editor and close-confirmation surfaces:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-settings-editor.ps1 -CaptureScreenshot
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-onboarding.ps1 -CaptureScreenshot
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-start-at-login.ps1 -CaptureScreenshot
```

Run the standard live interaction matrix after building the debug executable:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-interaction-matrix.ps1 -Suite standard
```

It writes ignored, versioned JSON and CSV results under
`Gradial APP\captures\interaction-matrix`.

Run the continuous-input matrix separately:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-continuous-matrix.ps1 -Suite standard
```

It writes ignored JSON and CSV evidence under
`Gradial APP\captures\continuous-matrix`. Automated results deliberately leave
the eight-participant usability study as `not_run`; the standard suite includes
a post-arm N-to-W wedge change and a 1,000-event fixed-origin pointer burst on
the real Windows input path. It also performs 20 rapid edge-assist invocations
that must draw within 100 ms and reach full opacity before release.

Run a short, non-qualifying local performance baseline after building the
release executable:

```powershell
cargo build --workspace --all-features --release
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-performance-report.ps1 -Mode Development -MarkersOnly
```

An elevated `-Mode Release` run enables PresentMon and uses the complete
five-minute warmup, 1,000-invocation, and 30-minute idle protocol.

See [`Documentation/development.md`](Documentation/development.md) for the
threading model, safety boundaries, timing capture, workspace layout, and
current limitations. The latest bounded interaction evidence is recorded in
[`Documentation/milestone-0-smoke-results.md`](Documentation/milestone-0-smoke-results.md),
and continuous-input prototype evidence is recorded in
[`Documentation/milestone-0-continuous-input-results.md`](Documentation/milestone-0-continuous-input-results.md).
The guided participant protocol is documented in
[`Documentation/milestone-0-continuous-study-results.md`](Documentation/milestone-0-continuous-study-results.md).
The per-display pool evidence is in
[`Documentation/milestone-1-overlay-pool-results.md`](Documentation/milestone-1-overlay-pool-results.md).
Four/eight geometry results are recorded in
[`Documentation/milestone-1-slice-geometry-results.md`](Documentation/milestone-1-slice-geometry-results.md).
Input recovery and tray results are recorded in
[`Documentation/milestone-1-input-recovery-results.md`](Documentation/milestone-1-input-recovery-results.md).
Renderer cache and animation results are recorded in
[`Documentation/milestone-1-renderer-animation-results.md`](Documentation/milestone-1-renderer-animation-results.md).
The repeatable interaction-matrix contract and current coverage are recorded in
[`Documentation/milestone-1-interaction-matrix-results.md`](Documentation/milestone-1-interaction-matrix-results.md).
The runtime-neutral continuous protocol, coordinator lease, live interaction
bridge, and schema-six configuration evidence are recorded in
[`Documentation/milestone-1-continuous-protocol-results.md`](Documentation/milestone-1-continuous-protocol-results.md),
[`Documentation/milestone-1-continuous-coordinator-results.md`](Documentation/milestone-1-continuous-coordinator-results.md),
[`Documentation/milestone-1-continuous-live-bridge-results.md`](Documentation/milestone-1-continuous-live-bridge-results.md),
and
[`Documentation/milestone-1-schema-six-results.md`](Documentation/milestone-1-schema-six-results.md).
Read-only plugin manifest and package-discovery evidence is recorded in
[`Documentation/milestone-2-plugin-discovery-results.md`](Documentation/milestone-2-plugin-discovery-results.md).
Frozen-source and restricted Lua-host evidence is recorded in
[`Documentation/milestone-2-lua-sandbox-results.md`](Documentation/milestone-2-lua-sandbox-results.md).
Strict lifecycle-adapter, atomic trust-state, and isolated worker evidence is
recorded in
[`Documentation/milestone-2-lua-lifecycle-results.md`](Documentation/milestone-2-lua-lifecycle-results.md),
[`Documentation/milestone-2-plugin-trust-state-results.md`](Documentation/milestone-2-plugin-trust-state-results.md),
[`Documentation/milestone-2-plugin-worker-results.md`](Documentation/milestone-2-plugin-worker-results.md),
[`Documentation/milestone-2-plugin-settings-results.md`](Documentation/milestone-2-plugin-settings-results.md),
and
[`Documentation/milestone-2-plugin-lifecycle-bridge-results.md`](Documentation/milestone-2-plugin-lifecycle-bridge-results.md).
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
Application-profile editor results are recorded in
[`Documentation/milestone-2-application-profile-editor-results.md`](Documentation/milestone-2-application-profile-editor-results.md).
Advanced profile matching and private tester results are recorded in
[`Documentation/milestone-2-profile-matching-editor-results.md`](Documentation/milestone-2-profile-matching-editor-results.md).
Activation binding and safe gesture-tester results are recorded in
[`Documentation/milestone-2-activation-binding-editor-results.md`](Documentation/milestone-2-activation-binding-editor-results.md).
Settings import/export and atomic draft-transfer results are recorded in
[`Documentation/milestone-2-settings-transfer-results.md`](Documentation/milestone-2-settings-transfer-results.md).
Required onboarding and safe production-interaction practice results are
recorded in
[`Documentation/milestone-2-onboarding-results.md`](Documentation/milestone-2-onboarding-results.md).
Appearance configuration and production-renderer results are recorded in
[`Documentation/milestone-2-appearance-results.md`](Documentation/milestone-2-appearance-results.md).
Custom icon import, validation, caching, fallback, and renderer results are
recorded in
[`Documentation/milestone-2-custom-icons-results.md`](Documentation/milestone-2-custom-icons-results.md).
Portable icon export/import and collision-handling results are recorded in
[`Documentation/milestone-2-portable-icon-transfer-results.md`](Documentation/milestone-2-portable-icon-transfer-results.md).
Recoverable managed-icon quarantine results are recorded in
[`Documentation/milestone-2-icon-quarantine-results.md`](Documentation/milestone-2-icon-quarantine-results.md).
Release logging and diagnostic-bundle preview/export results are recorded in
[`Documentation/milestone-3-diagnostic-bundle-results.md`](Documentation/milestone-3-diagnostic-bundle-results.md).
Per-user start-at-login ownership, repair, and native registry results are
recorded in
[`Documentation/milestone-3-start-at-login-results.md`](Documentation/milestone-3-start-at-login-results.md).
Performance protocol, report semantics, and the current non-qualifying
development baseline are recorded in
[`Documentation/milestone-3-performance-harness-results.md`](Documentation/milestone-3-performance-harness-results.md).
