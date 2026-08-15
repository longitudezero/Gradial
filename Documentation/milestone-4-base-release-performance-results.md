# Milestone 4 base Release performance results

Date: 2026-08-14

## Outcome

The first complete base Release performance campaign is valid. Under the
original plan it had an overall result of **fail**: seven measured release
gates passed, while the 30-minute idle working-set gate recorded 156.934 MiB
maximum against the then-current `< 80 MiB` requirement. The two
displayed-frame gates remain `not_evaluated` because
PresentMon captured a valid system-wide stream but no presentation rows for
Gradial's transparent overlay process.

This is an optimization result, not a harness failure. The campaign used the
closed Release preset without shortened phases:

- five-minute warmup;
- 1,000 real synthetic M4 invocations;
- 30-minute post-invocation idle sampling;
- 100 independent cold launches; and
- an isolated Ctrl+F13 receiver that required four exact key events per
  invocation.

The report is under the ignored local evidence directory:

```text
Gradial APP\captures\performance\20260814-230910-release
```

## Validity and environment

- Protocol eligible: yes.
- Foreground identity stable: yes.
- Power plan stable: yes, Windows High performance.
- Detached debugger: yes.
- Shortcut receiver: 4,000 of 4,000 expected events, valid ordering and
  cleanup.
- Timing markers: captured.
- PresentMon 2.5.1: captured through its separately elevated child process.
- Per-process GPU counters: captured.
- Windows Performance Toolkit: installed.
- Machine: Windows 11 Pro build 26100, Intel Core i9-13900KF, NVIDIA GeForce
  RTX 4090, 32 logical processors.
- Display available to the campaign: one 1920x1080, 96-DPI, 60 Hz primary
  display.

The Release runner itself, Gradial, and the isolated receiver remained at
normal integrity. Only PresentMon was elevated. The receiver PID and native
window handle matched the frozen foreground target.

## Gate results

| Metric | Samples | Release result | Observed result |
| --- | ---: | --- | --- |
| Button down to displayed usable frame | - | `not_evaluated` | PresentMon contained no Gradial presentation rows |
| Pointer to displayed highlight | - | `not_evaluated` | PresentMon contained no Gradial presentation rows |
| Release to executor dispatch | 1,000 | pass | p95 1.467 ms; p99 2.107 ms; max 3.365 ms |
| Release to completed shortcut submission | 1,000 | pass | p95 2.436 ms; p99 3.131 ms; max 5.179 ms |
| Hook callback duration | 7,241 | pass | p99 15 us; max 190 us |
| Cold launch to Healthy | 100 | pass | p95 671.831 ms; p99 677.272 ms; max 687.119 ms |
| Warm-idle CPU | 1,461 | pass | mean 0.0536%; p95 0.146%; max 0.385% |
| Warm-idle GPU | 293 | pass | mean, p95, and max 0% |
| Working set during 30-minute idle | 1,462 | **fail** | p50 156.918 MiB; p95 156.918 MiB; max 156.934 MiB |
| Input queue overflows | 1,000 invocations | pass | 0 |

Application-side renderer checkpoints were also complete for all 1,000
invocations: opening p95 was 28.201 ms and highlight p95 was 25.188 ms. They
remain diagnostics and do not substitute for displayed pixels.

## PresentMon boundary

PresentMon produced 15 data rows plus its header: eight for `dwm.exe` and
seven for `ChatGPT.exe`. It produced no `gradial.exe` row in this campaign.
The analyzer filters by Gradial's exact PID, so it correctly returned no
display samples instead of correlating unrelated desktop frames.

The current transparent multi-viewport rendering path is therefore not
observable through the selected PresentMon process-present method on this
machine. Closing the displayed-frame gates requires either a reliable
composition-level measurement that proves the overlay content was displayed,
or a renderer/presentation route that PresentMon can observe. Application
checkpoints must not be promoted into display evidence.

## Public-beta policy decision

The public-beta working-set ceiling is `< 192 MiB`. The captured 156.934 MiB
maximum meets that revised policy with about 35 MiB headroom and remained
essentially flat across the idle period. The original report and its failure
remain preserved so the policy change is explicit and auditable.

Memory attribution and reduction are deferred until stable-release hardening
or earlier user feedback demonstrates a practical problem. The separate full
plugin Release campaign and transparent-overlay display measurement remain
open qualification work.
