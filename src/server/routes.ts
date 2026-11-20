import { createHash } from 'node:crypto';

import type { Context, Hono } from 'hono';
import { bodyLimit } from 'hono/body-limit';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { isSupportedFormat, isValidStylejson } from './utils.js';
import {
    Chiitiler,
    type Crs,
    encode,
    type RenderedImage,
    type SupportedFormat,
} from '../render/index.js';
import type { Cache } from '../cache/index.js';
import { log } from '../log.js';

type RouteOptions = {
    cache: Cache;
    /** The largest image side in pixels. */
    maxSize?: number;
    /** `Cache-Control` of rendered images, if any. */
    cacheControl?: string;
    /** CRSs requests can render in with `?crs=<id>`. */
    crs?: CrsCatalog;
    /** Renders in progress (waiting included) beyond which requests answer 503. Default: no limit. */
    maxRenders?: number;
};

/** CRSs by id. */
type CrsCatalog = {
    ids: string[];
    /** The CRS of `id`, or undefined for an unknown id. */
    get: (id: string) => Promise<Crs | undefined>;
};

/** Bodies of POST requests (styles, inline GeoJSON included). */
const MAX_BODY_BYTES = 16 * 1024 * 1024;

type Input = { stylejson: string | StyleSpecification; crs?: Crs };

const isObject = (v: unknown): v is object =>
    v !== null && typeof v === 'object';

/**
 * `GET path?url=<style URL>` and `POST path` with `{ "style": {...}, "crs"?: {...} }`, both
 * rendering what `parse` reads from the request (or answering its error response) as an image, in
 * the CRS of the body, or else of `?crs=<id>`, if any.
 */
function imageRoute<P extends { ext: SupportedFormat }>(
    app: Hono,
    path: string,
    {
        cacheControl,
        crs: catalog,
        maxRenders = Number.POSITIVE_INFINITY,
        renders,
    }: RouteOptions & { renders: { count: number } },
    failure: string,
    parse: (c: Context, crs: Crs | undefined) => P | Response,
    render: (
        params: P & Pick<Input, 'stylejson'>,
        crs: Crs | undefined,
    ) => Promise<RenderedImage>,
) {
    const handle = async (
        c: Context,
        read: (c: Context) => Promise<Input | Response>,
    ) => {
        const input = await read(c);
        if (input instanceof Response) return input;
        let crs = input.crs;
        const id = c.req.query('crs');
        if (crs === undefined && id !== undefined) {
            crs = await catalog?.get(id);
            if (crs === undefined) return c.body('unknown crs', 400);
        }
        const params = parse(c, crs);
        if (params instanceof Response) return params;
        // Shed load rather than queue without bound: waiting requests hold memory and sockets, and
        // would answer too late anyway.
        if (renders.count >= maxRenders)
            return c.text('too many renders in progress', 503, {
                'Retry-After': '1',
            });
        renders.count++;
        try {
            const image = await encode(
                await render({ ...params, stylejson: input.stylejson }, crs),
                params.ext,
                Number(c.req.query('quality') ?? 100),
            );
            const type = params.ext === 'jpg' ? 'jpeg' : params.ext;
            const headers: Record<string, string> = {
                'Content-Type': `image/${type}`,
                ...(cacheControl ? { 'Cache-Control': cacheControl } : {}),
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
            log('ERROR', 'render failed', {
                method: c.req.method,
                path: c.req.path,
                error: e,
            });
            return c.text(failure, 500);
        } finally {
            renders.count--;
        }
    };
    app.get(path, (c) =>
        handle(c, async () => {
            const url = c.req.query('url');
            return url === undefined
                ? c.body('url is required', 400)
                : { stylejson: url };
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
                if (!isValidStylejson(body?.style))
                    return c.body('invalid stylejson', 400);
                const crs: unknown = body.crs;
                // the rest is checked by the renderer (400 when invalid)
                if (
                    crs !== undefined &&
                    !(
                        isObject(crs) &&
                        'projjson' in crs &&
                        isObject(crs.projjson) &&
                        (!('tileMatrixSet' in crs) ||
                            crs.tileMatrixSet === undefined ||
                            isObject(crs.tileMatrixSet))
                    )
                )
                    return c.body('invalid crs', 400);
                return { stylejson: body.style, crs: crs as Crs | undefined };
            }),
    );
}

/** A path parameter (routes always have theirs). */
function param(c: Context, name: string): string {
    return c.req.param(name) ?? '';
}

/**
 * `name.ext` or `name@<n>x.ext` (the retina suffix of Mapbox and TileServer GL: `n` device pixels per
 * CSS pixel, the same area in `n` times the pixels) → name, pixelRatio and ext if supported.
 */
function filename(
    c: Context,
    nameExt: string,
): { name: string; pixelRatio: number; ext: SupportedFormat } | Response {
    const [, name, n, ext] =
        nameExt.match(/^(.*?)(?:@(\d+)x)?\.([^.]*)$/) ?? [];
    if (!isSupportedFormat(ext)) return c.body('invalid format', 400);
    const pixelRatio = n === undefined ? 1 : Number(n);
    if (pixelRatio < 1) return c.body('invalid pixel ratio', 400);
    return { name, pixelRatio, ext };
}

function registerRoutes(app: Hono, routeOptions: RouteOptions) {
    // one count for every image route
    const options = { ...routeOptions, renders: { count: 0 } };
    const chiitiler = new Chiitiler(options);
    // CRS definitions, for clients showing the tiles (MapLibre GL JS's addProjection takes the grid)
    app.get('/crs', async (c) => {
        const { crs } = options;
        if (!crs) return c.json([]);
        const list = await Promise.all(
            crs.ids.map(async (id) => {
                const projjson = (await crs.get(id))?.projjson;
                return {
                    id,
                    name: (projjson as { name?: string })?.name ?? id,
                };
            }),
        );
        return c.json(list);
    });
    app.get('/crs/:id', async (c) => {
        const crs = await options.crs?.get(param(c, 'id'));
        return crs ? c.json(crs) : c.body('unknown crs', 404);
    });
    imageRoute(
        app,
        '/tiles/:z/:x/:y_ext',
        options,
        'failed to render tile',
        (c, crs) => {
            const z = Number(param(c, 'z'));
            const x = Number(param(c, 'x'));
            const file = filename(c, param(c, 'y_ext'));
            if (file instanceof Response) return file;
            const y = Number(file.name);
            // a CRS's grid need not be 2^z x 2^z: the renderer checks its tiles
            const n = crs ? Infinity : 2 ** z;
            if (x < 0 || y < 0 || z < 0 || x >= n || y >= n)
                return c.body('invalid xyz', 400);
            return {
                z,
                x,
                y,
                ext: file.ext,
                pixelRatio: file.pixelRatio,
                tileSize: Number(c.req.query('tileSize') ?? 512),
                margin: Number(c.req.query('margin') ?? 0),
            };
        },
        (p, crs) =>
            crs
                ? chiitiler.renderProjectedTile({ ...p, crs })
                : chiitiler.renderTile(p),
    );

    imageRoute(
        app,
        // only clip.<ext> and clip@<n>x.<ext>: other paths are not found
        '/:file{clip(?:@\\d+x)?\\.[^/]*}',
        options,
        'failed to render tile',
        (c) => {
            const file = filename(c, param(c, 'file'));
            if (file instanceof Response) return file;
            const bbox = c.req.query('bbox'); // ?bbox=minx,miny,maxx,maxy
            if (bbox === undefined) return c.body('bbox is required', 400);
            const [minx, miny, maxx, maxy] = bbox.split(',').map(Number);
            if (!(minx < maxx && miny < maxy))
                return c.body('invalid bbox', 400);
            return {
                ext: file.ext,
                pixelRatio: file.pixelRatio,
                bbox: [minx, miny, maxx, maxy] as [
                    number,
                    number,
                    number,
                    number,
                ],
                size: Number(c.req.query('size') ?? 1024),
            };
        },
        (p, crs) =>
            crs
                ? chiitiler.renderProjectedBbox({ ...p, crs })
                : chiitiler.renderBbox(p),
    );

    imageRoute(
        app,
        '/view/:zoom/:lat/:lon/:bearing/:dimensions_ext',
        options,
        'failed to render static image',
        (c) => {
            const file = filename(c, param(c, 'dimensions_ext'));
            if (file instanceof Response) return file;
            const [width, height] = file.name.split('x').map(Number);
            if (Number.isNaN(width) || Number.isNaN(height))
                return c.body('invalid dimensions', 400);
            const [zoom, lat, lon, bearing] = (
                ['zoom', 'lat', 'lon', 'bearing'] as const
            ).map((name) => Number(param(c, name)));
            const within = (v: number, min: number, max: number) =>
                v >= min && v <= max; // false for NaN
            if (
                !within(lat, -90, 90) ||
                !within(lon, -180, 180) ||
                !within(zoom, 0, 24) ||
                !within(bearing, 0, 360)
            )
                return c.body('invalid view', 400);
            return {
                ext: file.ext,
                pixelRatio: file.pixelRatio,
                center: [lon, lat] as [number, number],
                zoom,
                bearing,
                width,
                height,
            };
        },
        (p, crs) =>
            crs
                ? chiitiler.renderProjectedView({ ...p, crs })
                : chiitiler.renderView(p),
    );
}

export { type CrsCatalog, registerRoutes, type RouteOptions };
