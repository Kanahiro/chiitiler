/// <reference lib="dom" />
// for using native fetch in TypeScript

import { Hono } from 'hono';
import { stream } from 'hono/streaming';
import { serve } from '@hono/node-server';
import sharp from 'sharp';

import { renderTilePipeline } from './render/index.js';
import { type Cache } from './cache/index.js';
import { getDebugPage } from './debug.js';

type InitServerOptions = {
    cache: Cache;
    port: number;
    debug: boolean;
};

function initServer(options: InitServerOptions) {
    const hono = new Hono();
    if (options.debug) hono.get('/debug', getDebugPage);
    hono.get('/health', (c) => c.text('OK'));

    hono.get('/tiles/:z/:x/:y_ext', async (c) => {
        // path params
        const z = Number(c.req.param('z'));
        const x = Number(c.req.param('x'));
        let [_y, ext] = c.req.param('y_ext').split('.');
        const y = Number(_y);

        // query params
        const tileSize = Number(c.req.query('tileSize') ?? 512);
        const url = c.req.query('url') ?? null;
        const quality = Number(c.req.query('quality') ?? 100);
        const margin = Number(c.req.query('margin') ?? 0);

        if (url === null) return c.body('url is required', 400);

        let pipeline: sharp.Sharp;
        try {
            pipeline = await renderTilePipeline({
                url,
                z,
                x,
                y,
                tileSize,
                cache: options.cache,
                margin,
                ext,
                quality,
            });
        } catch (e) {
            console.error(`render error: ${e}`);
            return c.body('failed to render tile', 400);
        }

        c.header('Content-Type', `image/${ext}`);
        return stream(c, async (stream) => {
            for await (const chunk of pipeline) {
                stream.write(chunk);
            }
        });
    });

    return {
        start: () => serve({ port: options.port, fetch: hono.fetch }),
    };
}

export { initServer, type InitServerOptions };
