# Milestone 4 continuous hardening matrix results

Date: 2026-08-14

## Outcome

The third Milestone 4 hardening slice adds a repeatable display/DPI, input
recovery, plugin-management, and shutdown campaign for real continuous Lua
sessions. Every scenario uses an isolated schema-six configuration, enabled
plugin package, trust state, and `LOCALAPPDATA` tree. Gestures cross the normal
Windows M4 hook, production overlay, continuous coordinator, plugin worker,
and Lua callbacks.

The qualifying current-machine run passed all 12 scenarios:

| Measurement | Result |
| --- | ---: |
| Scenarios | 12 passed, 0 failed |
| Continuous sessions | 16 |
| Begin / Update / Commit / Cancel callbacks | 16 / 15 / 7 / 9 |
| Invalid lifecycle sessions | 0 |
| Updates after a terminal callback | 0 |
| Failed lifecycle callbacks | 0 |
| Unclean or timed-out shutdowns | 0 |
| Effective DPI exercised | 96 (100%) |

The ignored machine-readable evidence is under:

```text
Gradial APP\captures\continuous-hardening-matrix\20260814T152513854Z
```

The report is schema-versioned JSON with flat scenario and coverage CSVs.
Each scenario retains its disposable package, configuration, state, and
bounded application log so a failure can be inspected without reading the
user's Gradial data.

## Campaign matrix

Release scenarios use the optimized executable wherever no fault injection is
needed:

- one complete Begin/Update/Commit route on every available Windows display,
  recording its effective DPI and geometry;
- display-topology notification during an active session, requiring
  `DisplayTopologyChange`, one Cancel, a generation-one overlay rewarm, and a
  successful fresh session;
- resume notification during an active session, requiring
  `InputReregistration`, one Cancel, a replacement responsive hook, and a
  successful fresh session; and
- bounded shutdown before Begin, after Begin, after Update, at Commit
  submission, and after Cancel submission.

The debug executable adds deterministic fault and management signals while
leaving the production application state machines authoritative:

- heartbeat loss must cancel with `HookUnresponsive`, restart input
  automatically, and admit a fresh session;
- queue overflow must cancel with `InputQueueOverflow`, enter the manual
  Faulted state, pass M4 through, recover only after Retry Input, and then
  admit a fresh session;
- Reload must first cancel the active session with `ReloadRequested`, wait for
  its terminal callback, reload the same enabled adapter, and admit a fresh
  session; and
- Disable must first cancel with `PluginDisabled`, unload the adapter, persist
  disabled state, and reject a later continuous admission.

The two private plugin-management messages exist only in debug builds. They
select the first configured continuous plugin and call the same cancellation,
retry, worker-operation, and snapshot path used by the Plugins settings page;
they do not add a production command or user-facing control.

Every completed session must contain one Begin followed by exactly one Commit
or Cancel. The analyzer fails duplicate or missing terminals, callback
failures, and any Update after a terminal. Every process must exit with code
zero and retain successful action-executor, interaction-thread, hook-thread,
and plugin-worker shutdown records. Plugin timing summaries must report
`active_session_at_shutdown=false`.

## DPI and physical-event boundary

The runner enumerates Windows' active displays and creates one complete
continuous route for each. It reports 100%, 125%, 150%, 175%, and 200% as
`passed` only when that effective DPI is physically available. This machine
had one 96-DPI display, so 120, 144, 168, and 192 DPI are honestly
`unavailable`, not inferred from coordinate arithmetic.

Synthetic `WM_DISPLAYCHANGE` and resume notifications exercise Gradial's real
native notification handling and all downstream cancellation/rebuild logic.
They do not pretend to attach hardware, change a monitor's DPI, put Windows to
sleep, lock a session, or restart Explorer. Those external transitions remain
`not_run` and must be repeated on the release qualification hardware matrix.

## Reproduction

From `Gradial APP`, build both executables and run:

```powershell
cargo build --workspace --all-features
cargo build --workspace --all-features --release
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-continuous-hardening-matrix.ps1 -Build Both
```

`-Build release` runs the hardware, topology, resume, and shutdown portion.
`-Build debug` runs that portion plus deterministic heartbeat, overflow,
Reload, and Disable cases. The runner refuses to start while another Gradial
process is active.

This completes the automated Milestone 4 display/DPI, recovery, and shutdown
matrix item. It does not close unavailable physical-DPI or physical-state
rows, and it does not replace the separate displayed-frame performance and
participant-usability gates.
