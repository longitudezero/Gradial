# Milestone 3 loopback-capability results

## Outcome

This bounded Milestone 3 slice implements `network.loopback` without giving Lua
a socket, resolver, HTTP client, proxy, credential store, or response data. An
active callback can validate and enqueue one closed GET/POST request through
`gradial.network.loopback(request)`. A dedicated broker thread performs the
direct local HTTP operation outside Lua, the interaction runtime, and the input
hook.

The checked-in `examples/plugins/org.gradial.example.loopback` package is the
positive authoring example. It posts one JSON scalar on Commit and performs no
external work from Begin, Update, or Cancel.

## Reviewed manifest identity

- The manifest must declare `network.loopback` and one to sixteen grants.
- Each grant contains one canonical `http://127.0.0.1:PORT` or
  `http://[::1]:PORT` origin and a nonempty unique subset of GET and POST.
- Ports are explicit and nonzero. Hostnames, other addresses, HTTPS, paths,
  user information, default ports, and ambiguous spellings are invalid.
- The trust record stores the canonical origin/method grants with the broad
  permission. Changing an address, port, or method disables an enabled plugin
  until the new exact list is reviewed.
- The Plugins page and confirmation dialog display each origin and method.

## Request contract

The Lua request is a closed table with required `method` and `origin`, plus
optional `path`, `body`, and `content_type`. Path defaults to `/`; body defaults
empty. Paths are ASCII HTTP origin-form values of at most 2,048 bytes and
cannot contain spaces, controls, backslashes, fragments, or a scheme-relative
`//` prefix. GET cannot carry a body. Bodies are capped at 64 KiB.

Content type is a closed choice of JSON, form, UTF-8 plain text, or binary.
Plugins cannot provide arbitrary headers. Unknown fields, wrong Lua types,
invalid UTF-8 structural values, unsafe paths, and oversized bodies return
`invalid_loopback` before broker admission. Core request debug output redacts
the path, query, and body.

## Broker and transport

The broker queue holds 32 requests and each active session may accept thirty
requests in a rolling second. Every call rechecks the permission and exact
origin/method grant. Enqueue uses `try_send`; Lua never waits for network I/O
and receives only accepted/rejected status. Accepted means queued, not delivered.

The production runner converts the already validated origin directly to a
`SocketAddr`. It performs no DNS lookup, proxy discovery, redirect following,
ambient credential use, alternate-address attempt, retry, or non-loopback
fallback. The queue delay, connection, write, and response read share one 250
ms deadline. Request and response bodies are capped at 64 KiB, response headers
at 16 KiB, and only bounded HTTP/1.0 or HTTP/1.1 2xx identity responses succeed.
Lua receives no HTTP status, header, body, or transport detail.

## Lifecycle and privacy

Each continuous session owns one broker epoch and cancellation token. Commit
stops new admission and drains accepted work within a 350 ms coordinator
budget: the 100 ms Lua Commit ceiling plus the 250 ms capability deadline.
Permission/grant revocation, a denied destination, rate or
queue failure, transport failure, timeout, cancellation, plugin failure, or
shutdown fails the epoch closed. Every Cancel path closes it before Lua's
Cancel callback.

Logs contain only stable result codes, validated plugin ID, and session ID.
They never include request paths, query values, headers, bodies, response
content, Lua messages, context, target data, or native network errors.
Diagnostic bundles inherit the same boundary. The manifest-declared origin and
methods are visible only where required for review and local trust persistence.

## Evidence

Automated coverage includes:

- canonical IPv4/IPv6 parsing and rejection of hostnames, non-loopback
  addresses, HTTPS, missing/zero/ambiguous ports, user information, and paths;
- permission/grant consistency, uniqueness, count, and exact trust-reapproval
  behavior;
- closed Lua table parsing, protected host replacement, callback scoping, body,
  path, content-type, GET, and redacted-debug boundaries;
- positive, missing-permission, wrong-origin, revoked, rate-limited, busy,
  transport-failed, timed-out, cancelled, and shutdown broker paths;
- Commit draining and stable continuous-coordinator failure mapping;
- the checked-in example crossing the real discovery, trust, Lua worker, and
  fake broker boundary; and
- a real local TCP harness verifying the production POST fields, direct
  destination, response bounds, deadline, and no redirect following.

The complete repository check covers formatting, warning-denied Clippy,
PowerShell analyzer self-tests, all-feature debug and release suites,
documentation tests, and the debug workspace build. Final suite counts are
291 application, 96 core, and 27 Lua tests in debug, plus 287 application, 96
core, and 27 Lua tests in the optimized build.

## Deferred boundary

Milestone 3 is complete. Loopback v0.3 deliberately omits HTTPS, DNS names,
custom headers, redirects, proxies, authentication or stored secrets, retries,
streaming, chunked transfer encoding, response delivery, background sessions,
WebSockets, raw TCP/UDP, and non-loopback networking. Expanding any of those
surfaces requires a separately reviewed capability design. Milestone 4 retains
the performance report, hardening matrices, and consolidated author/reference
documentation; Windows code signing remains a final release task.
