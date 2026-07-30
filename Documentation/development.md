# Gradial development guide

## Current milestone scope

The foundation, Milestone 0 interaction spike, completed bounded Milestone 1
and Milestone 2 slices, and the first three bounded Milestone 3 slices establish:

- A Rust 2024 workspace pinned to the installed Rust 1.97.1 toolchain.
- A reusable `gradial-core` crate.
- A Windows `gradial` executable rendered by egui through wgpu.
- One prewarmed transparent, borderless, topmost, click-through overlay per
  active display.
- Per-Monitor V2 DPI awareness through the embedded application manifest.
- `%LOCALAPPDATA%\Gradial` directories for configuration, icons, and bounded
  local logs.
- Formatting, Clippy, unit-test, and build checks.
- A dedicated Win32 low-level mouse/keyboard hook thread.
- A saved M4/M5 activation chord with optional exact Ctrl/Alt/Shift
  requirements, plus Escape as cancellation.
- A fixed-capacity, single-producer/single-consumer input queue.
- An explicit, pure gesture state machine with four/eight slices and
  hysteresis.
- Hidden-at-idle overlay lifecycle, physical-pixel positioning, and selection
  highlighting.
- Automatic fixed-origin/edge-assist resolution from signed work-area geometry.
- A raw-input message window and virtual gesture pointer for edge assist.
- Focus checkpoints at activation, after the first rendered-frame checkpoint,
  and after close.
- An ETW provider whose invocation markers can be correlated with PresentMon.
- Display/DPI/work-area change detection that suspends new gestures, cancels an
  active gesture, replaces the overlay pool, and restarts input only after every
  new viewport is warm.
- An always-visible native tray icon with Healthy, Recovering, and Faulted input
  states plus Retry Input and Exit commands.
- Automatic full input-runtime replacement after three missed 250 ms
  heartbeats, and input re-registration after resume, session unlock, or
  Explorer restart.
- Persistent per-monitor geometry caches and bounded 60/35/50 ms
  open/selection/close animation state.
- A validated schema-five configuration with safe in-memory schema-one through
  schema-four migration plus atomic primary/backup
  replacement.
- Complete immutable target/display/modifier identity for every invocation.
- A compiled, deterministic application-profile resolver with explanation
  data.
- One immutable resolved menu per invocation, shared by gesture recognition
  and rendering.
- Prewarmed four/eight geometry caches on every display and rendering of
  configured profile labels.
- Validated built-in action definitions and optional menu-slice action UUIDs.
- Cycle-, depth-, step-, and delay-bounded sequence validation.
- Immutable release-time execution-plan compilation tied to the invocation's
  frozen configuration revision.
- One bounded action worker with immediate busy rejection, cancellable Delay
  execution, timeouts, and a two-second shutdown wait.
- Frozen-target freshness/integrity validation and scan-code keyboard
  shortcuts with synthesized-key cleanup.
- Literal UTF-16 Unicode text entry with per-event target revalidation and
  in-flight code-unit cleanup.
- HTTP/HTTPS URL and existing absolute file/folder opening through Windows
  shell associations, with literal payloads and redacted diagnostics.
- Direct launch of existing absolute `.exe` files through `CreateProcessW`,
  with exact argument-array preservation, validated working directories,
  inherited environment, immediate handle cleanup, and redacted diagnostics.
- Close, minimize, maximize, and restore for the frozen invocation window,
  with full target revalidation and one asynchronous message dispatch.
- A session-only, 20-item tray activity panel for action failures and
  cancellations, with clear-on-command behavior.
- Native Windows failure notifications that do not activate Gradial; successful
  actions and cancellations do not notify.
- A single-instance native settings window for global-menu four/eight geometry,
  labels, existing-action assignments, and a production-renderer preview.
- An action catalog that creates and edits keyboard shortcuts and Unicode text,
  records physical shortcut data, and supports rename, duplication, and
  reference-safe deletion.
- Leaf-action editors for websites, files/folders, direct applications, Delay,
  and invocation-window commands, with native Windows path choosers and
  literal application-argument rows.
- An ordered reusable-sequence editor with nested-sequence support,
  validator-backed candidate filtering, and live depth, expanded-step, and
  Delay metrics.
- An application-profile catalog with exact executable selection, explicit
  priority, complete-menu duplication, lifecycle controls, and independent
  four/eight-slice editing through the production preview.
- Optional exact window-class and title-regex rule controls plus a private
  countdown tester that uses the production resolver to explain matches,
  winner, and tie-break.
- A draft-only activation editor for M4/M5 and generic Ctrl/Alt/Shift, with a
  one-shot hold/flick/release tester that cannot dispatch an action.
- Draft-safe close confirmation, complete pre-save validation, atomic
  replacement, and immediate live configuration reload.
- Native JSON import into the private settings draft and atomic export of the
  current valid draft, including unsaved edits and an optional adjacent
  portable-icon directory.
- Required, versioned first-run onboarding that chooses the exact activation
  chord, requires the dedicated-button acknowledgment, and practices selection
  and cancellation through an action-free production interaction.
- Draft-safe accent, label-theme, and size-preset appearance editing through
  the production renderer, with immediate post-save application.
- Per-slice managed PNG/SVG icons with bounded import validation, load-time
  texture caching, production-preview rendering, and non-fatal placeholders.
- Reference-aware managed-icon restoration and recoverable quarantine across
  active, backup, and open-draft settings.
- A frozen diagnostic-bundle preview with sanitized bounded logs,
  app/input/display metadata, explicit configured-process-path inclusion, and
  atomic local ZIP export.
- A default-off, per-user Windows start-at-login control with exact
  registration inspection, stale-entry repair, and no administrator
  requirement.
- A versioned performance-report harness with QPC hook/dispatch timing,
  PresentMon displayed-frame correlation, phased resource sampling, and
  explicit evidence-status reporting.
- The first v0.3 continuous-input prototype: pure 48–160 DIP normalization,
  per-invocation continuous-slice masks, direction locking, center-stop
  behavior, a production-overlay scalar track, and a debug-only meter that
  cannot compile or dispatch configured actions.
- A dedicated continuous interaction matrix that checks four/eight directions,
  normalized target values, focus, edge assist, inert execution, and
  machine-readable coverage without fabricating the pending human study.

Configuration editing currently covers activation, the global menu, the
complete action catalog including sequences, and exact-executable application
profiles with their independent menus and complete matching rules. Required
onboarding, v1 appearance, and per-slice custom icons are complete. A release logs the
chosen slice, compiles its assigned plan, and submits it to the coordinator.
Every schema-two action family can execute through its bounded backend.

## Workspace layout

```text
Gradial APP\
    Cargo.toml
    crates\
        gradial-core\    Pure geometry, gesture state, and application paths
        gradial-app\     Windows input, persistence, executable, and renderer
    scripts\
        check.ps1          Complete local code verification
        check-focus.ps1    Validate the latest completed focus trace
        capture-timing.ps1 Capture ETW markers and presented-frame timings
        run-performance-report.ps1 Run development or release performance protocol
        run-continuous-matrix.ps1 Run the continuous direction/distance matrix
        smoke-interaction.ps1 Exercise a real hidden app with synthetic M4 input
        smoke-delay-executor.ps1 Run an isolated Delay action fixture
        smoke-input-executor.ps1 Shared isolated input-action fixture
        smoke-shortcut-executor.ps1 Verify scan-code shortcut delivery
        smoke-text-executor.ps1 Verify exact private Unicode delivery
        input-smoke-target.ps1 Capture key and Unicode messages safely
        smoke-open-executor.ps1 Verify an isolated Windows file association
        open-smoke-receiver.ps1 Capture the exact associated file path
        smoke-process-executor.ps1 Verify direct process launch in isolation
        smoke-window-executor.ps1 Verify all invocation-window commands
        window-smoke-target.ps1 Observe isolated window messages and states
        smoke-action-feedback.ps1 Verify failure activity, notification, focus, and privacy
        smoke-settings-editor.ps1 Verify native draft, diagnostic ZIP, save, and live reload
        smoke-start-at-login.ps1 Verify isolated Windows startup registration and repair
        smoke-onboarding.ps1 Verify required first run and safe production-menu practice
        run-interaction-matrix.ps1 Run the versioned validation suites
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
    config.json
    onboarding-v1.json
    icons\
        quarantine\
    logs\
```

`config.json` starts at schema 5 and revision 1 with the default unmodified M4
binding, original blue/light/standard appearance, stable global profile,
eight-slice menu, and empty action catalog.
Each menu slice has an optional action UUID and optional managed icon reference.
A successful later replacement
preserves the previous valid primary as `config.json.bak`; `config.json.tmp`
exists only during a save.

`onboarding-v1.json` is separate from user configuration. Only a valid
schema-one marker with `completed: true` suppresses onboarding at startup;
missing, invalid, unsupported, or incomplete marker content safely requires
the flow again. `onboarding-v1.json.tmp` is used only while atomically writing
successful completion.

Existing schema-one files are accepted only when they contain no schema-two
action fields. Gradial adds the action, binding, and appearance defaults in
memory. Schema-two files gain binding and appearance; schema-three files gain
appearance. Schema-one through schema-four all gain no icon assignments. Every
path validates the complete schema-five model and leaves the original bytes
untouched.
Migration is persisted only through a later explicit, successful save. This
prevents startup from silently rewriting either a valid legacy file or a file
containing ambiguous fields.

Settings import uses the same migration and complete model/profile/action-plan
validation path, then resets the imported revision to the current saved
revision and replaces only the editor's proposed draft. The source bytes,
running configuration, and persistent primary remain untouched. A dirty editor
requires an explicit **Import file** choice before its proposed draft is
replaced. For each unique icon reference, import looks for
`icons/<asset-id>.<format>` beside the selected JSON. A missing directory or
file is non-fatal and produces a placeholder warning. Any present asset must
pass the same size, dimension, PNG/SVG, and SVG-content checks as a directly
chosen icon. Valid sources are atomically copied into managed storage;
byte-identical existing IDs are reused and differing same-ID bytes reject the
whole draft import. Files created earlier in a failed import are removed.

Settings export validates the current proposed draft, serializes schema-five
JSON beside a uniquely named temporary file in the selected directory, flushes
it, and atomically moves it over the selected destination. It may include
unsaved edits but never changes the active revision. Gradial's three active
configuration paths and both onboarding-marker paths are protected export
destinations. Import and export preserve literal action
paths, arguments, labels, and text; diagnostics expose only fixed result
categories rather than selected paths or configuration values. Before the JSON
replacement, export validates every unique referenced managed icon and copies
it into the adjacent `icons` directory through a flushed temporary file.
Existing byte-identical assets are reused; conflicting same-ID files reject the
export without touching the selected JSON. Files and a directory created by a
failed attempt are removed, while unrelated existing files are never deleted.

Managed-icon reconciliation builds one protected set from the active
configuration, a valid backup, and the complete open draft when present. A
missing backup is an empty contribution; an unreadable or invalid backup
disables quarantine entirely. Before renderer preload, valid protected assets
found under `icons\quarantine` are restored to the managed root. Only regular
files whose names exactly match canonical lowercase
`<non-nil-UUID>.png|svg` are candidates for quarantine. Directories,
reparse/symlink entries, temporary files, noncanonical names, and all other
files remain untouched. Existing quarantine destinations are never
overwritten, and no quarantined file is permanently deleted.

Configuration is parsed and completely validated before input starts. Profile
regular expressions are compiled into the retained resolver at load rather
than during invocation. An invalid primary loads the backup without rewriting
either file. If neither file is usable, Gradial runs the built-in default in
memory, retains the broken files, and writes a `config_recovery_warning`. The
settings editor surfaces that warning until a successful explicit save.

Logs are local and rotate at 1 MiB. At most five log files are retained.
Gradial uses its own filter so a host-level `RUST_LOG` setting cannot silence
lifecycle and safety events. Gradial-authored records use component and event
codes; dependency warnings may also appear. Action records contain UUIDs,
step counts, and bounded timing metadata only. They do not contain
window titles, typed text, executable arguments, clipboard contents, URLs, or
selected files.

The Settings **Diagnostics** page flushes the asynchronous logger and freezes
only exact regular files named `gradial_rCURRENT.log` or
`gradial_rNNNNN.log`. Unknown files, directories, links/reparse entries, and
more than five or unexpectedly oversized owned logs fail or remain outside the
snapshot rather than broadening ownership. The frozen snapshot is what the
preview and later export use; new log records, display changes, and
configuration changes do not silently alter it. **Refresh preview** creates a
new snapshot.

Every ZIP contains `manifest.json` plus the previewed sanitized log copies.
The manifest records only the bundle format, creation time, Gradial version,
schema/revision, Windows version/build and architecture, hook-thread heartbeat status, display
bounds/work areas/DPI, log entry names/sizes, and the privacy decisions. The
page deliberately says **Hook thread responsive** rather than claiming that
Windows has retained the hook. Sanitization removes the current user/local
app-data roots and every configured application path from copied logs.

**Include configured application paths** starts off for every preview. When
selected, the page displays the exact case-insensitively deduplicated
application-profile and Run-application executable paths and adds only
`configured-process-paths.json`. It never adds arguments or working folders.
Configuration JSON, typed text, URLs, file/folder action paths, labels, window
classes/titles, clipboard data, icons, and a unique installation identifier
are always absent. ZIP creation is local, uses no network behavior, writes a
flushed adjacent temporary file, and atomically replaces the chosen `.zip`.
Every path inside `%LOCALAPPDATA%\Gradial` is a protected destination.

## Start at login

Start at login is Windows account state, not portable Gradial configuration.
The Settings **General** page inspects only:

```text
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
    Gradial
```

An absent value is **Off**. **On** requires a `REG_SZ` value that exactly
matches the current absolute executable path enclosed in quotes, with no
arguments. Any other type, path, quoting, or appended text is **Needs repair**;
Gradial does not silently accept or execute it.

Gradial never creates the value automatically. Enable and repair set only the
`Gradial` value, then immediately read it back before reporting success. Turn
off deletes only that value and verifies its absence. Gradial never deletes
the shared Run key, enumerates another application's values, or requests
elevation. Import, export, configuration save, onboarding, and app startup
never enable the option.

The current executable command and stored stale value are excluded from log
records and error messages. Logs contain only the requested operation, the
resulting state, a stable error code, and a native numeric error code when
Windows provides one.

## Required first-run onboarding

The normal decorated **Welcome to Gradial** window opens automatically unless
the versioned completion marker is valid. Its first page chooses M4 or M5 and
optional generic Ctrl, Alt, and Shift requirements. It states that both edges
of that exact chord are consumed while Gradial runs, including a quick
centered cancellation, and requires an explicit acknowledgment before
practice can begin.

Beginning practice saves a changed chord through the same complete validation,
atomic replacement, and live input-runtime restart used by Settings. An
unchanged chord does not create a needless configuration revision. A save or
runtime-reload failure leaves onboarding open with fixed recovery guidance.
Settings cannot open while onboarding is active, so the binding under
instruction cannot diverge from the live hook.

Practice still enters through the production low-level hook, invocation
snapshot, gesture controller, edge-assist geometry, cached renderer, and
per-display overlay pool. The resolver runs normally, then an immutable
eight-slice sample menu without action UUIDs replaces the resolved menu for
that invocation. The invocation is explicitly tagged as practice before
release, so action-plan compilation and executor submission return without
dispatch even if every underlying production slice has an action.

Onboarding records three bounded bridge events: menu opened, sample selected,
and centered/Escape cancellation. Completion requires a cancellation after a
successful selection. Other cancellation causes do not satisfy the teaching
step. Only after all steps complete does Gradial atomically write the marker
and close the window. Its close button cannot bypass an incomplete required
flow; **Exit Gradial** remains available from the tray.

The tray's **Restart onboarding** command reopens the chooser with the live
binding. Restart is an immediate replay and deliberately retains an existing
completion marker, so exiting partway through an optional replay does not
turn the next startup into a required flow.

## Input and interaction architecture

Gradial uses four execution contexts:

| Context | Responsibility | May block or allocate? |
| --- | --- | --- |
| Win32 input thread | Classify the saved M4/M5 chord, tester input, raw deltas, movement, Escape, and cancellation | No, inside callbacks |
| Interaction thread | Snapshot invocation state and advance the pure gesture controller | Yes, outside callbacks |
| eframe UI thread | Maintain the hidden controller and paint independent per-display viewports | Yes |
| Action worker | Execute one admitted immutable plan and observe cancellation/timeouts | Yes |

The input thread owns the low-level hooks, heartbeat timer, and a message-only
window registered for raw mouse input. Both event sources execute serially on
that one thread and feed one producer. The hook callback copies fixed-size
events into an `rtrb` SPSC ring buffer with a capacity of 4,096. It never logs,
waits, paints, takes a contended lock, enumerates displays, or calls action
code. Display work areas are immutable for the lifetime of one hook runtime.
When topology changes, an atomic gate first makes new activation attempts pass
through, then the interaction thread closes any active invocation, the old
hook runtime stops, and a replacement starts with the newly enumerated displays. Overflow
sets an atomic fault flag. The interaction thread then closes any active
invocation, closes the complete input runtime, and enters a fail-safe fault
state. The configured chord passes through while faulted; recovery requires
**Retry input** from the tray menu.

At activation-down, the interaction thread maps the signed physical cursor coordinate
to the same monitor ordering used by the overlay pool. It updates only that
viewport's small shared presentation state and requests that child viewport's
independent repaint. Open and close also post private native messages directly
to the selected window, so they do not wait for the hidden controller's
throttled repaint cycle. None of this work occurs in a low-level hook callback.

The hook thread owns a 250 ms message-loop heartbeat. The interaction thread
checks it whether or not a gesture is active. Once it is more than 750 ms old
(three missed intervals), new activation attempts are immediately passed through, any
active invocation is cancelled, and the entire hook/raw-input/queue/consumer
runtime is replaced. If replacement cannot start, Gradial remains faulted until
manual retry. On graceful shutdown, Gradial logs the maximum observed
hook-callback duration.

The tray health state is `Recovering input` during startup, input replacement,
and display rebuild; `Healthy` only after a new hook thread reports responsive;
and `Input faulted` after queue overflow or a failed start. Its menu contains
the non-interactive current state, **Open settings**, **Restart onboarding**,
**Retry input**, **Cancel current action**, recent activity, and
**Exit Gradial**.
Explorer's `TaskbarCreated` message restores the icon and re-registers input.
Power-resume and current-session unlock notifications also re-register the
complete runtime. These signals do not silently clear a queue-overflow fault.

The configured exact chord's button-down and matching release are suppressed.
A button or Ctrl/Alt/Shift state that does not exactly match passes through.
In fixed-origin mode, pointer movement passes through normally and absolute cursor
positions drive the gesture. In edge-assist mode, low-level movement is
suppressed so the system cursor remains at activation-down; raw mouse deltas accumulate
into a virtual gesture pointer instead. During an invocation, Escape and
another mouse-button click cancel the gesture; their matching releases are
suppressed to avoid leaking half of a click. Events stamped with Gradial's
private injection marker are passed through and never interpreted as
activation.

## Gesture contract

The pure controller uses physical cursor deltas but scales its thresholds by
the invocation monitor's effective DPI:

- center cancellation radius: 24 DIP;
- initial selection radius: 36 DIP;
- boundary hysteresis: 5 degrees;
- four or eight slices, with north at index 0 and indices increasing
  clockwise;
- release in the center/no-selection zone cancels;
- Escape closes once, then waits for the activation-button release before
  returning to idle.

Each invocation captures a UUID and source QPC timestamp plus immutable target,
display, input, and configuration identity. The target fields are foreground
window handle/PID, process creation time, optional full executable path, window
class, and window title. Display fields are signed cursor coordinate, monitor
device name, monitor rectangle, work area, effective DPI, resolved geometry,
and visual origin. Generic Ctrl/Alt/Shift state and the triggering M4/M5
identity are captured in the low-level button-down event. They must exactly
match the saved binding before the event is consumed. The configuration
revision is frozen before profile selection.

Only the fixed-size modifier bits are added by the hook callback. Process,
window, monitor, configuration, and profile work remains on the interaction
thread. Executable paths, classes, and titles are held for matching but never
written to routine logs.

### Continuous prototype

The debug-only `--continuous-meter` marks all resolved directions as
continuous for that invocation. It does not modify schema-five configuration
or saved action definitions. The pure controller:

- selects by the existing angle and 36 DIP entry threshold;
- arms at 48 DIP;
- locks that slice for the rest of the invocation;
- maps 48–160 DIP linearly to an unsigned `0.0..=1.0` scalar;
- clamps greater distance to `1.0`;
- commits the final scalar on activation release;
- cancels without a commit when released before arming; and
- stops an armed gesture when it returns inside 24 DIP.

The render thread receives only the locked slice and scalar. It draws a radial
track and percentage through `FoundationOverlayRenderer`; it performs no
callback or configured action work. The interaction outcome is
`ContinuousCommitted`, which the debug meter logs and closes without entering
action-plan compilation. This makes the meter a no-side-effect sample sink,
including when the selected configured slice normally has an action.

The normalized value uses the invocation DPI. Edge assist accumulates raw
physical deltas into the same virtual pointer and divides by that DPI before
normalization, so equivalent DIP travel produces the same value on every
monitor.

## Configuration and profile resolution

The configuration model contains one global profile and zero or more
application profiles. Every profile has an immutable UUID, name, and complete
four- or eight-slice menu. Each slice contains its visible label, an optional
action UUID, and an optional path-safe icon reference consisting only of a UUID
and closed PNG/SVG format. Application rules require an executable path and may add
window-class and title-regex predicates with an explicit priority from 0
through 100.

Executable separators and case are normalized without expanding environment
variables. Class matching is case-insensitive exact; title regex matching uses
Rust's non-backtracking regex engine and is opt-in. Eligible profiles are
ordered by highest priority, then most predicates, then lexicographically
smallest UUID. If the process path is unavailable or no rule matches, the
global profile wins.

The compiled resolver returns the winner, all matching profile IDs, their
priority/specificity, the exact tie-break step, and a reference-counted
immutable copy of that profile's complete menu. The interaction log records
the explanation without target path, class, title, or menu-label content.

The selected menu is frozen with the active invocation. Its slice count drives
gesture recognition and chooses the matching renderer cache; its label,
action-reference, and icon-reference lists remain paired. A title change after activation
therefore cannot switch profiles or produce a gesture/render/action mismatch.

## Settings editor and live reload

The notification-area menu's **Open settings** command opens one normal,
decorated, resizable taskbar window. Repeating the command focuses that same
window and preserves its draft. The editor begins from the running
configuration and modifies a private full-configuration copy; an unfinished
edit cannot affect an open gesture or a later invocation.

The **Activation** tab chooses M4 or M5 and generic Ctrl, Alt, and Shift
requirements. Matching is exact: a required key may be pressed on either
physical side, while an additional generic modifier prevents activation. The
draft displays the same M4/M5 center label used by the production overlay.
Changing these controls marks only the editor copy; the live hook retains the
saved binding until atomic save succeeds and the interaction runtime restarts.

**Start hold–flick–release test** arms a one-shot atomic path in the existing
low-level hook. The next physical M4 or M5 attempt is consumed before
production activation. The hook compares fixed-size button/modifier bits,
tracks signed origin and latest cursor coordinates, suppresses the matching
release, and reports either a chord mismatch, centered cancellation, or one of
eight flick directions. It does not enter the interaction controller or action
executor, and it never logs target-window data. Leaving the tab, discarding,
saving, or closing disarms the tester; a release already paired with a
consumed down remains suppressed.

The **Appearance** tab edits one global schema-five value shared by Global and
all application profiles. The accent is stored as three bounded RGB bytes.
Label theme is a closed light/dark choice, and size is a closed
compact/standard/large choice. The original `(124, 154, 255)` accent, light
labels, and standard size are the default and schema-one-through-three
migration values.

Compact, standard, and large multiply only renderer coordinates and font sizes
by `0.85`, `1.0`, and `1.15`. Input center/entry radii, hysteresis, direction
boundaries, edge assist, and the fixed overlay window remain unchanged. Dark
labels select a contrasting light ring and a bounded light backing behind each
label so long text remains readable over arbitrary foreground content.

The appearance preview passes the draft value to
`FoundationOverlayRenderer`; the Menu and Profiles previews use the same draft
appearance. Discard restores the saved value. Save atomically persists the
complete configuration, updates the existing prewarmed pool's appearance, and
restarts input against the new revision without rebuilding geometry caches.

The **Menu** tab can switch between four and eight clockwise directions, edit
each label, assign or clear any action in the validated catalog, and choose,
replace, or remove one PNG/SVG icon per direction. The native icon chooser
accepts only those two source types. Import rejects empty or unreadable files,
sources over 256 KiB, dimensions outside 1–512 logical pixels, invalid
PNG/SVG data, and SVG document/embedded-image/text features. Accepted bytes
are flushed into `%LOCALAPPDATA%\Gradial\icons` under a generated UUID name;
the configuration never stores a selected path. Removing or discarding an
assignment never deletes managed files. Reconciliation retains every reference
from the active configuration, backup, and open draft; once no protected
configuration needs an exact managed asset, it moves to the recoverable
quarantine.
Eight-to-four conversion retains the four cardinal slices. Diagonal slices are
stashed while the draft remains open and restored if the user switches back;
expanding a configuration that was already saved with four slices creates the
standard diagonal defaults. The selected direction follows its cardinal
equivalent across conversions.

The clickable live preview uses `FoundationOverlayRenderer` and
`ResolvedMenu`, so it shares production geometry, label placement, slice
ordering, and selection rendering. It does not contain a second gesture or
layout implementation. Directions with icons render a fitted 30-DIP image and
a smaller outward label; iconless directions preserve the original label
layout. A missing or damaged managed source renders the bounded built-in
question-mark placeholder.

The **Profiles** tab lists application-specific profiles separately from the
Global fallback. **New profile** opens the native application chooser and adds
nothing when that chooser is cancelled. A selected `.exe` receives a new UUID,
a readable unique name derived from the filename, priority 50, an exact-path
rule, and an independent copy of the current Global menu. The executable can
later be replaced through the same chooser. Priority is constrained to 0
through 100.

**Duplicate selected** copies the complete profile rule and menu under a new
UUID and unique name. **Delete selected** removes only that private draft
profile; normal discard and guarded-close behavior can still recover the
saved configuration. Optional window-class matching is an exact,
case-insensitive predicate. Optional window-title matching is a
case-sensitive Rust regular expression. Enabling either rule exposes its
field; an empty class, empty pattern, or invalid pattern has an inline error
and disables save. Disabling the checkbox removes that predicate.

**Test profiles** starts a three-second countdown so the user can switch to a
target application. At the deadline, Gradial privately captures the foreground
executable path, window class, and title, compiles the current complete draft
through `ProfileResolver`, and displays every match in resolver order with
priority, specificity, and immutable UUID. It shows the winning profile and
whether the decision came from sole eligibility, highest priority, highest
specificity, the UUID tie-break, unavailable executable identity, or Global
fallback. Rule edits re-resolve the retained target immediately.

Captured target values remain only in settings memory: they are neither added
to the draft nor written to diagnostics. **Clear captured details** purges them
explicitly, and every clean or discarded settings close also clears them.

Each application profile can independently switch between four and eight
directions, edit labels, assign or clear any catalog action, and select a slice
in its own production-renderer preview. A per-profile diagonal stash preserves
eight-slice edits while that profile temporarily uses four slices. Creating,
resizing, duplicating, or deleting a profile never mutates the Global menu or
another profile. Empty profile names, required paths, out-of-range priorities,
empty profile-menu labels, empty optional predicates, and invalid title
patterns receive field-specific save messages.

The **Actions** tab lists the complete catalog. Its **New action** menu creates
every leaf type: keyboard shortcut, Unicode text, website, file/folder,
application, Delay, and invocation-window command, plus reusable sequences. It
can rename or duplicate every action type. Creation and duplication generate
new UUIDs and readable unique names. A sequence can be created after at least
one regular action exists, and begins with one valid step so it never creates
an invalid empty draft node.

The shortcut recorder accepts one supported non-modifier physical key plus the
exact left/right Ctrl, Alt, Shift, and Windows modifiers held for it. It stores
the Windows scan code and extended-key flag shown before save; Escape cancels
recording. If a modifier was already down before recording began and no
left/right key transition is visible, it safely falls back to the left variant.
The first editor covers letters, top-row numbers, punctuation, Enter, Tab,
Backspace, Space, navigation keys, and F1 through F24. Unicode text is edited
directly without clipboard access and displays its UTF-8 byte count against
the existing 8 KiB limit.

Website fields accept only complete HTTP or HTTPS addresses. File/folder
actions allow direct absolute-path editing plus separate native file and folder
choosers. Application actions use an absolute `.exe` path, an optional absolute
working folder, and one UI row per argument. Rows are preserved literally,
including spaces and empty values; the editor never parses a shell command.
The application and working-folder controls use the native Windows common item
dialog from a dedicated STA thread and do not log selected paths. Delay is
constrained to 10 through 10,000 ms. Window commands are limited to close,
minimize, maximize, and restore for the invocation window.

New leaf actions begin with a syntactically valid, visible default so creating
one does not invalidate unrelated draft work: HTTPS example address, the
current user folder, Notepad from the Windows directory, 250 ms Delay, or
minimize as appropriate. The user must still assign the action to a menu before
it can run.

Draft validation translates invalid URLs, non-absolute paths, non-`.exe`
applications, null-containing arguments, and out-of-range Delay values into
field-specific messages. Complete configuration validation and plan
compilation remain authoritative before save.

Sequence actions display their ordered direct steps with **Up**, **Down**, and
**Remove** controls. The final step cannot be removed. **Add step** lists leaf
or nested-sequence candidates only when replacing the edited sequence with the
proposed list keeps the complete configuration valid. This automatically hides
the sequence itself, ancestors that would introduce cycles, and additions that
would make this sequence or any containing sequence exceed four nested
sequence levels, 32 executed leaf steps, or 30 seconds of total Delay.
Repeated safe actions remain allowed.

The sequence editor compiles the proposed list after each change and displays
direct-step count, expanded executed-step count, nested depth, and total Delay.
Invalid sequence shapes, missing references, cycles, excessive depth, expanded
steps, and Delay totals also have field-specific save messages as a defensive
boundary.

Deletion first scans global and application-profile menu slices plus sequence
steps. A referenced action is never removed; the editor reports the number and
kind of references so the user can clear them. An unreferenced action can be
deleted without rewriting any other configuration data.

**Save changes** is enabled only for a dirty draft whose required labels,
configuration references, profile rules, action catalog, and bounded action
plans all validate and compile. The application repeats that validation before
disk mutation, performs the existing atomic save/backup procedure, and updates
the retained resolver, action compiler, overlay revision, executor revision,
and interaction runtime only after replacement succeeds. A failed save keeps
the draft open and leaves the running configuration unchanged. A successful
save increments `config_revision`; fresh invocations immediately use that
revision and its saved M4/M5 chord.

A normal close with a clean draft closes immediately. A dirty close displays
exactly **Discard changes** and **Keep editing**. The former abandons the
private draft and closes; the latter returns to the same unsaved draft.
**Discard edits** inside the window resets the draft without closing. Startup
recovery state is shown as a persistent warning and clears only after a
successful explicit save.

## Action model and plan compilation

Schema two supports the complete v1 action family:

- keyboard shortcut by scan code, extended-key flag, and explicit left/right
  modifiers;
- UTF-8 Unicode text;
- HTTP or HTTPS URL;
- absolute file or folder path;
- absolute executable path, argument array, and optional absolute working
  directory;
- close, minimize, maximize, or restore the invocation window;
- delay from 10 through 10,000 ms;
- reusable linear sequence of action UUIDs.

The model rejects empty action names, duplicate action UUIDs, malformed
shortcuts, text above 8 KiB UTF-8, disallowed URLs, relative paths, executable
paths without an `.exe` extension, embedded nulls, missing menu/sequence
references, empty or oversized sequences, direct or indirect cycles, nesting
beyond four sequence levels, expansion beyond 32 leaf steps, and a statically
known delay total above 30 seconds.

The validated catalog is cloned into an immutable `ActionPlanCompiler` when
configuration loads. On release, the selected action UUID comes from the
invocation's frozen menu. The compiler requires the same frozen
`config_revision`, recursively replaces reusable sequences with ordered leaf
steps, and attaches the immutable plan to the invocation. Every plan records a
5-second per-step timeout and 30-second overall ceiling for the worker.

The single-worker executor accepts at most one pending or running plan. A
second dispatch returns `action_busy` immediately and is never queued. Every
compiled leaf operation is supported: Delay, SendShortcut, TypeText, OpenUrl,
OpenPath, RunExecutable, and Window.

Delay uses monotonic time and checks cancellation/shutdown in increments no
larger than 10 ms. The worker defensively enforces the compiled 32-step,
5-second per-step, and 30-second overall limits. The tray's **Cancel current
action** command is enabled only while the admission gate is occupied.
Shutdown requests cancellation and waits at most two seconds for the worker.

Before every shortcut step, the worker confirms that the captured HWND still
exists, resolves to the same PID and process start time, remains foreground,
and is at equal or lower integrity than Gradial. Identity/foreground failure
returns `stale_target`; higher integrity returns `elevated_target`; no key
event is emitted in either case.

Shortcut events use configured scan codes and extended-key flags rather than
layout-dependent characters. Current left/right physical modifier state is
read immediately before injection. Gradial presses only missing requested
modifiers, marks every synthetic event so its own hook ignores it, releases the
primary key, and releases synthesized modifiers in reverse order. A cleanup
guard repeats pending key-up work after failure, cancellation, or unwind and
never releases a modifier that was already physically held.

Text input is converted from the validated UTF-8 configuration value to UTF-16
only at dispatch. Every code unit is sent with `KEYEVENTF_UNICODE`, so BMP
characters and surrogate pairs are delivered literally without keyboard-layout
translation, clipboard access, or variable/environment expansion. Gradial
performs the full identity and integrity check before the first unit, then
checks the HWND, PID, and foreground identity before every key-down and key-up.
If the target changes, cancellation arrives, or Windows rejects an event, a
guard releases the one pending Unicode unit before execution stops. The same
boundaries enforce the plan's step and overall deadlines during long text.

OpenUrl accepts only the configuration model's HTTP/HTTPS URLs. OpenPath
rechecks that its absolute file or folder still exists immediately before
dispatch and returns `path_not_found` without calling the shell if it does
not. Both actions send one literal, null-terminated UTF-16 value to
`ShellExecuteExW` with the `open` verb, no parameters, no working directory,
no `cmd.exe`, and no environment-expansion flag. The worker initializes an STA
COM apartment and uses `SEE_MASK_NOASYNC` because it is a background thread
without a message loop; this completes the association handshake before the
call returns without waiting for the opened application's lifetime.
`SEE_MASK_FLAG_NO_UI` prevents shell error dialogs. Success means Windows
accepted the association request. Other rejection returns
`process_launch_failed`.

RunExecutable accepts only a validated absolute `.exe` path. Immediately
before creation, the worker requires that path to resolve to an existing
regular file and requires an optional working directory to resolve to an
existing directory. A confirmed missing executable or working directory
returns `path_not_found`; invalid path kinds, inspection errors, an oversized
Windows command line, or process-creation rejection return
`process_launch_failed`.

Arguments remain separate strings in the immutable plan. At the Windows
boundary, each value, including empty strings, quotes, spaces, and trailing
backslashes, is encoded and escaped into the one mutable UTF-16 command line
required by `CreateProcessW`. The validated executable is also supplied
separately as the application name. Gradial invokes no shell, performs no
environment interpolation, supplies no custom environment, disables handle
inheritance, and therefore gives the child the action worker's normal
environment and integrity. Once Windows creates the child, Gradial closes the
returned process/thread handles immediately and records success without
waiting for application readiness or exit.

Successful plans record `action_executed=true`. Empty slices still record
`EmptySlice`; a defensive compilation mismatch records `ActionPlanInvalid`.
The overlay begins its close animation immediately and never waits for action
completion. Logs record Unicode scalar counts and open-request kinds but never
text, URLs, paths, executable names, working directories, or argument values.

Executor completions and immediate dispatch rejections share one action
feedback boundary. Success is ignored. Cancellation adds one fixed
**Cancelled** entry, while failure adds one fixed message selected solely from
its stable `ActionErrorCode` and queues a native Windows notification. The
configured `safe_message` is deliberately not copied into either surface, so
an accidentally dynamic path, URL, target title, text value, or argument cannot
cross this boundary.

The store is memory-only, keeps the latest 20 failures/cancellations newest
first, and drops the oldest entry when full. Its pending notification queue is
bounded to the same size. The tray exposes the snapshot through **Recent
activity**, and **Clear activity** removes both retained entries and any
notification not yet submitted to Windows. Notifications use only
`Shell_NotifyIconW(NIF_INFO)` with an error glyph, no sound, and quiet-time
respect; the call does not activate or focus Gradial. A Windows notification
submission failure remains in recent activity and is recorded locally without
retry. There is no success notification and no persisted activity database.

Window actions use the exact HWND captured at activation-down. Immediately before
dispatch, the worker checks that the HWND still exists, still belongs to the
captured PID, and still represents the same process start time. It also
requires that window to remain foreground and checks target integrity against
Gradial's normal integrity. Stale identity returns `stale_target`, a higher
integrity target returns `elevated_target`, and inspection or dispatch failure
returns `window_command_failed`.

Close posts one `WM_CLOSE` request. Minimize, maximize, and restore each post
one matching `WM_SYSCOMMAND`. These are asynchronous queue operations: success
means Windows accepted the message, not that the application obeyed or
finished the transition. Gradial never enumerates windows, substitutes a
current HWND, activates a target, retries, waits for acknowledgement, or
terminates a process. Window titles and target identity values are omitted
from action logs; only the fixed command name is recorded.

## Edge behavior

The default `--geometry=auto` mode uses fixed origin whenever the required
extent centered at activation-down fits in the monitor work area. Discrete
invocations retain their 288 DIP square. The continuous meter requests a 400
DIP square so its 160 DIP track, large appearance scale, and marker remain
visible. Otherwise Gradial selects edge assist and clamps the visual origin far
enough inward for the complete overlay to fit. Work-area coordinates remain
signed, so monitors above or left of the primary display are handled without
unsigned conversion.

Comparison overrides are available without changing code:

```powershell
cargo run --package gradial-app -- --geometry=fixed-origin
cargo run --package gradial-app -- --geometry=edge-assist
```

The system cursor is never warped and Gradial does not synthesize pointer
movement. Raw input is used only during an active edge-assisted invocation.
The four/eight-direction mappings, forced modes, corner insets at 200% DPI, and
absolute-motion suppression are unit tested.

The resolved profile is authoritative for both four/eight geometry and labels.
Both counts use the same radii and five-degree boundary hysteresis; their
angular width, selection indices, highlight arc, and configured labels differ.

Debug builds retain `--slices=4|8` solely for repeatable interaction tests. The
override substitutes a complete direction-labelled test menu and is recorded
as `menu_source=debug_override`. Release builds reject the option and always
record `menu_source=configuration`:

```powershell
cargo run --package gradial-app -- --slices=4
cargo run --package gradial-app -- --geometry=edge-assist --slices=8
```

`--reduced-motion` removes scale, opacity, and selection-color interpolation
without changing recognition or close timing. It is a temporary startup
override until appearance configuration is persisted:

```powershell
cargo run --package gradial-app -- --slices=4 --reduced-motion
```

## Per-display overlay guarantees

The root viewport is a permanently hidden controller. At startup, Gradial
creates one independent 288 DIP deferred viewport for every active monitor.
Each child is moved onto its assigned monitor while hidden, checked with
`GetDpiForWindow`, painted enough times to make its graphics resources resident,
and only then exposed to the input runtime. Every child is:

- transparent and borderless;
- rendered with wgpu;
- topmost and omitted from the taskbar;
- initially inactive;
- configured with `WS_EX_NOACTIVATE` and `WS_EX_TOOLWINDOW`;
- shown and repositioned with no-activation flags;
- returning `MA_NOACTIVATE` for `WM_MOUSEACTIVATE`;
- returning `HTTRANSPARENT` for `WM_NCHITTEST`.

eframe/winit finalizes some extended styles after constructing a viewport.
Gradial therefore applies and reads back the required styles during prewarming,
then keeps the viewport hidden until activation. On an open effect, a private
message handled by that child's native subclass positions and shows it with
no-activation flags. The matching child repaint is independent of the invisible
root-controller repaint cycle.

Each child owns a persistent renderer. During pool construction it caches both
four- and eight-slice boundary segments, label offsets, and 24-segment
highlight arcs. Rendering selects one cache from the frozen menu and pairs its
offsets with the configured labels; no geometry or profile matching occurs on
activation's UI path. Display/DPI pool replacement constructs one complete
cache pair for each replacement child. Selection animation uses fixed-size
eight-entry state and can retarget from its current color without rebuilding
geometry.

Appearance remains a small copied render-frame value. Accent colors replace
only the foundation outline, divider, and selection colors. The label theme
chooses a complete contrast palette, and the size preset multiplies cached
coordinates at paint time. None of these choices invalidates the prewarmed
four/eight cache pair or enters the interaction thread's gesture calculations.

The icon manager enumerates the complete configuration before interaction
starts, enforces the source bounds again, decodes PNG pixels or rasterizes SVG
vectors, and registers egui textures. The same cache is shared by every
prewarmed display renderer and the Settings preview. A Settings import or icon
choice extends that cache while still editing the private draft; saving
recompiles the complete menu but never decodes an icon on activation. A cached
placeholder is retried after portable import or quarantine recovery restores
its managed source. Reconciliation runs before the startup preload and after
successful icon imports, configuration saves, or Settings close/discard
events. Quarantining an unprotected asset also drops its unused texture handle.

`WM_DISPLAYCHANGE`, `WM_DPICHANGED`, and `WM_SETTINGCHANGE` for the work area
invalidate the pool. Monitoring is deliberately disabled while windows are
being placed, because initial PMv2 placement can itself emit DPI messages. A
real change suspends M4, cancels an open invocation with
`DisplayTopologyChange`, hides the routed child, destroys the old runtime/pool,
enumerates signed work areas again, and prewarms a new generation. Input resumes
only after all new child windows report the expected DPI.

## Animation contract

Opening starts with the complete usable ring at 96% scale and 85% opacity, then
uses cubic ease-out to reach its final state at 60 ms. A selected slice fades
between its old and new color over 35 ms. Selection retargeting samples the
current transition before starting the next one, avoiding a flash or restart
from zero.

Release finalizes the gesture immediately and starts a 50 ms opacity fade. The
native overlay remains click-through and non-activating during that fade, then
receives its private hide request. Action dispatch will therefore not wait for
the close animation when actions are introduced. A new invocation can safely
supersede a closing one; the prior overlay is hidden and its focus checkpoint
is completed before the next wake request. Pool replacement and shutdown use
the same forced-hide fallback instead of waiting for an obsolete animation.

Close also registers its deadline with one pool-wide, bounded coordinator.
This is a lifecycle fallback rather than a second animation driver: the child
viewport normally completes its own close, while the coordinator guarantees
the final native hide if a completion repaint is missed. It checks the
invocation UUID before changing state, and the hide request plus state reset
are serialized under the same viewport lock so an old close cannot hide a
newer invocation. No thread is created per gesture or per monitor.

Standard animation requests a child-viewport repaint no more frequently than
every 8 ms. Reduced motion snaps the visuals but retains the 60 ms opening state
and 50 ms close lifecycle, scheduling only their completion frames. Gesture
selection is advanced on the interaction thread, so a valid flick can release
before the first rendered frame and still select its slice.

## Focus and timing diagnostics

Focus diagnostics compare the foreground window at activation, after the next
child-viewport frame checkpoint, and immediately after hiding the overlay. A
checkpoint passes when the same window remains foreground or when the target
process itself changed its foreground window. A successfully queued window
action marks its invocation as an expected target change, so a deliberate
minimize or close is not mislabeled as overlay focus theft at the final
checkpoint. Gradial never calls an API that activates or restores the
foreground window.

After completing an M4 gesture that was held long enough to render a frame,
validate the latest invocation with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-focus.ps1
```

Gradial registers ETW provider
`{3c55b348-69f8-4d33-8e81-62ba761d4f52}`. Its marker strings contain the
invocation UUID and the source hook's QPC value. Markers cover activation
dequeue, open and native-wake requests, overlay show, first-frame checkpoint,
highlight request and application, release dequeue, close request, and overlay
hide. They are intended to be correlated with actual display events; the UI
checkpoint alone is not claimed as proof that a frame reached the screen.

With Gradial already running, capture both streams for 15 seconds with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/capture-timing.ps1
```

The helper uses the installed PresentMon executable and Windows `logman`. It
writes ignored `.csv` and `.etl` diagnostics under `Gradial APP\captures`.
Run the full display-event capture from an elevated PowerShell. Pass
`-MarkersOnly` when only the Gradial ETW stream is needed.

For a complete machine-readable report, first build the release executable and
run:

```powershell
cargo build --workspace --all-features --release
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-performance-report.ps1 -Mode Development -MarkersOnly
```

Development mode is deliberately short: a two-second warmup, 20 invocations,
eight seconds of idle sampling, and three cold launches. It validates the
harness but marks every release gate `not_evaluated`.

From an elevated, otherwise idle PowerShell, the full protocol is:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-performance-report.ps1 -Mode Release
```

Release mode fixes the warmup at five minutes, performs 1,000 real synthetic
M4 invocations against an isolated 10 ms Delay fixture, retains a 30-minute
idle phase, and measures 100 cold launches. It writes Markdown,
schema-versioned JSON, flat CSV, raw ETW/PresentMon tables, application log,
resource samples, and helper output below `captures\performance`.

Renderer checkpoints are reported only as application diagnostics. The
displayed usable-frame and displayed-highlight gates remain `unavailable`
unless PresentMon produces correlatable displayed events. The shortcut
completion gate remains `not_run` because the Delay fixture does not synthesize
keys. Any foreground change or power-plan change invalidates a release run.
The complete protocol and current-machine development numbers are recorded in
`Documentation\milestone-3-performance-harness-results.md`.

## Manual interaction check

1. Start `cargo run --package gradial-app`. Complete required onboarding if it
   is presented. Afterward no overlay should be visible, and the Gradial
   notification-area icon should report Healthy.
2. Put another application in the foreground and note that it has keyboard
   focus.
3. Hold M4. The menu should appear centered on the cursor without changing
   the foreground application.
4. Move east. The `E` slice should highlight.
5. Release M4. The menu should hide; no application should launch.
6. Hold M4 again and press Escape. The menu should hide, and Escape should not
   reach the foreground application.
7. Put the cursor within five physical pixels of a work-area corner. Hold M4
   and move in an outward direction. The complete ring should be inset, the
   outward slice should highlight, and the system cursor should remain in
   place.
8. Change display scaling, work area, or the connected-display layout. An open
   gesture should cancel, and M4 should become available again after
   `overlay_pool_ready` reports the next generation.
9. Stop Gradial and inspect
   `%LOCALAPPDATA%\Gradial\logs\gradial_rCURRENT.log` for
   `hook_thread_responsive`, `invocation_started`, three
   `focus_checkpoint` records, `slice_selected` or `invocation_cancelled`, and
   `hook_thread_stopped`.
10. Open the tray icon and confirm its status line, **Open settings**,
    **Restart onboarding**, **Retry input**, **Cancel current action**, and
    **Exit Gradial** commands are present.
11. Repeat once with `--reduced-motion`; selection and close behavior should be
    identical without interpolated visuals.

This is a global input development build. Save important work before manual
testing. If input becomes faulted, use **Retry input**. Stop Gradial with its
tray **Exit Gradial** command, its terminal, or the VS Code debug controls.

The repeatable live smoke helper builds on the same contract. It temporarily
moves the cursor, starts the hidden executable, sends one or more synthetic M4
gestures, verifies expected slices, foreground identity, and edge cursor
retention, restores the cursor, and closes only the process it started:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -Slices 4
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry edge-assist
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -MonitorIndex 1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -SimulateDisplayChange
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -SimulateInputReregistration
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -SimulateInputFault heartbeat
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -SimulateInputFault queue-overflow
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -ReducedMotion
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -ContinuousMeter -ContinuousPercent 75
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -Geometry fixed-origin -QuickFlick
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -ActivationPoint top-left -AllDirections
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-interaction.ps1 -ActivationPoint center -Direction SW -InvocationCount 20 -QuickFlick -InterInvocationDelayMilliseconds 10
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-delay-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-shortcut-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-text-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-open-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-process-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-window-executor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-settings-editor.ps1 -CaptureScreenshot
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-onboarding.ps1 -CaptureScreenshot
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-continuous-matrix.ps1 -Suite standard
```

Debug `smoke-interaction.ps1` runs append the development-only
`--skip-onboarding` option so a fresh test machine reaches the interaction
under test without changing the user's completion marker. Release builds
reject that option; isolated release action fixtures create their own valid
marker inside their disposable `LOCALAPPDATA` trees.

Add `-CaptureTiming` to the second command to run the documented ETW and
PresentMon capture around the synthetic gesture. In a non-elevated development
shell, add `-TimingMarkersOnly`. Pass `-Configuration release` after building
with `cargo build --release` for performance-oriented checks. `-MonitorIndex`
selects an entry from Windows' current display list. `-Slices 4` or `-Slices 8`
selects and validates the corresponding startup geometry.
`-SimulateDisplayChange` holds M4, posts the same notification used by the
native rebuild path, verifies the active invocation is cancelled, waits until
generation 1 is fully rewarmed, and then sends a fresh gesture.
Every run confirms that Windows can locate Gradial's live notification icon.
`-SimulateInputReregistration` posts a resume notification and verifies that a
new hook runtime becomes responsive. Debug-only `-SimulateInputFault heartbeat`
verifies automatic replacement; `queue-overflow` verifies the manual Faulted
state, M4 pass-through, and recovery through the same Retry Input command path
used by the tray menu.
`-ReducedMotion` confirms that the same cached timing contract is initialized
without interpolation. `-QuickFlick` submits press, movement, and release
without an animation wait, proving that rendering is not a prerequisite for
selection. Every normal run also correlates the selected invocation with its
animated hide and checks the observed close lifecycle against the 50 ms target
with scheduler tolerance.
`smoke-delay-executor.ps1` creates a temporary schema-two configuration with a
50 ms Delay on every slice, runs the release executable through the same
gesture checks, requires one successful executor completion per invocation,
and removes the isolated data directory afterward. It never reads or writes
the user's Gradial configuration.
`smoke-shortcut-executor.ps1` opens a temporary normal-integrity receiver,
foregrounds it, and uses an isolated configuration to dispatch Ctrl+F13. The
receiver requires Ctrl down, F13 down/up, and Ctrl up in order, proving the
production target-validation and `SendInput` path without sending keys to the
user's applications. It then closes the receiver and removes all temporary
data.
`smoke-text-executor.ps1` uses the same isolated receiver and dispatches a
literal string containing environment-like tokens, BMP Unicode, and a
surrogate pair. It requires the exact UTF-16 message sequence, confirms the
tokens were not expanded, and searches Gradial's isolated log for a private
sentinel to prove the payload was not recorded.
`smoke-open-executor.ps1` creates a randomly named per-user test extension and
hidden receiver, dispatches an existing path containing environment-like and
command-shell metacharacters, and requires the exact literal path. It also
checks that the private path sentinel is absent from Gradial's isolated log.
Its `finally` block removes the association, receiver data, and isolated
configuration; it never changes a normal file association or opens one of the
user's applications.
`smoke-process-executor.ps1` compiles a private temporary `.exe`, assigns it
through an isolated configuration, and checks empty, spaced, quoted,
trailing-backslash, shell-character, environment-like, and Unicode arguments.
The receiver also confirms the exact configured working directory and
inherited environment. It remains alive after Gradial reports completion,
proving that the worker closed its handles without waiting for child exit. The
harness checks redacted metadata, stops only the exact receiver path, and
removes its isolated data.
`smoke-window-executor.ps1` starts four separate normal-integrity WinForms
targets and an isolated release configuration for each command. It observes
the exact `WM_CLOSE` or `WM_SYSCOMMAND`, verifies minimized/maximized/restored
state or normal close, and checks that a private target title is absent from
Gradial's log. Cleanup closes only the receiver processes it started and
removes the isolated configurations.
`smoke-action-feedback.ps1` assigns a deliberately missing private executable
through an isolated release configuration. One real M4 invocation must finish
with `path_not_found`, enter recent activity, and produce exactly one accepted
Windows failure-notification request. The shared interaction harness checks
the foreground application after notification submission, while the wrapper
confirms that neither its private path sentinel nor its configured argument
appears in the local log. The isolated configuration is always removed.
`smoke-settings-editor.ps1` starts the debug executable with a private
schema-two eight-slice configuration and opens its real settings viewport. It
seeds one canonical unreferenced PNG in managed storage, requires startup to
move it into quarantine, and compares SHA-256 hashes to prove the recoverable
move preserved its bytes. It opens and safely cancels the real native JSON
import/export and diagnostic-ZIP choosers, migrates
a schema-two fixture into the private draft, and confirms that neither the
source nor active `config.json` changes. It edits the activation draft to exact
Ctrl+M5, arms the safe tester, submits a
native hold/east-flick/release gesture, and requires that no production action
run during the test. It also edits the appearance draft to RGB `(224, 96, 72)`,
dark labels, and large size. It generates a 48×48 transparent PNG, imports it
through the real managed storage/cache path, assigns it to north, and
optionally captures the real production preview with the fitted icon and
contrast backings. It creates and
persists every leaf-action type plus a base and nested sequence, exercises
duplication, unreferenced deletion, sequence addition/removal/reordering, and
records an exact left-Ctrl+F14 combination through native key messages. It
also creates, duplicates, and deletes application profiles, round-trips one
profile through four/eight geometry, and retains an eight-slice menu with its
own nested-sequence assignment while Global later becomes four slices. That
profile enables exact window-class and title-regex predicates. The harness
runs the real three-second tester countdown, injects a deterministic private
target for resolver display, and verifies that its path, class, and title are
absent from the local log. The application fixture includes three exact
argument rows—one containing spaces and one empty—plus a working folder. The
harness opens the real native application chooser, observes its window, and
cancels it without changing the draft. It then selects the nested sequence and
observes four expanded steps, depth two, and 275 ms total Delay before
converting the Global draft to four slices. It exports that complete unsaved
draft, verifies schema five at the unchanged revision with the exact
appearance, icon reference, literal executable path, working folder, and
argument rows, verifies the referenced PNG was copied byte-for-byte into the
adjacent portable `icons` directory, and
again confirms that the active configuration is untouched. After the guarded close it requires
revision 2 on disk with the exact complete profile rule, priority, independent
menu, action payloads, ordered sequence UUIDs, Ctrl+M5 binding, appearance, and
the one shared managed icon source;
the old profile-free schema-two revision remains in `config.json.bak`. It then
previews a frozen diagnostic snapshot, reads a default ZIP to prove configured
application paths remain excluded, and reads an explicitly opted-in ZIP to
prove exactly one deduplicated executable path is included without arguments
or a working folder. It requires no diagnostic temporary debris. Finally, it
requires the renderer appearance update and a fresh Ctrl+M5 invocation to
freeze the saved binding and use the reloaded Global menu. Optional screenshots
cover appearance, icons, activation, the menu,
advanced profile tester, payload, sequence, shortcut-recorder, Windows chooser,
JSON import/export choosers, and confirmation surfaces. The fixture never reads or writes the user's
Gradial configuration.
`smoke-onboarding.ps1` starts the debug executable in a disposable
`LOCALAPPDATA` tree without a completion marker and requires the welcome
window. Its configuration assigns a real Delay action to every production
direction. The harness chooses and acknowledges exact Ctrl+M5, confirms the
atomic revision-2 binding reload, then sends native side-button gestures
through the live hook to select the east sample and cancel through the center.
It requires the onboarding progress events while rejecting every action
dispatch/completion record, proving that practice cannot reach the configured
actions beneath it. It completes the flow, validates the schema-one marker,
restarts Gradial against the same isolated data, and requires onboarding to
remain closed. Optional screenshots capture the binding and completed-practice
pages; cleanup closes only the process it started and removes the temporary
tree.

`smoke-start-at-login.ps1` uses a debug-only, unique
`GradialSmoke-<UUID>` value in the real current-user Run key. It drives the
native Settings window through Off, On, externally stale, repaired, and Off
again; requires one quoted argument-free executable command; checks that
logs contain neither the command nor the isolated value name; and confirms
that the production `Gradial` value is unchanged. Cleanup removes only that
unique test value and its disposable local-data tree, including after
failure. The release build ignores the test-name environment hook.

For matrix validation, first build the selected configuration, then run one of
the versioned suites:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-interaction-matrix.ps1 -Suite quick
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-interaction-matrix.ps1 -Suite standard
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-interaction-matrix.ps1 -Suite extended
```

The quick suite is a short regression subset. Standard adds four-slice center
coverage, all four corners and edge midpoints, per-monitor routing, topology,
and recovery cases. Extended defaults to ten repetitions of all eight
directions from all four corners, yielding 320 automated repetitions, and
raises the rapid-invocation count to 100. Counts can be set explicitly with
`-CornerTrialsPerDirection` and `-RapidInvocationCount`. Automated repetitions
do not measure the plan's human direction-error or selection-time gates.

Each run creates an ignored timestamped directory under
`captures\interaction-matrix` containing `interaction-matrix.json`,
`scenarios.csv`, and `coverage.csv`. The JSON starts with `schema_version: 1`
and includes host/display metadata, exact scenario arguments, captured output,
exit codes, timings, and a coverage table. Missing mixed-DPI, negative-origin,
or high-refresh hardware is reported as `unavailable`; physical events and
long-run campaigns that were not performed are reported as `not_run`.

## Checks

`scripts\check.ps1` runs, in order:

1. PowerShell syntax checks for every script in `scripts`.
2. The deterministic performance-report percentile/QPC analyzer self-test.
3. `cargo fmt --all --check`
4. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
5. `cargo test --workspace --all-features`
6. `cargo build --workspace --all-features`

The script stops on the first failure.

The harness makes the plan's automated interaction checks repeatable and can
produce a 320-repetition corner stress dataset, but it cannot create absent
hardware or replace physical state changes and the required participant
validation. The 1,000-invocation presented-frame latency run,
10,000-invocation focus campaign, and remaining hardware/human datasets stay
separate release gates.
