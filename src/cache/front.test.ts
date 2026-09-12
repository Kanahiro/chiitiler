import { afterEach, describe, expect, it, vi } from 'vitest';
import { withMemoryCache } from './front.js';
import type { Cache, Value } from './index.js';

function backingCache() {
    return {
        name: 'test',
        get: vi.fn<Cache['get']>().mockResolvedValue(undefined),
        set: vi.fn<Cache['set']>().mockResolvedValue(undefined),
    };
}

afterEach(() => vi.restoreAllMocks());

describe('withMemoryCache', () => {
    it('promotes backing hits and expires from insertion, not last access', async () => {
        // lru-cache retains the performance object at module load time.
        let now = 1;
        vi.spyOn(performance, 'now').mockImplementation(() => now);
        const backing = backingCache();
        backing.get.mockResolvedValue(Buffer.from('old'));
        const cache = withMemoryCache(backing);
        expect(await cache.get('key')).toEqual(Buffer.from('old'));
        now += 9000;
        expect(await cache.get('key')).toEqual(Buffer.from('old'));
        expect(backing.get).toHaveBeenCalledTimes(1);
        backing.get.mockResolvedValue(Buffer.from('new'));
        now += 1001;
        expect(await cache.get('key')).toEqual(Buffer.from('new'));
        expect(backing.get).toHaveBeenCalledTimes(2);
    });

    it('shares concurrent reads and does not retain misses or errors', async () => {
        const backing = backingCache();
        const cache = withMemoryCache(backing);
        expect(await Promise.all([cache.get('key'), cache.get('key')])).toEqual([undefined, undefined]);
        expect(backing.get).toHaveBeenCalledTimes(1);
        backing.get.mockRejectedValueOnce(new Error('temporary'));
        await expect(cache.get('key')).rejects.toThrow('temporary');
        backing.get.mockResolvedValue(Buffer.from('recovered'));
        expect(await cache.get('key')).toEqual(Buffer.from('recovered'));
        expect(backing.get).toHaveBeenCalledTimes(3);
    });

    it('serves writes immediately while backing persistence is pending', async () => {
        const backing = backingCache();
        let finish!: () => void;
        backing.set.mockImplementation(() => new Promise<void>((resolve) => { finish = resolve; }));
        const cache = withMemoryCache(backing);
        const value = Buffer.from('value');
        const writing = cache.set('key', value);
        expect(await cache.get('key')).toBe(value);
        expect(backing.get).not.toHaveBeenCalled();
        expect(backing.set).toHaveBeenCalledWith('key', value);
        finish();
        await writing;
    });

    it('does not let an older backing read overwrite a newer write', async () => {
        const backing = backingCache();
        let finish!: (value: Value) => void;
        backing.get.mockImplementation(() => new Promise<Value>((resolve) => { finish = resolve; }));
        const cache = withMemoryCache(backing);
        const reading = cache.get('key');
        await cache.set('key', Buffer.from('new'));
        finish(Buffer.from('old'));
        await reading;
        expect(await cache.get('key')).toEqual(Buffer.from('new'));
    });

    it('evicts the least recently used payload to stay within the byte limit', async () => {
        const backing = backingCache();
        const cache = withMemoryCache(backing, { maxBytes: 6 });
        await cache.set('a', Buffer.from('aaa'));
        await cache.set('b', Buffer.from('bbb'));
        await cache.get('a');
        await cache.set('c', Buffer.from('ccc'));
        expect(await cache.get('a')).toEqual(Buffer.from('aaa'));
        expect(await cache.get('c')).toEqual(Buffer.from('ccc'));
        expect(await cache.get('b')).toBeUndefined();
        expect(backing.get).toHaveBeenCalledExactlyOnceWith('b');
    });

    it('persists oversized values without retaining an older cached value', async () => {
        const backing = backingCache();
        const cache = withMemoryCache(backing, { maxBytes: 3 });
        await cache.set('key', Buffer.from('old'));
        const large = Buffer.from('large');
        await cache.set('key', large);
        backing.get.mockResolvedValue(large);
        expect(await cache.get('key')).toBe(large);
        expect(await cache.get('key')).toBe(large);
        expect(backing.get).toHaveBeenCalledTimes(2);
        expect(backing.set).toHaveBeenLastCalledWith('key', large);
    });

    it('propagates persistence failures while retaining the fetched buffer', async () => {
        const backing = backingCache();
        backing.set.mockRejectedValue(new Error('write failed'));
        const cache = withMemoryCache(backing);
        await expect(cache.set('key', Buffer.from('value'))).rejects.toThrow('write failed');
        expect(await cache.get('key')).toEqual(Buffer.from('value'));
    });

    it.each([
        { ttlSeconds: 0 }, { ttlSeconds: -1 }, { ttlSeconds: Infinity },
        { maxBytes: 0 }, { maxBytes: -1 }, { maxBytes: 1.5 },
    ])('rejects invalid limits %j', (options) => {
        expect(() => withMemoryCache(backingCache(), options)).toThrow();
    });
});
