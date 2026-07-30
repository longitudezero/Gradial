# Milestone 3 start-at-login results

## Scope

This bounded Milestone 3 slice adds an explicit, per-user start-at-login
control:

- default off;
- no administrator requirement;
- one owned Windows startup value;
- exact current-executable registration with no arguments;
- visible Off, On, Needs repair, and unavailable states;
- explicit stale-entry repair;
- read-back verification after every change;
- no coupling to portable Gradial configuration.

The signed per-user installer and the remaining release performance, soak, and
recovery matrices are outside this slice.

## Windows ownership

Gradial accesses only the named value:

```text
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run
    Gradial
```

It does not enumerate or modify another application's value and never deletes
the shared Run key. Turn off treats an already absent value as success. Access
denial and other registry failures retain stable read, write, delete, or
verification categories.

The debug validation hook accepts only a bounded value name beginning with
`GradialSmoke-`. Release builds ignore that hook and always own only
`Gradial`.

## Exact state model

Gradial builds one command from its current absolute executable path:

```text
"C:\absolute path\to\gradial.exe"
```

It appends no arguments. Embedded nulls, quotes, relative paths, and
unreasonably long paths are rejected before registry access.

The state resolver reports:

- **Off** when the value is absent;
- **On** only when a bounded, terminated `REG_SZ` exactly equals the expected
  quoted command;
- **Needs repair** for another registry type, malformed text, a different
  executable path, changed quoting, or any appended argument;
- **Status unavailable** when the current executable or Windows registry
  cannot be inspected.

Enable and repair write only the expected `REG_SZ`, then read it back and
require **On**. Turn off removes only the named value, then reads it back and
requires **Off**. No UI operation reports success before verification.

## Settings behavior

The **General** tab is independent from the menu-configuration draft. Changing
start at login:

- does not dirty `config.json`;
- does not require **Save changes**;
- is unaffected by configuration import or export;
- is refreshed when Settings opens, when the tab is entered, or when the user
  chooses **Refresh status**;
- offers **Repair** and **Turn off** instead of presenting an ambiguous
  checkbox when a stale value exists.

The page explains that the setting applies only to the current Windows
account, requires no administrator access, and records no startup arguments.

## Privacy

Gradial never logs the current executable command, the stored command, or the
registry value name. Structured records contain only:

- requested enable, disable, or repair operation;
- Off, On, or Needs repair result;
- configuration revision for correlation;
- a stable error code;
- the numeric Windows error code when present.

The service's debug representation also redacts the expected command.

## Automated coverage

The complete debug suite contains 226 tests:

- 174 `gradial-app` tests;
- 52 `gradial-core` tests.

New tests verify:

- missing, exact, stale-path, appended-argument, and wrong-type states;
- enable, repair, and disable touching only the owned value name;
- immediate read-back verification;
- stable registry failure categories;
- absolute quoting with no arguments;
- rejection of relative and quoted executable paths;
- redacted service debug output;
- independence from the configuration draft and save lifecycle;
- bounded Settings-event debug output.

Formatting, Clippy with warnings denied, the complete workspace build, and the
release `gradial-app` build also pass.

## Native Windows evidence

`smoke-start-at-login.ps1 -CaptureScreenshot` passes against the real
current-user Windows Run key with a unique debug-only value name. It:

1. requires the isolated value to begin absent and Gradial to report **Off**;
2. enables the option and reads back exactly one quoted, argument-free current
   executable;
3. replaces the isolated value externally with a stale path and appended
   argument;
4. refreshes Settings and requires **Needs repair**;
5. repairs the value and requires the exact current executable again;
6. turns the option off and requires the isolated value to be absent;
7. rejects the executable command, stale command, and test value name from the
   local log;
8. proves the production `Gradial` value is unchanged;
9. removes only its unique value and disposable local-data tree in cleanup.

The optional screenshot captures the actual General-page repair state.

## Remaining boundary

Start at login is complete for this release checkpoint. Installer work must
preserve the same per-user/no-elevation model and must not enable start at
login without the user's explicit choice. Performance reports, the 24-hour
soak, recovery matrices, and the final known-limitations document remain open.
