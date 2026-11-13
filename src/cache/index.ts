import { memoryCache } from './memory.js';
import { s3Cache } from './s3.js';
import { gcsCache } from './gcs.js';
import { fileCache } from './file.js';

export { withMemoryCache } from './front.js';
export type { MemoryCacheOptions } from './memory.js';
export type { S3CacheOptions } from './s3.js';
export type { GCSCacheOptions } from './gcs.js';
export type { fileCacheOptions } from './file.js';

type Value = Buffer;
type Cache = {
    name: string;
    get: (key: string) => Promise<Value | undefined>;
    set: (key: string, value: Value) => Promise<void>;
    /** Short-lived in-process layer (set by withMemoryCache), applied to every source. */
    memory?: Cache;
};

const noneCache: () => Cache = () => ({
    name: 'none',
    get: async () => undefined,
    set: async () => undefined,
});

export { noneCache, memoryCache, s3Cache, gcsCache, fileCache, type Value, type Cache };
