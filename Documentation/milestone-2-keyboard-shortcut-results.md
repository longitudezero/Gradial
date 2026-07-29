# Milestone 2 target validation and keyboard shortcut results

Date: 2026-07-29

This record covers the fifth bounded Milestone 2 slice: frozen target delivery
to the action worker, target freshness/integrity validation, and the first
OS-affecting action backend. Delay remains enabled. Unicode text, URL/path
opening, executable launch, and invocation-window commands remain disabled.

## Frozen target contract

Release submits two immutable, reference-counted values to the single worker:
the compiled execution plan and the invocation's captured target context. The
worker does not recapture a newer foreground application or look up a profile
again.

Immediately before every keyboard-shortcut step, Gradial verifies:

1. The captured HWND is still a live window.
2. The HWND still belongs to the captured PID.
3. The PID still has the captured process start time, preventing HWND/PID reuse.
4. The HWND is still the foreground window.
5. The target process integrity RID is not higher than Gradial's RID.

A missing, replaced, reused, or background target returns `stale_target`.
Higher integrity returns `elevated_target`. Failure to inspect integrity safely
returns `input_injection_failed`. No shortcut event is emitted after any of
these failures.

Target paths, classes, titles, and shortcut values are omitted from execution
logs. Completion records continue to contain only safe IDs, counts, timings,
result codes, and configuration revision.

## Scan-code shortcut execution

The backend consumes the validated configuration's scan code and extended-key
flag directly. Requested left/right modifiers map to their Windows scan-code
identities:

- left/right Control;
- left/right Alt;
- left/right Shift;
- left/right Windows.

Physical state is read immediately before injection for each requested
modifier. A modifier already held by the user is neither synthesized nor
released. Missing modifiers are pressed in configured order, the primary key
is pressed and released, and synthesized modifiers are released in reverse
order.

Every event carries Gradial's existing injected-input marker, so the low-level
hook passes it through without feeding it back into gesture handling.

## Cleanup and cancellation

A guard records only successful key-down events emitted by Gradial. Pending
key-up events run on normal completion, first failure, cancellation, and Rust
unwind. Cancellation is checked before target validation and between each
injection boundary, including the primary key-down/key-up gap.

If Windows rejects an event, the action returns `input_injection_failed`.
Cleanup retries the still-pending key-up and continues in reverse order. A
persistent cleanup failure is recorded with a count only; key identity is not
logged.

Plans are preflighted before their first step. Delay and SendShortcut may be
combined in a bounded sequence. Any plan containing text, open, launch, or
window operations returns `unsupported_action` before a Delay or shortcut
prefix can run.

## Automated coverage

Unit and executor tests prove:

- a matching live target at equal or lower integrity is accepted;
- missing HWND, changed PID, changed process start time, and lost foreground
  are all `stale_target`;
- higher integrity is distinct from an integrity-inspection failure;
- stale target validation is reached through the real executor before input;
- shortcuts use scan codes, extended flags, and Gradial's input marker;
- physically held modifiers are never synthesized or released;
- successful cleanup releases the primary key and modifiers in reverse order;
- cancellation between primary key-down and key-up leaves no synthesized key
  down;
- a one-time injected failure at every send boundary leaves no synthesized key
  down;
- all other action families remain preflight-disabled.

## Isolated Win32 evidence

`smoke-shortcut-executor.ps1` creates a temporary schema-two configuration and
a purpose-built normal-integrity foreground receiver. The release executable
dispatches Ctrl+F13 to that receiver. The receiver observed exactly:

1. Control down, scan code 29;
2. F13 down, scan code 100;
3. F13 up, scan code 100;
4. Control up, scan code 29.

The invocation retained foreground identity, completed the configured close
animation, and recorded successful executor shutdown. The fixture neither
reads nor writes the user's Gradial configuration and removes its receiver,
logs, and temporary local-data root afterward.

## Validation evidence

The completed implementation passed:

- 101 debug workspace tests and 99 release workspace tests;
- Clippy for all targets and features in debug and release;
- debug and release builds;
- the isolated Ctrl+F13 receiver, with the complete four-event sequence and a
  51 ms observed close;
- the existing isolated Delay fixture with two successful invocations;
- an action-free four-slice debug smoke check;
- all eight quick-matrix scenarios available in the current one-monitor setup,
  covering 31 selected invocations with no failures.

The successful matrix artifacts are under
`Gradial APP/captures/interaction-matrix/20260729T135017050Z`.

## Current boundary

This slice provides no shortcut recorder/editor UI and no activity history or
failure toast. Target validation currently protects keyboard shortcuts; it
will also protect Unicode text when that backend is added. Live elevated and
target-replacement campaigns remain part of the hardening milestone, while
their decision paths are covered with deterministic tests here.

Unicode text entry was completed in the subsequent
[`milestone-2-unicode-text-results.md`](milestone-2-unicode-text-results.md)
slice.
