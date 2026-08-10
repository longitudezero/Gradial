# Milestone 4 plugin-continuous soak results

Date: 2026-08-10

## Outcome

The second Milestone 4 hardening slice adds a qualifying, repeatable
10,000-session soak for a real Lua continuous plugin. It drives the normal
Windows M4 hook, production overlay, unlocked continuous direction changes,
continuous coordinator, bounded plugin lanes, Lua adapter, and clean shutdown.

The qualifying release run passed every evaluated contract:

| Measurement | Result |
| --- | ---: |
| Observed continuous sessions | 10,000 |
| Begin / Update / Commit / Cancel callbacks | 10,000 / 9,966 / 10,000 / 0 |
| Invalid lifecycle sessions | 0 |
| Updates after a terminal callback | 0 |
| Failed callbacks | 0 |
| Coalesced visual updates | 30,000 |
| Callback-duration p95 | at most 100 microseconds |
| Worker queue-wait p95 | at most 100 microseconds |
| Hook callback p99 | 2 microseconds |
| Working-set p95, sessions 1,000-1,999 | 157.625 MiB |
| Working-set p95, final 1,000 sessions | 160.895 MiB |
| Post-warm-up working-set growth | 3.270 MiB, limit 10 MiB |
| Archived evidence parts | 45 |

Execution, plugin load state, frame checkpoints, input/renderer health, log
privacy, bounded timing storage, update coalescing, clean shutdown, and
post-warm-up memory growth all passed. The ignored raw report is under:

```text
Gradial APP\captures\plugin-continuous-soak\20260810-205130-soak
```

## Soak boundary

`run-plugin-continuous-soak.ps1` is a small release-only entry point over the
versioned plugin performance reporter. A qualifying run must use exactly
10,000 invocations. Shorter runs remain useful diagnostics but report
`not_evaluated` for the overall soak gate.

The interaction helper writes progress every 25 invocations. The parent
process samples Gradial's working set, private memory, and CPU against that
progress. The memory contract compares p95 samples from sessions 1,000-1,999
with the last 1,000 sessions and requires no more than 10 MiB growth.

Gradial's production logger remains bounded to its current file plus four
rotations. The harness copies completed rotations into an ignored evidence
directory, hashes exact file contents, and de-duplicates identical snapshots.
This preserves all lifecycle records for the test without changing production
retention. Analysis requires:

- one Begin and exactly one Commit or Cancel for every observed segment;
- no duplicate terminal, post-terminal Update, or failed callback;
- at least 10,000 segments, allowing valid unlocked-direction handoffs to
  cancel one segment and begin another during a physical gesture;
- one activation-down and activation-up per requested gesture;
- observable update coalescing with a bounded pending-update slot;
- zero input overflow, heartbeat miss, focus rejection, renderer failure, or
  continuous failure; and
- clean action, interaction, hook, and plugin-worker shutdown records plus a
  zero Gradial process exit code.

Plugin callback timing is retained in five fixed 15-bucket histograms: all
callbacks and Begin, Update, Commit, and Cancel separately. Each summary also
records successful and failed results and whether a session remained active at
shutdown. Timing storage therefore cannot grow with session count.

## Renderer retention found by the gate

The first complete run passed every functional contract but failed the memory
gate: working-set p95 grew by 55.734 MiB. Bounded Lua-memory tests and controlled
logger and executor experiments did not reproduce or change the growth.

Allocation-size and call-stack probes localized it to egui's main-thread font
texture atlas and glyph cache. Gradial had multiplied every font size by the
continuously varying 60 ms opening and 50 ms closing animation scale. Small
frame-timing differences therefore produced new rasterized glyph sizes on
successive invocations, growing two font-cache buffers from roughly 10 MiB at
session 1,000 to roughly 36 MiB at session 3,000.

The renderer now rasterizes text at the stable Compact, Standard, or Large
preset scale. Radial positions, ring geometry, icons, strokes, opacity, and the
presence animation continue to use the lifecycle scale. A regression test
requires opening and visible frames to share one text scale. In the instrumented
3,000-session confirmation, live Rust allocation stayed near 3.1 MiB and
working-set p95 grew by only 0.426 MiB before the temporary profiler was
removed.

## Reproduction

From `Gradial APP`, build the release executable and run:

```powershell
cargo build --workspace --all-features --release
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-plugin-continuous-soak.ps1
```

The runner refuses to start while another Gradial process is active. It uses a
disposable `LOCALAPPDATA`, package, trust state, and schema-six configuration;
it does not read or change the user's Gradial data.

This soak qualifies the Section 11.2 10,000-session resource requirement. It
does not replace the separate elevated PresentMon displayed-frame campaign,
full publication-to-callback latency measurement, display/DPI and recovery
matrices, or participant usability study.
