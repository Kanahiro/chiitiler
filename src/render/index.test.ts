import { readFile } from 'node:fs/promises';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { getRenderedClip, getRenderedTile } from './index.js';
import type { Cache } from '../cache/index.js';
import type { Protocols } from '../source/index.js';

const PNG = [0x89, 0x50, 0x4e, 0x47];

const circles = (tiles: string): StyleSpecification => ({
    version: 8,
    sources: { dir: { type: 'vector', tiles: [tiles], maxzoom: 6 } },
    layers: [
        {
            id: 'dir',
            type: 'circle',
            source: 'dir',
            'source-layer': 'P2921',
        },
    ],
});

const tile = (path: string) =>
    readFile(`localdata/tiles/${path}`).then(
        (b) => new Uint8Array(b),
        () => null,
    );

afterEach(() => {
    vi.unstubAllGlobals();
});

describe('library resources', () => {
    it('reads the style and its tiles through custom protocols', async () => {
        const urls: string[] = [];
        const protocols: Protocols = {
            test: async (url) => {
                urls.push(url);
                if (url === 'test://style.json')
                    return new TextEncoder().encode(
                        JSON.stringify(circles('test://tiles/{z}/{x}/{y}.pbf')),
                    );
                return tile(url.replace('test://tiles/', ''));
            },
        };
        const png = await getRenderedTile({
            stylejson: 'test://style.json',
            z: 5,
            x: 28,
            y: 12,
            tileSize: 256,
            margin: 0,
            ext: 'png',
            quality: 100,
            cache: { get: async () => undefined, set: async () => {} },
            protocols,
        });
        expect([...png.slice(0, 4)]).toEqual(PNG);
        expect(urls[0]).toBe('test://style.json');
        expect(urls).toContain('test://tiles/4/14/6.pbf');
    });

    it('keeps http sources in a cache of your own', async () => {
        const fetch = vi.fn(async (url: string) => {
            const bytes = await tile(url.replace('https://tiles.test/', ''));
            return bytes === null
                ? new Response(null, { status: 404 })
                : new Response(bytes);
        });
        vi.stubGlobal('fetch', fetch);
        const store = new Map<string, Uint8Array>();
        const cache: Cache = {
            get: async (key) => store.get(key),
            set: async (key, value) => {
                store.set(key, value);
            },
        };
        const style = circles('https://tiles.test/{z}/{x}/{y}.pbf');
        const render = (stylejson: StyleSpecification) =>
            getRenderedTile({
                stylejson,
                z: 5,
                x: 28,
                y: 12,
                tileSize: 256,
                margin: 0,
                ext: 'png',
                quality: 100,
                cache,
            });
        await render(style);
        const url = 'https://tiles.test/4/14/6.pbf';
        expect(store.get(url)?.length).toBeGreaterThan(0);
        const fetched = fetch.mock.calls.length;

        // another style: a new renderer, which reads the tile from the cache
        await render({
            ...style,
            layers: [{ id: 'bg', type: 'background' }, ...style.layers],
        });
        expect(fetch.mock.calls.length).toBe(fetched);
    });

    it('sizes clips like 1.x: the long side is `size`, the short one rounded up', async () => {
        const size = async (n: number) => {
            const png = await getRenderedClip({
                stylejson: { version: 8, sources: {}, layers: [] },
                bbox: [100, 30, 150, 60],
                size: n,
                cache: { get: async () => undefined, set: async () => {} },
                ext: 'png',
                quality: 100,
            });
            const view = new DataView(png.buffer, png.byteOffset);
            return [view.getUint32(16), view.getUint32(20)];
        };
        expect(await size(1024)).toEqual([1024, 901]);
        expect(await size(512)).toEqual([512, 451]);
    });
});
