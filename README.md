<div align="center">

<img src="./logo.svg" alt="chiitiler" width="200" />

# chiitiler

**A tiny raster tile server for MapLibre styles.**

Point it at any `style.json` and get back PNG / WebP / JPEG tiles, static images, or map cut-outs.

[![Release](https://img.shields.io/github/v/release/Kanahiro/chiitiler?label=ghcr.io%2Fkanahiro%2Fchiitiler&color=0ea5e9)](https://github.com/Kanahiro/chiitiler/pkgs/container/chiitiler)
[![Unit Tests](https://img.shields.io/github/actions/workflow/status/Kanahiro/chiitiler/test:unit.yml?label=unit)](https://github.com/Kanahiro/chiitiler/actions)
[![Integration](https://img.shields.io/github/actions/workflow/status/Kanahiro/chiitiler/test:integration.yml?label=integration)](https://github.com/Kanahiro/chiitiler/actions)
[![codecov](https://codecov.io/gh/Kanahiro/chiitiler/graph/badge.svg?token=9RVLAJG126)](https://codecov.io/gh/Kanahiro/chiitiler)
[![License](https://img.shields.io/github/license/Kanahiro/chiitiler?color=blue)](./LICENSE)

[Live Demo](https://spatialty-io.github.io/chiitiler-demo/) · [Quickstart](#quickstart) · [HTTP API](#http-api) · [Library](#library-usage) · [Deployment](#deployment)

</div>

---

## From zero to a rendered tile in 30 seconds

**1. Run the server** — one command, no config file, no database, no API key.

```bash
docker run --rm -p 3000:3000 ghcr.io/kanahiro/chiitiler:latest
```

**2. Open a tile in your browser** — pass any MapLibre style URL as `?url=`.

```
http://localhost:3000/tiles/0/0/0.png?url=https://tile.openstreetmap.jp/styles/osm-bright/style.json
```

You're done. That same endpoint works as an XYZ tile source for Leaflet, MapLibre, OpenLayers, QGIS, Cesium, or anywhere else that speaks `{z}/{x}/{y}`.

> **No Docker?** Run it from source with [pnpm](https://pnpm.io) and a Rust toolchain:
> ```bash
> git clone https://github.com/Kanahiro/chiitiler && cd chiitiler && pnpm install
> pnpm build:native
> CHIITILER_DEBUG=true pnpm tsx src/main.ts
> ```
> Then visit `http://localhost:3000/debug` to preview styles interactively.

## Features

- **Zero-config** — no config file, no YAML, no database. Just a style URL.
- **No GPU, no system libraries** — rendered by [ml2drender](./crates), a pure-Rust 2D MapLibre renderer, as a Node-API addon
- **Works with any MapLibre style** — remote URL or `POST` the JSON inline
- **Multiple outputs** — slippy tiles (`/tiles`), bounding-box clips (`/clip`), free-form camera shots (`/camera`)
- **Serverless-friendly** — small footprint, runs on AWS Lambda via Web Adapter (see [`cdk/`](./cdk))
- **Pluggable caching** — `memory` · `file` · `s3` · `gcs` backends for shared source assets
- **Many protocols** — `http(s)` · `s3` · `gs` · `file` · `mbtiles` · `pmtiles` · `cog`
- **Library or server** — import the renderer directly into your Node.js pipeline
- **Built-in debug UI** — `/debug` and `/editor` for live style preview

## In Production

- **[MIERUNE/tiles](https://github.com/MIERUNE/tiles)** — [live example](https://mierune.github.io/tiles/color.html#11.62/43.064/141.3375)
- **[PLATEAU VIEW](https://plateauview.mlit.go.jp/)** — Cesium.js imagery via `/tiles`
- **[qgis-amazonlocationservice-plugin](https://github.com/dayjournal/qgis-amazonlocationservice-plugin)** — QGIS integration
- **[Allmaps Latest](https://bsky.app/profile/latest.allmaps.org)** — Bluesky bot
- **[Kumoy](https://www.kumoy.io/)** - used for thumbnail image of designed maps

## Presentations

- [FOSS4G Hiroshima 2026 — Presentation slides](https://drive.google.com/file/d/122JW5gJbrxnUVCVsSlcQMb22StPTnZQV/view?usp=sharing)

## HTTP API

| Method | Endpoint | Description |
| --- | --- | --- |
| `GET` / `POST` | `/tiles/{z}/{x}/{y}.{ext}` | Slippy-map raster tile |
| `GET` / `POST` | `/clip.{ext}` | Bounding-box cut-out |
| `GET` / `POST` | `/camera/{zoom}/{lat}/{lon}/{bearing}/{pitch}/{width}x{height}.{ext}` | Free-form camera shot |
| `GET` | `/debug`, `/editor` | Debug UI (requires `CHIITILER_DEBUG=true`) |

`ext` is one of `png`, `jpeg`, `jpg`, `webp`.

**Query parameters**

| Name | Default | Notes |
| --- | --- | --- |
| `url` | — | Style JSON URL (required for `GET`) |
| `tileSize` | `512` | Tile size in pixels |
| `quality` | `100` | JPEG quality (WebP is always lossless) |
| `margin` | `0` | Extra pixels rendered around the tile (half on each side) and cropped, e.g. to keep labels across tile edges |
| `bbox` | — | `/clip` bounding box: `minLon,minLat,maxLon,maxLat` |
| `size` | `1024` | `/clip` longest edge in pixels |

For `POST`, send the style object as JSON body: `{ "style": { ... } }`.

**Rendering limits** (ml2drender renders 2D only)

- `pitch` must be `0` on `/camera`; other values return `400`.
- `fill-extrusion` layers are drawn as flat fills. 3D, terrain, globe, sky and light are not rendered.
- Bidirectional text and Arabic shaping are not supported, and glyphs come only from the style's `glyphs` (no local font fallback).
- JPEG has no alpha: transparent areas become black. Chroma is subsampled 4:2:0 at every quality (as sharp did by default).

## Library Usage

Chiitiler is also published to npm. Renderers return the encoded image as a `Uint8Array`.

```ts
import { getRenderedTileBuffer, ChiitilerCache } from 'chiitiler';

const cache = ChiitilerCache.fileCache({ dir: './.cache', ttl: 3600 });

const png = await getRenderedTileBuffer({
    stylejson: 'https://tile.openstreetmap.jp/styles/osm-bright/style.json',
    z: 5, x: 27, y: 12,
    tileSize: 512,
    ext: 'png',
    quality: 100,
    margin: 0,
    cache,
});
```

Available renderers: `getRenderedTileBuffer`, `getRenderedClipBuffer`, `getRenderedCameraBuffer`. (The `*Stream` variants returning `Sharp` instances were removed with sharp.)

A renderer is kept per style, cache and protocols (compared by identity): create the cache and the
protocols object once and pass the same ones to every call, or each call starts a new renderer.

### Your own cache

Any object with `get` and `set` is a cache. Keys are resource URLs, values `Uint8Array`s; return
`undefined` from `get` for a miss. It holds `http(s)://` sources (including `pmtiles://http(s)://`
tiles), e.g. in Redis to share them between processes or hosts:

```ts
import { getRenderedTileBuffer, type Cache } from 'chiitiler';

const cache: Cache = {
    get: async (url) => {
        const hit = await redis.getBuffer(url);
        return hit === null ? undefined : new Uint8Array(hit);
    },
    set: async (url, bytes) => {
        await redis.set(url, Buffer.from(bytes), 'EX', 3600);
    },
};
```

`ChiitilerCache.withMemoryCache(cache)` puts the short-lived in-process layer described under
[Cache](#cache) in front of it.

### Custom protocols

`protocols` maps a scheme to a loader of its URLs, for the style URL and every source, sprite and
glyph URL in it. Custom protocols take precedence over the built-in ones, so `http` / `https` can be
replaced too (e.g. to add credentials). Return `null` when there is nothing at the URL, like HTTP 404:
a missing vector tile renders empty, a missing raster tile is replaced by its parent. Throw for other
failures: the render fails rather than returning a map with holes. Results go through the cache's
`memory` layer, not the cache itself (cache inside the loader if needed).

```ts
import { getRenderedTileBuffer, type Protocols } from 'chiitiler';

const protocols: Protocols = {
    db: async (url) => {
        // db://tiles/{z}/{x}/{y}
        const [z, x, y] = url.split('/').slice(-3).map(Number);
        return (await db.tile(z, x, y)) ?? null; // Uint8Array | null
    },
};

const png = await getRenderedTileBuffer({
    stylejson: { version: 8, sources: { t: { type: 'vector', tiles: ['db://tiles/{z}/{x}/{y}'] } }, layers: [/* … */] },
    z: 5, x: 27, y: 12, tileSize: 512, ext: 'png', quality: 100, margin: 0,
    cache, protocols,
});
```

Types are re-exported as well, so you can annotate call sites without depending on
`@maplibre/maplibre-gl-style-spec` resolution in your own tree:

```ts
import type {
    StyleSpecification,
    GetRenderedTileOptions,
    GetRenderedClipOptions,
    GetRenderedCameraOptions,
    SupportedFormat,
    Cache,
    Protocol,
    Protocols,
} from 'chiitiler';
```

Prefer chiitiler's `StyleSpecification` over importing it from
`@maplibre/maplibre-gl-style-spec` directly: if your tree resolves a different major of
that package, the two structurally-different types are not assignable to each other.

chiitiler doesn't override the `User-Agent` on outbound HTTP requests by default, so the runtime's default is used (Node sends `node`). Some tile providers (e.g. OpenStreetMap) require an identifying User-Agent — set one with `setUserAgent('my-app/1.0')` or the `CHIITILER_USER_AGENT` environment variable.

## Configuration

The server is configured with environment variables only; it takes no command-line arguments
(not even 1.x's `tile-server` command). Empty values count as unset, and an invalid value stops
startup with an error naming the variable.

```bash
docker run --rm -p 3000:3000 -e CHIITILER_CACHE_METHOD=file -e CHIITILER_PROCESSES=0 ghcr.io/kanahiro/chiitiler:latest
```

### Server

| Variable | Default |
| --- | --- |
| `CHIITILER_PORT` | `3000` |
| `CHIITILER_DEBUG` | `false` |
| `CHIITILER_USER_AGENT` | (none) |
| `CHIITILER_PROCESSES` | `1` (set `0` for all CPUs) |
| `CHIITILER_MAX_SIZE` | `4096` |
| `CHIITILER_CACHE_CONTROL` | (none) |
| `CHIITILER_CORS` | (none) |

`CHIITILER_PROCESSES` runs that many worker processes; `0` uses all available CPUs.
Within a process, rendering and encoding run on libuv's thread pool (up to 4 renders of a style in
parallel), so the server keeps answering (e.g. `/health`) while it draws.

`CHIITILER_MAX_SIZE` bounds every image side in pixels (tiles with their margin, `/clip` sizes, `/camera`
dimensions); larger requests answer `400`. `POST` bodies are limited to 16 MiB (`413`).

`CHIITILER_CACHE_CONTROL` sets `Cache-Control` on rendered images (e.g. `public, max-age=86400` for a CDN to
keep them; chiitiler doesn't cache rendered images itself). `GET` images carry an `ETag`, so
`If-None-Match` revalidations answer `304`. `CHIITILER_CORS` adds CORS headers for `*` or a comma-separated
list of origins.

On `SIGTERM` / `SIGINT` the server stops accepting connections and exits once the requests in
progress finish (at most 10 seconds).

### Cache

A renderer is kept per style in a 10-entry LRU. It keeps decoded tiles, glyphs and
sprites for 5 minutes, after which the next request starts from fetched bytes again
(through the cache below). Styles given by URL are read again after 60 seconds, past the
backing cache (only the memory layer below applies), so edits upstream show up within a minute.

| Variable | Default |
| --- | --- |
| `CHIITILER_CACHE_METHOD` | `none` |
| `CHIITILER_CACHE_TTL_SEC` | `3600` |
| `CHIITILER_MEMORYCACHE_MAXITEMCOUNT` | `1000` |
| `CHIITILER_FRONT_CACHE_TTL_SEC` | `10` (seconds) |
| `CHIITILER_FRONT_CACHE_MAX_BYTES` | `67108864` (64 MiB) |
| `CHIITILER_FILECACHE_DIR` | `./.cache` |
| `CHIITILER_S3CACHE_BUCKET` | — |
| `CHIITILER_S3_REGION` | `us-east-1` |
| `CHIITILER_S3_ENDPOINT` | — |
| `CHIITILER_S3_FORCE_PATH_STYLE` | `false` |
| `CHIITILER_GCS_CACHE_BUCKET` | — |
| `CHIITILER_GCS_PROJECT_ID` | — |
| `CHIITILER_GCS_KEY_FILENAME` | — |
| `CHIITILER_GCS_CACHE_PREFIX` | — |
| `CHIITILER_GCS_API_ENDPOINT` | — |

Chiitiler caches *source assets* (vector tiles, glyphs, sprites) — not final rasters — so cached data is reused across requests. Standard AWS / GCP credentials (`AWS_ACCESS_KEY_ID`, `GOOGLE_APPLICATION_CREDENTIALS`, etc.) are respected.

For the `file`, `s3` and `gcs` cache methods, the tile server adds a per-process memory front
cache (not for `none`, which declines caching, or `memory`, which is already in-process): by default, buffers are retained for 10 seconds from insertion, with an
LRU limit of 64 MiB of buffer payloads (JavaScript overhead is additional). It applies to
every source (`http(s)://`, `s3://`, `gs://`, `file://`, `mbtiles://`, `pmtiles://`, `cog://`).
Only `http(s)://` sources (including `pmtiles://http(s)://` tiles) are also stored in the
backing cache; reads promote backing-cache hits into memory, and writes populate memory
immediately and also write to the backing cache.

Set `CHIITILER_FRONT_CACHE_TTL_SEC` to a positive, finite number of seconds and
`CHIITILER_FRONT_CACHE_MAX_BYTES` to a positive integer number of bytes to override
these limits. Set either limit to `0`
(e.g. `CHIITILER_FRONT_CACHE_TTL_SEC=0`) to disable the front cache entirely. Otherwise,
invalid values stop startup. These settings do not affect library callers.

The front-cache TTL does not guarantee source freshness: promotion can retain a
value for up to the configured TTL beyond its backing-cache expiry, and the S3/GCS cache
implementations do not check expiry on reads. Each worker has its own memory limit.
Library callers can opt in and customize the limits (either limit set to `0`
returns the backing cache unchanged):

```ts
const cache = ChiitilerCache.withMemoryCache(
    ChiitilerCache.fileCache({ dir: './.cache', ttl: 3600 }),
    { ttlSeconds: 10, maxBytes: 64 * 1024 * 1024 },
);
```

## Deployment

- **Docker** — `ghcr.io/kanahiro/chiitiler:latest` (configured with `CHIITILER_*` environment variables)
- **Docker Compose** — see [`docker-compose.yml`](./docker-compose.yml) (includes RustFS + fake-gcs-server for local testing)
- **AWS Lambda** — ready-to-deploy CDK app in [`cdk/`](./cdk)

> [!WARNING]
> **Do not expose chiitiler directly to the public internet.** Chiitiler fetches whatever the request tells it to: the `url` style and every source, glyph, and sprite URL inside the style (`GET` or `POST`). This allows SSRF — requests to internal networks or cloud metadata endpoints — and reads of local files (`file://`, `mbtiles://`) and buckets (`s3://`, `gs://`) with the server's credentials. Accept requests only from trusted clients, e.g. behind an authenticating proxy or a private network.

## Migrating from 1.x

2.0 renders with [ml2drender](./crates) instead of MapLibre Native: no GPU, Xvfb or sharp, and about
a third of the memory. Images are close to MapLibre's but not pixel-identical (antialiasing and label
placement differ).

- **Not rendered:** 3D (`fill-extrusion` is drawn as a flat fill), terrain, globe, sky, bidirectional
  and Arabic text shaping, and glyphs the style's `glyphs` URL doesn't serve (no local font fallback).
  `/camera` accepts only pitch `0` (otherwise `400`).
- **Formats:** WebP is lossless, so `quality` applies to JPEG only; JPEG uses 4:2:0 chroma and
  flattens transparency over black. `.jpg` responses are `image/jpeg` (were `image/jpg`).
- **Errors:** parameters the renderer rejects (e.g. a `tileSize` that isn't a positive integer) answer
  `400` instead of `500`.
- **Server:** the `tile-server` command and its options are removed: start chiitiler without
  arguments and set the `CHIITILER_*` environment variable each flag had (see
  [Configuration](#configuration)); any argument now stops startup with an error.
  Unknown `CHIITILER_CACHE_METHOD` values and other invalid values stop startup too (1.x fell back to
  defaults). `CHIITILER_PREWARM` is removed (startup is fast without it). Images are at most 4096 px on a side unless `CHIITILER_MAX_SIZE` says otherwise.
- **Caches:** `file`, `s3` and `gcs` caches use new file and object names (SHA-256 of the URL; the
  old escaped names could collide) and the file cache stores raw bytes: entries written by 1.x are
  not read, and can be deleted. The Docker image has no entrypoint script; the bundle is `build/main.mjs`.
- **Library:**
  - `getRendered*Buffer` resolve to a `Uint8Array`; the `*Stream` variants (Sharp) and `prewarm()`
    are removed.
  - Cache values are `Uint8Array` and `name` is optional, so any `{ get, set }` object is a cache.
  - New `protocols` option for custom URL schemes.
  - Renderers are reused per style, cache and protocols object: create the cache once, not per call.
  - The package ships the renderer for linux-x64, linux-arm64 (glibc 2.35+), darwin-arm64 and
    win32-x64; elsewhere, build it from a checkout with `pnpm build:native`.

## Develop

Requires Node.js 24.22.1+, pnpm (`corepack enable`) and Rust; [uv](https://docs.astral.sh/uv/) for the Python build.

```bash
git clone https://github.com/Kanahiro/chiitiler.git
cd chiitiler
pnpm install
pnpm build:native     # build the renderer (native/ml2drender.<platform>-<arch>.node)
pnpm dev              # tsx watch mode
pnpm lint             # biome lint + format check
pnpm test:unit        # vitest
pnpm test:integration # end-to-end
pnpm test:benchmark   # see bench/BENCHMARK.md
pnpm build            # bundle to build/main.mjs (loads ../native at runtime)
pnpm test:rust        # cargo test (renderer)
pnpm fixtures         # once: fetch MapLibre render/expression test fixtures
pnpm test:render      # MapLibre render tests → target/test-report/render
```

The renderer lives in [`crates/`](./crates) (copied from ml2drender; see [`crates/AGENTS.md`](./crates/AGENTS.md)):
`crates/core` is the sans-IO renderer, independent of its bindings: `crates/napi` (the Node-API addon
chiitiler uses) and `crates/wasm` (WebAssembly exports, for runtimes without native addons).
`crates/cli` is the test runner, and [`src/render/ml2drender.ts`](./src/render/ml2drender.ts) the
TypeScript host that drives the `needs()` → fetch → `provide()` loop through chiitiler's sources and cache.

The npm package ships the addon for linux-x64, linux-arm64 (glibc 2.35+), darwin-arm64 and
win32-x64; elsewhere, build it with `pnpm build:native`. x86-64 builds target x86-64-v2 (SSE4.2).

The same renderer builds for other hosts:

| Target | Binding | Build | Test |
| --- | --- | --- | --- |
| Node.js (chiitiler) | `crates/napi` | `pnpm build:native` → `native/ml2drender.<platform>-<arch>.node` | `pnpm test:unit` |
| Browsers, Deno, Workers (WASM) | `crates/wasm` + [`packages/ml2drender`](./packages/ml2drender) (TypeScript API) | `pnpm build:wasm` → `packages/ml2drender/wasm/` | `pnpm test:wasm` |
| Python ≥ 3.9 | `crates/python` (PyO3, abi3) | `pnpm build:python` → `target/python/*.whl` | `pnpm test:python` |

```python
import ml2drender, urllib.request

def load(url):  # bytes, or None when missing (like HTTP 404)
    try:
        return urllib.request.urlopen(url).read()
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None
        raise

style = urllib.request.urlopen('https://tile.openstreetmap.jp/styles/osm-bright/style.json').read().decode()
view = ml2drender.View.tile(5, 27, 12, 512)
rgba = ml2drender.render(style, view, load)
png = ml2drender.encode_png(rgba, *view.size)
```

## Architecture

```mermaid
graph LR
    subgraph sources
        direction LR
        A[style.json]
        B[z/x/y.pbf]
        C[z/x/y.png/webp/jpg]
        D[sprite]
        E[glyphs]
    end

    subgraph chiitiler
        cache
        render
        server
    end

    sources --> cache --> render --> server --/tiles/z/x/y--> png/webp/jpg
    cache <--get/set--> memory/file/s3/gcs
```

## Credits

Inspired by [`maptiler/tileserver-gl`](https://github.com/maptiler/tileserver-gl) and [`developmentseed/titiler`](https://github.com/developmentseed/titiler).

## License

[MIT](./LICENSE) © Kanahiro Iguchi
