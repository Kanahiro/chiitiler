import { describe, expect, it, vi } from 'vitest';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { getRenderPool } from './pool.js';
import { noneCache } from '../cache/index.js';

vi.mock('@maplibre/maplibre-gl-native', () => ({
    default: {
        Map: class {
            constructor(public options: { mode: string }) {}
            load() {}
            release() {}
        },
    },
}));

describe('getRenderPool', () => {
    it('reuses a Map after a long idle period and releases it on close', async () => {
        vi.useFakeTimers();
        const style: StyleSpecification = {
            version: 8, name: 'long-idle', sources: {}, layers: [],
        };
        const pool = await getRenderPool(style, noneCache(), 'tile');
        try {
            const map = await pool.acquire();
            const release = vi.spyOn(map, 'release');
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
            const release = vi.spyOn(map, 'release');
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
                        expect(map).toHaveProperty('options.mode', mode);
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
