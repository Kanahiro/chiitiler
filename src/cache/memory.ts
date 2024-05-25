import MemoryCache from 'memory-cache-node';

import { type Cache, type Value } from './index.js';

type MemoryCacheOptions = {
    ttl: number;
    maxItemCount: number;
    checkInterval?: number;
};

const memoryCache: (options: MemoryCacheOptions) => Cache = function (
    options: MemoryCacheOptions,
) {
    const itemsExpirationCheckIntervalInSecs = options.checkInterval ?? 60;
    const MEMORY_CACHE_KVS = new MemoryCache.MemoryCache<string, Value>(
        itemsExpirationCheckIntervalInSecs,
        options.maxItemCount,
    );
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
