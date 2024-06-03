 ✓ tests/test.benchmark.ts (10) 16483ms
   ✓ warmup (1) 646ms
     name                   hz     min      max     mean     p75      p99     p995     p999      rme  samples
   · warmup chiitiler  95.2816  7.3406  97.4360  10.4952  8.7575  97.4360  97.4360  97.4360  ±35.66%       48
   ✓ render (3) 5956ms
     name                hz     min     max    mean     p75     p99    p995    p999     rme  samples
   · render as png   7.3053  123.95  174.96  136.89  138.29  174.96  174.96  174.96  ±7.50%       10   slowest
   · render as webp  7.4315  128.56  156.33  134.56  135.08  156.33  156.33  156.33  ±4.45%       10
   · render as jpeg  7.7476  119.67  164.67  129.07  128.02  164.67  164.67  164.67  ±7.17%       10   fastest
   ✓ render with margin (3) 4962ms
     name                            hz      min     max    mean     p75     p99    p995    p999      rme  samples
   · render as png with margin   8.4934  99.6513  212.14  117.74  115.82  212.14  212.14  212.14  ±20.46%       10   slowest
   · render as webp with margin  9.2359   100.81  117.71  108.27  113.70  117.71  117.71  117.71   ±4.09%       10
   · render as jpeg with margin  9.5553  87.0908  145.33  104.65  113.14  145.33  145.33  145.33  ±12.31%       10   fastest
   ✓ render tileSize=2048 (3) 4917ms
     name                 hz      min      max     mean      p75      p99     p995     p999      rme  samples
   · render as png   12.7360  69.7504   107.44  78.5176  77.0831   107.44   107.44   107.44  ±10.14%       10
   · render as webp   5.1332   188.34   202.71   194.81   198.29   202.71   202.71   202.71   ±2.05%       10   slowest
   · render as jpeg  30.5531  28.0281  42.8927  32.7299  33.8336  42.8927  42.8927  42.8927   ±5.82%       16   fastest


 BENCH  Summary

  warmup chiitiler - tests/test.benchmark.ts > warmup

  render as jpeg - tests/test.benchmark.ts > render
    1.04x faster than render as webp
    1.06x faster than render as png

  render as jpeg with margin - tests/test.benchmark.ts > render with margin
    1.03x faster than render as webp with margin
    1.13x faster than render as png with margin

  render as jpeg - tests/test.benchmark.ts > render tileSize=2048
    2.40x faster than render as png
    5.95x faster than render as webp