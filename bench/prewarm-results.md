# Prewarm benchmark

20 fresh processes per mode; alternating pair order. Node v24.18.0, darwin arm64, Apple M5 Pro.
Local vector fixture, PNG 512×512, one process, cache=none. Readiness uses /health only; no tile requests before measurement. HTTP timings include full body. Startup includes Node/tsx loading and readiness polling (10 ms interval).

| Median (ms) | off | on |
|---|---:|---:|
| startupMs | 280.0 | 315.3 |
| firstMs | 44.7 | 11.2 |
| secondMs | 5.4 | 4.6 |
| startupThroughFirstMs | 326.8 | 325.8 |

First-request median reduction: 75.0%.
Paired improvement median: 33.7 ms; faster with prewarm in 20/20 pairs.
Prewarm duration median: 35.0 ms. All 80 tile responses are HTTP 200, 512×512 PNG and byte-identical (12094 bytes).

These are process-cold measurements with shared OS caches, not machine-cold or AWS Lambda measurements. The fixture has no glyphs/sprites or remote downloads.
