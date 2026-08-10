# Milestone 3 shortcut-capability results

## Outcome

This bounded Milestone 3 slice implements `input.shortcut` without exposing raw
Windows input to Lua. A continuous plugin can submit one readable, complete
keyboard chord during Begin, Update, or Commit. The protected host call returns
an accepted/rejected status immediately; a dedicated bounded worker performs
the effect later. Cancel is deliberately unable to submit an effect.

The slice also includes
`examples/plugins/org.gradial.example.shortcut-bands`, a small reviewed example
that converts radial movement into bounded shortcut steps. It submits no more
than one request per callback.

## Contract and limits

- The manifest must declare and the user must approve `input.shortcut`.
- Chords use lowercase, `+`-separated key names with side-specific modifiers
  before exactly one primary key and a 128-byte maximum.
- A chord always means complete press/release input. Lua cannot request raw
  events, unmatched key-down, held state, text, mouse input, or an event array.
- The host returns only `true, nil` or `false` plus a stable bounded reason.
- One active session can accept at most 30 shortcut requests per rolling
  second through a 32-item nonblocking queue.
- Permission denial, revocation, rate limiting, queue failure, target loss,
  integrity rejection, input failure, cancellation, or timeout fails the
  session closed.

## Target and lifecycle integrity

The broker epoch owns the same complete target snapshot frozen at invocation
down. The Windows shortcut backend revalidates handle, PID, process start time,
foreground identity, and integrity immediately before every injection. It
retains the existing reverse-order synthesized-key cleanup on every exit path.
Lua and the interaction/render threads never perform the Windows call.

Commit closes submission and waits only within its existing terminal deadline
for accepted broker work. Timeout cancels pending work. Every cancel path
closes the broker before calling the plugin's Cancel callback, so cleanup code
cannot create a late side effect. Runtime shutdown cancels open epochs and has
the existing two-second bound.

## Privacy

The chord object has a redacted debug representation. Broker and release logs
record stable component/result codes only; they do not contain chord text,
target values, Lua messages, parameters, or host payloads. Lua receives no
success details beyond acceptance into the broker.

## Evidence

Automated coverage includes:

- every supported key family plus malformed, duplicate, ambiguous, and
  oversized chord parsing;
- exact callback scoping and protected, read-only Lua host objects;
- permitted, denied, revoked, rate-limited, cancelled, busy/failure, stale or
  elevated target, cleanup failure, and terminal-timeout broker paths;
- proof that Cancel cannot enqueue an external effect;
- end-to-end Lua-worker-to-broker admission with and without permission; and
- loading and executing the checked-in banded-shortcut example through the
  real plugin worker and a fake bounded broker.

The existing shortcut-backend tests continue to cover per-event target checks
and reverse-order key cleanup. The complete repository check covers formatting,
warning-denied Clippy, PowerShell analyzer self-tests, all-feature debug and
release suites, documentation tests, and the debug workspace build. The debug
suites pass 270 application, 89 core, and 25 Lua tests; the optimized suites
pass 266 application, 89 core, and 25 Lua tests.

## Deferred boundary

`notification` and `network.loopback` remain separate capability slices. This
broker does not add text entry, mouse input, raw device events, shell commands,
network access, plugin responses, stored secrets, or background plugin work.
