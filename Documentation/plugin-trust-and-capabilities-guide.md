# Gradial plugin trust, permissions, and capabilities

Gradial v0.3 supports local Lua plugins for continuous radial controls. The
system deliberately gives plugins room to implement their own mappings while
keeping external effects behind a small, visible permission set.

The central rule is still:

> Lua plugins are local scripts. Enable only plugins you wrote or trust.
> Gradial limits their access and resource use, but v0.3 does not promise safe
> execution of hostile downloaded code.

This guide explains that trust decision, every current permission, and the
enforced capability boundaries. The
[Lua plugin author guide](plugin-author-guide.md) covers installation and
authoring, the [manifest reference](plugin-manifest-reference.md) defines the
declarations, and the
[Continuous API v1 reference](continuous-api-v1-reference.md) defines callback
and host-function syntax.

## What trust means in v0.3

A plugin's Lua state runs inside the Gradial process on a dedicated worker
thread. Gradial removes general file, process, socket, registry, clipboard,
window, input, and dynamic-code APIs; applies memory, instruction, and
wall-clock limits; and brokers approved external effects away from Lua and the
input/render paths.

Those controls are valuable. They prevent many mistakes, stop unbounded work,
and keep a plugin from simply asking Lua to read a file or open an internet
connection. They are not a formal hostile-code security boundary. A malicious
package could try to exploit a defect in Lua, its embedding, or Gradial itself.

For that reason, v0.3 has no claim equivalent to a browser sandbox:

- there is no plugin marketplace, publisher identity, signature verification,
  reputation service, or automatic safe-update channel;
- a package ID and version are author-declared labels, not proof of origin;
- a SHA-256 package fingerprint detects that admitted manifest/Lua bytes
  changed, but does not prove who wrote them; and
- even a plugin with `permissions = []` executes Lua entry code when enabled,
  reloaded, or restored at Gradial startup.

Prefer plugins you wrote, can inspect, or obtained from an author and channel
you trust. Stronger isolation for less-trusted community distribution is a
future WASM admission goal, not a v0.3 guarantee.

The fact that someone could distribute a separate malicious program does not
make plugin security unimportant. Enabling a plugin asks an app that stays
running to evaluate that code again at future startups and gestures. Gradial's
job is to avoid turning that convenience into silent authority: make effects
visible, grant only what was requested, bound failures, and leave the final
source-trust decision with the user.

## Freedom inside the boundary

Permissions restrict data and external effects, not the plugin's mapping
ideas. Within the Lua resource limits, a plugin can use ordinary control flow,
tables, functions, math, strings, UTF-8 operations, frozen modules, parameters,
and private session state to build continuous, banded, nonlinear, reversed, or
application-specific behavior.

The boundary becomes narrow only when code asks Gradial or Windows to affect
something outside Lua. v0.3 exposes complete shortcuts, plain notifications,
and exact-origin local HTTP. It intentionally does not expose raw input,
arbitrary Windows calls, files, shell/process launch, the clipboard, general
networking, background timers, native modules, or stored secrets.

If an integration needs a broader application API, a separately installed
companion service may expose a deliberately small loopback endpoint. That
service is outside Gradial's sandbox and must be trusted and secured on its own.

## When plugin code runs

| Operation | Reads package files | Evaluates Lua |
| --- | --- | --- |
| Startup discovery | Yes | Only for packages already effectively enabled after fresh reconciliation |
| **Rescan plugins** | Yes | No |
| Parameter or menu editing | No plugin execution | No |
| **Approve and enable** | Fresh discovery and validation | Yes, only after approval is durably recorded |
| **Enable** with unchanged approval | Fresh discovery and validation | Yes |
| **Reload** | Fresh discovery and validation | Yes |
| Continuous gesture | Uses the admitted frozen source image | Begin, Update, then Commit or Cancel |
| **Disable** | No replacement code | Only a best-effort Cancel callback for an active gesture; no new package is loaded |

Entry-module evaluation occurs during Enable, Reload, and startup restoration,
even when no continuous action is assigned. Lifecycle callbacks occur only for
a user-held invocation whose wedge has an available configured continuous
action. Plugins have no autonomous background callback or timer.

## First approval and later changes

### First enable

An unapproved plugin opens **Trust and enable this plugin?**. The dialog shows
the validated name and ID, the trust warning, every requested permission, and
every exact loopback origin/method grant. **Cancel** runs nothing and changes
nothing.

**Approve and enable** is internally two fail-closed operations:

1. Persist the exact approved permission and loopback-grant set.
2. Repeat complete discovery, validate and load a fresh candidate under normal
   Lua limits, persist its exact loaded identity/fingerprint, then admit it.

Code is not evaluated before the approval step succeeds. A load or state-save
failure leaves the candidate unavailable.

### Permission or grant changes

Any change to the permission set requires review again, including a removal.
Changing a loopback address, port, or allowed method also counts as a permission
change. An enabled plugin is disabled and unloaded before the new set can be
approved.

Already delivered context data and already completed effects cannot be taken
back. If a change is discovered during an active session, reconciliation waits
for its bounded terminal handling, then removes the adapter. New invocations
do not receive the old permission set.

### Source and ordinary manifest changes

For an enabled package, changing Lua source, version, action metadata,
parameters, or other non-permission manifest fields produces **Reload
required**, not a new permission review. A Rescan remains code-free and unloads
a changed enabled adapter after any active segment reaches its bounded terminal
path; the new files cannot run until **Reload** validates them.

Permission approval is therefore not source-code approval for every future
revision. Gradial shows no source diff and cannot authenticate the update's
author. Inspect or otherwise re-establish trust in changed source before
choosing Reload. A disabled plugin with unchanged permissions can similarly
load its current files through Enable without another trust dialog.

Gradial does not watch package files continuously. Changes are noticed at
startup, Rescan, or another plugin operation. Finish copying/editing a package
before requesting one of those operations.

## Machine-local trust state

Plugin trust is operational state for this Windows account, separate from
portable menu configuration. Enable, Disable, Reload, and approval apply
immediately and do not depend on **Save changes**. Configuration import/export
does not carry or alter plugin trust.

The bounded atomic trust record stores only:

- enabled and faulted flags;
- canonical approved permissions and exact loopback grants;
- the last successfully loaded plugin, manifest, and API versions; and
- the SHA-256 fingerprint of the admitted manifest and Lua source image.

It stores no Lua or manifest text, package path, action parameters, credentials,
window context, target identity, or capability payload. A bad primary record
recovers from its valid backup. If neither copy is usable, Gradial fails closed
to an empty in-memory state, leaving plugins disabled while the rest of the app
remains available.

## Permission overview

Every permission is optional and closed by name:

| Permission | Data or effect | Important review question |
| --- | --- | --- |
| `context.application` | Gives Lua the frozen executable path and window class. | Does the mapping truly need application identity? |
| `context.window_title` | Gives Lua the frozen window title. | Could titles contain document, account, or private activity names? |
| `input.shortcut` | Requests complete keyboard chords against the frozen target. | What commands can those chords trigger in that application? |
| `notification` | Requests a bounded, identified Gradial notification. | Why does this gesture need to interrupt the user? |
| `network.loopback` | Sends bounded HTTP requests to exact reviewed local services. | What does the service do with the request and any included data? |

Permissions compose. For example, a plugin approved for both
`context.window_title` and `network.loopback` can intentionally place the title
in a request body to its reviewed local service. Review the complete set as one
capability, not each row in isolation.

Declaring a permission is not enough. It must match the machine-local approved
set, the package must be effectively enabled, and every capability request is
checked again at its call boundary.

## Context permissions

Without either context permission, a session receives only its API/session
identity, action ID, approved parameters, and normalized samples.

### `context.application`

When approved and available, `context.application` contains exactly:

- `executable_path`: the frozen foreground executable path; and
- `window_class`: the frozen native window class.

If Gradial cannot capture the executable path, the complete application table
is omitted rather than sending a partial identity.

### `context.window_title`

When approved, `context.window_title` contains the frozen foreground window
title. A valid empty title is an empty string; an unapproved or unavailable
title is `nil`.

### Shared context rules

Context is captured when the activation button is pressed and remains read-only
and unchanged for that invocation. It never contains an HWND, PID, process
handle, process start time, cursor position, monitor/geometry data, physical
modifier state, activation binding, configuration object, or another plugin's
state.

Context permissions reveal data only; they do not grant an effect. Gradial's
logs and diagnostic exports do not gain permission to record a path/title just
because a plugin may receive it.

## `input.shortcut`

This capability accepts one readable complete keyboard chord and performs the
effect later on a dedicated bounded broker.

### Request boundary

- The chord is at most 128 bytes.
- Lowercase `+`-separated names put zero or more side-specific modifiers before
  exactly one supported primary key.
- A request always represents complete press and release input.
- There is no raw scan-code API, arbitrary event array, text or mouse input,
  unmatched key-down, or held-key control.
- Begin, Update, and Commit can request a chord. Cancel cannot.

Lua receives only `true, nil` for broker acceptance or `false, reason` for
rejection. It does not receive target details or eventual injection results.

### Rate, queue, and target enforcement

- One active session may have at most 30 accepted requests in a rolling second.
- The application-wide shortcut queue holds 32 requests and never blocks Lua.
- Exceeding the rate, losing permission, queue failure, target failure,
  cancellation, shutdown, or an injection error fails the capability epoch
  closed.
- Immediately before a chord begins, Gradial revalidates the frozen window handle,
  PID, process start time, foreground identity, and integrity level. It will not
  inject into a replaced/background window or a target at higher integrity than
  Gradial.
- Cancellation and shutdown are checked between synthesized events. Every exit
  path performs best-effort reverse-order key release, so a Gradial-generated
  key is not intentionally left held.

Running Gradial elevated broadens which target integrity levels it can reach;
normal non-elevated use preserves the narrower boundary.

### User-visible risk

A complete shortcut is safer than raw input, but it can still perform powerful
commands in the foreground application: deleting content, confirming a dialog,
or triggering an application-defined macro. Review the plugin's chord choices
and mapping behavior, especially effects produced repeatedly from Update.

## `notification`

This capability requests a non-focus-stealing native information notification
through Gradial's existing notification-area owner.

### Content boundary

- Text must be one nonempty UTF-8 line of at most 128 bytes.
- Control characters and URL-shaped schemes, domains, email/IP/localhost
  addresses, Windows paths, and UNC paths are rejected.
- Text is literal: no markup, sound, link, action button, callback, or custom
  title is available.
- The fixed title is **Gradial plugin notification**. The validated plugin ID
  is shown independently of plugin-controlled text.

### Rate and delivery

- One session may have one accepted notification.
- One plugin may have five accepted notifications across sessions in a rolling
  minute.
- The application-wide plugin-notification handoff holds 20 messages and never
  blocks Lua.
- Queued work carries a cancellation token. Revocation, cancellation, plugin
  failure, or shutdown can discard it before native submission.
- Work accepted from a successful Commit remains eligible for best-effort
  delivery. Windows/Explorer delivery is not guaranteed.

The fixed identity and closed content reduce impersonation and click-through
risk, but users should still treat notification text as content from the named
plugin, not as a Gradial security message.

## `network.loopback`

Loopback permits bounded cleartext HTTP to an exact local service. It is not
general network access, but it can still be powerful: a local media player,
automation server, or companion process may translate one request into a
significant action or forward data elsewhere.

### Reviewed grant

The manifest and trust dialog list one through 16 unique origins. Each must be
exactly `http://127.0.0.1:PORT` or `http://[::1]:PORT` with an explicit nonzero
canonical port, plus a nonempty subset of `GET` and `POST`. Every request must
match both origin and method exactly.

Gradial uses the literal socket address directly. It performs no DNS lookup,
proxy discovery, redirect following, ambient Windows credential use,
alternate-address attempt, retry, HTTPS negotiation, or non-loopback fallback.

### Request and transport boundary

- Paths are ASCII origin-form values of at most 2,048 bytes. Spaces, controls,
  backslashes, fragments, and a `//` prefix are rejected.
- Request bodies are limited to 64 KiB; GET requires an empty body.
- Content type is limited to binary, JSON, form data, or UTF-8 plain text.
- Plugins cannot set arbitrary headers, credentials, cookies, or authorization.
- One active session may have 30 accepted requests in a rolling second.
- The application-wide broker queue holds 32 requests and never blocks Lua.
- Queue delay, connect, write, and response read share one 250 ms deadline.
- Response headers are limited to 16 KiB and the body to 64 KiB. Only bounded
  HTTP/1.0 or HTTP/1.1 2xx identity responses count as success.
- Lua receives no HTTP status, headers, body, native error, or other response
  detail. Acceptance means queued, not delivered.

Request paths, queries, and bodies can contain data that the plugin received
through context or parameters. Gradial does not log them, but the reviewed
local service necessarily receives successful requests. Review that service's
own storage, forwarding, and authentication behavior.

## Capability epochs, cancellation, and revocation

Each continuous segment opens bounded capability epochs tied to its session ID,
plugin ID, approved permissions, frozen target where applicable, and
cancellation state.

- Begin, Update, and Commit may submit approved work.
- Permission and exact loopback grants are rechecked at every request.
- Cancel closes request admission before the plugin's Cancel callback, so no
  late external effect can be created there.
- Returning to the wedge, changing direction, target loss, Reload, Disable,
  plugin fault, input recovery, display change, user cancellation, and shutdown
  all close active work through bounded stop paths.
- Pending work is cancelled where safe. A shortcut already injected, a
  notification already submitted to Windows, or an HTTP request already
  received by a service cannot be undone.
- Commit closes further submission but allows already accepted terminal work
  to finish within its bounded capability/coordinator deadline.

Host calls return stable accepted/rejected values documented in the
[Continuous API reference](continuous-api-v1-reference.md#common-capability-return-convention).
Do not retry a rejection in a loop. Permission, rate, queue, target, transport,
or lifecycle failures may stop the complete continuous action even if Lua
observed a `false` result first.

## Privacy and diagnostics

Gradial's release logs retain stable operational metadata such as validated
plugin ID, opaque session ID, stage, timing, and safe result/error codes. They
exclude:

- Lua source and runtime error messages;
- manifest descriptions and action parameters;
- executable paths, window classes/titles, and target identity;
- shortcut chord contents and synthesized input values;
- notification text; and
- loopback paths, queries, headers, bodies, responses, and native errors.

The default diagnostic ZIP contains only its manifest and bounded owned logs.
It excludes plugin packages, plugin trust state, portable configuration, and
all capability payloads. The separate **Include configured application paths**
option adds only the deduplicated paths from menu configuration; it does not
opt other plugin/context values into the bundle. Gradial never uploads the ZIP.

These are Gradial's logging/export boundaries, not a restriction on a plugin's
approved behavior. A plugin that receives private context can use it internally
or pass it to an approved capability. The Milestone 4
[privacy evidence](milestone-4-plugin-privacy-results.md) verifies the current
release-log and diagnostic surfaces with recognizable private values.

## Safe installation checklist

1. Obtain the complete package from an author/channel you trust. v0.3 cannot
   authenticate the publisher.
2. Inspect `plugin.toml`, especially the ID, entry, permissions, and every
   loopback origin/method.
3. Inspect the entry and module `.lua` files. A permission-free package is
   still executable code.
4. Copy the complete package, choose **Rescan plugins**, and confirm the name,
   ID, version, API, controls, and permissions shown by Gradial.
5. Prefer testing a new mapping without effects first, then enable only the
   smallest necessary capability.
6. Run Gradial as a normal user. Do not elevate it merely to make a shortcut
   plugin reach an elevated application.
7. After an update, inspect or otherwise re-establish source trust before
   choosing Reload, even when no new permission review appears.
8. To stop trusting a package, choose **Disable** before removing its directory,
   then Rescan. Disable keeps menu references portable but prevents admission.

## Author safety checklist

- Request only permissions used by the declared action.
- Document why each context field or external effect is necessary.
- Treat parameters, titles, paths, and local-service data as potentially
  private. Do not use configuration parameters as a secret store.
- Start with a no-side-effect session mapping; add one capability at a time.
- Use complete scalar state and bounded band transitions. Do not turn every
  Update into an avoidable external burst.
- Prefer Commit for a single final notification or local request.
- Handle `false, reason` without tight retries, blocking, or fallback to a
  broader effect.
- Keep Cancel effect-free and never describe it as undoing completed effects.
- If a companion service is required, document its installation, exact local
  port/methods, data handling, authentication, and any onward network access.
- Never ask users to disable Gradial protections or run the application
  elevated as a routine setup step.

## If something looks wrong

- Choose **Disable** to stop new sessions. Settings first cancels an active
  plugin gesture through the bounded terminal path.
- Use **Rescan plugins** after unexpected file changes. It executes no Lua and
  unloads packages that are no longer effectively authorized.
- Treat an unexpected **Permission review required** or **Reload required** as
  a reason to inspect the package, not merely click through.
- A **Faulted** plugin remains stopped until a deliberate Reload or Disable.
- Preserve suspicious package files before editing if you need to investigate
  them; Gradial's safe error UI intentionally does not expose private Lua
  messages or source details.
