import {
    MapMode,
    OffscreenDeviceHandle,
    RenderDriverKind,
    ResourceKind,
    ResourceResponseStatus,
    RuntimeHandle,
    Status,
    type MapHandle,
    type ResourceResponse,
    type RenderSessionHandle,
} from '@maplibre/native-ffi';
import { Pool } from 'lightning-pool';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { LRUCache } from 'lru-cache';

import { getSource } from '../source/index.js';
import type { Cache } from '../cache/index.js';

type RenderWorker = {
    map: MapHandle;
    session: RenderSessionHandle;
    width: number;
    height: number;
};

/**
 * Every URL in a pooled style is tagged as chiitiler://<poolId>/<original>.
 * - The resource provider is per runtime, so the tag routes a request back to
 *   the Cache of the pool that issued it.
 * - MapLibre Native serves file://, mbtiles:// and pmtiles:// with built-in
 *   file sources that bypass the provider; the tag keeps every URL on chiitiler's
 *   own source loaders.
 */
const TAG = /^chiitiler:\/\/(\d+)\/(.*)$/s;
const tag = (id: number, url: string) => `chiitiler://${id}/${url}`;

function tagStyle(style: StyleSpecification, id: number): StyleSpecification {
    const s = structuredClone(style) as any;
    for (const src of Object.values<any>(s.sources ?? {})) {
        if (Array.isArray(src.tiles)) src.tiles = src.tiles.map((u: string) => tag(id, u));
        if (typeof src.url === 'string') src.url = tag(id, src.url);
        if (Array.isArray(src.urls)) src.urls = src.urls.map((u: string) => tag(id, u));
        if (typeof src.data === 'string') src.data = tag(id, src.data);
    }
    if (typeof s.glyphs === 'string') s.glyphs = tag(id, s.glyphs);
    if (typeof s.sprite === 'string') s.sprite = tag(id, s.sprite);
    else if (Array.isArray(s.sprite))
        s.sprite = s.sprite.map((sp: any) => ({ ...sp, url: tag(id, sp.url) }));
    return s;
}

// TileJSON fetched through a tagged source.url names untagged tile URLs.
function tagTileJson(buf: Buffer, id: number): Uint8Array {
    try {
        const json = JSON.parse(buf.toString());
        if (!Array.isArray(json?.tiles)) return buf;
        json.tiles = json.tiles.map((u: string) => tag(id, u));
        return Buffer.from(JSON.stringify(json));
    } catch {
        return buf;
    }
}

const poolCaches = new Map<number, Cache>();
let nextPoolId = 0;

let shared: Promise<{ runtime: RuntimeHandle; device: OffscreenDeviceHandle }> | undefined;
function getShared() {
    shared ??= (async () => {
        const runtime = await RuntimeHandle.create();
        runtime.discarding.setResourceProvider({
            callback: (req, handle) => {
                const m = TAG.exec(req.requestedUrl);
                const id = m ? Number(m[1]) : -1;
                const url = m ? m[2] : req.requestedUrl;
                const answer = (res: ResourceResponse) => {
                    try {
                        handle.complete(res);
                    } catch (err) {
                        // The library cancels pending requests when a render
                        // fails or a Map is released; late answers are moot.
                        if ((err as { status?: number }).status !== Status.invalidState) throw err;
                    }
                };
                getSource(url, poolCaches.get(id)).then(
                    (buf) =>
                        answer(
                            buf === null || buf.length === 0
                                ? // 204/空ボディ/404: 空タイルとして扱わせる
                                  { status: ResourceResponseStatus.noContent }
                                : {
                                      status: ResourceResponseStatus.ok,
                                      bytes: req.kind === ResourceKind.source ? tagTileJson(buf, id) : buf,
                                  },
                        ),
                    // 5xx/ネットワークエラー: 空タイルとして描画すると
                    // 欠損タイルがCDN等にキャッシュされうるので、
                    // エラーを返してレンダリング自体を失敗させる
                    (err) =>
                        answer({
                            status: ResourceResponseStatus.error,
                            errorMessage: err instanceof Error ? err.message : String(err),
                        }),
                );
            },
        });
        // chiitiler caches sources itself (Cache); a second in-memory copy in
        // the runtime's ambient cache only costs memory.
        await runtime.setMaximumAmbientCacheSize(0n);
        // One device (EGL surfaceless; llvmpipe without a GPU) whose maps take
        // turns in one GL context, instead of a context per map. Without a GPU
        // this cuts memory by far more than the turns cost in throughput.
        const device = OffscreenDeviceHandle.create({ singleContext: true });
        return { runtime, device };
    })();
    return shared;
}

const poolIds = new WeakMap<Pool<RenderWorker>, number>();
const mapPoolCache = new LRUCache<string, Pool<RenderWorker>>({
    max: 10,
    dispose: (pool) => {
        // Forget the pool's Cache only once its Maps are gone.
        pool.close(() => poolCaches.delete(poolIds.get(pool)!));
    },
});

async function getRenderPool(
    style: StyleSpecification,
    cache: Cache,
    mode: 'tile' | 'static',
) {
    // A Map's mode is fixed at creation; tile and static requests cannot share it.
    const cacheKey = `${mode}:${JSON.stringify(style)}`;

    const pool = mapPoolCache.get(cacheKey);
    if (pool !== undefined) return pool;

    const id = nextPoolId++;
    poolCaches.set(id, cache);
    const tagged = tagStyle(style, id);

    const newPool = new Pool<RenderWorker>({
        create: async () => {
            const { runtime, device } = await getShared();
            const map = runtime.createMap({
                initialExtent: { width: 512, height: 512, scaleFactor: 1 },
                mapMode: mode === 'tile' ? MapMode.tile : MapMode.static,
                // Pooled maps jump across the world; kept tiles rarely get reused
                // and Cache already holds their sources.
                tileCacheEnabled: false,
            });
            await map.ready;
            map.discarding.setStyleJson(tagged);
            const session = map.attachOffscreen(
                { logicalSize: { width: 512, height: 512 } },
                { driver: RenderDriverKind.coreWorker },
                device,
            );
            await session.ready;
            return { map, session, width: 512, height: 512 };
        },
        destroy: async ({ map, session }) => {
            await session.release();
            await map.release();
        },
    }, {
        // Keep initialized Maps until their pool is evicted or closed. Disable
        // idle housekeeping rather than running a timer in warm Lambda environments.
        houseKeepInterval: 0,
    });
    poolIds.set(newPool, id);
    mapPoolCache.set(cacheKey, newPool);
    return newPool;
}

export { getRenderPool, tagStyle };
