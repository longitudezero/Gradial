# Milestone 2 URL, file, and folder open results

Date: 2026-07-29

This record covers the seventh bounded Milestone 2 slice: opening HTTP/HTTPS
URLs and existing absolute files or folders through Windows shell
associations. Delay, keyboard shortcuts, and Unicode text remain enabled.
Executable launch and invocation-window commands remain disabled.

## Shell association boundary

OpenUrl receives only the schema-two model's validated HTTP or HTTPS values.
OpenPath receives only absolute paths and rechecks existence immediately before
dispatch. A missing file or folder stops without a shell call and returns the
stable `path_not_found` code.

The production backend encodes the URL or path as one literal,
null-terminated UTF-16 value and passes it to `ShellExecuteExW` with the
explicit `open` verb. It supplies no command-line parameters or working
directory, never invokes `cmd.exe`, and does not enable the shell's environment
substitution flag. Windows receives characters such as `%PATH%`, `$env:PATH`,
and `&` literally.

The worker initializes the STA COM apartment recommended for ShellExecuteEx.
Because it is a background thread without a message loop, it uses
`SEE_MASK_NOASYNC` to complete the shell association handshake before returning
and `SEE_MASK_FLAG_NO_UI` to suppress shell-generated error dialogs. Success
means Windows accepted the request; Gradial does not wait for the application
to become ready or exit. This follows Microsoft's
[ShellExecuteExW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecuteexw)
and
[SHELLEXECUTEINFO](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-shellexecuteinfoa)
threading requirements.

## Failure and privacy behavior

Association or launch rejection returns `process_launch_failed`. Path
inspection errors use the same stable code, while a confirmed missing path is
kept distinct as `path_not_found`. Cancellation is checked before dispatch.
The five-second step and 30-second action deadlines are checked before
dispatch and again by the coordinator after Windows returns.

Routine records contain the invocation UUID, action UUID, configuration
revision, request kind (`url` or `path`), result, timing, and—on native
rejection—the numeric Windows error. They never contain the URL, path, query
string, filename, or environment-like tokens.

## Automated coverage

Unit and executor tests prove:

- HTTP/HTTPS values reach the backend character-for-character as literal
  Windows text;
- existing absolute file and folder paths are inspected and passed unchanged;
- missing paths return `path_not_found` before shell dispatch;
- native rejection returns `process_launch_failed` with a redacted message;
- cancellation and expired deadlines stop before shell dispatch;
- missing-path behavior is reached through the production single-worker
  executor;
- open actions may participate in validated linear sequences;
- a sequence containing a still-disabled window action fails preflight before
  any enabled prefix runs.

## Isolated Windows evidence

`smoke-open-executor.ps1` creates a random extension and ProgID under the
current user's Windows Classes registry. That unique association points to a
hidden receiver and cannot replace a normal file association. The release
executable opens an existing fixture whose filename contains a private
sentinel, `%PATH%`, and `&`.

The receiver observed the exact literal absolute path. Gradial's isolated log
contained the safe `request_kind=path` metadata and no private sentinel. The
fixture removed both registry keys, its isolated configuration and logs, and
all temporary files; a post-run audit found zero matching keys, folders, or
Gradial processes.

URL dispatch and folder dispatch share the same production ShellExecuteExW
call. Their exact values and preconditions are covered with the injected
backend tests so the automated suite does not open the user's browser or
Explorer.

## Validation evidence

The completed implementation passed:

- 116 debug workspace tests and 114 release workspace tests;
- Clippy for all targets and features in debug and release;
- debug and release builds;
- the isolated literal-path association and payload-log privacy check, with a
  52 ms observed close;
- existing Unicode and shortcut regressions, each with a 52 ms observed close;
- the existing Delay regression with two successful invocations and a 51 ms
  observed close;
- an action-free four-slice debug smoke check;
- all eight quick-matrix scenarios available in the current one-monitor setup,
  covering 31 selected invocations with no failures.

The successful matrix artifacts are under
`Gradial APP/captures/interaction-matrix/20260729T151943431Z`.

## Current boundary

There is no settings editor, URL/path picker, activity history, or failure
toast yet. Open actions deliberately provide no application-ready or
application-exit acknowledgement after Windows accepts the association.
Direct `.exe` launch was enabled in the next bounded slice; its boundary and
evidence are in
[`milestone-2-process-launch-results.md`](milestone-2-process-launch-results.md).
Invocation-window commands remain disabled.
