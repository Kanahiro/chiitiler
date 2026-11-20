import { createHash } from 'node:crypto';

import type { Context, Hono } from 'hono';
import { bodyLimit } from 'hono/body-limit';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { isSupportedFormat, isValidStylejson } from './utils.js';
import { Chiitiler, type Crs, type SupportedFormat } from '../render/index.js';
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
};

/** CRSs by id. */
type CrsCatalog = {
    ids: string[];
    /** The CRS of `id`, or undefined for an unknown id. */
    get: (id: string) => Promise<Crs | undefined>;
};

/** Bodies of POST requests (styles, inline GeoJSON included). */
const MAX_BODY_BYTES = 16 * 1024 * 1024;

type Common = {
    stylejson: string | StyleSpecification;
    ext: SupportedFormat;
    quality: number;
};

type Input = { stylejson: Common['stylejson']; crs?: Crs };

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
    { cacheControl, crs: catalog }: RouteOptions,
    failure: string,
    parse: (c: Context, crs: Crs | undefined) => P | Response,
    render: (
        params: P & Common,
        crs: Crs | undefined,
    ) => Promise<Uint8Array<ArrayBuffer>>,
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
        try {
            const image = await render(
                {
                    ...params,
                    stylejson: input.stylejson,
                    quality: Number(c.req.query('quality') ?? 100),
                },
                crs,
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

/** `name.ext` → ext if supported. */
function format(c: Context, nameExt: string): SupportedFormat | Response {
    const ext = nameExt.split('.')[1];
    return isSupportedFormat(ext) ? ext : c.body('invalid format', 400);
}

function registerRoutes(app: Hono, options: RouteOptions) {
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
            const y = Number(param(c, 'y_ext').split('.')[0]);
            // a CRS's grid need not be 2^z x 2^z: the renderer checks its tiles
            const n = crs ? Infinity : 2 ** z;
            if (x < 0 || y < 0 || z < 0 || x >= n || y >= n)
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
        (p, crs) =>
            crs
                ? chiitiler.renderProjectedTile({ ...p, crs })
                : chiitiler.renderTile(p),
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
            const ext = format(c, param(c, 'dimensions_ext'));
            if (ext instanceof Response) return ext;
            const [width, height] = param(c, 'dimensions_ext')
                .split('.')[0]
                .split('x')
                .map(Number);
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
                ext,
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
