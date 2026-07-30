# Milestone 3 performance-harness results

## Scope

This bounded Milestone 3 slice adds a repeatable performance-report protocol
and records a short baseline on the current machine. It does not claim release
qualification.

The implementation adds:

- self-timestamped QPC fields and QPC frequency to every Gradial ETW marker;
- an accepted-executor-dispatch marker tied to the activation-release QPC;
- a fixed, allocation-free hook-callback histogram with shutdown p99 and
  maximum reporting;
- process-ID filtering for PresentMon capture;
- warmup, invocation, and idle phases in the existing real-input smoke helper;
- one runner that emits Markdown, versioned JSON, flat CSV, raw ETW/PresentMon
  capture, application log, resource samples, and helper output;
- an analyzer self-test included in the normal local check.

The runner uses an isolated `LOCALAPPDATA`, a completed onboarding marker, and
a harmless 10 ms Delay action on all eight slices. It never reads or changes
the user's Gradial configuration.

## Evidence contract

`run-performance-report.ps1` has two closed presets:

| Preset | Warmup | Invocations | Post-invocation idle | Cold launches |
| --- | ---: | ---: | ---: | ---: |
| Development | 2 seconds | 20 | 8 seconds | 3 |
| Release | 5 minutes | 1,000 | 30 minutes | 100 |

Development is a quick harness check and local comparison point. Its metrics
are always `not_evaluated` against release gates.

Release evaluation additionally requires:

- a detached debugger;
- no foreground-window change during the campaign;
- an unchanged Windows power plan;
- PresentMon display events for displayed-frame gates;
- the complete preset rather than custom shortened values.

Every metric separately records `measured`, `unavailable`, or `not_run`, plus
`pass`, `fail`, or `not_evaluated` for its gate. Missing display evidence is
never replaced with a renderer checkpoint. A release run becomes invalid if
focus or the power plan changes.

## Current-machine development baseline

The short baseline completed on 30 July 2026 with:

- Windows 11 Pro build 26100;
- Intel Core i9-13900KF, 32 logical processors;
- NVIDIA GeForce RTX 4090;
- two 96-DPI, 60 Hz displays: 2560×1440 primary and 1920×1080 secondary;
- the Windows High performance power plan;
- stable foreground identity and power plan.

PresentMon 2.5.1 and Windows Performance Toolkit are installed. The current
PowerShell was not elevated, so PresentMon display evidence was correctly
reported unavailable. Gradial ETW markers were captured.

| Metric | Samples | p50 | p95 | p99 | Maximum | Result |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Button down to application frame checkpoint | 20 | 29.883 ms | 44.298 ms | 44.536 ms | 44.536 ms | measured, diagnostic only |
| Pointer to application highlight checkpoint | 20 | 16.897 ms | 18.155 ms | 18.186 ms | 18.186 ms | measured, diagnostic only |
| Button down to displayed usable frame | — | — | — | — | — | unavailable |
| Pointer to displayed highlight | — | — | — | — | — | unavailable |
| Release to executor dispatch | 20 | 1.058 ms | 1.358 ms | 1.390 ms | 1.390 ms | measured |
| Release to completed shortcut submission | — | — | — | — | — | not run |
| Hook callback duration | 60 | — | — | 19 µs | 19 µs | measured |
| Cold launch to logged Healthy state | 3 | 805.420 ms | 822.166 ms | 822.166 ms | 822.166 ms | measured |
| Warm-idle CPU samples | 19 | 0% | 0.371% | 0.371% | 0.371% | measured; mean 0.068% |
| Warm-idle GPU | — | — | — | — | — | unavailable |
| Working set during short idle | 20 | 185.688 MiB | 185.688 MiB | 185.688 MiB | 185.688 MiB | measured |
| Input queue overflows | 20 invocations | 0 | 0 | 0 | 0 | measured |

The overall result is `not_evaluated`. In particular, the three-sample cold
observation and short-idle working set are above their eventual release
targets. They are visible follow-up measurements, not release failures or
passes, because neither used the release protocol. The CPU mean, executor
dispatch, hook duration, and zero-overflow observations are encouraging but
likewise not release-qualified.

The ignored raw report is under:

```text
Gradial APP\captures\performance\20260730-181638-development
```

## Required coverage still open

No required hardware/display configuration has a release-qualified report
yet:

- one 60 Hz monitor at 100%;
- one monitor at 120 Hz or faster;
- two monitors at 100%/150%;
- two monitors at 125%/200%;
- a negative-coordinate secondary display;
- primary-display changes and plug/unplug;
- sleep/resume, lock/unlock, and Explorer restart;
- center, every edge midpoint, and every corner;
- standard, elevated, closing, and borderless-fullscreen targets.

The completed shortcut-submission gate remains `not_run`; it needs the
isolated shortcut receiver so the report can prove the final `SendInput`
submission without typing into a real application. Elevated PresentMon capture
and GPU-counter availability must also be established before a release run.

## Automated coverage

The complete suite contains 227 tests:

- 175 `gradial-app` tests;
- 52 `gradial-core` tests.

The new hook test proves nearest-rank p99 behavior, including the bounded
overflow bucket. The performance analyzer self-test proves nearest-rank
percentiles and QPC interval conversion. PowerShell syntax validation,
formatting, Clippy with warnings denied, the complete workspace test/build,
and the release build pass.

Run the quick local report from `Gradial APP` with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-performance-report.ps1 -Mode Development -MarkersOnly
```

Run the release protocol from an elevated, otherwise idle PowerShell with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-performance-report.ps1 -Mode Release
```

The release command is intentionally long. It must not be shortened and its
result must not be treated as valid if focus or the active power plan changes.
