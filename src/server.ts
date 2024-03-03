/// <reference lib="dom" />
// for using native fetch in TypeScript

import { Hono } from 'hono';
import { serve } from '@hono/node-server';
import sharp from 'sharp';
import type { StyleSpecification } from 'maplibre-gl';

import { getRenderer } from './tiling.js';
import { type Cache } from './cache/index.js';
import { getDebugPage } from './debug.js';
import { getSource } from './source.js';

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

        if (['png', 'jpg', 'webp'].indexOf(ext) === -1)
            return c.body('Invalid extension', 400);

        // query params
        const tileSize = Number(c.req.query('tileSize') ?? 512);
        const url = c.req.query('url') ?? null;

        if (url === null) return c.body('url is required', 400);

        // load style.json, without cache
        const buf = await getSource(url);
        if (buf === null) return c.body('Invalid url', 400);
        const style = (await JSON.parse(buf.toString())) as StyleSpecification;

        const { render } = getRenderer(style, {
            tileSize,
            cache: options.cache,
        });
        const pixels = await render(z, x, y);

        const image = sharp(pixels, {
            raw: {
                width: tileSize,
                height: tileSize,
                channels: 4,
            },
        });

        let imgBuf: Buffer;
        if (ext === 'jpg') {
            imgBuf = await image.jpeg({ quality: 20 }).toBuffer();
        } else if (ext === 'webp') {
            imgBuf = await image.webp({ quality: 100 }).toBuffer();
        } else {
            imgBuf = await image.png().toBuffer();
        }

        return c.body(imgBuf, 200, { 'Content-Type': `image/${ext}` });
    });

    if (options.debug) hono.get('/debug', getDebugPage);

    return {
        start: () => serve({ port: options.port, fetch: hono.fetch }),
    };
}

export { initServer, type InitServerOptions };
