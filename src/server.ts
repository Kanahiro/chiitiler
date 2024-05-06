/// <reference lib="dom" />
// for using native fetch in TypeScript

import { Hono } from 'hono';
import { stream } from 'hono/streaming';
import { serve } from '@hono/node-server';
import sharp from 'sharp';

import { renderTile } from './render/index.js';
import { type Cache } from './cache/index.js';
import { getDebugPage } from './debug.js';

type InitServerOptions = {
    cache: Cache;
    port: number;
    debug: boolean;
};


function initServer(options: InitServerOptions) {
    const hono = new Hono();
    hono.get('/health', (c) => c.body('OK', 200));

    hono.get('/tiles/:z/:x/:y_ext', async (c) => {
        // path params
        const z = Number(c.req.param('z'));
        const x = Number(c.req.param('x'));
        let [_y, ext] = c.req.param('y_ext').split('.');
        const y = Number(_y);

        // query params
        const tileSize = Number(c.req.query('tileSize') ?? 512);
        const url = c.req.query('url') ?? null;
        const quality = Number(c.req.query('quality') ?? 80);

        if (url === null) return c.body('url is required', 400);

        const pixels = await renderTile(url, z, x, y, {
            tileSize,
            cache: options.cache,
        });

        let pipeline: sharp.Sharp;
        switch (ext) {
            case 'png':
                pipeline = sharp(pixels, {
                    raw: {
                        width: tileSize,
                        height: tileSize,
                        channels: 4,
                    },
                }).png({
                    effort: 1
                });
                break;
            case 'jpeg':
            case 'jpg':
                pipeline = sharp(pixels, {
                    raw: {
                        width: tileSize,
                        height: tileSize,
                        channels: 4,
                    },
                }).jpeg({
                    quality
                });
                break;
            case 'webp':
                pipeline = sharp(pixels, {
                    raw: {
                        width: tileSize,
                        height: tileSize,
                        channels: 4,
                    },
                }).webp({
                    quality, effort: 1
                });
                break;
            default:
                return c.body('unsupported image format', 400);
        }


        c.header('Content-Type', `image/${ext}`);
        return stream(c, async (stream) => {
            for await (const chunk of pipeline) {
                stream.write(chunk);
            }
        });
    });

    if (options.debug) hono.get('/debug', getDebugPage);

    return {
        start: () => serve({ port: options.port, fetch: hono.fetch }),
    };
}

export { initServer, type InitServerOptions };
