import { describe, it, expect, vi, afterEach } from 'vitest';

import { getHttpSource } from './http.js';
import { memoryCache } from '../cache/memory.js';

function mockFetch(response: Response) {
    return vi
        .spyOn(globalThis, 'fetch')
        .mockResolvedValue(response as unknown as Response);
}

describe('getHttpSource', () => {
    afterEach(() => {
        vi.restoreAllMocks();
    });

    it('returns null for 204 No Content without caching', async () => {
        const cache = memoryCache({ ttl: 60, maxItemCount: 10 });
        const setSpy = vi.spyOn(cache, 'set');
        mockFetch(new Response(null, { status: 204 }));

        const uri = 'https://example.com/xyz/9/455/201?format=webp';
        const data = await getHttpSource(uri, cache);

        expect(data).toBeNull();
        expect(setSpy).not.toHaveBeenCalled();
        expect(await cache.get(uri)).toBeUndefined();
    });

    it('returns null for empty-body 200 without caching', async () => {
        const cache = memoryCache({ ttl: 60, maxItemCount: 10 });
        const setSpy = vi.spyOn(cache, 'set');
        mockFetch(new Response(new ArrayBuffer(0), { status: 200 }));

        const uri = 'https://example.com/empty.webp';
        const data = await getHttpSource(uri, cache);

        expect(data).toBeNull();
        expect(setSpy).not.toHaveBeenCalled();
    });

    it('returns null for 404', async () => {
        const cache = memoryCache({ ttl: 60, maxItemCount: 10 });
        mockFetch(new Response('not found', { status: 404 }));

        const data = await getHttpSource('https://example.com/missing.webp', cache);

        expect(data).toBeNull();
    });

    it('returns and caches a non-empty body', async () => {
        const cache = memoryCache({ ttl: 60, maxItemCount: 10 });
        const body = new Uint8Array([1, 2, 3, 4]);
        mockFetch(new Response(body, { status: 200 }));

        const uri = 'https://example.com/tile.webp';
        const data = await getHttpSource(uri, cache);

        expect(data).not.toBeNull();
        expect(data!.length).toBe(4);
        expect(await cache.get(uri)).not.toBeUndefined();
    });
});
