import { describe, it, expect, vi, afterEach } from 'vitest';

import { getHttpSource } from './http.js';
import { setUserAgent } from './userAgent.js';
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

    it('does not send a User-Agent header by default', async () => {
        const fetchSpy = mockFetch(
            new Response(new Uint8Array([1]), { status: 200 }),
        );

        await getHttpSource('https://example.com/tile.webp');

        const [, init] = fetchSpy.mock.calls[0];
        expect(new Headers(init?.headers).get('User-Agent')).toBeNull();
    });

    it('sends a user-specified User-Agent header', async () => {
        const fetchSpy = mockFetch(
            new Response(new Uint8Array([1]), { status: 200 }),
        );
        setUserAgent('my-app/1.0');

        try {
            await getHttpSource('https://example.com/tile.webp');
        } finally {
            setUserAgent(undefined);
        }

        const [, init] = fetchSpy.mock.calls[0];
        expect(new Headers(init?.headers).get('User-Agent')).toBe('my-app/1.0');
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
