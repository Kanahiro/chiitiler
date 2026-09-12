import { LRUCache } from 'lru-cache';
import type { Cache, Value } from './index.js';

/**
 * Keep recent source buffers in this process to avoid backing-cache I/O.
 * TTL starts on insertion, so promotion can extend the backing cache's lifetime
 * by up to ttlSeconds. maxBytes bounds buffer payloads, not total process memory.
 * Either limit set to zero disables the front cache and returns backing unchanged.
 */
export function withMemoryCache(
    backing: Cache,
    { ttlSeconds = 10, maxBytes = 64 * 1024 * 1024 } = {},
): Cache {
    if (ttlSeconds === 0 || maxBytes === 0) return backing;

    if (!Number.isFinite(ttlSeconds) || ttlSeconds <= 0) {
        throw new Error('ttlSeconds must be positive and finite');
    }
    if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) {
        throw new Error('maxBytes must be a positive safe integer');
    }
    const memory = new LRUCache<string, Value>({
        maxSize: maxBytes,
        sizeCalculation: (value) => Math.max(1, value.byteLength),
        ttl: ttlSeconds * 1000,
        ttlResolution: 0,
    });
    const reads = new Map<string, Promise<Value | undefined>>();

    return {
        name: backing.name,
        get: async (key) => {
            const hit = memory.get(key);
            if (hit !== undefined) return hit;
            const pending = reads.get(key);
            if (pending !== undefined) return pending;

            const read = backing.get(key).then((value) => {
                // A newer write must not be overwritten by an older read.
                if (value !== undefined && reads.get(key) === read) memory.set(key, value);
                return value;
            }).finally(() => {
                if (reads.get(key) === read) reads.delete(key);
            });
            reads.set(key, read);
            return read;
        },
        set: async (key, value) => {
            reads.delete(key);
            // Populate before awaiting persistence; callers may not await set.
            memory.set(key, value);
            await backing.set(key, value);
        },
    };
}
