# Release timing instrumentation results

Date: 2026-08-14

## Outcome

The two previously missing release measurements are now implemented and
proven on their short, non-qualifying production routes:

- activation release to completed `SendInput` shortcut submission; and
- continuous outer-edge/publication/release input to Lua Begin, Update, and
  terminal callback start.

This closes an instrumentation gap. It does not replace the long elevated
release campaigns, their required sample counts, or PresentMon evidence.

## Shortcut boundary

The action worker emits `shortcut_submitted` only after the existing shortcut
backend returns success. At that point every required key-down and key-up has
been submitted; cancellation or failure still follows the existing strict
best-effort cleanup path and produces no successful marker.

`run-performance-report.ps1` now uses Ctrl+F13 on all eight fixture wedges and
starts Gradial's isolated normal-integrity input receiver before the campaign.
The run is invalid unless the receiver observes, for every invocation and in
exact order:

1. left Ctrl down;
2. F13 down;
3. F13 up; and
4. left Ctrl up.

The report correlates the hook's activation-release QPC with the completed
submission QPC. The release gate is p95 below 15 ms and p99 below 25 ms. Raw
receiver events are archived beside the ETW evidence rather than being sent to
a real application.

The 20-invocation optimized development validation captured all 80 expected
receiver events. The new metric produced 20 samples: p50 1.526 ms, p95 1.894
ms, p99/max 2.306 ms. Its gate correctly remained `not_evaluated` because the
development preset is not release-qualified.

## Continuous callback boundary

The interaction/coordinator path now carries the originating high-resolution
input timestamp without substituting a later queue timestamp:

- Begin carries the outer-edge crossing that armed the live segment;
- each delivered Update carries the newest scalar publication it represents,
  including coalescing and the configured cadence wait; and
- Commit or Cancel carries the activation release or transition that requested
  that terminal callback.

The plugin worker captures another QPC immediately before entering Lua. It
writes the ETW record only after the callback returns, embedding the earlier
callback-start QPC so event-writing cost is not added to callback dispatch.
Markers retain the invocation and session identity but no scalar, parameters,
target context, or plugin payload.

The plugin reporter evaluates the v0.3 gates only for a complete optimized
Release campaign:

| Metric | Gate |
| --- | ---: |
| Outer edge to Begin callback start | p95 < 20 ms; p99 < 35 ms |
| Latest publication to Update callback start at 30 Hz | p95 <= 38.333 ms |
| Activation release to terminal callback start | p95 < 20 ms; p99 < 35 ms |

Active and deliberately slow fixtures must each supply at least 1,000 samples
for every callback and displayed-frame gate. Overall release evaluation also
requires correlatable PresentMon open/continuous-feedback evidence, passing
scenario contracts, and an evaluable idle-plugin working-set comparison.
Missing evidence yields `not_evaluated`; measured threshold violations yield
`fail`.

The optimized development validation produced the following non-qualifying
timing evidence:

| Fixture | Begin samples / p95 | Update samples / p95 | Terminal samples / p95 |
| --- | ---: | ---: | ---: |
| active | 10 / 1.791 ms | 31 / 26.527 ms | 10 / 2.355 ms |
| slow | 10 / 1.757 ms | 30 / 25.129 ms | 10 / 2.261 ms |

All five fixture scenarios completed. Their latency gates remained
`not_evaluated`, as designed, because the development run had ten rather than
1,000 invocations and no elevated PresentMon capture.

## Verification

- all 420 Rust/Lua tests pass, including a new test that proves distinct Begin,
  Update, and Commit source timestamps reach the backend callback context;
- both performance analyzer self-tests pass, including inclusive/strict gate
  boundaries and repeated same-invocation Update markers;
- the optimized workspace build passes;
- the isolated shortcut and five-state plugin production-route validations
  complete successfully.

The ignored raw development evidence is under:

```text
Gradial APP\captures\performance\20260814-191333-development
Gradial APP\captures\plugin-performance\20260814-191413-development
```

The next performance step is to run the unchanged long Release commands from
an elevated, otherwise idle shell. Optimization is warranted only for a gate
that fails with valid qualifying evidence.
