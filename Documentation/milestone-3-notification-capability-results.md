# Milestone 3 notification-capability results

## Outcome

This bounded Milestone 3 slice implements the `notification` plugin capability
without giving Lua access to the Windows shell. An active callback can request
one literal message through `gradial.notification(text)`. Gradial validates and
queues it immediately; the normal application loop later performs the native
non-focus-stealing notification request.

The checked-in `examples/plugins/org.gradial.example.notification` package is
the positive authoring example. It requests one fixed message from Commit and
does nothing externally from Begin, Update, or Cancel.

## Contract and limits

- The manifest must declare and the user must approve `notification`.
- Text must be nonempty, single-line UTF-8 of at most 128 bytes.
- Control characters and URL-shaped schemes, domains, email/IP/localhost
  addresses, Windows paths, and UNC paths are rejected.
- Text is always literal. There is no markup, sound, URL target, callback
  destination, or action button.
- One session can accept one request. One plugin can accept five requests in a
  rolling minute across all of its sessions.
- The application handoff is nonblocking and holds at most twenty messages.
- Rejections return only stable bounded reasons and do not themselves fault
  Lua; permission, rate, queue, cancellation, and shutdown failures still fail
  the capability epoch closed.

## Lifecycle and native delivery

The broker epoch owns the validated plugin ID, reviewed permission state, and a
cancellation token. Permission is checked for every request. A queued message
retains the token, so permission revocation, gesture cancellation, plugin
failure, or runtime shutdown can discard it before native submission.

Every cancellation path closes the epoch before Lua's Cancel callback runs;
notification requests from Cancel are rejected. Commit closes the successful
epoch after Lua returns and leaves its accepted item deliverable. No Windows
call occurs on the plugin worker, action coordinator, interaction thread, or
input hook.

The existing tray icon submits a silent information notification with quiet
time respected. Its fixed title is **Gradial plugin notification**. The first
body line is the validated plugin ID and the second is plugin-controlled plain
text, so the requester cannot disguise its identity. Explorer submission is
best effort and never activates Gradial's controller window.

## Privacy

The core text value and queued request have redacted debug representations.
Logs include only stable event/result codes and the already public validated
plugin ID. They never include notification text, Lua messages, parameters,
context, target data, or capability payloads. Diagnostic bundles inherit the
same bounded log boundary.

## Evidence

Automated coverage includes:

- accepted Unicode/literal text and rejection of empty, control, oversized,
  malformed, and URL-shaped content;
- exact protected Lua callback scoping and read-only host replacement rules;
- permitted, denied, revoked, per-session-limited, per-plugin-limited, busy,
  cancelled, and shutdown broker paths;
- cancellation of pending delivery and preservation after successful Commit;
- native field/flag verification, plugin identification, and debug redaction;
- end-to-end Lua-worker-to-broker admission with and without permission; and
- loading and executing the checked-in notification example through the real
  plugin worker and a fake bounded broker.

The complete repository check covers formatting, warning-denied Clippy,
PowerShell analyzer self-tests, all-feature debug and release suites,
documentation tests, and the debug workspace build. The debug suites pass 281
application, 92 core, and 26 Lua tests; the optimized suites pass 277
application, 92 core, and 26 Lua tests.

## Deferred boundary

`network.loopback` remains a separate capability slice. Notifications cannot
synthesize input, access target details, open files or URLs, start processes,
send network traffic, persist messages, add actions, or receive user responses.

Subsequent status (2026-08-10): `network.loopback` is now implemented as its
own exact-origin bounded broker, completing the Milestone 3 capability set. See
`milestone-3-loopback-capability-results.md` for its boundary and evidence.
