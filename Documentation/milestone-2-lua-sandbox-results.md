# Milestone 2 Lua sandbox foundation results

Date: 2026-08-10

## Outcome

Gradial now contains a bounded Lua 5.4 host foundation without enabling or
executing discovered plugins. The new `gradial-lua` crate embeds a vendored Lua
runtime through `mlua`, owns no filesystem or Windows APIs, and accepts only an
immutable source image produced by the existing package discovery boundary.

Normal application startup still performs manifest discovery only. It never
calls `LoadedLuaPlugin::load`; copying a package below
`%LOCALAPPDATA%\Gradial\plugins` therefore executes zero Lua code. The explicit
load API exists for the later enablement/runtime slice and is exercised only by
isolated tests in this milestone.

## Frozen source image

Discovery now retains the accepted Lua text rather than retaining paths for a
later reopen. `PluginSourceImage`:

- owns UTF-8 source independently from package files;
- normalizes each relative path to `/` separators;
- rejects absolute, traversing, alternate-stream, duplicate, non-Lua, missing
  entry, non-UTF-8, and bytecode-marker inputs;
- rechecks the 256-file, 256 KiB-per-file, and 1 MiB source-set limits; and
- has a redacted `Debug` representation containing counts only.

An integration fixture discovers a valid package, replaces its `main.lua` with
different invalid text, and then explicitly loads the previously frozen image.
The original image loads unchanged, proving that the Lua host does not reopen
or race package paths.

## Reviewed Lua environment

Every explicit load creates a fresh Lua 5.4 state with basic value/iteration
functions and only `math`, `string`, `table`, and `utf8`. The environment
removes:

- `os`, `io`, `package`, `debug`, and coroutines;
- global `require`, `load`, `loadfile`, and `dofile`;
- `collectgarbage`, `print`, `warn`, and `rawset`; and
- bytecode-producing `string.dump`.

All chunks are forced to text mode. The state contains no filesystem, socket,
process, registry, clipboard, window, input, renderer, configuration, context,
or capability function. The `gradial` table is a protected proxy; in this
slice it exposes only the source-image module resolver.

## Private module resolver

Lua source uses the deliberately familiar form:

```lua
local mapping = gradial.require("lib.mapping")
```

The resolver accepts a closed lowercase module grammar and maps the example
only to `lib/mapping.lua` inside the immutable image. It never accepts path
separators or traversal and never accesses the filesystem. Recursive modules
share the plugin's state and environment, successful return values are cached,
nil returns use the conventional `true` value, and dependency cycles fail
deterministically. Invalid, missing, cyclic, syntax-erroring, and runtime-erroring
modules produce stable non-content errors.

## Resource enforcement

The initial host boundary applies:

- a 16 MiB allocator limit per Lua state;
- a 100 ms wall deadline for entry and requested module loading;
- a one-million-instruction load budget; and
- a host-owned global hook every 1,000 Lua instructions.

The hook records terminal state outside Lua before returning an error. A script
cannot clear that state with `pcall`/`xpcall`; even a caught hook error still
fails the load. Tests distinguish allocator exhaustion, instruction exhaustion,
and wall deadline expiry. Limits are validated before state creation and cannot
be raised above the v0.3 16 MiB/one-second host ceilings through this API.

Lua and embedding failures are reduced to closed error codes. Source text,
runtime messages, tracebacks, module contents, and host values are not retained
in the public error or `Debug` output. Rust panic catching is disabled inside
Lua so a future host callback panic cannot be swallowed by plugin `pcall`.

## Isolation evidence

Focused fixtures prove:

- each load owns a new sendable Lua state with no global leakage;
- allowed standard libraries work and excluded surfaces are `nil`;
- the protected host proxy cannot be overwritten through the exposed API;
- rooted recursive modules load once and return their cached value;
- invalid names, missing modules, and cycles remain distinct;
- entry and nested-module syntax errors contain no source sentinel;
- the entry must return a table;
- allocator, instruction, deadline, and caught-hook cases fail as specified;
- runtime messages are redacted; and
- discovery rejects a bytecode marker and freezes bytes before later loading.

The repository check passed PowerShell harness self-tests, formatting, strict
Clippy, all-feature tests, documentation tests, and debug builds. The debug
suite passed 217 application tests, 81 core tests, and 13 Lua-host tests. The
separate all-feature release suite passed 213 application tests, 81 core tests,
and the same 13 Lua-host tests.

## Superseded deferred boundary

The callback-validation and isolated execution portion originally deferred by
this report is now implemented by `LuaLifecycleAdapter`; see
`milestone-2-lua-lifecycle-results.md`. Coordinator integration, plugin
workers, enablement/approval state, Settings, and external capabilities remain
deferred. The application still has no normal Lua-loading call site.
