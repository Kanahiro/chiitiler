import { describe, test, expect } from 'vitest';
import sizeof from 'image-size';

describe('intergration test', () => {
    test('health', async () => {
        const res = await fetch('http://localhost:3000/health');
        expect(res.status).toBe(200);
        expect(await res.text()).toBe('OK');
    });

    test('GET /tiles', async () => {
        const res = await fetch(
            'http://localhost:3000/tiles/0/0/0.png?url=file://localdata/style.json',
        );
        expect(res.status).toBe(200);
        expect(res.headers.get('content-type')).toBe('image/png');
        const png = new Uint8Array(await res.arrayBuffer());
        const pngsize = sizeof(png);
        expect(pngsize.width).toBe(512);
        expect(pngsize.height).toBe(512);

        const res2 = await fetch(
            'http://localhost:3000/tiles/0/0/0.webp?url=file://localdata/style.json&tileSize=256',
        );
        expect(res2.headers.get('content-type')).toBe('image/webp');
        const webp = new Uint8Array(await res2.arrayBuffer());
        const webpsize = sizeof(webp);
        expect(webpsize.width).toBe(256);
        expect(webpsize.height).toBe(256);

        const res3 = await fetch(
            'http://localhost:3000/tiles/0/0/0.jpeg?url=file://localdata/style.json&quality=50&margin=100',
        );
        expect(res3.headers.get('content-type')).toBe('image/jpeg');
    });

    test('POST /tiles valid', async () => {
        const res = await fetch('http://localhost:3000/tiles/0/0/0.png', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                style: {
                    version: 8,
                    sources: {
                        point: {
                            type: 'geojson',
                            data: {
                                type: 'FeatureCollection',
                                features: [
                                    {
                                        type: 'Feature',
                                        properties: {},
                                        geometry: {
                                            type: 'Point',
                                            coordinates: [140, 40],
                                        },
                                    },
                                ],
                            },
                        },
                    },
                    layers: [
                        {
                            id: 'geojson',
                            type: 'circle',
                            source: 'point',
                            paint: {
                                'circle-radius': 10,
                                'circle-color': '#f00',
                            },
                        },
                    ],
                },
            }),
        });
        expect(res.status).toBe(200);
        expect(res.headers.get('content-type')).toBe('image/png');
        const png = new Uint8Array(await res.arrayBuffer());
        const pngsize = sizeof(png);
        expect(pngsize.width).toBe(512);
        expect(pngsize.height).toBe(512);
    });
    test('GET /tiles render failure -> 500 text/plain', async () => {
        // unresolvable style url: render throws, must not be reported as a
        // client error nor mislabeled with the image content-type set earlier
        const res = await fetch(
            'http://localhost:3000/tiles/0/0/0.png?url=file://localdata/nonexistent.json',
        );
        expect(res.status).toBe(500);
        expect(res.headers.get('content-type')).toMatch(/^text\/plain/);
        expect(await res.text()).toBe('failed to render tile');
    });

    test('POST /tiles with unreachable raster source -> 500', async () => {
        // 5xx/network errors mean the tile's existence is unknown: rendering
        // a transparent tile would let CDNs cache a missing-data tile as if
        // it were valid, so the whole render must fail.
        // 127.0.0.1:1 refuses connection -> getSource throws.
        const res = await fetch('http://localhost:3000/tiles/0/0/0.png', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                style: {
                    version: 8,
                    sources: {
                        r: {
                            type: 'raster',
                            tiles: [
                                'http://127.0.0.1:1/xyz/{z}/{x}/{y}?format=webp',
                            ],
                            tileSize: 256,
                            maxzoom: 22,
                        },
                    },
                    layers: [{ id: 'r', type: 'raster', source: 'r' }],
                },
            }),
        });
        expect(res.status).toBe(500);
        expect(res.headers.get('content-type')).toMatch(/^text\/plain/);
        expect(await res.text()).toBe('failed to render tile');
    });

    test('POST /tiles with 404 raster source -> 200 transparent', async () => {
        // 404 means "no tile there" (normal for sparse tilesets) and must
        // render as a transparent tile, not fail the whole render. The
        // extension-less tile URL also covers the path the old
        // transparent-image fallback could not resolve. The server itself
        // returns 404 for unknown paths.
        const res = await fetch('http://localhost:3000/tiles/0/0/0.png', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                style: {
                    version: 8,
                    sources: {
                        r: {
                            type: 'raster',
                            tiles: [
                                'http://localhost:3000/notfound/{z}/{x}/{y}?format=webp',
                            ],
                            tileSize: 256,
                            maxzoom: 22,
                        },
                    },
                    layers: [{ id: 'r', type: 'raster', source: 'r' }],
                },
            }),
        });
        expect(res.status).toBe(200);
        expect(res.headers.get('content-type')).toBe('image/png');
        const png = new Uint8Array(await res.arrayBuffer());
        const pngsize = sizeof(png);
        expect(pngsize.width).toBe(512);
        expect(pngsize.height).toBe(512);
    });

    test('POST /tiles invalid', async () => {
        const res = await fetch('http://localhost:3000/tiles/0/0/0.png', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                style: {
                    version: 8,
                    sources: {}, // invalid style
                    layers: [
                        {
                            id: 'geojson',
                            type: 'circle',
                            source: 'point',
                            paint: {
                                'circle-radius': 10,
                                'circle-color': '#f00',
                            },
                        },
                    ],
                },
            }),
        });
        expect(res.status).toBe(400);
        expect(await res.text()).toBe('invalid stylejson');
    });

    test('POST /tiles missing/malformed body', async () => {
        // empty object - missing style key (#136)
        const res1 = await fetch('http://localhost:3000/tiles/0/0/0.png', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({}),
        });
        expect(res1.status).toBe(400);
        expect(await res1.text()).toBe('invalid stylejson');

        // non-JSON body
        const res2 = await fetch('http://localhost:3000/tiles/0/0/0.png', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: 'not-json',
        });
        expect(res2.status).toBe(400);
        expect(await res2.text()).toBe('invalid stylejson');

        // null style
        const res3 = await fetch('http://localhost:3000/tiles/0/0/0.png', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ style: null }),
        });
        expect(res3.status).toBe(400);
        expect(await res3.text()).toBe('invalid stylejson');
    });

    test('GET /bbox', async () => {
        const res = await fetch(
            'http://localhost:3000/clip.png?bbox=100,30,150,60&url=file://localdata/style.json',
        );
        expect(res.status).toBe(200);
        expect(res.headers.get('content-type')).toBe('image/png');
        const png = new Uint8Array(await res.arrayBuffer());
        const pngsize = sizeof(png);
        expect(pngsize.width).toBe(1024); // default size is 1024
        expect(pngsize.height).toBe(901); // shorter axis is calculated from bbox aspect ratio

        const res2 = await fetch(
            'http://localhost:3000/clip.webp?bbox=100,30,150,60&url=file://localdata/style.json&quality=50&size=512',
        );
        expect(res2.headers.get('content-type')).toBe('image/webp');
        const webp = new Uint8Array(await res2.arrayBuffer());
        const webpsize = sizeof(webp);
        expect(webpsize.width).toBe(512);
        expect(webpsize.height).toBe(451);
    });

    test('GET /bbox render failure -> 500 text/plain', async () => {
        const res = await fetch(
            'http://localhost:3000/clip.png?bbox=100,30,150,60&url=file://localdata/nonexistent.json',
        );
        expect(res.status).toBe(500);
        expect(res.headers.get('content-type')).toMatch(/^text\/plain/);
        expect(await res.text()).toBe('failed to render tile');
    });

    test('POST /bbox valid', async () => {
        const res = await fetch(
            'http://localhost:3000/clip.png?bbox=100,30,150,60',
            {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    style: {
                        version: 8,
                        sources: {
                            point: {
                                type: 'geojson',
                                data: {
                                    type: 'FeatureCollection',
                                    features: [
                                        {
                                            type: 'Feature',
                                            properties: {},
                                            geometry: {
                                                type: 'Point',
                                                coordinates: [140, 40],
                                            },
                                        },
                                    ],
                                },
                            },
                        },
                        layers: [
                            {
                                id: 'geojson',
                                type: 'circle',
                                source: 'point',
                                paint: {
                                    'circle-radius': 10,
                                    'circle-color': '#f00',
                                },
                            },
                        ],
                    },
                }),
            },
        );
        expect(res.status).toBe(200);
        expect(res.headers.get('content-type')).toBe('image/png');
        const png = new Uint8Array(await res.arrayBuffer());
        const pngsize = sizeof(png);
        expect(pngsize.width).toBe(1024);
        expect(pngsize.height).toBe(901);
    });

    test('POST /bbox missing/malformed body', async () => {
        const res1 = await fetch(
            'http://localhost:3000/clip.png?bbox=100,30,150,60',
            {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({}),
            },
        );
        expect(res1.status).toBe(400);
        expect(await res1.text()).toBe('invalid stylejson');

        const res2 = await fetch(
            'http://localhost:3000/clip.png?bbox=100,30,150,60',
            {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: 'not-json',
            },
        );
        expect(res2.status).toBe(400);
        expect(await res2.text()).toBe('invalid stylejson');
    });

    test('GET /static', async () => {
        const res = await fetch(
            'http://localhost:3000/view/10/56.7/123.4/0/1024x1024.png?url=file://localdata/style.json',
        );
        expect(res.status).toBe(200);
        expect(res.headers.get('content-type')).toBe('image/png');
        const png = new Uint8Array(await res.arrayBuffer());
        const pngsize = sizeof(png);
        expect(pngsize.width).toBe(1024);
        expect(pngsize.height).toBe(1024);

        const res2 = await fetch(
            'http://localhost:3000/view/10/56.7/123.4/0/512x512.webp?url=file://localdata/style.json',
        );
        expect(res2.headers.get('content-type')).toBe('image/webp');
        const webp = new Uint8Array(await res2.arrayBuffer());
        const webpsize = sizeof(webp);
        expect(webpsize.width).toBe(512);
        expect(webpsize.height).toBe(512);
    });

    test('GET /static render failure -> 500 text/plain', async () => {
        const res = await fetch(
            'http://localhost:3000/view/10/56.7/123.4/0/1024x1024.png?url=file://localdata/nonexistent.json',
        );
        expect(res.status).toBe(500);
        expect(res.headers.get('content-type')).toMatch(/^text\/plain/);
        expect(await res.text()).toBe('failed to render static image');
    });

    test('POST /static valid', async () => {
        const res = await fetch(
            'http://localhost:3000/view/10/56.7/123.4/0/1024x1024.png',
            {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    style: {
                        version: 8,
                        sources: {
                            point: {
                                type: 'geojson',
                                data: {
                                    type: 'FeatureCollection',
                                    features: [
                                        {
                                            type: 'Feature',
                                            properties: {},
                                            geometry: {
                                                type: 'Point',
                                                coordinates: [140, 40],
                                            },
                                        },
                                    ],
                                },
                            },
                        },
                        layers: [
                            {
                                id: 'geojson',
                                type: 'circle',
                                source: 'point',
                                paint: {
                                    'circle-radius': 10,
                                    'circle-color': '#f00',
                                },
                            },
                        ],
                    },
                }),
            },
        );
        expect(res.status).toBe(200);
        expect(res.headers.get('content-type')).toBe('image/png');
        const png = new Uint8Array(await res.arrayBuffer());
        const pngsize = sizeof(png);
        expect(pngsize.width).toBe(1024);
        expect(pngsize.height).toBe(1024);
    });

    test('POST /static missing/malformed body', async () => {
        const res1 = await fetch(
            'http://localhost:3000/view/10/56.7/123.4/0/1024x1024.png',
            {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({}),
            },
        );
        expect(res1.status).toBe(400);
        expect(await res1.text()).toBe('invalid stylejson');

        const res2 = await fetch(
            'http://localhost:3000/view/10/56.7/123.4/0/1024x1024.png',
            {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: 'not-json',
            },
        );
        expect(res2.status).toBe(400);
        expect(await res2.text()).toBe('invalid stylejson');
    });
});

describe('custom CRS', () => {
    const style = 'url=file://localdata/style.json';
    const size = async (res: Response) => {
        const { width, height } = sizeof(
            new Uint8Array(await res.arrayBuffer()),
        );
        return [width, height];
    };
    const post = (path: string, body: object) =>
        fetch(`http://localhost:3000${path}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(body),
        });
    const red = {
        version: 8,
        sources: {},
        layers: [
            {
                id: 'bg',
                type: 'background',
                paint: { 'background-color': '#ff0000' },
            },
        ],
    };

    test('GET /crs lists the presets by name', async () => {
        const res = await fetch('http://localhost:3000/crs');
        expect(res.status).toBe(200);
        const list = await res.json();
        expect(list).toHaveLength(36);
        expect(list).toContainEqual({
            id: 'EPSG-6677',
            name: 'JGD2011 / Japan Plane Rectangular CS IX',
        });
    });

    test('GET /crs/{id}', async () => {
        const res = await fetch('http://localhost:3000/crs/EPSG-6677');
        expect(res.status).toBe(200);
        const crs = await res.json();
        expect(crs.projjson.name).toBe(
            'JGD2011 / Japan Plane Rectangular CS IX',
        );
        expect(Object.keys(crs.tileMatrixSet).sort()).toEqual([
            'extentAtZoom0',
            'origin',
        ]);
        expect(
            (await fetch('http://localhost:3000/crs/EPSG-9999')).status,
        ).toBe(404);
    });

    test('GET /tiles?crs=', async () => {
        const res = await fetch(
            `http://localhost:3000/tiles/1/1/0.png?crs=EPSG-6677&${style}`,
        );
        expect(res.status).toBe(200);
        expect(res.headers.get('content-type')).toBe('image/png');
        const projected = new Uint8Array(await res.arrayBuffer());
        expect(sizeof(projected)).toMatchObject({ width: 512, height: 512 });
        // not the Web Mercator tile of the same z/x/y
        const mercator = await fetch(
            `http://localhost:3000/tiles/1/1/0.png?${style}`,
        );
        expect(Buffer.from(projected)).not.toEqual(
            Buffer.from(await mercator.arrayBuffer()),
        );

        const world = await fetch(
            `http://localhost:3000/tiles/0/0/0.webp?crs=EPSG-8857&tileSize=256&${style}`,
        );
        expect(world.status).toBe(200);
        expect(world.headers.get('content-type')).toBe('image/webp');
        expect(await size(world)).toEqual([256, 256]);
    });

    test('GET /clip?crs= takes a bbox in CRS units', async () => {
        const res = await fetch(
            `http://localhost:3000/clip.png?crs=EPSG-6677&bbox=-100000,-100000,100000,0&size=512&${style}`,
        );
        expect(res.status).toBe(200);
        expect(await size(res)).toEqual([512, 256]);
    });

    test('GET /view?crs=', async () => {
        const res = await fetch(
            `http://localhost:3000/view/3/35.68/139.77/30/512x256.png?crs=EPSG-6677&${style}`,
        );
        expect(res.status).toBe(200);
        expect(await size(res)).toEqual([512, 256]);
    });

    test('POST with a CRS in the body, over ?crs=', async () => {
        const crs = await (
            await fetch('http://localhost:3000/crs/EPSG-6677')
        ).json();
        for (const path of [
            '/tiles/0/0/0.png',
            '/tiles/0/0/0.png?crs=EPSG-8857',
            '/tiles/0/0/0.png?crs=unknown',
        ]) {
            const res = await post(path, { style: red, crs });
            expect(res.status, path).toBe(200);
            expect(await size(res), path).toEqual([512, 512]);
        }
        const viaQuery = await post('/tiles/0/0/0.png?crs=EPSG-6677', {
            style: red,
        });
        expect(viaQuery.status).toBe(200);
    });

    test('invalid CRSs answer 400', async () => {
        const unknown = await fetch(
            `http://localhost:3000/tiles/0/0/0.png?crs=EPSG-9999&${style}`,
        );
        expect(unknown.status).toBe(400);
        expect(await unknown.text()).toBe('unknown crs');

        // past the grid of zoom 0
        const outside = await fetch(
            `http://localhost:3000/tiles/0/1/0.png?crs=EPSG-6677&${style}`,
        );
        expect(outside.status).toBe(400);

        const malformed = await post('/tiles/0/0/0.png', {
            style: red,
            crs: { projjson: 'EPSG:6677' },
        });
        expect(malformed.status).toBe(400);
        expect(await malformed.text()).toBe('invalid crs');

        const unsupported = await post('/tiles/0/0/0.png', {
            style: red,
            crs: {
                projjson: { type: 'Foo' },
                tileMatrixSet: { origin: [0, 0], extentAtZoom0: 1 },
            },
        });
        expect(unsupported.status).toBe(400);
    });
});
