import { getFilesystemSource } from './fs.js';
import { getHttpSource } from './http.js';
import { getPmtilesSource } from './pmtiles.js';
import { getMbtilesSource } from './mbtiles.js';
import { getS3Source } from './s3.js';
import { getGCSSource } from './gcs.js';
import { type Cache, noneCache } from '../cache/index.js';

/**
 * Loads the bytes of a URL of a custom scheme, or null when there are none (like HTTP 404:
 * a missing vector tile is empty, a missing raster tile is replaced by its parent). Throw for
 * other failures: the render fails rather than caching a map with holes.
 */
type Protocol = (url: string) => Promise<Uint8Array | null>;
/**
 * Custom protocols by scheme (`'myproto'` for `myproto://…`), for the style URL and every source,
 * sprite and glyph URL in it. They take precedence over built-in ones, so `http` / `https` can be
 * replaced too (e.g. to add credentials). Results go through the cache's front cache, not the
 * cache itself (cache inside the loader if needed).
 *
 * @example Tiles from a database
 * ```ts
 * const protocols: Protocols = {
 *     db: async (url) => {
 *         // db://tiles/{z}/{x}/{y}
 *         const [z, x, y] = url.split('/').slice(-3).map(Number);
 *         return (await db.tile(z, x, y)) ?? null;
 *     },
 * };
 * const chiitiler = new Chiitiler({ cache, protocols });
 * ```
 */
type Protocols = Record<string, Protocol>;

/**
 * Every source reads through the short-lived in-process front cache.
 * Only http(s) sources also persist to the backing cache (in fetchSource).
 * A missing resource is kept there too, as empty bytes (an empty result is one): areas without
 * data (the sea) are otherwise asked for again by every render.
 */
async function cached(
    uri: string,
    { front: cache = noneCache() }: Cache,
    load: () => Promise<Uint8Array | null>,
): Promise<Uint8Array | null> {
    const val = await cache.get(uri);
    if (val !== undefined) return val.length === 0 ? null : val;
    const buf = await load();
    cache.set(uri, buf ?? new Uint8Array());
    return buf === null || buf.length === 0 ? null : buf;
}

async function fetchSource(
    uri: string,
    cache: Cache,
    protocols?: Protocols,
): Promise<Uint8Array | null> {
    const scheme = uri.slice(0, Math.max(0, uri.indexOf('://')));
    if (protocols !== undefined && Object.hasOwn(protocols, scheme))
        return protocols[scheme](uri);
    if (uri.startsWith('http://') || uri.startsWith('https://'))
        return getHttpSource(uri, cache);
    if (uri.startsWith('file://')) return getFilesystemSource(uri);
    if (uri.startsWith('s3://')) return getS3Source(uri);
    if (uri.startsWith('gs://')) return getGCSSource(uri);
    if (uri.startsWith('mbtiles://')) return getMbtilesSource(uri);
    if (uri.startsWith('pmtiles://')) return getPmtilesSource(uri, cache);
    return null;
}

/** In-flight fetches by protocol set (handlers may differ for the same URL), then by URL. */
const inFlight = new WeakMap<object, Map<string, Promise<Uint8Array | null>>>();
const BUILT_IN = {};

/**
 * retrieve sources from the uri.
 * Concurrent requests for the same uri share a single in-flight fetch
 * (single-flight) to avoid duplicate downloads.
 * @param uri
 * @param cache {Cache} - Cache Strategy. Its front cache applies to every source; the backing cache only to http(s).
 * @param protocols - Custom protocols, before the built-in ones.
 * @returns
 */
async function getSource(
    uri: string,
    cache: Cache = noneCache(),
    protocols?: Protocols,
): Promise<Uint8Array | null> {
    let pending = inFlight.get(protocols ?? BUILT_IN);
    if (pending === undefined) {
        pending = new Map();
        inFlight.set(protocols ?? BUILT_IN, pending);
    }
    const existing = pending.get(uri);
    if (existing !== undefined) return existing;

    const promise = cached(uri, cache, () =>
        fetchSource(uri, cache, protocols),
    ).finally(() => {
        pending.delete(uri);
    });
    pending.set(uri, promise);
    return promise;
}

export { getSource, type Protocol, type Protocols };
