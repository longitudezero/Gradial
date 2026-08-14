# Milestone 4 plugin-performance baseline results

Date: 2026-08-10

## Outcome

The first bounded Milestone 4 slice adds one repeatable plugin-performance
campaign covering the five required runtime states in both the development and
optimized executables:

- **disabled**: a discovered but disabled package whose top-level Lua would
  fail if executed;
- **idle-enabled**: a healthy loaded plugin with no continuous wedge assigned;
- **active**: the healthy fixture driven through the production Begin, Update,
  and Commit route;
- **slow**: the same route with deliberate, bounded Lua work in every
  callback; and
- **faulted**: an already-faulted package whose top-level Lua would fail if it
  were loaded again.

Every scenario uses a disposable `LOCALAPPDATA`, a schema-six configuration,
real synthetic M4 input, the production overlay and hook, and the normal
plugin trust file. The fixtures never read or change the user's Gradial data.

`run-plugin-performance-report.ps1` emits versioned JSON, flat CSV, Markdown,
per-scenario resource samples, application logs, ETW marker tables, and
PresentMon output when the shell is elevated. Its deterministic analyzer and
fixture-fingerprint self-tests run as part of `scripts/check.ps1`.

## Callback observability

The plugin worker now timestamps each accepted lifecycle request at bounded
lane submission and records:

- queue wait to callback dispatch;
- Lua callback duration;
- total submission-to-result duration;
- stage and stable result/error code; and
- fixed-size per-stage p50, p95, p99, maximum, and count aggregates at
  shutdown.

The aggregate uses closed microsecond buckets and cannot grow with session
count. Plugin and capability informational events are now retained by the
existing rotating file logger. They contain IDs, stages, result codes, and
timing only—not source, parameters, target context, window data, capability
payloads, or credentials.

Queue wait starts when the coordinator submits to the worker lane. It is a
useful worker-isolation measurement, but it does not include the configured
update-period wait and therefore does not by itself qualify the full
latest-publication-to-Update-start release gate.

As of 14 August 2026, separate ETW callback-start markers close that gap. They
retain the originating outer-edge, latest-publication, or activation-release
QPC through the coordinator and capture the endpoint immediately before Lua.
The original queue and callback histograms remain useful diagnostics.

## Current-machine development baseline

The short campaign completed on the current machine with ten invocations and
three seconds of idle sampling per build/scenario. It is a harness and local
comparison point, not release qualification. Callback and queue percentiles
are conservative upper bounds from the worker's fixed histogram.

| Build | Scenario | Callbacks | Callback p95 | Queue p95 | Idle CPU mean | Idle working-set max | Open checkpoint p95 | Visual checkpoint p95 | Hook p99 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| debug | disabled | 0 | — | — | 0.100% | 169.742 MiB | 28.781 ms | 20.001 ms | 31 µs |
| debug | idle-enabled | 0 | — | — | 0.066% | 170.641 MiB | 19.396 ms | 15.067 ms | 23 µs |
| debug | active | 52 | ≤100 µs | ≤250 µs | 0% | 170.914 MiB | 19.791 ms | 16.546 ms | 23 µs |
| debug | slow | 50 | ≤1,000 µs | ≤100 µs | 0% | 171.086 MiB | 21.084 ms | 23.832 ms | 21 µs |
| debug | faulted | 0 | — | — | 0.083% | 169.789 MiB | 19.182 ms | 18.362 ms | 24 µs |
| release | disabled | 0 | — | — | 0.034% | 155.488 MiB | 19.851 ms | 14.571 ms | 27 µs |
| release | idle-enabled | 0 | — | — | 0.067% | 155.840 MiB | 34.821 ms | 27.707 ms | 26 µs |
| release | active | 51 | ≤73 µs | ≤100 µs | 0.051% | 156.520 MiB | 35.757 ms | 26.952 ms | 16 µs |
| release | slow | 50 | ≤1,000 µs | ≤100 µs | 0.067% | 156.586 MiB | 35.278 ms | 29.273 ms | 26 µs |
| release | faulted | 0 | — | — | 0.034% | 155.695 MiB | 36.033 ms | 26.385 ms | 18 µs |

The enabled-idle working-set delta against the disabled case was 0.898 MiB in
debug and 0.352 MiB in release. The slow fixture was measurably slower than
the ordinary active fixture in both builds while remaining far inside the Lua
phase ceilings.

All ten scenario contracts passed:

- disabled and faulted packages had zero startup loads and zero callbacks;
- idle-enabled loaded exactly one plugin and made zero callbacks;
- active and slow loaded exactly one plugin and delivered at least one Begin
  and one Commit for every invocation;
- every invocation produced the expected application open and visual frame
  checkpoints;
- every case produced zero hook-heartbeat misses, input-queue overflows, focus
  failures, and renderer failures; and
- the private fixture source sentinels were absent from every retained log.

PresentMon 2.5.1 and Windows Performance Toolkit were detected, but the shell
was not elevated. Displayed-frame results therefore remain unavailable rather
than being replaced with application checkpoints. The ignored raw report is
under:

```text
Gradial APP\captures\plugin-performance\20260810-185531-development
```

## Protocol and remaining qualification

Run the short two-build campaign from `Gradial APP` with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-plugin-performance-report.ps1 -Mode Development -Build Both -MarkersOnly
```

The release protocol fixes each selected build/scenario at a five-minute
warmup, 1,000 real invocations, and a 30-minute idle phase. Run it from an
elevated, otherwise idle PowerShell so PresentMon can supply displayed-frame
evidence:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-plugin-performance-report.ps1 -Mode Release -Build release
```

The complete release run remains open. In particular, this development
baseline does not qualify the 30-minute memory/CPU limits, the 1,000-session
latency distributions, displayed-frame gates, or the later 10,000-session
soak. It establishes a repeatable measurement boundary before any
evidence-driven optimization.

The reporter now evaluates the full callback-start and displayed-frame gates.
An optimized Release campaign produces overall `pass` or `fail` only when the
active and slow fixtures each provide at least 1,000 samples for every required
latency gate and the idle-plugin working-set comparison is evaluable. Missing
or non-elevated evidence still produces `not_evaluated`. The short optimized
validation and exact gates are recorded in
[`milestone-4-release-timing-instrumentation-results.md`](milestone-4-release-timing-instrumentation-results.md).
