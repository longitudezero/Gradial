# Milestone 2 direct executable-launch results

Date: 2026-07-29

This record covers the eighth bounded Milestone 2 action slice: direct launch
of an existing Windows executable with argument-array preservation and an
optional working directory. Delay, keyboard shortcuts, Unicode text, and
URL/file/folder open remain enabled. Invocation-window commands remain
disabled.

## Configuration and runtime boundary

RunExecutable requires an absolute executable path with a case-insensitive
`.exe` extension. `.cmd`, `.bat`, `.ps1`, `.com`, and arbitrary shell strings
are rejected by construction. The optional working directory must also be
absolute. Arguments remain a JSON array and may be empty or contain spaces,
quotes, backslashes, Unicode, environment-like tokens, and shell
metacharacters. Embedded nulls remain invalid.

Immediately before creation, the worker defensively rechecks that the
executable is an existing regular file and that an optional working directory
is an existing directory. Missing paths return `path_not_found`. A directory
masquerading as an executable, a file supplied as the working directory, an
inspection error, or a command line above Windows' 32,767 UTF-16-unit boundary
returns `process_launch_failed`.

## Windows process-creation boundary

The immutable execution plan retains `arguments` as an array. Only at the
Windows API boundary does Gradial quote each value into the mutable UTF-16
command-line buffer required by `CreateProcessW`. Its quoting doubles
backslashes before a quote or a closing delimiter, preserving empty values,
embedded quotes, and trailing backslashes. The absolute executable is supplied
separately as `lpApplicationName`, so Windows does not search `PATH` for the
program.

Gradial calls `CreateProcessW` directly with handle inheritance disabled. It
passes no shell, no custom environment block, and no expansion layer. The
child therefore inherits Gradial's normal environment and integrity without
interpreting `%PATH%`, `$env:PATH`, `&`, `|`, or `>` as syntax. A configured
working directory is supplied as one literal null-terminated UTF-16 path.

Success means Windows created the child process. Gradial immediately closes
the returned process and primary-thread handles and never waits for application
readiness or process exit. Cancellation, shutdown, and the five-second step
and 30-second action budgets are checked before the irreversible creation
call; the coordinator also measures the synchronous return against those
budgets. The call itself is not moved to a detached timeout thread because
that could launch a process after Gradial had reported cancellation.

This implementation follows Microsoft's
[CreateProcessW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw)
and
[parsing C command-line arguments](https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments)
contracts.

## Failure and privacy behavior

Native process-creation rejection returns `process_launch_failed`; a confirmed
missing executable or working directory returns `path_not_found`. Cancellation
or shutdown before creation performs no launch. Once Windows reports success,
Gradial does not attempt to terminate or otherwise manage the independent
child.

Routine records contain only the invocation UUID, action UUID, configuration
revision, argument count, whether a working directory was configured, result,
and bounded timing. Native failures add the numeric Windows error and a fixed
phase name. Logs never contain the executable path, working directory,
argument values, or inherited environment values.

## Automated coverage

Unit and executor tests prove:

- Windows' own `CommandLineToArgvW` recovers the exact executable and every
  generated argument boundary, including empty, spaced, quoted,
  trailing-backslash, environment-like, and Unicode values;
- the exact executable, command line, and optional working directory reach the
  backend without shell expansion;
- missing executable and working-directory paths return `path_not_found`
  before creation;
- wrong file/directory kinds, shell-file extensions, embedded nulls, oversized
  command lines, and native rejection fail safely;
- native rejection uses a stable code and redacted safe message;
- cancellation and expired deadlines stop before process creation;
- the production single-worker executor admits RunExecutable and reaches its
  missing-path boundary;
- plans containing the remaining disabled window action fail preflight before
  any enabled prefix runs.

## Isolated Windows evidence

`smoke-process-executor.ps1` compiles a temporary Windows `.exe` receiver into
a random isolated directory. Both its executable filename and configured
working directory contain spaces, a private sentinel, and environment-like
text. The release app launches it with an acknowledgement path plus exact test
arguments covering empty values, spaces, quotes, consecutive and trailing
backslashes, `%PATH%`, `$env:PATH`, shell metacharacters, BMP Unicode, and a
surrogate pair.

The receiver observed every argument exactly, the exact configured working
directory, and a private inherited environment value. It was still running
after Gradial recorded successful action completion and shut down, proving
that Gradial did not wait for child exit. The isolated Gradial log contained
only `argument_count` and `working_directory_configured=true`; it contained no
private sentinel. The harness then stopped only the process whose executable
path matched the receiver and removed all temporary files.

The live release check completed one real M4 invocation with a 51 ms observed
close lifecycle.

## Validation evidence

The completed implementation passed:

- 123 debug workspace tests and 121 release workspace tests;
- Clippy for all targets and features in debug and release;
- debug and release builds;
- PowerShell syntax validation for every development script;
- the isolated exact-argument, working-directory, inherited-environment,
  no-wait, and payload-log privacy check.

## Current boundary

There is no settings editor, executable picker, custom environment editor,
child-process lifecycle management, activity history, or failure toast yet.
Arbitrary shell execution remains deliberately out of scope. Invocation-window
commands remain the only disabled schema-two leaf action family.
