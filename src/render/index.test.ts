import { readdir, readFile } from 'node:fs/promises';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { Chiitiler } from './index.js';
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
        const chiitiler = new Chiitiler({
            cache: { get: async () => undefined, set: async () => {} },
            protocols,
        });
        const png = await chiitiler.renderTile({
            stylejson: 'test://style.json',
            z: 5,
            x: 28,
            y: 12,
            tileSize: 256,
            margin: 0,
            ext: 'png',
            quality: 100,
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
        const chiitiler = new Chiitiler({ cache });
        const render = (stylejson: StyleSpecification) =>
            chiitiler.renderTile({
                stylejson,
                z: 5,
                x: 28,
                y: 12,
                tileSize: 256,
                margin: 0,
                ext: 'png',
                quality: 100,
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
        const chiitiler = new Chiitiler({
            cache: { get: async () => undefined, set: async () => {} },
        });
        const size = async (n: number) => {
            const png = await chiitiler.renderBbox({
                stylejson: { version: 8, sources: {}, layers: [] },
                bbox: [100, 30, 150, 60],
                size: n,
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

describe('formats', () => {
    it('encodes WebP lossy below quality 100 and lossless at 100', async () => {
        const chiitiler = new Chiitiler({
            cache: { get: async () => undefined, set: async () => {} },
            protocols: {
                test: async (url) =>
                    url === 'test://style.json'
                        ? new TextEncoder().encode(
                              JSON.stringify(
                                  circles('test://tiles/{z}/{x}/{y}.pbf'),
                              ),
                          )
                        : tile(url.replace('test://tiles/', '')),
            },
        });
        const fourcc = async (quality: number) => {
            const webp = await chiitiler.renderTile({
                stylejson: 'test://style.json',
                z: 5,
                x: 28,
                y: 12,
                tileSize: 256,
                margin: 0,
                ext: 'webp',
                quality,
            });
            return new TextDecoder().decode(webp.slice(8, 16));
        };
        // A transparent tile: lossy WebP keeps its alpha in the extended format.
        expect(await fourcc(80)).toBe('WEBPVP8X');
        expect(await fourcc(100)).toBe('WEBPVP8L');
    });
});

describe('preset CRSs', () => {
    it('renders a tile in each CRS of crs/', async () => {
        const chiitiler = new Chiitiler({
            cache: { get: async () => undefined, set: async () => {} },
        });
        const style: StyleSpecification = {
            version: 8,
            sources: {},
            layers: [{ id: 'bg', type: 'background' }],
        };
        const files = await readdir('crs');
        expect(files.length).toBeGreaterThan(0);
        for (const file of files) {
            const crs = JSON.parse(await readFile(`crs/${file}`, 'utf8'));
            const png = await chiitiler.renderProjectedTile({
                stylejson: style,
                crs,
                z: 0,
                x: 0,
                y: 0,
                tileSize: 64,
                margin: 0,
                ext: 'png',
                quality: 100,
            });
            expect([...png.slice(0, 4)], file).toEqual(PNG);
        }
    });
});
