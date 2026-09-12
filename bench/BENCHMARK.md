# Benchmark

`npm run test:benchmark` spawns `tile-server`, validates an image and warms up each scenario at its measured concurrency, then runs [autocannon](https://github.com/mcollina/autocannon)
against each scenario and reports throughput + latency percentiles.

## Setup

- Style: [`tests/fixtures/bench-style.json`](../tests/fixtures/bench-style.json)
  — single `file://` vector source, no external network dependencies,
  so results measure warm HTTP serving, local source access, rendering and encoding without remote I/O.
- Target tile: `/tiles/5/28/12.png`.
- Server: single process (`CHIITILER_PROCESSES=1`, `CHIITILER_CACHE_METHOD=none`).
- Each scenario runs for `CHIITILER_BENCH_DURATION` seconds (default
  `10`).

## Scenarios

| Scenario | What it stresses |
|---|---|
| `png c=1` | baseline render + PNG encode |
| `png c=10` | render under concurrency (renderer-pool saturation) |
| `webp c=1` | WebP encode path |
| `jpeg c=1` | JPEG encode path |
| `png tileSize=1024 c=1` | larger output buffer |

## Running

```sh
npm run test:benchmark
```

Knobs via env:

- `CHIITILER_BENCH_DURATION` — seconds per scenario (default `10`).
- `CHIITILER_BENCH_WARMUP` — warmup seconds per scenario (default `3`).
- `CHIITILER_BENCH_TARGET` — checkout containing server code and its installed dependencies (default current directory). Fixtures always come from the harness working directory.
- `CHIITILER_BENCH_PORT` — port to spawn the server on (default `3030`).
- `CHIITILER_BENCH_OUTPUT` — if set, writes the JSON result array to
  the given path.
- `CHIITILER_BENCH_MARKDOWN` — if set, writes the markdown table
  (without baseline comparison) to the given path.
- `GITHUB_STEP_SUMMARY` — when set (as in CI), the markdown table is
  also appended to the step summary.

On macOS you can run it directly. On headless Linux (including the CI
runner), wrap with `xvfb-run -a` because `@maplibre/maplibre-gl-native`
needs a display.

## First-request prewarm comparison

Run manually from the repository root (not included in npm scripts or CI):

```sh
CHIITILER_BENCH_RUNS=20 \
CHIITILER_BENCH_OUTPUT=bench/prewarm-results.json \
CHIITILER_BENCH_MARKDOWN=bench/prewarm-results.md \
node --import tsx bench/benchmark-prewarm.ts
```

Recorded results: [summary](./prewarm-results.md) and [raw samples](./prewarm-results.json).

This separate benchmark starts a fresh Node process for every sample and
alternates the order of prewarm off/on within each pair. It polls only
`/health` before measuring the first tile response, including its entire
body. It also records a second request, startup time, and time from process
launch through the first response, so work moved into startup is visible.
The fixture, single-process setting, and disabled cache match the throughput
benchmark. Every response must be a valid 512×512 PNG with identical bytes
across both modes; failed prewarming aborts the run.

`CHIITILER_BENCH_RUNS` defaults to 20 samples per mode. These are process-cold
measurements: OS caches remain shared. macOS results do not establish the
effect of Linux GL initialization or AWS Lambda INIT CPU allocation.

## Baseline comparison in CI

CI compares the PR event's fixed base SHA with the fixed PR merge SHA (`github.sha`).
Both commits are extracted into separate directories and installed with `npm ci`.
The current checkout supplies the shared measurement harness and fixtures; target
server code and dependencies come from each extracted commit. Thus dependency
changes are included, while fixture changes are applied equally to both sides.
Each server runs with the harness working directory so relative fixture paths match.

PR runs use two pairs on one runner, alternating base/current and current/base
order. Each measurement starts a fresh server and runs all five scenarios in a
fixed order, with 3 seconds of warmup and 5 seconds of measurement per scenario.
Nominal load time is 160 seconds, plus setup/startup (roughly 3–4 minutes total,
depending on the runner). A new push cancels the previous benchmark for that PR.

Manual workflow runs retain five pairs and 10 seconds of measurement with the
same 3-second warmup: 650 seconds of load, plus setup/startup. Use these longer
runs to investigate small differences; the short PR run is a coarse performance
check with fewer samples. Renderer and style caches remain warm within a run
even with source cache disabled.

The PR comment shows median throughput/latencies, the median **paired** throughput
percentage change, its observed min/max and the count of faster pairs. The range
is not a confidence interval. The report includes the minimum response count per
run on each side. p99 is diagnostic, not a regression gate, and is omitted if any
run in the scenario has fewer than 100 responses. Low counts do not invalidate
otherwise successful throughput measurements. Raw samples,
commit SHAs, CPU and Node information are uploaded in `benchmark-results/`.

Any failed run, HTTP/transport error, timeout, zero responses per
scenario, invalid metric, duplicate or mismatched scenario, or incomplete pair
makes the comparison unavailable and fails the job. Before load, each scenario
must return an image with the expected format and dimensions. This is not a
pixel-level correctness test; rendering correctness belongs in integration tests.

Use **Run workflow → aa=true** (default) to compare the selected commit with itself.
Repeat A/A runs to establish the noise floor before interpreting small PR deltas.
With `aa=false`, manual runs compare the selected commit with the main SHA resolved
at setup. Shared OS caches and runner load remain sources of noise; these results
are not startup, remote I/O or representative navigation benchmarks.

Local paired comparison (install both checkouts' dependencies first):

```sh
CHIITILER_BENCH_BASELINE=/absolute/path/to/base \
CHIITILER_BENCH_CURRENT=/absolute/path/to/current \
node --import tsx tests/run-benchmark-comparison.ts
```

Use the same checkout path on both sides for A/A. On Linux, prefix with
`xvfb-run -a`. `CHIITILER_BENCH_ROUNDS` defaults to 5 (minimum 2);
`CHIITILER_BENCH_DURATION` and `CHIITILER_BENCH_WARMUP` apply to every run.
Optional `CHIITILER_BENCH_BASE_SHA` and `CHIITILER_BENCH_CURRENT_SHA` label local
results (otherwise `local`). Regenerate a report with:

```sh
node --import tsx tests/compare-benchmarks.ts benchmark-results/results.json benchmark-results/benchmark.md
```

## Sample output

M1 MacBook Air, `CHIITILER_BENCH_DURATION=5`:

| Scenario | Conns | Req/s (mean) | p50 (ms) | p90 (ms) | p99 (ms) | Errors | non-2xx |
|---|---:|---:|---:|---:|---:|---:|---:|
| png c=1 | 1 | 190.0 | 4.0 | 5.0 | 9.0 | 0 | 0 |
| png c=10 | 10 | 419.0 | 24.0 | 33.0 | 47.0 | 0 | 0 |
| webp c=1 | 1 | 112.0 | 8.0 | 9.0 | 13.0 | 0 | 0 |
| jpeg c=1 | 1 | 190.6 | 5.0 | 6.0 | 10.0 | 0 | 0 |
| png tileSize=1024 c=1 | 1 | 178.0 | 5.0 | 6.0 | 11.0 | 0 | 0 |
