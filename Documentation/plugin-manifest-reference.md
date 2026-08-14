# Gradial plugin manifest v1 reference

Every local Lua package contains a UTF-8 `plugin.toml`. Parsing is bounded and
code-free: Gradial does not evaluate Lua while discovering or rescanning a
package. The schema is closed, so misspelled or unknown fields fail validation
instead of being silently ignored.

This reference describes `manifest_version = 1` and `api_version = 1` as
implemented by Gradial v0.3.

## Minimal manifest

```toml
manifest_version = 1
id = "org.example.gradial.meter"
name = "Example meter"
version = "0.1.0"
api_version = 1
entry = "main.lua"
permissions = []

[[actions]]
id = "measure"
name = "Measure continuously"
kind = "continuous"
description = "Keeps the distance value in private session memory."
update_hz = 30
```

The package directory must be named `org.example.gradial.meter`, exactly
matching `id`, and `main.lua` must exist inside it.

## Top-level fields

| Field | Required | Contract |
| --- | --- | --- |
| `manifest_version` | Yes | Integer `1`. Other values are invalid. |
| `id` | Yes | Package identifier; see [identifiers](#identifiers-and-display-names). |
| `name` | Yes | User-facing display name. |
| `version` | Yes | Valid semantic version such as `0.1.0` or `1.0.0-beta.1`. |
| `api_version` | Yes | Integer `1` for the current Lua API. A well-formed future version is discovered as incompatible rather than invalid. |
| `entry` | Yes | Relative path to a lowercase `.lua` file inside the package. |
| `permissions` | No | Unique permission strings. Defaults to `[]`. |
| `loopback` | No | Exact loopback grants. Required and nonempty only with `network.loopback`; otherwise it must be absent or empty. |
| `actions` | No | Array of continuous action tables. Defaults to `[]`; at most 64. |

The complete manifest is limited to 64 KiB.

## Identifiers and display names

Plugin IDs, action IDs, and parameter keys use the same identifier grammar:

- 1 through 96 bytes;
- ASCII lowercase letters `a-z`, digits `0-9`, period `.`, underscore `_`, or
  hyphen `-`; and
- no spaces, uppercase letters, or other punctuation.

Plugin IDs should use a distinctive reverse-domain style. Action IDs and
parameter keys need only be unique within their containing package or action.

Plugin names, action names, and parameter labels are trimmed, nonempty
user-facing strings of at most 128 UTF-8 bytes with no control characters.
Action descriptions default to empty and are limited to 1,024 UTF-8 bytes.

## Entry path

`entry` is a package-relative Lua source path, for example `main.lua` or
`src/main.lua`. It must:

- end in lowercase `.lua`;
- remain inside the package, with no absolute root, drive/colon, empty
  component, `.` component, or `..` component; and
- identify an ordinary UTF-8 source file admitted into the frozen package
  snapshot.

Use forward slashes in portable manifests. Symlinks, junctions, reparse
points, alternate data streams, native modules, and Lua bytecode are not
admitted.

## Permissions

`permissions` is a unique array containing only these values:

| Permission | Makes available |
| --- | --- |
| `context.application` | Frozen `context.application.executable_path` and `context.application.window_class`, when available. |
| `context.window_title` | Frozen `context.window_title`, when available. |
| `input.shortcut` | `gradial.input.shortcut(...)` for complete bounded keyboard chords. |
| `notification` | `gradial.notification(...)` for one-line native notifications. |
| `network.loopback` | `gradial.network.loopback({...})` for reviewed local HTTP requests. |

Permissions are not implied by calling a function: the manifest request must
be approved by the user before that function can work. Changing the permission
set requires review again.

### Loopback grants

With `network.loopback`, declare one through 16 unique exact origins:

```toml
permissions = ["network.loopback"]
loopback = [
    { origin = "http://127.0.0.1:32123", methods = ["POST"] },
    { origin = "http://[::1]:32124", methods = ["GET", "POST"] },
]
```

Each `origin` must be canonical cleartext HTTP using literal `127.0.0.1` or
`[::1]` and an explicit decimal port from 1 through 65535. Hostnames, HTTPS,
paths, defaulted ports, and leading zeroes are invalid. `methods` must be a
nonempty unique subset of uppercase `GET` and `POST`. Origin/method changes
require another user review.

## Actions

Each `[[actions]]` table accepts exactly these fields:

| Field | Required | Contract |
| --- | --- | --- |
| `id` | Yes | Identifier unique within the package. |
| `name` | Yes | User-facing display name. |
| `kind` | Yes | The only v1 value is `"continuous"`. |
| `description` | No | User-facing explanation, default empty, at most 1,024 bytes. |
| `update_hz` | Yes | Integer from 10 through 60 inclusive. |
| `parameters` | No | Array of parameter tables, default empty, at most 32 per action. |

`update_hz` is the maximum delivery cadence requested from Gradial, not a
guarantee that every interval produces a callback. Gradial coalesces pending
motion and may skip sequence numbers rather than queue unbounded updates.

## Parameters

Every parameter is a `[[actions.parameters]]` table. All types share:

| Field | Required | Contract |
| --- | --- | --- |
| `key` | Yes | Identifier unique within the action. |
| `label` | Yes | User-facing display name. |
| `type` | Yes | `boolean`, `integer`, `number`, `string`, or `choice`. |
| `required` | No | Boolean, default `false`. |
| `default` | No | Value of the declared type satisfying its constraints. |

Optional parameters without defaults start absent and are controlled by **Use
this optional setting** in Settings. Required parameters without defaults get
a deterministic valid editor value: `false`, the numeric minimum, a string of
`x` characters at the minimum length, or the first choice. An explicit default
is recommended for intentional first-use behavior.

### Boolean

```toml
[[actions.parameters]]
key = "show_feedback"
label = "Show feedback"
type = "boolean"
required = true
default = true
```

No fields beyond the five shared fields are accepted.

### Integer

```toml
[[actions.parameters]]
key = "bands"
label = "Number of bands"
type = "integer"
required = true
default = 6
minimum = 2
maximum = 12
```

`minimum` and `maximum` are required signed integers and `minimum <= maximum`.
The default, when present, must be inside the inclusive range.

### Number

```toml
[[actions.parameters]]
key = "gain"
label = "Gain"
type = "number"
required = true
default = 1.0
minimum = 0.0
maximum = 2.0
```

`minimum` and `maximum` are required finite numbers with `minimum <= maximum`.
The default must also be finite and inside the inclusive range.

### String

```toml
[[actions.parameters]]
key = "label_prefix"
label = "Label prefix"
type = "string"
required = false
default = "Volume"
minimum_length = 0
maximum_length = 40
```

`maximum_length` is required and may be at most 4,096 characters.
`minimum_length` defaults to zero and cannot exceed the maximum. Gradial counts
Unicode characters, not UTF-8 bytes, for these two bounds.

### Choice

```toml
[[actions.parameters]]
key = "increase_shortcut"
label = "Increase shortcut"
type = "choice"
required = true
default = "right"
choices = ["right", "up", "page_up", "numpad_plus"]
```

`choices` is required and contains 1 through 64 unique, nonempty strings. Each
choice is limited to 4,096 UTF-8 bytes. A default must exactly match one choice.

The serialized parameter map for one configured action is limited to 16 KiB.
Gradial validates the complete map before saving configuration and again before
a plugin session begins.

## Package and source bounds

Discovery admits only immediate package directories below Gradial's Plugins
folder. The package root may contain at most 4,096 filesystem entries, with at
most 32 directory levels. Its frozen Lua source set is limited to 256 `.lua`
files and 1 MiB total; each source file is limited to 256 KiB and must be valid
UTF-8. The Plugins root itself is limited to 1,024 entries.

Ordinary documentation and asset files may live in the package, but they are
inert and never become Lua loader candidates. Reparse points and files outside
the package are rejected rather than followed.

Additional modules use Gradial's frozen loader:

```lua
local mapping = gradial.require("lib.mapping")
```

That name resolves within the package to `lib/mapping.lua`. Module names are
bounded lowercase identifiers separated by periods; path separators and
traversal are not accepted. Modules are cached per loaded adapter.

## Change and trust behavior

- **Rescan plugins** parses and fingerprints packages without running Lua.
- **Enable** or **Approve and enable** validates, loads, and starts using the
  approved source snapshot.
- Ordinary source or manifest edits require **Reload** before use.
- Permission or loopback grant changes require another explicit review.
- Disabled, invalid, incompatible, unavailable, faulted, or reload-pending
  packages do not admit new continuous sessions.
- Plugin enablement is machine-local and applies immediately; action/menu
  configuration remains portable and changes only with **Save changes**.

## Validation checklist

- Directory name exactly equals `id`.
- `plugin.toml` is UTF-8, no larger than 64 KiB, and contains no unknown fields.
- IDs use only the lowercase identifier alphabet and are unique in scope.
- `version` is valid SemVer; manifest and API versions are `1`.
- `entry` names an admitted lowercase `.lua` source file inside the package.
- Permissions are unique and minimal.
- `network.loopback` and its nonempty grants appear together.
- Action update rates are within 10-60 Hz.
- Parameter keys are unique and defaults satisfy every bound.
- The entry module returns `begin`, `update`, `commit`, and `cancel` functions.

For installation, assignment, lifecycle examples, and a safe testing sequence,
continue with the [Lua plugin author guide](plugin-author-guide.md).
