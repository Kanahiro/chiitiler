# Migrating from 1.x

2.0 renders with [ml2drender](https://github.com/Kanahiro/ml2drender) instead of MapLibre Native: no GPU, Xvfb or sharp, faster,
smaller and lighter (below). Images are close to MapLibre's but not pixel-identical (antialiasing and label
placement differ).

| 1.25.3 → 2.0.2 | 1.25.3 | 2.0.2 | |
|---|---:|---:|---:|
| png tiles, 1 connection (req/s) | 23.9 | 43.3 | +81% |
| png tiles, 10 connections (req/s) | 56.0 | 163.9 | +194% |
| webp tiles (req/s) | 25.9 | 47.2 | +83% |
| jpg tiles (req/s) | 31.7 | 42.5 | +36% |
| 1024×1024 png view (req/s) | 6.9 | 7.8 | +13% |
| Memory, idle (PSS) | 180 MB | 61 MB | −66% |
| Memory, peak under load (PSS) | 984 MB | 589 MB | −40% |
| Docker image, compressed (amd64) | 176 MB | 66 MB | −62% |

Measured on 4 vCPUs, one process, no source cache, with the `tests/benchmark.ts` fixture (Florence,
z12–z15); 1.x on Xvfb with Mesa's software GL, as in its Docker image. Throughput is the median of 5
rounds of 10 s; memory is one run, the peak across 20 s each of png at 1 and 10 connections and the
view at 1 and 4 connections (with Xvfb for 1.x). Memory grows with load and is kept after it.

- **Not rendered:** 3D (`fill-extrusion` is drawn as a flat fill), terrain, globe, sky, bidirectional
  and Arabic text shaping, and glyphs the style's `glyphs` URL doesn't serve (no local font fallback).
  `/camera` is renamed `/view` and has no pitch segment:
  `/view/{zoom}/{lat}/{lon}/{bearing}/{width}x{height}.{ext}`.
- **Formats:** WebP is lossy at `quality` below 100 (alpha stays lossless); JPEG uses 4:2:0 chroma and
  flattens transparency over black. `.jpg` responses are `image/jpeg` (were `image/jpg`).
- **`margin`** is per side (pixels rendered on each side of the tile), as in other renderers: 1.x's
  was the total, so `margin=100` of 1.x is `margin=50`.
- **Errors:** parameters the renderer rejects (e.g. a `tileSize` that isn't a positive integer) answer
  `400` instead of `500`.
- **Server:** the `tile-server` command and its options are removed: start chiitiler without
  arguments and set the `CHIITILER_*` environment variable each flag had (see
  [Configuration](./README.md#configuration)); any argument now stops startup with an error.
  Unknown `CHIITILER_CACHE_METHOD` values and other invalid values stop startup too (1.x fell back to
  defaults). `CHIITILER_PREWARM` is removed (startup is fast without it). Images are at most 4096 px on a side unless `CHIITILER_MAX_SIZE` says otherwise.
- **Sources:** `cog://` is removed: serve COGs as raster tiles from another server (e.g. TiTiler).
- **Caches:** `file`, `s3` and `gcs` caches use new file and object names (SHA-256 of the URL; the
  old escaped names could collide) and the file cache stores raw bytes: entries written by 1.x are
  not read, and can be deleted. The `memory` method and `CHIITILER_MEMORYCACHE_MAXITEMCOUNT` are
  removed: the front cache now also sits in front of `none`, so use `none` and raise
  `CHIITILER_FRONT_CACHE_TTL_SEC` (default `10`) / `CHIITILER_FRONT_CACHE_MAX_BYTES` instead.
- **Docker:** the image has no entrypoint script; the bundle is `build/main.mjs`. The image is
  distroless (no shell): its entrypoint is `node`, so a `command` override passes only the script and
  its arguments. The AWS Lambda Web Adapter and the `cdk/` app are removed.
- **Library:**
  - `getRenderedTileBuffer`, `getRenderedClipBuffer` and `getRenderedCameraBuffer` are replaced by
    the `renderTile`, `renderBbox` and `renderView` methods of a `Chiitiler` (`renderView` takes no
    `pitch`; its options type is `RenderViewOptions`, and so on), which resolve to a `RenderedImage`
    (RGBA pixels, encoded with `toPng()`, `toJpeg(quality)`, `toWebp(quality)` or `toAvif(quality)`)
    and take no `ext` or `quality`.
    `cache`, `protocols` and `maxSize` are given once to `new Chiitiler(...)`, the style to every
    call. The `*Stream` variants (Sharp)
    and `prewarm()` are removed.
  - Only the package root is importable (`exports`): `chiitiler/dist/...` paths are not.
  - Cache values are `Uint8Array` and `name` is optional, so any `{ get, set }` object is a cache.
  - `memoryCache` is removed (use `withFrontCache(noneCache(), …)`), `withMemoryCache` is renamed
    `withFrontCache`, and `Cache.memory` is renamed `Cache.front`.
  - New `protocols` option for custom URL schemes.
  - `@maplibre/maplibre-gl-style-spec` is a peer dependency, and `@aws-sdk/client-s3` and
    `@google-cloud/storage` optional ones: install them for `s3://` / `gs://` sources and caches.
  - The renderer comes from `@ml2drender/node`, which has prebuilt addons for linux-x64 and
    linux-arm64 (glibc), darwin-x64, darwin-arm64 and win32-x64.
