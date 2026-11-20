import { readdir, readFile } from 'node:fs/promises';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { Chiitiler, encode } from './index.js';
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
        const image = await chiitiler.renderTile({
            stylejson: 'test://style.json',
            z: 5,
            x: 28,
            y: 12,
            tileSize: 256,
            margin: 0,
        });
        expect([...(await image.toPng()).slice(0, 4)]).toEqual(PNG);
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
            const image = await chiitiler.renderBbox({
                stylejson: { version: 8, sources: {}, layers: [] },
                bbox: [100, 30, 150, 60],
                size: n,
            });
            expect(image.data.length).toBe(image.width * image.height * 4);
            return [image.width, image.height];
        };
        expect(await size(1024)).toEqual([1024, 901]);
        expect(await size(512)).toEqual([512, 451]);
    });
});

describe('formats', () => {
    it('encodes WebP lossy below quality 100 and lossless at 100, and AVIF', async () => {
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
        const image = await chiitiler.renderTile({
            stylejson: 'test://style.json',
            z: 5,
            x: 28,
            y: 12,
            tileSize: 256,
            margin: 0,
        });
        const fourcc = async (quality: number) =>
            new TextDecoder().decode(
                (await image.toWebp(quality)).slice(8, 16),
            );
        // A transparent tile: lossy WebP keeps its alpha in the extended format.
        expect(await fourcc(80)).toBe('WEBPVP8X');
        expect(await fourcc(100)).toBe('WEBPVP8L');
        const avif = await image.toAvif(60);
        expect(new TextDecoder().decode(avif.slice(4, 12))).toBe('ftypavif');
    });
});

describe('encode', () => {
    it('encodes edited or new RGBA pixels, and rejects a size the bytes do not have', async () => {
        const data = new Uint8Array(3 * 2 * 4).fill(255);
        const png = await encode({ data, width: 3, height: 2 }, 'png');
        expect([...png.slice(0, 4)]).toEqual(PNG);
        const view = new DataView(png.buffer, png.byteOffset);
        expect([view.getUint32(16), view.getUint32(20)]).toEqual([3, 2]);
        const jpeg = await encode({ data, width: 3, height: 2 }, 'jpg', 80);
        expect([...jpeg.slice(0, 2)]).toEqual([0xff, 0xd8]);
        await expect(
            encode({ data, width: 2, height: 2 }, 'png'),
        ).rejects.toThrow(RangeError);
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
            const image = await chiitiler.renderProjectedTile({
                stylejson: style,
                crs,
                z: 0,
                x: 0,
                y: 0,
                tileSize: 64,
                margin: 0,
            });
            expect([...(await image.toPng()).slice(0, 4)], file).toEqual(PNG);
        }
    });
});

describe('cache writes', () => {
    it('a cache that rejects writes costs a warning, not the process', async () => {
        const body = await tile('5/28/12.pbf');
        vi.stubGlobal('fetch', async () => new Response(body));
        const error = vi.spyOn(console, 'error').mockImplementation(() => {});
        const chiitiler = new Chiitiler({
            cache: {
                get: async () => undefined,
                set: async () => {
                    throw new Error('cache down');
                },
            },
        });
        await chiitiler.renderTile({
            stylejson: circles('https://example.com/{z}/{x}/{y}.pbf'),
            z: 5,
            x: 28,
            y: 12,
            tileSize: 256,
            margin: 0,
        });
        // the write is not awaited by the source: let it settle
        await new Promise((resolve) => setTimeout(resolve, 10));
        expect(error).toHaveBeenCalledWith(
            expect.stringContaining('cache write failed'),
        );
        error.mockRestore();
    });
});
