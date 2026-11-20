import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { createHash } from 'node:crypto';
import { availableParallelism } from 'node:os';
import { LRUCache } from 'lru-cache';

import {
    type Crs,
    image as rgba,
    MapRenderer,
    type RenderedImage,
} from './ml2drender.js';
import type { Cache } from '../cache/index.js';
import { log } from '../log.js';
import { getSource, type Protocols } from '../source/index.js';

type SupportedFormat = 'png' | 'jpeg' | 'jpg' | 'webp' | 'avif';

type ChiitilerOptions = {
    /** Where fetched sources are kept (see `ChiitilerCache`). */
    cache: Cache;
    /** Custom protocols by scheme, taking precedence over the built-in ones. */
    protocols?: Protocols;
    /** The largest image side in pixels, tile margins included. Default: 4096. */
    maxSize?: number;
};

type RenderTileOptions = {
    stylejson: string | StyleSpecification;
    z: number;
    x: number;
    y: number;
    tileSize: number;
    /** Extra pixels rendered around the tile (half on each side) and cropped away. */
    margin: number;
    /** Device pixels per CSS pixel (`@2x` = 2): sizes stay in CSS pixels. Default: 1. */
    pixelRatio?: number;
};

type RenderBboxOptions = {
    stylejson: string | StyleSpecification;
    bbox: [number, number, number, number];
    size: number;
    /** Device pixels per CSS pixel (`@2x` = 2): sizes stay in CSS pixels. Default: 1. */
    pixelRatio?: number;
};

type RenderViewOptions = {
    stylejson: string | StyleSpecification;
    bearing: number;
    zoom: number;
    center: [number, number];
    height: number;
    width: number;
    /** Device pixels per CSS pixel (`@2x` = 2): sizes stay in CSS pixels. Default: 1. */
    pixelRatio?: number;
};

const DEFAULT_MAX_SIZE = 4096;

/** How long a style read from a URL is used before it is read again. */
const STYLE_TTL_MS = 60_000;

/** Renderers of a style draw this many renders in parallel on libuv's thread pool (4 threads). */
const CONCURRENCY = Math.min(4, availableParallelism());

/**
 * Encode RGBA pixels (`width` × `height` × 4 bytes, e.g. a `RenderedImage`'s, edited or not) as `ext`.
 * `quality` (default 100) applies to JPEG, AVIF, and WebP below 100.
 */
function encode(
    pixels: Pick<RenderedImage, 'data' | 'width' | 'height'>,
    ext: SupportedFormat,
    quality = 100,
): Promise<Uint8Array<ArrayBuffer>> {
    const { data, width, height } = pixels;
    if (
        !(Number.isInteger(width) && Number.isInteger(height)) ||
        width < 1 ||
        height < 1 ||
        data.length !== width * height * 4
    )
        return Promise.reject(
            new RangeError('data must be width × height × 4 bytes of RGBA'),
        );
    const image = rgba(data, width);
    switch (ext) {
        case 'png':
            return image.toPng();
        case 'jpeg':
        case 'jpg':
            return image.toJpeg(quality);
        case 'webp':
            return image.toWebp(quality);
        case 'avif':
            return image.toAvif(quality);
    }
}

/**
 * `cache` whose writes never reject: sources store without awaiting, and a failed write (a cache
 * that is down) must cost a cache miss, not an unhandled rejection that exits the process.
 */
function loggingWrites(cache: Cache): Cache {
    return {
        ...cache,
        set: async (key, value) => {
            try {
                await cache.set(key, value);
            } catch (error) {
                log('WARNING', 'cache write failed', {
                    cache: cache.name,
                    error,
                });
            }
        },
        front: cache.front && loggingWrites(cache.front),
    };
}

/**
 * Renders any number of styles, fetching their sources through one cache and set of protocols.
 * Renderers of recently used styles are kept with their decoded tiles, glyphs and sprites: create
 * one instance and render every style with it.
 */
class Chiitiler {
    private readonly cache: Cache;
    private readonly protocols?: Protocols;
    private readonly maxSize: number;
    private readonly styles = new LRUCache<
        string,
        { text: string; style: StyleSpecification; loadedAt: number }
    >({ max: 5 });
    // Each renderer keeps decoded tiles, glyphs and sprites for a few minutes
    // (ml2drender's default), so styles are reused.
    private readonly renderers = new LRUCache<string, Promise<MapRenderer>>({
        max: 10,
    });

    constructor({ cache, protocols, maxSize }: ChiitilerOptions) {
        this.cache = loggingWrites(cache);
        this.protocols = protocols;
        this.maxSize = maxSize ?? DEFAULT_MAX_SIZE;
    }

    /** Render a slippy-map tile. */
    renderTile(options: RenderTileOptions): Promise<RenderedImage> {
        return this.tile(options);
    }

    /** Render tile `z/x/y` of `crs`'s grid (`tileSize` its width). */
    renderProjectedTile(
        options: RenderTileOptions & { crs: Crs },
    ): Promise<RenderedImage> {
        return this.tile(options, options.crs);
    }

    /** Render a lon/lat bounding box. */
    renderBbox(options: RenderBboxOptions): Promise<RenderedImage> {
        return this.bbox(options);
    }

    /**
     * Render a box of `crs` (`bbox` in its units, easting first, y up).
     * `crs` needs no grid.
     */
    renderProjectedBbox(
        options: RenderBboxOptions & { crs: Crs },
    ): Promise<RenderedImage> {
        return this.bbox(options, options.crs);
    }

    /** Render a 2D camera (center, zoom, bearing; no pitch). */
    renderView(options: RenderViewOptions): Promise<RenderedImage> {
        return this.view(options);
    }

    /**
     * Render a 2D camera in `crs` (center in degrees, zoom of its grid: tile 0/0/0 512 px wide at
     * zoom 0).
     */
    renderProjectedView(
        options: RenderViewOptions & { crs: Crs },
    ): Promise<RenderedImage> {
        return this.view(options, options.crs);
    }

    /** Invalid CRSs, grids and tiles reject with a `RangeError`. */
    private async tile(
        {
            stylejson,
            z,
            x,
            y,
            tileSize,
            margin,
            pixelRatio = 1,
        }: RenderTileOptions,
        crs?: Crs,
    ): Promise<RenderedImage> {
        const m = 2 * Math.floor(margin / 2);
        // An OGC grid's tiles may be taller than wide.
        const level =
            crs?.tileMatrixSet && 'tileMatrices' in crs.tileMatrixSet
                ? crs.tileMatrixSet.tileMatrices[z]
                : undefined;
        const height = level
            ? (tileSize * level.tileHeight) / level.tileWidth
            : tileSize;
        this.checkSize([tileSize + m, height + m], pixelRatio);
        const renderer = await this.renderer(stylejson);
        const tile = {
            z,
            x,
            y,
            tileSize,
            pixelRatio,
            margin: Math.floor(margin / 2),
        };
        return crs
            ? renderer.renderProjected(crs, { tile })
            : renderer.renderTile(tile);
    }

    private async bbox(
        { stylejson, bbox, size, pixelRatio = 1 }: RenderBboxOptions,
        crs?: Crs,
    ): Promise<RenderedImage> {
        this.checkSize([size], pixelRatio);
        const renderer = await this.renderer(stylejson);
        return crs
            ? renderer.renderProjected(crs, { box: { bbox, size, pixelRatio } })
            : renderer.render({ bbox, size, pixelRatio });
    }

    private async view(
        options: RenderViewOptions,
        crs?: Crs,
    ): Promise<RenderedImage> {
        const pixelRatio = options.pixelRatio ?? 1;
        this.checkSize([options.width, options.height], pixelRatio);
        const renderer = await this.renderer(options.stylejson);
        const view = {
            center: options.center,
            zoom: options.zoom,
            bearing: options.bearing,
            width: options.width,
            height: options.height,
            pixelRatio,
        };
        return crs
            ? renderer.renderProjected(crs, { view })
            : renderer.renderView(view);
    }

    /**
     * Refuse images larger than `maxSize` device pixels on a side (CSS `sides` times `pixelRatio`):
     * one request must not take the process down.
     */
    private checkSize(sides: number[], pixelRatio: number) {
        if (sides.some((side) => side * pixelRatio > this.maxSize))
            throw new RangeError(
                `image sides must be at most ${this.maxSize} px`,
            );
    }

    /** The style, and its JSON text (renderers are keyed by its hash). */
    private async loadStyle(
        stylejson: string | StyleSpecification,
    ): Promise<{ style: StyleSpecification; text: string }> {
        if (typeof stylejson !== 'string')
            return { style: stylejson, text: JSON.stringify(stylejson) };
        const cached = this.styles.get(stylejson);
        const now = performance.now();
        if (cached !== undefined && now - cached.loadedAt < STYLE_TTL_MS)
            return cached;
        // Styles change upstream: read them past the backing cache (which may never expire them),
        // through the short-lived front cache only.
        const frontOnly: Cache = {
            get: async () => undefined,
            set: async () => {},
            front: this.cache.front,
        };
        const bytes = await getSource(stylejson, frontOnly, this.protocols);
        if (bytes === null) throw new Error('style not found');
        const text = new TextDecoder().decode(bytes);
        const style: StyleSpecification = JSON.parse(text);
        this.styles.set(stylejson, { text, style, loadedAt: now });
        return { style, text };
    }

    private async renderer(
        stylejson: string | StyleSpecification,
    ): Promise<MapRenderer> {
        const { style, text } = await this.loadStyle(stylejson);
        // By content: concurrent and repeated reads of a style parse to distinct objects, but must
        // share one renderer and its decoded resources. A hash keeps large inline styles out of keys.
        const key = createHash('sha256').update(text).digest('base64url');
        let renderer = this.renderers.get(key);
        if (renderer === undefined) {
            renderer = MapRenderer.init(style, {
                concurrency: CONCURRENCY,
                // null = 404 (empty vector tile, missing raster); errors fail the render
                loadResource: async (url) => {
                    let buf: Uint8Array | null;
                    try {
                        buf = await getSource(url, this.cache, this.protocols);
                    } catch (error) {
                        // A failed resource is the server's failure (500), whatever its class:
                        // RangeErrors and TypeErrors (undici's) must not read as bad requests.
                        if (
                            error instanceof RangeError ||
                            error instanceof TypeError
                        )
                            throw new Error(error.message, { cause: error });
                        throw error;
                    }
                    return buf === null || buf.length === 0 ? null : buf;
                },
            });
            this.renderers.set(key, renderer);
            // a failed init is retried by the next request
            renderer.catch(() => {
                if (this.renderers.get(key) === renderer)
                    this.renderers.delete(key);
            });
        }
        return renderer;
    }
}

export {
    Chiitiler,
    type ChiitilerOptions,
    type Crs,
    encode,
    type RenderBboxOptions,
    type RenderedImage,
    type RenderTileOptions,
    type RenderViewOptions,
    type SupportedFormat,
};
