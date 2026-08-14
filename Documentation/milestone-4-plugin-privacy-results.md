# Milestone 4 plugin privacy evidence

Date: 2026-08-14

This slice adds a repeatable Windows campaign for the v0.3 requirement that
plugin-owned private values remain outside release logs and default diagnostic
exports. The qualifying current-machine run passed all six privacy-surface
checks across eleven recognizable sentinels.

## Exercised production route

`Gradial APP/scripts/run-plugin-privacy-evidence.ps1` creates a disposable
`LOCALAPPDATA` tree and an enabled, permission-approved Lua fixture. It drives
two real M4 gestures through the optimized executable:

1. A successful continuous session proves that a private configured parameter,
   foreground application path, and foreground window title reach Lua. The
   plugin then sends a complete shortcut, one native notification, and one
   POST to a literal IPv4 loopback target.
2. A second continuous session throws a recognizable private message from its
   Commit callback. Gradial records only `lua_runtime_error`, unloads the
   adapter, and persists the plugin's faulted state.

The foreground helper is compiled into an executable whose filename contains
the application-context sentinel. It records the synthesized F14 press and
release. The local TCP helper records the exact path and body and returns a
bounded 204 response. The received body must contain the configured parameter,
window title, application-path marker, and loopback-body sentinel. These
positive receivers prove the campaign is testing values that actually crossed
the capability boundary, rather than merely scanning unused fixture text.

After both optimized runs, the debug settings automation freezes the real
diagnostic preview and exports:

- the default bundle; and
- the separately opted-in configured-process-path bundle.

The debug executable is used only to operate existing test-only Settings keys.
The logs under test were produced by the optimized interaction route and are
included in both frozen bundles.

## Private values

The campaign injects separate recognizable values into:

- a configured plugin parameter;
- Lua source text;
- a manifest description;
- the foreground window title;
- the foreground executable filename;
- the shortcut chord;
- notification text;
- loopback path and body;
- a Lua runtime-error message; and
- one configured executable path reserved for explicit opt-in validation.

The report stores only SHA-256 hashes for those values. Raw fixture files and
the exact receiver payloads stay in the disposable tree and are deleted after
analysis.

## Result

The current run produced three bounded Gradial log files in each diagnostic
snapshot: two optimized-run log files and the current debug-export log. All
checks passed:

| Check | Result |
| --- | --- |
| Release-log plugin payload exclusion | Pass |
| Release-log Lua failure redaction | Pass |
| Default bundle closed-entry policy | Pass |
| Default bundle sentinel exclusion | Pass |
| Opt-in bundle exact configured path | Pass |
| Opt-in bundle exclusion of every non-path sentinel | Pass |

The default ZIP contained only `manifest.json` and the three owned log
entries. It contained no configuration, plugin package, plugin state, process
path document, source, parameters, context, shortcut value, notification text,
HTTP data, or Lua message.

The opted-in ZIP added exactly `configured-process-paths.json`. That document
contained the one configured executable path, while its ten non-path sentinels
remained absent from every entry. The configured path did not occur in the
bundled logs or manifest.

The stable failure line retained the plugin ID, session ID, callback stage,
timing, and `lua_runtime_error`; it did not retain the Lua message or source.
The successful route retained stable notification queued/shown metadata but
not its text. The shortcut and loopback brokers similarly exposed no private
payload in their logs.

Ignored machine-readable evidence is written below
`Gradial APP/captures/plugin-privacy`:

- `report.json` contains the pass/fail checks, sentinel hashes, exact ZIP entry
  names, and route metadata;
- `release-and-export-logs.txt` is the combined scanned log evidence;
- `diagnostics-default.zip` is the closed default export; and
- `diagnostics-process-path-opt-in.zip` is the explicit-path export.

## Automation boundary

Run after building both configurations and closing any existing Gradial
process:

```powershell
cargo build --workspace --all-features
cargo build --workspace --all-features --release
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-plugin-privacy-evidence.ps1
```

`-SelfTest` runs without Gradial. It accepts a synthetic safe log/default/
opt-in set, then injects a forbidden parameter sentinel and proves the analyzer
rejects it. `scripts/check.ps1` runs this deterministic self-test after the
other Milestone 4 analyzer tests.

This campaign proves the current log and diagnostic-export boundaries. It does
not claim that a user who explicitly chooses a configured-process-path export
has kept that selected path private; the separate ZIP name, manifest flag,
notice, exact path count, and analyzer exception make that disclosure
deliberate and narrowly scoped.
