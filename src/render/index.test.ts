import * as http from 'http';

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { getRenderedClip } from './index.js';
import { noneCache } from '../cache/index.js';

// Regression: empty-body raster tiles (204/404/empty 200) must not fail the
// whole render. Before the fix, a zero-length buffer was handed to mbgl as
// image data and map.render() rejected. Now such tiles render as transparent.

function startServer(
    handler: http.RequestListener,
): Promise<{ port: number; close: () => Promise<void> }> {
    return new Promise((resolve) => {
        const server = http.createServer(handler);
        server.listen(0, () => {
            const port = (server.address() as { port: number }).port;
            resolve({
                port,
                close: () =>
                    new Promise((r) => server.close(() => r(undefined))),
            });
        });
    });
}

function rasterStyle(port: number): StyleSpecification {
    return {
        version: 8,
        sources: {
            // extension-less tile URL: exercises the path where the old
            // handleFileExt fallback could not pick a transparent image.
            r: {
                type: 'raster',
                tiles: [`http://localhost:${port}/xyz/{z}/{x}/{y}?format=webp`],
                tileSize: 256,
                maxzoom: 22,
            },
        },
        layers: [{ id: 'r', type: 'raster', source: 'r' }],
    };
}

async function renderAgainst(handler: http.RequestListener) {
    const { port, close } = await startServer(handler);
    try {
        const sharp = await getRenderedClip({
            stylejson: rasterStyle(port),
            bbox: [139.5, 35.5, 140.0, 36.0],
            size: 256,
            cache: noneCache(),
            ext: 'png',
            quality: 80,
        });
        return await sharp.toBuffer();
    } finally {
        await close();
    }
}

describe('getRenderedClip with empty raster tiles', () => {
    it('204 No Content → resolves', async () => {
        const buf = await renderAgainst((_req, res) => {
            res.writeHead(204);
            res.end();
        });
        expect(buf.length).toBeGreaterThan(0);
    });

    it('404 Not Found → resolves', async () => {
        const buf = await renderAgainst((_req, res) => {
            res.writeHead(404);
            res.end();
        });
        expect(buf.length).toBeGreaterThan(0);
    });

    it('empty-body 200 → resolves', async () => {
        const buf = await renderAgainst((_req, res) => {
            res.writeHead(200, { 'Content-Type': 'image/webp' });
            res.end();
        });
        expect(buf.length).toBeGreaterThan(0);
    });
});
