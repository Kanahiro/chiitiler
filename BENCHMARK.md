v1.13.0 stream mode: disabled

```planetext
 ✓ tests/test.benchmark.ts (10) 15606ms
   ✓ warmup (1) 623ms
     name                  hz     min      max    mean     p75      p99     p995     p999     rme  samples
   · warmup chiitiler  121.48  7.3079  11.4564  8.2318  8.4843  11.4564  11.4564  11.4564  ±2.95%       61
   ✓ render (3) 5785ms
     name                hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · render as png   7.6663  120.66  142.94  130.44  133.33  142.94  142.94  142.94  ±3.70%       10   slowest
   · render as webp  7.8825  120.75  136.90  126.86  128.98  136.90  136.90  136.90  ±3.12%       10
   · render as jpeg  8.3103  116.32  126.72  120.33  122.50  126.72  126.72  126.72  ±1.90%       10   fastest
   ✓ render with margin (3) 4292ms
     name                             hz      min     max     mean      p75     p99    p995    p999     rme  samples
   · render as png with margin   10.1489  90.6246  121.02  98.5326   102.42  121.02  121.02  121.02  ±6.68%       10
   · render as webp with margin   9.9230  91.8121  117.60   100.78   106.96  117.60  117.60  117.60  ±5.89%       10   slowest
   · render as jpeg with margin  11.1188  82.7890  116.44  89.9379  89.0628  116.44  116.44  116.44  ±7.68%       10   fastest
   ✓ render tileSize=2048 (3) 4904ms
     name                         hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · render as png: 2048px   12.3602  70.9358  96.1278  80.9046  88.8528  96.1278  96.1278  96.1278  ±7.47%       10
   · render as webp: 2048px   5.0496   187.04   233.38   198.04   197.61   233.38   233.38   233.38  ±4.74%       10   slowest
   · render as jpeg: 2048px  29.6039  28.5111  46.9135  33.7793  37.3103  46.9135  46.9135  46.9135  ±8.23%       15   fastest


 BENCH  Summary

  warmup chiitiler - tests/test.benchmark.ts > warmup

  render as jpeg - tests/test.benchmark.ts > render
    1.05x faster than render as webp
    1.08x faster than render as png

  render as jpeg with margin - tests/test.benchmark.ts > render with margin
    1.10x faster than render as png with margin
    1.12x faster than render as webp with margin

  render as jpeg: 2048px - tests/test.benchmark.ts > render tileSize=2048
    2.40x faster than render as png: 2048px
    5.86x faster than render as webp: 2048px
```
