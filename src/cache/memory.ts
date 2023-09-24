import { type Cache, type Value } from './index.js';

const MEMORY_CACHE_KVS: Record<string, Value> = {};
const memoryCache: () => Cache = function () {
    return {
        name: 'memory',
        set: async function (key: string, value: Value) {
            MEMORY_CACHE_KVS[key] = value;
        },
        get: async function (key: string): Promise<Value | undefined> {
            return (MEMORY_CACHE_KVS[key] as Value) ?? undefined;
        },
    };
};

export { memoryCache };
