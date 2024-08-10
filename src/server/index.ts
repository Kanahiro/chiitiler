import { Hono } from 'hono/quick';
import { serve } from '@hono/node-server';
import {
    type StyleSpecification,
    validateStyleMin,
} from '@maplibre/maplibre-gl-style-spec';

import { type Cache } from '../cache/index.js';
import { getDebugPage, getEditorgPage } from './debug.js';
import {
    getRenderedTileBuffer,
    getRenderedBboxBuffer,
    type SupportedFormat,
} from '../render/index.js';

function isValidStylejson(stylejson: any): stylejson is StyleSpecification {
    return validateStyleMin(stylejson).length === 0;
}

function isValidXyz(x: number, y: number, z: number) {
    if (x < 0 || y < 0 || z < 0) return false;
    if (x >= 2 ** z || y >= 2 ** z) return false;
    return true;
}

function isSupportedFormat(ext: string): ext is SupportedFormat {
    return ['png', 'jpeg', 'jpg', 'webp'].includes(ext);
}

type InitServerOptions = {
    cache: Cache;
    port: number;
    debug: boolean;
};

type InitializedServer = {
    app: Hono;
    tiles: Hono;
    clip: Hono;
    start: () => void;
};

function initServer(options: InitServerOptions): InitializedServer {
    const tiles = new Hono()
        .get('/:z/:x/:y_ext', async (c) => {
            const url = c.req.query('url');
            if (url === undefined) return c.body('url is required', 400);

            // path params
            const z = Number(c.req.param('z'));
            const x = Number(c.req.param('x'));
            let [_y, ext] = c.req.param('y_ext').split('.');
            const y = Number(_y);

            if (!isValidXyz(x, y, z)) return c.body('invalid xyz', 400);
            if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

            // query params
            const tileSize = Number(c.req.query('tileSize') ?? 512);
            const quality = Number(c.req.query('quality') ?? 100);
            const margin = Number(c.req.query('margin') ?? 0);

            let buf: Buffer;
            try {
                buf = await getRenderedTileBuffer({
                    stylejson: url,
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
            return c.body(buf);
        })
        .post('/:z/:x/:y_ext', async (c) => {
            // body
            const { style } = await c.req.json();
            if (!isValidStylejson(style))
                return c.body('invalid stylejson', 400);

            // path params
            const z = Number(c.req.param('z'));
            const x = Number(c.req.param('x'));
            let [_y, ext] = c.req.param('y_ext').split('.');
            const y = Number(_y);

            if (!isValidXyz(x, y, z)) return c.body('invalid xyz', 400);
            if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

            // query params
            const tileSize = Number(c.req.query('tileSize') ?? 512);
            const quality = Number(c.req.query('quality') ?? 100);
            const margin = Number(c.req.query('margin') ?? 0);

            let buf: Buffer;
            try {
                buf = await getRenderedTileBuffer({
                    stylejson: style,
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
            return c.body(buf);
        });

    const clip = new Hono()
        .get('/:filename_ext', async (c) => {
            // path params
            const [filename, ext] = c.req.param('filename_ext').split('.');
            if (filename !== 'clip') return c.body('not found', 404);
            if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

            // query params
            const bbox = c.req.query('bbox'); // ?bbox=minx,miny,maxx,maxy
            if (bbox === undefined) return c.body('bbox is required', 400);
            const [minx, miny, maxx, maxy] = bbox.split(',').map(Number);
            if (minx >= maxx || miny >= maxy)
                return c.body('invalid bbox', 400);

            const url = c.req.query('url');
            if (url === undefined) return c.body('url is required', 400);
            const quality = Number(c.req.query('quality') ?? 100);
            const size = Number(c.req.query('size') ?? 1024);

            try {
                const buf = await getRenderedBboxBuffer({
                    stylejson: url,
                    bbox: [minx, miny, maxx, maxy],
                    size,
                    cache: options.cache,
                    ext,
                    quality,
                });
                c.header('Content-Type', `image/${ext}`);
                return c.body(buf);
            } catch (e) {
                console.error(`render error: ${e}`);
                return c.body('failed to render bbox', 400);
            }
        })
        .post('/:filename_ext', async (c) => {
            // body
            const { style } = await c.req.json();
            if (!isValidStylejson(style))
                return c.body('invalid stylejson', 400);

            // path params
            const [filename, ext] = c.req.param('filename_ext').split('.');
            if (filename !== 'clip') return c.body('not found', 404);
            if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

            // query params
            const bbox = c.req.query('bbox'); // ?bbox=minx,miny,maxx,maxy
            if (bbox === undefined) return c.body('bbox is required', 400);
            const [minx, miny, maxx, maxy] = bbox.split(',').map(Number);
            if (minx >= maxx || miny >= maxy)
                return c.body('invalid bbox', 400);

            const quality = Number(c.req.query('quality') ?? 100);
            const size = Number(c.req.query('size') ?? 1024);

            try {
                const buf = await getRenderedBboxBuffer({
                    stylejson: style,
                    bbox: [minx, miny, maxx, maxy],
                    size,
                    cache: options.cache,
                    ext,
                    quality,
                });
                c.header('Content-Type', `image/${ext}`);
                return c.body(buf);
            } catch (e) {
                console.error(`render error: ${e}`);
                return c.body('failed to render bbox', 400);
            }
        });

    const hono = new Hono();
    if (options.debug) {
        hono.get('/debug', getDebugPage);
        hono.get('/editor', getEditorgPage);
    }
    hono.get('/health', (c) => c.text('OK'));
    hono.route('/tiles', tiles);
    hono.route('/', clip);

    return {
        app: hono,
        tiles,
        clip,
        start: () => serve({ port: options.port, fetch: hono.fetch }),
    };
}

export { initServer, type InitServerOptions };
