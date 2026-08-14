# Gradial Lua plugin author guide

This guide takes a plugin from a permission-free distance meter to an assigned
continuous control. Gradial v0.3 plugins are local Lua packages for continuous
actions: Gradial measures a normalized value from `0.0` to `1.0`, drives the
plugin lifecycle, and exposes a small set of reviewed host capabilities. The
plugin supplies the application-specific meaning.

Start without side effects. The checked-in
[`org.gradial.example.meter`](../Gradial%20APP/examples/plugins/org.gradial.example.meter)
package is the safest base because it requests no permissions and changes
nothing outside its private session table.

## 1. Copy the meter package

Open **Settings > Plugins**, choose **Open plugins folder**, and copy the
complete `org.gradial.example.meter` directory into that folder. A package has
this minimum layout:

```text
org.gradial.example.meter/
|-- plugin.toml
`-- main.lua
```

The immediate directory name must exactly match the manifest `id`. If you
rename the example, change both names. Use a globally distinctive, lowercase
ID such as `io.example.gradial.volume` rather than leaving the example ID in a
plugin you intend to share.

## 2. Discover and trust it

In **Settings > Plugins**:

1. Choose **Rescan plugins**. Rescan reads and validates files but never runs
   Lua.
2. Open **Review and enable** for the package.
3. Read the requested permissions and choose **Approve and enable**.

The approval is required even when `permissions = []`: a local script is still
code that will run on this computer. Enabling applies immediately and does not
depend on **Save changes**.

## 3. Create and assign the control

1. Open **Settings > Actions** and choose **+ New action**.
2. Under **Continuous plugin controls**, select **Measure continuously**.
3. Give the configured action a useful name and set any parameters.
4. Open **Settings > Menu**, select a profile and wedge, then enable **Enable
   distance beyond this wedge**.
5. Select the continuous action without removing the wedge's ordinary action.
6. Choose **Save changes**.

The two actions deliberately coexist. Releasing M4 over the wedge runs its
ordinary action. Moving beyond the wedge begins the continuous control;
releasing there commits it. Moving inward over the wedge switches back to the
ordinary action, and the selected direction remains live rather than locked.
Releasing in the center cancels the menu.

## 4. Understand the minimal plugin

The meter manifest declares one continuous action and no host permissions:

```toml
manifest_version = 1
id = "org.gradial.example.meter"
name = "No-side-effect meter"
version = "0.1.0"
api_version = 1
entry = "main.lua"
permissions = []

[[actions]]
id = "measure"
name = "Measure continuously"
kind = "continuous"
description = "Tracks the current scalar only in private session memory."
update_hz = 30
```

Its entry file returns the four required callbacks:

```lua
return {
    begin = function(context, parameters, initial)
        return {
            action_id = context.action_id,
            last_value = initial.value,
            update_count = 0,
        }
    end,

    update = function(session, sample)
        session.last_value = sample.value
        session.update_count = session.update_count + 1
    end,

    commit = function(session, final_sample)
        session.last_value = final_sample.value
    end,

    cancel = function(session, reason)
        session.cancel_reason = reason
    end,
}
```

`begin` receives the frozen invocation context, the configured parameter map,
and the first sample. It must return one ordinary Lua table. Gradial gives that
same private table to later callbacks. `update` may be called at the declared
rate while the value changes; coalescing means sequence numbers can skip.
Exactly one of `commit` or `cancel` ends a successfully begun session.

Every sample contains:

- `sequence`: a monotonically increasing integer;
- `value`: the current normalized value from `0.0` to `1.0`;
- `delta`: the change from the preceding delivered sample; and
- `elapsed_ms`: milliseconds since this continuous session began.

The host-owned `context`, `parameters`, and sample tables are read-only. Keep
mutable state only in the session table returned by `begin`.

## 5. Add a user parameter

Parameters make one plugin action reusable without asking users to edit Lua.
For example, this required integer appears as a bounded numeric control in
Settings:

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

Read it in Lua as `parameters.bands`. The
[`org.gradial.example.shortcut-bands`](../Gradial%20APP/examples/plugins/org.gradial.example.shortcut-bands)
package shows a complete banded mapping with integer and choice parameters.
Gradial also supports boolean, number, string, and choice definitions; see the
[manifest reference](plugin-manifest-reference.md#parameters).

Prefer an explicit valid `default` for predictable first-use behavior. An
optional parameter without a default is absent from the Lua table until the
user enables it in Settings, so test it against `nil`.

## 6. Choose the smallest capability

Lua has no general operating-system, file, process, or internet access. A
plugin can affect the host only through capabilities named in `permissions`
and approved by the user.

| Goal | Permission | Host call | Checked-in example |
| --- | --- | --- | --- |
| Read the frozen executable path and window class | `context.application` | `context.application` | None |
| Read the frozen window title | `context.window_title` | `context.window_title` | None |
| Request one complete keyboard chord | `input.shortcut` | `gradial.input.shortcut(...)` | [`shortcut-bands`](../Gradial%20APP/examples/plugins/org.gradial.example.shortcut-bands) |
| Show a bounded plain notification | `notification` | `gradial.notification(...)` | [`notification`](../Gradial%20APP/examples/plugins/org.gradial.example.notification) |
| Send GET or POST to an exact local service | `network.loopback` | `gradial.network.loopback({...})` | [`loopback`](../Gradial%20APP/examples/plugins/org.gradial.example.loopback) |

Request only what the action needs. Adding or changing a permission or
loopback grant disables the package until the user reviews the new request.
Capability calls are accepted into bounded host queues; acceptance is not a
promise that the eventual keyboard, notification, or network operation will
succeed. Never put an external side effect in `cancel`.

## 7. Edit and reload safely

Gradial does not watch plugin files automatically.

- **Rescan plugins** discovers packages and previews file or manifest changes
  without executing code.
- **Reload** validates and loads a fresh source snapshot. Source edits do not
  become active until this succeeds.
- A changed permission or loopback grant shows **Permission review required**;
  use **Review and enable** after inspecting it.
- A callback failure faults and unloads the plugin. Fix the source, then use
  **Reload**. Gradial reports a stable error category rather than private Lua
  values.
- **Disable** unloads the plugin but does not erase configured action or menu
  references. Re-enable it when ready.

Reload and Disable safely cancel an active continuous gesture first. Complete
file edits before rescanning or reloading so Gradial never sees a half-copied
package.

## 8. A practical test loop

1. Begin with `permissions = []` and store the scalar only in the session
   table.
2. Assign the action and confirm that the on-screen percentage and hybrid
   wedge behavior feel correct.
3. Add and exercise parameter boundaries, including absent optional values.
4. Add one capability, review it, and test both its success and denial paths.
5. Test commit, returning to the wedge, center cancellation, target changes,
   rapid invocations, Reload, and Disable.

When building Gradial from source, the inert development meter can test the
input geometry without loading a plugin:

```powershell
cd "Gradial APP"
cargo run --package gradial-app -- --continuous-meter --skip-onboarding
```

It makes every direction a no-side-effect `0-100%` meter. It does not run or
inspect plugin callbacks. Add `--continuous-full-scale-dip=160` through `400`
to test a different distance scale.

## 9. Troubleshooting

| Status or symptom | What to check |
| --- | --- |
| Package does not appear | It is an immediate directory under the Plugins folder and contains `plugin.toml`. Choose **Rescan plugins**. |
| **Invalid** | The folder name, manifest, package boundary, source encoding, or source limits are invalid. Compare against the [manifest reference](plugin-manifest-reference.md). |
| **Incompatible** | `api_version` is not supported by this Gradial build. |
| **Permission review required** | Permissions or loopback grants changed. Review and approve the new request. |
| **Reload required** | Files differ from the loaded snapshot. Complete the edit and choose **Reload**. |
| **Faulted** | A load or callback limit/contract failed. Fix the package and choose **Reload**. |
| Action is absent from **+ New action** | The plugin must be enabled and the action must pass manifest validation. |
| Moving beyond a wedge does nothing | Enable **Enable distance beyond this wedge**, select the configured continuous action, and save Settings. |
| Context field is `nil` | Declare and obtain approval for its context permission; the field can also be unavailable from the target. |
| Host call returns `false, reason` | Treat the reason as a bounded denial or retry signal; do not spin or flood the host. |

## Reference

- [Plugin manifest v1 reference](plugin-manifest-reference.md)
- [No-side-effect meter](../Gradial%20APP/examples/plugins/org.gradial.example.meter)
- [Banded shortcut example](../Gradial%20APP/examples/plugins/org.gradial.example.shortcut-bands)
- [Commit notification example](../Gradial%20APP/examples/plugins/org.gradial.example.notification)
- [Loopback commit example](../Gradial%20APP/examples/plugins/org.gradial.example.loopback)
