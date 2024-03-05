import MemoryCache from 'memory-cache-node';

import { type Cache, type Value } from './index.js';

const itemsExpirationCheckIntervalInSecs = 60; // check every 60 seconds
const maxItemCount = 1000; // 500KB * 1000 items = 500MB
const MEMORY_CACHE_KVS = new MemoryCache.MemoryCache<string, Value>(
    itemsExpirationCheckIntervalInSecs,
    maxItemCount,
);

type MemoryCacheOptions = {
    ttl: number;
    maxItemCount: number;
};

const memoryCache: (options: MemoryCacheOptions) => Cache = function (
    options: MemoryCacheOptions,
) {
    return {
        name: 'memory',
        set: async function (key: string, value: Value) {
            MEMORY_CACHE_KVS.storeExpiringItem(key, value, options.ttl);
        },
        get: async function (key: string): Promise<Value | undefined> {
            const item = MEMORY_CACHE_KVS.retrieveItemValue(key);
            return item;
        },
    };
};

export { memoryCache };
