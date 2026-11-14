import { createHash } from 'node:crypto';

import type { Context, Hono } from 'hono';
import { bodyLimit } from 'hono/body-limit';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { isSupportedFormat, isValidStylejson } from './utils.js';
import {
    renderView,
    renderBbox,
    renderTile,
    type SupportedFormat,
} from '../render/index.js';
import type { Cache } from '../cache/index.js';

type RouteOptions = {
    cache: Cache;
    /** The largest image side in pixels. */
    maxSize?: number;
    /** `Cache-Control` of rendered images, if any. */
    cacheControl?: string;
};

/** Bodies of POST requests (styles, inline GeoJSON included). */
const MAX_BODY_BYTES = 16 * 1024 * 1024;

type Common = {
    stylejson: string | StyleSpecification;
    ext: SupportedFormat;
    quality: number;
    cache: Cache;
    maxSize?: number;
};

/**
 * `GET path?url=<style URL>` and `POST path` with `{ "style": {...} }`, both rendering what
 * `parse` reads from the request (or answering its error response) as an image.
 */
function imageRoute<P extends { ext: SupportedFormat }>(
    app: Hono,
    path: string,
    options: RouteOptions,
    failure: string,
    parse: (c: Context) => P | Response,
    render: (params: P & Common) => Promise<Uint8Array<ArrayBuffer>>,
) {
    const handle = async (
        c: Context,
        style: (c: Context) => Promise<Common['stylejson'] | Response>,
    ) => {
        const params = parse(c);
        if (params instanceof Response) return params;
        const stylejson = await style(c);
        if (stylejson instanceof Response) return stylejson;
        try {
            const image = await render({
                ...params,
                stylejson,
                quality: Number(c.req.query('quality') ?? 100),
                cache: options.cache,
                maxSize: options.maxSize,
            });
            const type = params.ext === 'jpg' ? 'jpeg' : params.ext;
            const headers: Record<string, string> = {
                'Content-Type': `image/${type}`,
                ...(options.cacheControl
                    ? { 'Cache-Control': options.cacheControl }
                    : {}),
            };
            if (c.req.method !== 'GET') return c.body(image, 200, headers);
            // Lets clients and caches revalidate (304 without the image). Hashed here, from the
            // bytes at hand: hono/etag copies and reads the response stream, ~10% of a tile's time.
            headers.ETag = `"${createHash('sha1').update(image).digest('base64url')}"`;
            const match = c.req.header('If-None-Match');
            if (
                match
                    ?.split(',')
                    .some((t) => t.trim().replace(/^W\//, '') === headers.ETag)
            )
                return c.body(null, 304, headers);
            return c.body(image, 200, headers);
        } catch (e) {
            // out-of-range parameters the route does not check itself
            if (e instanceof RangeError) return c.text(e.message, 400);
            console.error(`render error: ${e}`);
            return c.text(failure, 500);
        }
    };
    app.get(path, (c) =>
        handle(c, async () => {
            const url = c.req.query('url');
            return url ?? c.body('url is required', 400);
        }),
    );
    app.post(
        path,
        bodyLimit({
            maxSize: MAX_BODY_BYTES,
            onError: (c) => c.text('request body too large', 413),
        }),
        (c) =>
            handle(c, async () => {
                const body = await c.req.json().catch(() => null);
                return isValidStylejson(body?.style)
                    ? body.style
                    : c.body('invalid stylejson', 400);
            }),
    );
}

/** A path parameter (routes always have theirs). */
function param(c: Context, name: string): string {
    return c.req.param(name) ?? '';
}

/** `name.ext` → ext if supported. */
function format(c: Context, nameExt: string): SupportedFormat | Response {
    const ext = nameExt.split('.')[1];
    return isSupportedFormat(ext) ? ext : c.body('invalid format', 400);
}

function registerRoutes(app: Hono, options: RouteOptions) {
    imageRoute(
        app,
        '/tiles/:z/:x/:y_ext',
        options,
        'failed to render tile',
        (c) => {
            const z = Number(param(c, 'z'));
            const x = Number(param(c, 'x'));
            const y = Number(param(c, 'y_ext').split('.')[0]);
            if (x < 0 || y < 0 || z < 0 || x >= 2 ** z || y >= 2 ** z)
                return c.body('invalid xyz', 400);
            const ext = format(c, param(c, 'y_ext'));
            if (ext instanceof Response) return ext;
            return {
                z,
                x,
                y,
                ext,
                tileSize: Number(c.req.query('tileSize') ?? 512),
                margin: Number(c.req.query('margin') ?? 0),
            };
        },
        renderTile,
    );

    imageRoute(
        app,
        // only clip.<ext>: other paths are not found
        '/:file{clip\\.[^/]*}',
        options,
        'failed to render tile',
        (c) => {
            const ext = format(c, param(c, 'file'));
            if (ext instanceof Response) return ext;
            const bbox = c.req.query('bbox'); // ?bbox=minx,miny,maxx,maxy
            if (bbox === undefined) return c.body('bbox is required', 400);
            const [minx, miny, maxx, maxy] = bbox.split(',').map(Number);
            if (!(minx < maxx && miny < maxy))
                return c.body('invalid bbox', 400);
            return {
                ext,
                bbox: [minx, miny, maxx, maxy] as [
                    number,
                    number,
                    number,
                    number,
                ],
                size: Number(c.req.query('size') ?? 1024),
            };
        },
        renderBbox,
    );

    imageRoute(
        app,
        '/camera/:zoom/:lat/:lon/:bearing/:pitch/:dimensions_ext',
        options,
        'failed to render static image',
        (c) => {
            const ext = format(c, param(c, 'dimensions_ext'));
            if (ext instanceof Response) return ext;
            const [width, height] = param(c, 'dimensions_ext')
                .split('.')[0]
                .split('x')
                .map(Number);
            if (Number.isNaN(width) || Number.isNaN(height))
                return c.body('invalid dimensions', 400);
            const [zoom, lat, lon, bearing, pitch] = (
                ['zoom', 'lat', 'lon', 'bearing', 'pitch'] as const
            ).map((name) => Number(param(c, name)));
            const within = (v: number, min: number, max: number) =>
                v >= min && v <= max; // false for NaN
            if (
                !within(lat, -90, 90) ||
                !within(lon, -180, 180) ||
                !within(zoom, 0, 24) ||
                !within(bearing, 0, 360) ||
                !within(pitch, 0, 180)
            )
                return c.body('invalid camera', 400);
            // the path keeps 1.x's pitch, but the renderer is 2D
            if (pitch !== 0) return c.body('pitch is not supported', 400);
            return {
                ext,
                center: [lon, lat] as [number, number],
                zoom,
                bearing,
                width,
                height,
            };
        },
        renderView,
    );
}

export { registerRoutes, type RouteOptions };
