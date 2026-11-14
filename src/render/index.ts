import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { availableParallelism } from 'node:os';
import { LRUCache } from 'lru-cache';

import { MapRenderer, type RenderedImage } from './ml2drender.js';
import type { Cache } from '../cache/index.js';
import { getSource, type Protocols } from '../source/index.js';

type SupportedFormat = 'png' | 'jpeg' | 'jpg' | 'webp';

type GetRenderedTileOptions = {
    stylejson: string | StyleSpecification;
    z: number;
    x: number;
    y: number;
    tileSize: number;
    cache: Cache;
    /** Custom protocols by scheme, taking precedence over the built-in ones. */
    protocols?: Protocols;
    /** Extra pixels rendered around the tile (half on each side) and cropped away. */
    margin: number;
    ext: SupportedFormat;
    /** JPEG quality; WebP is always lossless. */
    quality: number;
    /** The largest image side in pixels, margin included. Default: 4096. */
    maxSize?: number;
};

type GetRenderedClipOptions = {
    stylejson: string | StyleSpecification;
    bbox: [number, number, number, number];
    size: number;
    cache: Cache;
    /** Custom protocols by scheme, taking precedence over the built-in ones. */
    protocols?: Protocols;
    ext: SupportedFormat;
    quality: number;
    /** The largest image side in pixels. Default: 4096. */
    maxSize?: number;
};

type GetRenderedCameraOptions = {
    stylejson: string | StyleSpecification;
    cache: Cache;
    /** Custom protocols by scheme, taking precedence over the built-in ones. */
    protocols?: Protocols;
    ext: SupportedFormat;
    quality: number;
    bearing: number;
    /** Only 0: the renderer is 2D. */
    pitch: number;
    zoom: number;
    center: [number, number];
    height: number;
    width: number;
    /** The largest image side in pixels. Default: 4096. */
    maxSize?: number;
};

const DEFAULT_MAX_SIZE = 4096;

/** Refuse images larger than `maxSize` on a side: one request must not take the process down. */
function checkSize(sides: number[], maxSize = DEFAULT_MAX_SIZE) {
    if (sides.some((side) => side > maxSize))
        throw new RangeError(`image sides must be at most ${maxSize} px`);
}

/** Where resources come from: what a loaded style or renderer is cached for besides the style. */
type Resources = { cache: Cache; protocols?: Protocols };

const ids = new WeakMap<object, number>();
let lastId = 0;
/** A number for `o` by identity (0 for none). */
function idOf(o: object | undefined): number {
    if (o === undefined) return 0;
    let n = ids.get(o);
    if (n === undefined) {
        n = ++lastId;
        ids.set(o, n);
    }
    return n;
}
/** A key for the cache and protocols objects (by identity) of `resources`. */
function resourcesKey({ cache, protocols }: Resources): string {
    return `${idOf(cache)}/${idOf(protocols)}`;
}

/** How long a style read from a URL is used before it is read again. */
const STYLE_TTL_MS = 60_000;
const styleCache = new LRUCache<
    string,
    { text: string; style: StyleSpecification; loadedAt: number }
>({ max: 5 });
async function loadStyle(
    stylejson: string | StyleSpecification,
    resources: Resources,
): Promise<StyleSpecification> {
    if (typeof stylejson !== 'string') return stylejson;
    const key = `${resourcesKey(resources)} ${stylejson}`;
    const cached = styleCache.get(key);
    const now = performance.now();
    if (cached !== undefined && now - cached.loadedAt < STYLE_TTL_MS)
        return cached.style;
    // Styles change upstream: read them past the backing cache (which may never expire them),
    // through the short-lived memory layer only.
    const memoryOnly: Cache = {
        get: async () => undefined,
        set: async () => {},
        memory: resources.cache.memory,
    };
    const bytes = await getSource(stylejson, memoryOnly, resources.protocols);
    if (bytes === null) throw new Error('style not found');
    const text = new TextDecoder().decode(bytes);
    // An unchanged style keeps its object, and with it its renderer and their decoded resources.
    if (cached !== undefined && cached.text === text) {
        cached.loadedAt = now;
        return cached.style;
    }
    const style: StyleSpecification = JSON.parse(text);
    styleCache.set(key, { text, style, loadedAt: now });
    return style;
}

/** Renderers of a style draw this many renders in parallel on libuv's thread pool (4 threads). */
const CONCURRENCY = Math.min(4, availableParallelism());

// Each renderer keeps decoded tiles, glyphs and sprites for a few minutes
// (ml2drender's default), so styles are reused.
const rendererCache = new LRUCache<string, Promise<MapRenderer>>({
    max: 10,
});
async function getRenderer(
    stylejson: string | StyleSpecification,
    resources: Resources,
): Promise<MapRenderer> {
    const style = await loadStyle(stylejson, resources);
    // A renderer fetches through the cache and protocols it was made with. A style read from a
    // URL is one object until it changes, cheaper to key by than its JSON.
    const key = `${resourcesKey(resources)} ${
        typeof stylejson === 'string'
            ? `url:${idOf(style)}`
            : `json:${JSON.stringify(style)}`
    }`;
    let renderer = rendererCache.get(key);
    if (renderer === undefined) {
        renderer = MapRenderer.init(style, {
            concurrency: CONCURRENCY,
            // null = 404 (empty vector tile, missing raster); errors fail the render
            loadResource: async (url) => {
                const buf = await getSource(
                    url,
                    resources.cache,
                    resources.protocols,
                );
                return buf === null || buf.length === 0 ? null : buf;
            },
        });
        rendererCache.set(key, renderer);
        // a failed init is retried by the next request
        renderer.catch(() => {
            if (rendererCache.get(key) === renderer) rendererCache.delete(key);
        });
    }
    return renderer;
}

function encode(
    image: RenderedImage,
    ext: SupportedFormat,
    quality: number,
): Promise<Uint8Array<ArrayBuffer>> {
    switch (ext) {
        case 'png':
            return image.toPng();
        case 'jpeg':
        case 'jpg':
            return image.toJpeg(quality);
        case 'webp':
            return image.toWebp();
    }
}

/** Render a slippy-map tile and encode it as `ext`. */
async function getRenderedTile({
    stylejson,
    z,
    x,
    y,
    tileSize,
    cache,
    protocols,
    margin,
    ext,
    quality,
    maxSize,
}: GetRenderedTileOptions): Promise<Uint8Array<ArrayBuffer>> {
    checkSize([tileSize + 2 * Math.floor(margin / 2)], maxSize);
    const renderer = await getRenderer(stylejson, { cache, protocols });
    const image = await renderer.renderTile({
        z,
        x,
        y,
        tileSize,
        margin: Math.floor(margin / 2),
    });
    return encode(image, ext, quality);
}

/** Render a lon/lat bounding box and encode it as `ext`. */
async function getRenderedClip({
    stylejson,
    bbox,
    size,
    cache,
    protocols,
    ext,
    quality,
    maxSize,
}: GetRenderedClipOptions): Promise<Uint8Array<ArrayBuffer>> {
    checkSize([size], maxSize);
    const renderer = await getRenderer(stylejson, { cache, protocols });
    const image = await renderer.render({ bbox, size });
    return encode(image, ext, quality);
}

/** Render a free camera (pitch 0 only) and encode it as `ext`. */
async function getRenderedCamera(
    options: GetRenderedCameraOptions,
): Promise<Uint8Array<ArrayBuffer>> {
    if (options.pitch !== 0) throw new RangeError('pitch is not supported');
    checkSize([options.width, options.height], options.maxSize);
    const renderer = await getRenderer(options.stylejson, options);
    const image = await renderer.renderView({
        center: options.center,
        zoom: options.zoom,
        bearing: options.bearing,
        width: options.width,
        height: options.height,
    });
    return encode(image, options.ext, options.quality);
}

export {
    getRenderedTile,
    getRenderedClip,
    getRenderedCamera,
    type GetRenderedClipOptions,
    type GetRenderedTileOptions,
    type GetRenderedCameraOptions,
    type SupportedFormat,
};
