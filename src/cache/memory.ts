import { LRUCache } from 'lru-cache';

import type { Cache, Value } from './index.js';

type MemoryCacheOptions = {
    ttl: number;
    maxItemCount: number;
};

const memoryCache: (options: MemoryCacheOptions) => Cache = (
    options: MemoryCacheOptions,
) => {
    const MEMORY_CACHE = new LRUCache<string, Value>({
        max: options.maxItemCount,
        ttl: options.ttl * 1000,
    });
    return {
        name: 'memory',
        set: async (key: string, value: Value) => {
            MEMORY_CACHE.set(key, value);
        },
        get: async (key: string): Promise<Value | undefined> => {
            const item = MEMORY_CACHE.get(key);
            return item;
        },
    };
};

export { memoryCache, type MemoryCacheOptions };
