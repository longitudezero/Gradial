# Milestone 2 Unicode text-entry results

Date: 2026-07-29

This record covers the sixth bounded Milestone 2 slice: literal Unicode text
entry, continuous target validation, failure-safe input cleanup, and payload
privacy. Delay and keyboard shortcuts remain enabled. URL/path opening,
executable launch, and invocation-window commands remain disabled.

## Literal Unicode delivery

The worker converts the validated UTF-8 configuration string with Rust's
`encode_utf16` iterator only when the TypeText step executes. Each UTF-16 code
unit is submitted with `KEYEVENTF_UNICODE`, a zero virtual-key value, and
Gradial's private input marker. This preserves BMP characters and non-BMP
surrogate pairs without depending on the active keyboard layout.

Text never passes through the clipboard, a command shell, or an environment
expander. Strings such as `%PATH%` and `$env:PATH` are therefore entered
literally. The existing schema limit of 8 KiB UTF-8 bounds the operation before
it reaches the worker.

## Target and cleanup boundary

Before the first text event, Gradial applies the same complete frozen-target
check used for shortcuts: HWND/PID/start-time identity, foreground ownership,
and integrity level. During the multi-event send, a lightweight check confirms
that the HWND remains live, belongs to the captured PID, and remains foreground
before every key-down and key-up. The worker also checks cancellation, the
five-second step deadline, and the 30-second plan deadline at those boundaries.

Only one UTF-16 code unit can be pending at a time. After a successful
key-down, a guard owns its matching key-up. Normal completion, cancellation,
lost foreground, injection failure, or Rust unwind all release that pending
unit before execution stops. A stale target returns `stale_target`; a
higher-integrity target returns `elevated_target`; inspection or Windows input
failure returns `input_injection_failed`.

## Privacy contract

Execution logs identify the invocation, source action UUID, configuration
revision, and Unicode scalar count. They never include the source text or its
UTF-16 units. Cleanup diagnostics record only the number of unresolved units.
Window titles, clipboard content, and environment values remain outside the
action log.

## Automated coverage

Unit and executor tests prove:

- ASCII, BMP Unicode, and a non-BMP character produce the exact marked UTF-16
  key-down/key-up sequence;
- surrogate pairs remain ordered and each code unit is released;
- full target validation fails before the first event;
- the live HWND/PID/foreground check runs between every event;
- cancellation or target loss after key-down still emits its matching key-up;
- an in-flight step timeout also emits the pending key-up before stopping;
- a one-time failure at every event boundary leaves no Unicode unit down;
- TypeText reaches target validation through the production executor;
- mixed plans containing a still-disabled action fail preflight before any
  Delay, shortcut, or text prefix can run.

## Isolated Win32 evidence

`smoke-text-executor.ps1` creates an isolated schema-two configuration and a
purpose-built normal-integrity foreground receiver. The release executable
enters a private test payload containing `%PATH%`, `$env:PATH`, Greek and
accented BMP characters, and a crab emoji. The receiver observed the exact
literal UTF-16 sequence, including the emoji's surrogate pair.

The fixture then checks Gradial's isolated log for its private sentinel and
fails if any payload text is present. It does not read or write the user's
Gradial configuration, does not use the clipboard, and removes its target,
logs, and temporary local-data root afterward.

## Validation evidence

The completed implementation passed:

- 109 debug workspace tests and 107 release workspace tests;
- Clippy for all targets and features in debug and release;
- debug and release builds;
- the exact Unicode receiver and payload-log privacy check, with a 52 ms
  observed close;
- the existing Ctrl+F13 receiver regression, with a 51 ms observed close;
- the existing isolated Delay fixture, with two successful invocations and a
  50 ms observed close;
- an action-free debug smoke check;
- all eight quick-matrix scenarios available in the current one-monitor setup,
  covering 31 selected invocations with no failures.

The successful matrix artifacts are under
`Gradial APP/captures/interaction-matrix/20260729T141321456Z`.

## Current boundary

This slice provides no text recorder/editor UI and no activity history or
failure toast. Full validation detects process-start-time reuse before entry,
while the between-event check intentionally avoids reopening the process for
every code unit and protects live HWND/PID/foreground ownership. Physical
elevated and mid-send target-replacement campaigns remain hardening gates;
their result paths and cleanup invariants are covered deterministically here.
