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
