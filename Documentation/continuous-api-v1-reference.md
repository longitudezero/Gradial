# Gradial Continuous API v1 reference

The Continuous API connects Gradial's distance gesture to a local Lua plugin.
Gradial owns input, direction selection, normalized distance, target identity,
coalescing, and lifecycle ordering. A plugin receives complete scalar samples
and gives them application-specific meaning through approved host
capabilities.

This document describes `api_version = 1` as implemented by Gradial v0.3. For
package metadata and parameter declarations, use the
[plugin manifest v1 reference](plugin-manifest-reference.md). For a guided
first plugin, use the [Lua plugin author guide](plugin-author-guide.md).

## Entry contract

The manifest `entry` file is evaluated when Gradial enables or reloads the
plugin. It must return a table containing all four callback functions:

```lua
return {
    begin = function(context, parameters, initial)
        return { last_value = initial.value }
    end,

    update = function(session, sample)
        session.last_value = sample.value
    end,

    commit = function(session, final_sample)
        session.last_value = final_sample.value
    end,

    cancel = function(session, reason)
        session.cancel_reason = reason
    end,
}
```

The entry value must be a table, and `begin`, `update`, `commit`, and `cancel`
must each be functions. Gradial captures those functions at load time.

Entry evaluation has no active session. Well-formed capability calls made
there are rejected with `false, "inactive_session"`. Keep entry work to
defining functions and loading frozen Lua modules.

## Lifecycle

One outside-wedge segment has one lifecycle:

```text
Begin accepted --> zero or more Update --> Commit
                                  `-----> Cancel

Begin rejected -------------------------> end (no terminal callback)
```

The rules are:

- Every segment has a fresh opaque `session_id` and starts with sequence `0`.
- A successfully accepted Begin receives at most one terminal Commit or
  Cancel.
- No Update occurs after a terminal decision.
- Requests from different sessions are not interleaved in v0.3, and only one
  plugin callback runs at a time.
- If release or cancellation occurs while Begin is pending, Gradial retains
  the terminal decision. If Begin succeeds, the terminal callback follows it;
  if Begin is rejected, no terminal callback occurs.
- Commit and Cancel are terminal even when their callback fails.
- Cancel is best-effort. It means stop producing effects and release private
  session resources; it does not undo effects already produced.

Do not assume Cancel will follow a callback error. A runtime, shape, resource,
or deadline failure faults the plugin and can make another Lua call impossible.

## Callback contracts

### `begin(context, parameters, initial)`

Begin must return exactly one Lua table. Gradial retains that table as the
segment's private mutable `session` state and passes the same table to later
callbacks.

```lua
begin = function(context, parameters, initial)
    return {
        action_id = context.action_id,
        gain = parameters.gain,
        last_value = initial.value,
    }
end
```

No value, a non-table value, or more than one value faults the plugin.

### `update(session, sample)`

Update receives the retained session table and the latest delivered complete
sample. It must return no values.

```lua
update = function(session, sample)
    session.last_value = sample.value
end
```

Updates are optional, cadence-limited, and loss-tolerant. Use `sample.value`
as authoritative current state rather than treating every callback as an
irreplaceable step.

### `commit(session, final_sample)`

Commit receives the retained session table and an exact final sample. It must
return no values. The final sample is delivered even when its value equals the
latest Update value.

```lua
commit = function(session, final_sample)
    session.last_value = final_sample.value
end
```

Commit is the normal place for one final external effect. Gradial removes the
private session from its registry before calling Commit, so there is no retry
or later Cancel for that segment.

### `cancel(session, reason)`

Cancel receives the retained session table and one stable snake-case reason
string. It must return no values.

```lua
cancel = function(session, reason)
    session.cancel_reason = reason
end
```

Cancel should stop plugin-side work and return promptly. Host capabilities
reject calls made during Cancel, so do not attempt a notification, shortcut,
or loopback request there.

### Returning no values

Update, Commit, and Cancel must fall through, use a bare `return`, or otherwise
produce zero Lua results. `return nil` produces one result and therefore
violates the callback contract.

## Context

`context` is a read-only table frozen from the invocation target and approved
manifest permissions.

| Field | Lua type | Contract |
| --- | --- | --- |
| `api_version` | integer | `1` for this API. |
| `session_id` | string | Opaque UUID for this segment. Do not parse it for behavior. |
| `action_id` | string | Manifest action ID selected for this segment. |
| `application` | table or `nil` | Present only with `context.application` and an available executable path. |
| `window_title` | string or `nil` | Present only with `context.window_title` and an available title. An empty string is valid. |

When present, `context.application` is also read-only:

| Field | Lua type | Contract |
| --- | --- | --- |
| `executable_path` | string | Frozen target executable path. |
| `window_class` | string | Frozen native window class; it may be empty. |

The context contains no plugin ID, process ID, window or process handle,
screen coordinate, monitor identity, raw input state, configuration object, or
another plugin's data. Values do not change if the foreground application or
title changes later.

Writing to `context` or `context.application` raises a runtime error. Use the
mutable session table for derived values.

## Parameters

`parameters` is a read-only table containing the validated configuration for
the selected manifest action. The Lua mapping is:

| Manifest type | Lua type |
| --- | --- |
| `boolean` | boolean |
| `integer` | integer |
| `number` | number |
| `string` | string |
| `choice` | string |

Required values are present. Optional values without defaults are absent until
enabled by the user, so they read as `nil`.

```lua
local bands = parameters.bands
local prefix = parameters.label_prefix
if prefix == nil then
    prefix = ""
end
```

The complete parameter map is frozen for the invocation and validated before
Begin. It is limited to 16 KiB in serialized form. Parameter definitions,
defaults, and bounds belong in `plugin.toml`, not in dynamic Lua UI code.

## Samples

The `initial`, `sample`, and `final_sample` arguments share one read-only shape:

| Field | Lua type | Contract |
| --- | --- | --- |
| `sequence` | integer | Strictly increases within the segment. Initial is `0`; gaps indicate coalescing. |
| `value` | number | Finite normalized current value in `0.0..=1.0`. |
| `delta` | number | `value` minus the previously delivered value. Initial is `0.0`. |
| `elapsed_ms` | integer | Monotonic milliseconds since Begin was accepted. Initial is `0`. |

At most one ordinary Update waits for plugin delivery. A newer observation
replaces an older undelivered observation and advances the sequence, so a
plugin might see `4` after `1`. The retained Update still contains complete
current state.

`delta` is relative to the preceding delivered sample, not every physical
mouse observation. Commit clears any pending ordinary Update, advances the
sequence, and computes its final delta from the last delivered value. The final
sample is never discarded by coalescing.

The action's manifest `update_hz` requests a cadence from 10 through 60 Hz. It
is a maximum delivery rate, not a periodic timer: no value change means there
may be no Update, and scheduling or coalescing can make delivery slower.

## Hybrid gesture and segment handoff

Releasing over a wedge uses its ordinary discrete action. Crossing beyond the
wedge's outer boundary starts its continuous segment. The direction remains
live throughout the held invocation:

- Moving inward over the wedge cancels the segment with
  `returned_to_discrete`. The held invocation remains open in discrete mode.
- Moving back outward starts a new segment with a new session ID.
- Moving outside into another wedge cancels the old segment with
  `direction_changed` before submitting the new wedge's Begin.
- Releasing while a segment is active commits the currently live wedge and
  scalar.
- Releasing in the center closes without an application effect.

Effects produced before `returned_to_discrete` or `direction_changed` remain.
A stop reason is not an undo request.

## Cancellation reasons

Handle every reason as a request to stop. Plugins may distinguish known values
for private cleanup, but should also tolerate an unfamiliar future string.

| Reason | Meaning |
| --- | --- |
| `returned_to_discrete` | The pointer moved back inside the outer boundary, or the invocation returned to a non-continuous selection. |
| `direction_changed` | The live outside direction moved to another wedge; its new segment begins only after this one stops. |
| `user_requested` | The user or action coordinator requested cancellation. |
| `escape` | The user pressed Escape. |
| `other_mouse_button` | Another mouse button cancelled the gesture. |
| `input_queue_overflow` | The bounded input queue overflowed and input handling entered its safe state. |
| `hook_unresponsive` | The input hook missed its health threshold. |
| `input_reregistration` | Gradial is rebuilding or recovering input registration. |
| `display_topology_change` | Display, work-area, or DPI topology changed. |
| `target_stale` | The frozen target was replaced, lost foreground eligibility, or failed freshness/integrity validation. |
| `plugin_faulted` | Plugin execution or its backend failed. The fault may prevent this callback from being delivered. |
| `capability_unavailable` | A required capability service became unavailable. This is defined by v1 even where the current route reports a more specific stop. |
| `reload_requested` | Reload was requested for the active plugin. |
| `plugin_disabled` | Disable was requested for the active plugin. |
| `configuration_changed` | Reserved v1 reason for invalidation by a configuration replacement; current invocations normally keep their frozen configuration. |
| `session_locked` | Reserved v1 reason for an unavailable locked desktop session. |
| `shutdown` | Gradial is shutting down. |

Cancellation has no final sample. Keep the latest needed value in the session
table during Begin and Update.

## Host functions

The read-only `gradial` table contains the v1 module loader and capability
functions:

| Function | Required manifest permission |
| --- | --- |
| `gradial.require(name)` | None |
| `gradial.input.shortcut(chord)` | `input.shortcut` |
| `gradial.notification(text)` | `notification` |
| `gradial.network.loopback(request)` | `network.loopback` plus a matching grant |

### Common capability return convention

Every capability call returns two values immediately:

```lua
local accepted, reason = gradial.notification("Completed")
if not accepted then
    session.last_rejection = reason
end
```

- Success returns `true, nil`.
- Rejection returns `false, reason`.
- Success means bounded host work was accepted, not that the eventual external
  operation succeeded.
- A rejection is not an invitation to retry in a tight loop. Some denials
  close the capability request epoch or cause Gradial to stop the action.
- Capability calls are valid during Begin, Update, and Commit. During Cancel
  they return `false, "cancelled"`; outside a callback they return
  `false, "inactive_session"`.

Stable v1 rejection strings are:

| Reason | Meaning |
| --- | --- |
| `inactive_session` | No matching active callback/session exists. |
| `permission_denied` | The required permission is absent, unapproved, or revoked. |
| `allowlist_denied` | A loopback origin or method is outside the approved manifest grant. |
| `rate_limited` | The capability's bounded rate or count has been exceeded. |
| `busy` | The bounded host queue or service cannot accept work. |
| `invalid_shortcut` | The shortcut argument is not a valid complete chord. |
| `invalid_notification` | The notification argument violates its bounded plain-text contract. |
| `invalid_loopback` | The loopback request table or one of its fields is invalid. |
| `cancelled` | The request epoch is closed, Gradial is stopping, or the call was made during Cancel. |

The following signatures summarize request shapes. Capability policy, rates,
target checks, and privacy rules will be covered by the separate trust and
capability guide.

### `gradial.input.shortcut(chord)`

`chord` is one readable complete shortcut string, with zero or more modifiers
and exactly one primary key. The function never exposes held-key or key-down
control.

```lua
local accepted, reason = gradial.input.shortcut("left_control+right_shift+right")
```

### `gradial.notification(text)`

`text` is one bounded nonempty plain line. Gradial identifies the plugin and
does not interpret the value as markup, a URL, or an action.

```lua
local accepted, reason = gradial.notification("Continuous gesture committed.")
```

### `gradial.network.loopback(request)`

The request is a closed table. `method` and `origin` are required; the other
fields have the defaults shown below.

| Field | Lua type | Default |
| --- | --- | --- |
| `method` | string: `GET` or `POST` | Required |
| `origin` | exact approved origin string | Required |
| `path` | string | `/` |
| `body` | string, including binary bytes | empty |
| `content_type` | closed content-type string | `application/octet-stream` |

Accepted content types are `application/octet-stream`, `application/json`,
`application/x-www-form-urlencoded`, and `text/plain; charset=utf-8`. GET
requires an empty body. Unknown fields make the request invalid. Lua receives
no response status, headers, or body.

```lua
local accepted, reason = gradial.network.loopback({
    method = "POST",
    origin = "http://127.0.0.1:32123",
    path = "/gradial/value",
    body = string.format('{"value":%.6f}', session.last_value),
    content_type = "application/json",
})
```

## Modules and Lua environment

Gradial provides a restricted Lua 5.4 environment with the `math`, `string`,
`table`, and `utf8` libraries plus ordinary safe language and base functions.
It removes general system and dynamic-code surfaces including `os`, `io`,
`package`, `debug`, `coroutine`, global `require`, `load`, `loadfile`, `dofile`,
`collectgarbage`, `print`, `warn`, `rawset`, and `string.dump`.

Load another frozen package source file through `gradial.require`:

```lua
local mapping = gradial.require("lib.mapping")
```

The name is 1 through 192 bytes and contains nonempty components separated by
periods. Components use lowercase ASCII letters, digits, `_`, or `-`.
`lib.mapping` resolves only to `lib/mapping.lua` in the admitted source image.
Absolute paths, separators, traversal, missing modules, and cycles are
rejected. A module is evaluated once and cached; a module returning no value
is cached as `true`.

Load commonly used modules from the entry file. A module first required inside
a callback shares that callback's much smaller deadline and instruction
budget. Module state and entry globals remain in memory until Reload, Disable,
fault, or shutdown, but they are not persistent storage. Keep gesture-specific
state in the returned session table.

## Execution limits and faults

One enabled plugin owns one restricted Lua state. The production ceilings are:

| Phase or resource | Wall-clock ceiling | Instruction ceiling |
| --- | ---: | ---: |
| Entry and entry-loaded modules | 100 ms | 1,000,000 |
| Begin | 100 ms | 1,000,000 |
| Update | 16 ms | 160,000 |
| Commit | 100 ms | 1,000,000 |
| Cancel | 50 ms | 500,000 |
| Lua memory per enabled plugin | 16 MiB | — |

Instruction and deadline checks occur through a host-owned hook every 1,000
Lua instructions. Plugin code cannot raise or disable these limits, and
catching the internal stop with `pcall` does not make an over-budget execution
succeed.

An uncaught Lua error, invalid callback result, memory limit, instruction
limit, or deadline violation faults and unloads the plugin adapter. All private
sessions are dropped, new sessions are rejected, and only a safe stable error
category reaches Gradial's UI and logs. Source text, parameters, context
values, and Lua error messages are not logged. Fix the package and choose
**Reload** before testing it again.

Capability functions enqueue bounded work outside Lua. A Commit that has
accepted terminal capability work may keep the action coordinator lease until
that work finishes or reaches its separate capability deadline; this does not
increase the Lua Commit callback's 100 ms ceiling.

## Complete checked-in examples

- [`org.gradial.example.meter`](../Gradial%20APP/examples/plugins/org.gradial.example.meter)
  implements the complete lifecycle with no host effects.
- [`org.gradial.example.shortcut-bands`](../Gradial%20APP/examples/plugins/org.gradial.example.shortcut-bands)
  maps complete scalar samples into bounded shortcut bands.
- [`org.gradial.example.notification`](../Gradial%20APP/examples/plugins/org.gradial.example.notification)
  requests one notification during Commit.
- [`org.gradial.example.loopback`](../Gradial%20APP/examples/plugins/org.gradial.example.loopback)
  sends one allowlisted loopback request during Commit.
