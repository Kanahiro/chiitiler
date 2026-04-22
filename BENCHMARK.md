# Benchmark

`npm run test:benchmark` spawns `tile-server`, primes it with a warmup
request, then runs [autocannon](https://github.com/mcollina/autocannon)
against each scenario and reports throughput + latency percentiles.

## Setup

- Style: [`tests/fixtures/bench-style.json`](./tests/fixtures/bench-style.json)
  — single `file://` vector source, no external network dependencies,
  so numbers reflect render + encode cost only.
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

## Baseline comparison in CI

Each PR run also executes the benchmark against `main`'s source on the
same runner, then diffs the two via
[`tests/compare-benchmarks.ts`](./tests/compare-benchmarks.ts). The
resulting table is posted as a sticky comment on the PR so regressions
are visible at a glance. Running on the same runner back-to-back keeps
machine-level noise from dominating the comparison.

Manual comparison:

```sh
CHIITILER_BENCH_OUTPUT=a.json npm run test:benchmark
# ...make changes...
CHIITILER_BENCH_OUTPUT=b.json npm run test:benchmark
npx tsx tests/compare-benchmarks.ts a.json b.json
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
