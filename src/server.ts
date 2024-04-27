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

        // query params
        const tileSize = Number(c.req.query('tileSize') ?? 512);
        const url = c.req.query('url') ?? null;
        const quality = Number(c.req.query('quality') ?? 80);

        if (url === null) return c.body('url is required', 400);

        // load style.json
        const buf = await getSource(url, options.cache);
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
        switch (ext) {
            case 'jpg':
            case 'jpeg':
                imgBuf = await image.jpeg({ quality }).toBuffer();
                break;
            case 'webp':
                imgBuf = await image.webp({ quality }).toBuffer();
                break;
            case 'png':
                imgBuf = await image.png().toBuffer();
                break;
            default:
                return c.body('Invalid extension', 400);
        }

        return c.body(imgBuf, 200, { 'Content-Type': `image/${ext}` });
    });

    if (options.debug) hono.get('/debug', getDebugPage);

    return {
        start: () => serve({ port: options.port, fetch: hono.fetch }),
    };
}

export { initServer, type InitServerOptions };
