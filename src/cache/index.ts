import { memoryCache } from './memory.js';
import { s3Cache } from './s3.js';
import { gcsCache } from './gcs.js';
import { fileCache } from './file.js';

export { withMemoryCache } from './front.js';
export type { MemoryCacheOptions } from './memory.js';
export type { S3CacheOptions } from './s3.js';
export type { GCSCacheOptions } from './gcs.js';
export type { fileCacheOptions } from './file.js';

type Value = Uint8Array;
/**
 * Where fetched source bytes are kept (implement `get` and `set` to bring your own).
 * Keys are resource URLs. Only http(s) sources (including `pmtiles://http(s)://` tiles) use it;
 * `memory` (see `withMemoryCache`) serves every source.
 */
type Cache = {
    /** A label (built-in caches name their kind). */
    name?: string;
    /** The bytes stored under `key`, or undefined for a miss. */
    get: (key: string) => Promise<Value | undefined>;
    /** Store `value` under `key`. Rejections are logged by the built-in caches; yours may throw. */
    set: (key: string, value: Value) => Promise<void>;
    /** Short-lived in-process layer (set by withMemoryCache), applied to every source. */
    memory?: Cache;
};

const noneCache: () => Cache = () => ({
    name: 'none',
    get: async () => undefined,
    set: async () => undefined,
});

export {
    noneCache,
    memoryCache,
    s3Cache,
    gcsCache,
    fileCache,
    type Value,
    type Cache,
};
