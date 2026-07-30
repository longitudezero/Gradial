# Milestone 2 activation-binding editor results

## Scope

This bounded slice completes the v1 activation-binding path:

- Mouse Button 4 or Mouse Button 5;
- optional generic Ctrl, Alt, and Shift requirements;
- exact chord matching;
- draft-only settings edits;
- a safe one-shot hold/flick/release tester;
- live runtime replacement only after a successful save.

Left/right modifier identity is deliberately collapsed for activation. Either
physical Ctrl key satisfies Ctrl, for example. This is separate from action
shortcuts, which continue to preserve left/right modifier identity.

## Configuration and migration

Schema three adds this top-level value:

```json
{
  "activation_binding": {
    "button": "m4",
    "modifiers": {
      "control": false,
      "alt": false,
      "shift": false
    }
  }
}
```

The built-in default remains unmodified M4. The model uses closed enums and
booleans, so there is no unsupported mouse-button value or duplicate modifier
state to validate.

Schema-one files still receive the schema-two action defaults in memory and
now also receive the M4 binding. Schema-two files receive only the binding
default. Both migrations advance to schema three in memory, fully validate,
and preserve the source bytes until the user explicitly saves. A successful
save writes schema three through the existing temporary-file, flush, atomic
replace, and validated-backup path.

## Production input behavior

The immutable input-runtime configuration now contains the complete saved
binding. At every physical X-button down, the hook captures the three generic
modifier bits and compares the complete chord:

1. the button must be the saved M4 or M5 value;
2. every required generic modifier must be down;
3. every unrequested generic modifier must be up.

A mismatch passes through without entering the interaction queue. A match
suppresses the activation down and its paired release and sends the fixed-size
button identity, modifier bits, geometry, timestamp, and cursor point through
the existing bounded queue. The invocation snapshot freezes the actual M4/M5
identity instead of assuming M4. The overlay center label also reflects that
identity.

While a gesture is open, the other side button remains a normal
non-activation cancellation input. Its down and paired release are consumed
without leaking half a click. Gradial-tagged injected events continue to pass
through before binding classification.

Saving settings stops the old interaction runtime only after the complete
proposed schema-three configuration validates, compiles, and reaches disk.
The replacement hook starts with the new binding. A failed save leaves both
the old runtime and the private draft intact.

## Safe settings tester

The Activation tab exposes M4/M5, Ctrl, Alt, and Shift controls and explains
exact matching and dedication behavior. Its tester is intentionally separate
from production invocation:

- arming stores only a compact expected-chord value in atomics;
- the next physical M4 or M5 down is intercepted before production handling;
- a mismatched chord is reported and its release remains paired and consumed;
- a matched chord tracks signed origin and latest cursor coordinates;
- release reports centered cancellation or one of eight clockwise directions;
- no interaction snapshot, profile resolution, menu opening, or action plan is
  created.

The hook callback performs no allocation, logging, waiting, or locking for the
tester. Settings polls the atomic result at a 50 ms repaint interval. Leaving
Activation, discarding, saving, or closing disarms the test. If a consumed
button is still held during disarm, its eventual release is still consumed.

## Automated coverage

The debug verification suite contains 173 tests:

- 125 `gradial-app` tests;
- 48 `gradial-core` tests.

Coverage added by this slice includes:

- exact generic-modifier matching and extra-modifier rejection;
- M5 activation with Ctrl/Shift while M4 passes through;
- the triggering M5 identity in the fixed-size input event;
- draft edit/discard isolation;
- chord display and eight-direction tester interpretation;
- centered safe cancellation;
- schema-one and schema-two in-memory migration;
- rejection of activation-binding fields smuggled into an older schema;
- the schema-three default binding.

Release builds contain 171 tests because two development-option tests are
debug-only.

## Native Windows evidence

`smoke-settings-editor.ps1 -CaptureScreenshot` passed with a private isolated
schema-two configuration. The harness:

1. opened the real settings viewport;
2. created an unsaved exact Ctrl+M5 draft;
3. armed the one-shot tester;
4. submitted a native Ctrl-held M5 down, 116-pixel east flick, and M5 release;
5. observed an east result without a production invocation or action;
6. completed the existing profile, menu, action, chooser, sequence, guarded
   close, and privacy checks;
7. saved schema three at revision 2;
8. verified that the old schema-two revision remained in `config.json.bak`;
9. submitted a fresh production Ctrl+M5 east gesture;
10. observed the frozen `MouseButton5` identity, Ctrl state, reloaded
    four-slice menu, and successful action completion.

The captured activation surface is
`Gradial APP/captures/settings-binding-editor-smoke.png`; captures remain
ignored build evidence rather than source-controlled assets.

The standard release M4 Delay smoke check also remains required. It proves
that the default binding and existing executor path are unchanged for
configurations migrated from schema two.

## Remaining boundary

Activation configuration is now complete for the v1 scope. Appearance,
icons, import/export, and onboarding remain separate future slices.
