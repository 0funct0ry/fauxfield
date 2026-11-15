---
title: Performance
description: The performance budgets, how each is measured, and the latest results.
---

Fauxfield keeps eight budgets. Each has a bench or script, and a budget that is missed fails the
run (release builds only; debug builds are far slower and only report).

| Budget | Target | Measured by |
|---|---|---|
| Cold start to interactive (GUI, 10 Servers, 500 Routes) | < 400 ms | `FAUXFIELD_TRACE=startup` trace and `ui.state` `perf.startup`; headless part asserted < 150 ms by `startup` |
| Frame time scrolling Traffic with 100k rows | < 8 ms | `data_table` bench (model side) and `ui.audit` UX150 (paint) in `m17-04-perf` |
| Engine overhead, static response, no delay | p99 < 0.5 ms | `engine_overhead` bench |
| Throughput, static JSON, 1 Server, loopback | > 30k req/s on 8 cores | `engine_overhead` bench (gated on the dedicated runner) |
| Template render, 1 KB output, 10 `fake()` calls | < 100 µs | `render` bench |
| Hot reload after a file save | < 50 ms | `startup` bench |
| Idle memory, GUI, 10 Servers | < 150 MB | `scripts/idle_rss.sh` (`make idle-rss`); the physical footprint on macOS, VmRSS on Linux |
| Headless binary size (stripped) | < 15 MB | `scripts/size_check.sh` (`make size-check`) |

## Running

```bash
make bench                              # the whole suite, release
make bench-crate CRATE=fauxfield-engine # one package (ARGS=<filter>)
make bench-gui                          # Traffic scroll bench and idle RSS (needs a display)
```

`cargo xtask perf-workspace [DIR]` generates the 10-Server, 500-Route workspace
(`fixtures/perf-workspace`).

## Startup trace

`FAUXFIELD_TRACE=startup fauxfield run <workspace>` (or the app) writes
`target/trace/startup.json` in Chrome trace format; open it in Perfetto. `FAUXFIELD_TRACE_FILE`
sets another path. With the variable unset, tracing costs one atomic load.

## CI gate

The `bench` job runs the suite on every pull request; the hard budgets above fail the job on any miss.
On the dedicated runner (label `perf`) the criterion results are also compared with the baseline saved
from `main` and the job fails on a regression above 10 %; shared runners only report. The throughput
budget is asserted only on the dedicated runner (`FAUXFIELD_BENCH_DEDICATED=1`).

## Results

Machine: Apple M3 Pro, 11 cores, 18 GB, macOS, rustc 1.89, release profile.

| Budget | Before | After | Target |
|---|---|---|---|
| Headless startup (open, apply, bind 10 Servers) | n/a (new) | 13 ms | < 150 ms |
| Engine overhead p99 | n/a (new) | 52 µs | < 500 µs |
| Throughput | n/a (new) | about 100k req/s | > 30k req/s |
| Template render, 1 KB / 10 `fake()` | 16 µs | 25 µs | < 100 µs |
| Hot reload | 86 ms | 43 to 48 ms | < 50 ms |
| Traffic scroll, 100k rows, model side per frame | 10 µs | 18 µs | < 8 ms |
| Headless binary size | 20.8 MB | 13.3 MB | < 15 MB |
| Idle memory, GUI, 10 Servers | n/a (new) | 99 MB physical footprint (150 MB RSS) | < 150 MB |
| Cold start, GUI, shell and engine (trace) | n/a (new) | about 45 ms (`shell.new` 34 ms, `host.build` 11 ms), plus window creation | < 400 ms |

What changed: the file watcher waited a fixed 40 ms, checked only on 25 ms ticks, and so added up to
65 ms to every reload; it now wakes exactly when the burst has settled (5 ms). The release profile
uses fat LTO, optimises Fauxfield's own crates for size and third-party crates for minimum size
(`gpui` and `fauxfield-gui` stay at full speed), which is what brought the binary under 15 MB; the
template and scroll numbers rose slightly and remain far inside their budgets.
