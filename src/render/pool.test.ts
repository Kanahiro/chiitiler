import { describe, expect, it, vi } from 'vitest';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { getRenderPool, tagStyle } from './pool.js';
import { noneCache } from '../cache/index.js';

vi.mock('@maplibre/native-ffi', () => {
    const noop = () => {};
    return {
        MapMode: { static: 1, tile: 2 },
        RenderDriverKind: {},
        ResourceKind: {},
        ResourceResponseStatus: {},
        Status: {},
        OffscreenDeviceHandle: { create: () => ({}) },
        RuntimeHandle: {
            create: async () => ({
                discarding: { setResourceProvider: noop },
                setMaximumAmbientCacheSize: async () => {},
                createMap: (options: unknown) => ({
                    options,
                    ready: Promise.resolve(),
                    discarding: { setStyleJson: noop },
                    attachOffscreen: () => ({ ready: Promise.resolve(), release: async () => {} }),
                    release: async () => {},
                }),
            }),
        },
    };
});

describe('tagStyle', () => {
    it('routes every style URL through chiitiler, TileJSON urls included', () => {
        const style = {
            version: 8,
            glyphs: 'file://fonts/{fontstack}/{range}.pbf',
            sprite: [{ id: 'a', url: 'https://example.com/sprite' }],
            sources: {
                v: { type: 'vector', tiles: ['pmtiles://a.pmtiles/{z}/{x}/{y}'] },
                t: { type: 'raster', url: 'https://example.com/tiles.json' },
                g: { type: 'geojson', data: 's3://b/a.geojson' },
                inline: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
            },
            layers: [],
        } as unknown as StyleSpecification;
        const tagged = tagStyle(style, 7) as any;
        expect(tagged.glyphs).toBe('chiitiler://7/file://fonts/{fontstack}/{range}.pbf');
        expect(tagged.sprite[0].url).toBe('chiitiler://7/https://example.com/sprite');
        expect(tagged.sources.v.tiles[0]).toBe('chiitiler://7/pmtiles://a.pmtiles/{z}/{x}/{y}');
        expect(tagged.sources.t.url).toBe('chiitiler://7/https://example.com/tiles.json');
        expect(tagged.sources.g.data).toBe('chiitiler://7/s3://b/a.geojson');
        expect(tagged.sources.inline.data).toEqual(style.sources.inline.data);
        expect((style as any).glyphs).toBe('file://fonts/{fontstack}/{range}.pbf');
    });
});

describe('getRenderPool', () => {
    it('reuses a Map after a long idle period and releases it on close', async () => {
        vi.useFakeTimers();
        const style: StyleSpecification = {
            version: 8, name: 'long-idle', sources: {}, layers: [],
        };
        const pool = await getRenderPool(style, noneCache(), 'tile');
        try {
            const map = await pool.acquire();
            const release = vi.spyOn(map.map, 'release');
            pool.release(map);
            await vi.advanceTimersByTimeAsync(60 * 60 * 1000);
            expect(release).not.toHaveBeenCalled();
            const reused = await pool.acquire();
            pool.release(reused);
            expect(reused).toBe(map);
            const closing = pool.close();
            await vi.advanceTimersByTimeAsync(100);
            await closing;
            expect(release).toHaveBeenCalledTimes(1);
        } finally {
            const closing = pool.close();
            await vi.advanceTimersByTimeAsync(100);
            await closing;
            vi.useRealTimers();
        }
    });

    it('still releases Maps when their style/mode pool is evicted from the LRU', async () => {
        const cache = noneCache();
        const style: StyleSpecification = {
            version: 8, name: 'eviction-victim', sources: {}, layers: [],
        };
        const victim = await getRenderPool(style, cache, 'tile');
        const pools = [victim];
        try {
            const map = await victim.acquire();
            const release = vi.spyOn(map.map, 'release');
            victim.release(map);
            for (let i = 0; i < 10; i++) {
                pools.push(await getRenderPool({ ...style, name: `eviction-${i}` }, cache, 'tile'));
            }
            await vi.waitFor(() => expect(release).toHaveBeenCalledTimes(1));
        } finally {
            await Promise.all(pools.map((pool) => pool.close()));
        }
    });

    it.each(['tile', 'static'] as const)(
        'keeps modes separate when %s is requested first',
        async (firstMode) => {
            const style: StyleSpecification = {
                version: 8,
                name: firstMode,
                sources: {},
                layers: [],
            };
            const cache = noneCache();
            const secondMode = firstMode === 'tile' ? 'static' : 'tile';
            const first = await getRenderPool(style, cache, firstMode);
            const second = await getRenderPool(style, cache, secondMode);
            try {
                expect(second).not.toBe(first);
                expect(await getRenderPool(structuredClone(style), cache, firstMode)).toBe(first);
                expect(await getRenderPool(style, cache, secondMode)).toBe(second);

                for (const [pool, mode] of [[first, firstMode], [second, secondMode]] as const) {
                    const map = await pool.acquire();
                    try {
                        expect(map).toHaveProperty('map.options.mapMode', mode === 'tile' ? 2 : 1);
                    } finally {
                        pool.release(map);
                    }
                }
            } finally {
                await first.close();
                await second.close();
            }
        },
    );
});
