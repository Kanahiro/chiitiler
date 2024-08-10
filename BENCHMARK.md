v1.12.2

```planetext
 ✓ tests/test.benchmark.ts (10) 16034ms
   ✓ warmup (1) 729ms
     name                  hz     min      max    mean     p75      p99     p995     p999     rme  samples
   · warmup chiitiler  118.70  7.5679  11.9859  8.4244  8.6950  11.9859  11.9859  11.9859  ±2.49%       60
   ✓ render (3) 5875ms
     name                hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · render as png   7.8713  119.50  134.62  127.04  129.59  134.62  134.62  134.62  ±2.59%       10
   · render as webp  7.3926  125.52  152.92  135.27  137.71  152.92  152.92  152.92  ±5.29%       10   slowest
   · render as jpeg  8.3924  112.76  122.70  119.16  121.61  122.70  122.70  122.70  ±1.69%       10   fastest
   ✓ render with margin (3) 4541ms
     name                             hz      min     max     mean     p75     p99    p995    p999      rme  samples
   · render as png with margin   10.1103  88.4602  115.17  98.9094  108.64  115.17  115.17  115.17   ±7.21%       10   fastest
   · render as webp with margin   9.3138  97.1585  117.95   107.37  113.95  117.95  117.95  117.95   ±4.84%       10   slowest
   · render as jpeg with margin   9.8386  85.4708  146.32   101.64  106.88  146.32  146.32  146.32  ±12.84%       10
   ✓ render tileSize=2048 (3) 4888ms
     name                         hz      min      max     mean      p75      p99     p995     p999     rme  samples
   · render as png: 2048px   13.0710  69.0875  83.4170  76.5054  78.3272  83.4170  83.4170  83.4170  ±3.65%       10
   · render as webp: 2048px   5.1159   186.24   224.75   195.47   193.55   224.75   224.75   224.75  ±4.07%       10   slowest
   · render as jpeg: 2048px  33.7627  26.7925  34.6572  29.6185  29.8150  34.6572  34.6572  34.6572  ±3.51%       17   fastest


 BENCH  Summary

  warmup chiitiler - tests/test.benchmark.ts > warmup

  render as jpeg - tests/test.benchmark.ts > render
    1.07x faster than render as png
    1.14x faster than render as webp

  render as png with margin - tests/test.benchmark.ts > render with margin
    1.03x faster than render as jpeg with margin
    1.09x faster than render as webp with margin

  render as jpeg: 2048px - tests/test.benchmark.ts > render tileSize=2048
    2.58x faster than render as png: 2048px
    6.60x faster than render as webp: 2048px
```
