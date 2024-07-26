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
});
