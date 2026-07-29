# Milestone 2 invocation-window action results

Date: 2026-07-29

This record covers the ninth bounded Milestone 2 action slice: normal close,
minimize, maximize, and restore requests for the window captured at M4-down.
This completes execution support for every schema-two leaf action family.

## Frozen-target boundary

Window actions never resolve a new target at execution time. They receive the
immutable invocation context and use only its captured HWND. Immediately before
dispatch, the existing full target validator checks:

1. the HWND still exists;
2. it still belongs to the captured PID;
3. the captured PID still has the same process start time;
4. the HWND remains the foreground window;
5. the target is at equal or lower integrity than Gradial.

The validator repeats identity and foreground checks after reading integrity,
closing the inspection race as far as a normal user process can before the
single asynchronous dispatch. A changed, reused, destroyed, or background
target returns `stale_target`. A higher-integrity target returns
`elevated_target`; an inspection failure returns `window_command_failed`.

Cancellation, shutdown, and the five-second step and 30-second action budgets
are checked before validation and again immediately before dispatch. No
command is posted if any checkpoint fails.

## Asynchronous Windows commands

Close posts one `WM_CLOSE` to the captured HWND. This requests the
application's normal close path, allowing it to prompt, save, ignore, or cancel
according to its own behavior. Gradial never terminates the target process.

Minimize, maximize, and restore each post one `WM_SYSCOMMAND` carrying
`SC_MINIMIZE`, `SC_MAXIMIZE`, or `SC_RESTORE`. `PostMessageW` adds the command
to the target thread's queue and returns immediately. Success means Windows
accepted that one queue operation; it does not mean the application processed
or obeyed the command.

There is no window enumeration, current-window substitution, focus activation,
retry, acknowledgement wait, forced close, process termination, fullscreen
toggle, centering, always-on-top change, desktop move, monitor move, or
FancyZones integration.

The dispatch follows Microsoft's
[PostMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew),
[WM_CLOSE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-close),
and
[WM_SYSCOMMAND](https://learn.microsoft.com/en-us/windows/win32/menurc/wm-syscommand)
contracts.

## Focus and privacy behavior

Minimize and close legitimately allow another application to become
foreground. A successfully queued window command marks only that invocation
as an expected target change for the final overlay focus checkpoint. This
prevents an intentional action result from being mislabeled as Gradial focus
theft. The marker is single-slot because the executor admits only one plan and
is consumed only by the matching invocation. Gradial itself still calls no
focus or activation API.

Routine action records contain the invocation UUID, action UUID, configuration
revision, fixed command name, result, and timing. Native rejection adds only
the numeric Windows error and a fixed phase. They do not contain HWND/PID,
process path, window class, or window title.

## Automated coverage

Unit and executor tests prove:

- every command validates once and dispatches once to the exact captured HWND;
- close maps to `WM_CLOSE`, while minimize, maximize, and restore map to the
  correct `WM_SYSCOMMAND` values;
- stale and elevated targets retain their stable public errors;
- inspection and native dispatch rejection use `window_command_failed`;
- cancellation before validation or between validation and dispatch posts
  nothing;
- an expired deadline posts nothing;
- native rejection is not retried;
- the production executor admits a Window plan and reaches target validation.

## Isolated Windows evidence

`smoke-window-executor.ps1` starts four separate normal-integrity WinForms
targets. Each receives its own isolated release configuration so one real M4
invocation selects exactly one window command. The receiver records its native
messages, and the harness independently checks the resulting window state.

The live run observed:

- `SC_MINIMIZE` and an iconic target;
- `SC_MAXIMIZE` and a maximized target;
- `SC_RESTORE` and a return from maximized to normal state;
- `WM_CLOSE`, normal target-process exit, and destruction of the captured
  HWND.

All four actions completed successfully against only their frozen receiver
windows. Their close-animation observations were 50, 51, 51, and 51 ms. Each
target title contained a private sentinel; none appeared in the isolated
Gradial logs. Cleanup closed only the exact receiver processes started by the
harness and removed all isolated files.

## Validation evidence

The completed implementation passed:

- 128 debug workspace tests and 126 release workspace tests;
- Clippy for all targets and features in debug and release;
- debug and release builds;
- PowerShell syntax validation for every development script;
- four isolated live release invocations covering every command, resulting
  state, expected focus change, and target-title privacy.

## Current boundary

Every schema-two leaf action can now execute. There is still no settings
editor, failure toast, activity history, arbitrary shell action, force-close
action, custom environment editor, application-specific fullscreen toggle, or
advanced window-placement action.
