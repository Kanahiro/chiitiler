import { Hono } from 'hono';
import { serve } from '@hono/node-server';
import {
    type StyleSpecification,
    validateStyleMin,
} from '@maplibre/maplibre-gl-style-spec';

import { type Cache } from '../cache/index.js';
import { getDebugPage } from '../debug.js';
import { tileResponse } from './response.js';

function isValidStylejson(stylejson: any): stylejson is StyleSpecification {
    return validateStyleMin(stylejson).length === 0;
}

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
        const url = c.req.query('url') ?? null;
        if (url === null) return c.body('url is required', 400);
        const res = await tileResponse(c, {
            mode: 'url',
            cache: options.cache,
            url,
        });
        return res;
    });

    hono.post('/tiles/:z/:x/:y_ext', async (c) => {
        // body
        const { style } = await c.req.json();
        if (!isValidStylejson(style)) return c.body('invalid stylejson', 400);
        const res = await tileResponse(c, {
            mode: 'style',
            cache: options.cache,
            style,
        });
        return res;
    });

    return {
        start: () => serve({ port: options.port, fetch: hono.fetch }),
    };
}

export { initServer, type InitServerOptions };
