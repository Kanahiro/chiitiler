// TODO: other cache implementation

type Value = Buffer | string;
type Strategy = 'none' | 'memory';

const getCache = function (strategy: Strategy): Cache {
    if (strategy === 'none') {
        return {
            get: () => undefined,
            set: () => undefined,
        };
    } else if (strategy === 'memory') {
        return memoryCache;
    } else {
        throw new Error(`Invalid cache strategy: ${strategy}`);
    }
};

type Cache = {
    get: (key: string) => Value | undefined;
    set: (key: string, value: Value) => void;
};

const MEMORY_CACHE_KVS: Record<string, Value> = {};
const memoryCache: Cache = {
    set: function (key: string, value: Value) {
        MEMORY_CACHE_KVS[key] = value;
    },
    get: function (key: string): Value | undefined {
        return (MEMORY_CACHE_KVS[key] as Value) ?? undefined;
    },
};

export { getCache, type Cache, type Strategy, type Value };
