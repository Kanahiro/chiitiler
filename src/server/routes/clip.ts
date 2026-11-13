import { Hono } from 'hono';
import { isSupportedFormat, isValidStylejson } from '../utils.js';
import { getRenderedClip } from '../../render/index.js';
import type { Cache } from '../../cache/index.js';

function createClipRouter(options: { cache: Cache }) {
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

            c.header('Content-Type', `image/${ext === 'jpg' ? 'jpeg' : ext}`);

            try {
                const buf = await getRenderedClip({
                    stylejson: url,
                    bbox: [minx, miny, maxx, maxy],
                    size,
                    cache: options.cache,
                    ext,
                    quality,
                });

                return c.body(buf);
            } catch (e) {
                // out-of-range parameters the route does not check itself
                if (e instanceof RangeError) return c.text(e.message, 400);
                console.error(`render error: ${e}`);
                // c.text overrides the image/* Content-Type set above; a render
                // failure is a server-side error so respond 500, not 400.
                return c.text('failed to render tile', 500);
            }
        })
        .post('/:filename_ext', async (c) => {
            // body
            const body = await c.req.json().catch(() => null);
            if (!isValidStylejson(body?.style))
                return c.body('invalid stylejson', 400);
            const { style } = body;

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

            c.header('Content-Type', `image/${ext === 'jpg' ? 'jpeg' : ext}`);

            try {
                const buf = await getRenderedClip({
                    stylejson: style,
                    bbox: [minx, miny, maxx, maxy],
                    size,
                    cache: options.cache,
                    ext,
                    quality,
                });

                return c.body(buf);
            } catch (e) {
                // out-of-range parameters the route does not check itself
                if (e instanceof RangeError) return c.text(e.message, 400);
                console.error(`render error: ${e}`);
                // c.text overrides the image/* Content-Type set above; a render
                // failure is a server-side error so respond 500, not 400.
                return c.text('failed to render tile', 500);
            }
        });
    return clip;
}

export { createClipRouter };
