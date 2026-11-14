import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { availableParallelism } from 'node:os';
import { LRUCache } from 'lru-cache';

import { MapRenderer, type RenderedImage } from './ml2drender.js';
import type { Cache } from '../cache/index.js';
import { getSource, type Protocols } from '../source/index.js';

type SupportedFormat = 'png' | 'jpeg' | 'jpg' | 'webp';

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
    ext: SupportedFormat;
    /** JPEG quality; WebP is always lossless. */
    quality: number;
};

type RenderBboxOptions = {
    stylejson: string | StyleSpecification;
    bbox: [number, number, number, number];
    size: number;
    ext: SupportedFormat;
    quality: number;
};

type RenderViewOptions = {
    stylejson: string | StyleSpecification;
    ext: SupportedFormat;
    quality: number;
    bearing: number;
    zoom: number;
    center: [number, number];
    height: number;
    width: number;
};

const DEFAULT_MAX_SIZE = 4096;

const ids = new WeakMap<object, number>();
let lastId = 0;
/** A number for `o` by identity. */
function idOf(o: object): number {
    let n = ids.get(o);
    if (n === undefined) {
        n = ++lastId;
        ids.set(o, n);
    }
    return n;
}

/** How long a style read from a URL is used before it is read again. */
const STYLE_TTL_MS = 60_000;

/** Renderers of a style draw this many renders in parallel on libuv's thread pool (4 threads). */
const CONCURRENCY = Math.min(4, availableParallelism());

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
        this.cache = cache;
        this.protocols = protocols;
        this.maxSize = maxSize ?? DEFAULT_MAX_SIZE;
    }

    /** Render a slippy-map tile and encode it as `ext`. */
    async renderTile({
        stylejson,
        z,
        x,
        y,
        tileSize,
        margin,
        ext,
        quality,
    }: RenderTileOptions): Promise<Uint8Array<ArrayBuffer>> {
        this.checkSize([tileSize + 2 * Math.floor(margin / 2)]);
        const renderer = await this.renderer(stylejson);
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
    async renderBbox({
        stylejson,
        bbox,
        size,
        ext,
        quality,
    }: RenderBboxOptions): Promise<Uint8Array<ArrayBuffer>> {
        this.checkSize([size]);
        const renderer = await this.renderer(stylejson);
        const image = await renderer.render({ bbox, size });
        return encode(image, ext, quality);
    }

    /** Render a 2D camera (center, zoom, bearing; no pitch) and encode it as `ext`. */
    async renderView(
        options: RenderViewOptions,
    ): Promise<Uint8Array<ArrayBuffer>> {
        this.checkSize([options.width, options.height]);
        const renderer = await this.renderer(options.stylejson);
        const image = await renderer.renderView({
            center: options.center,
            zoom: options.zoom,
            bearing: options.bearing,
            width: options.width,
            height: options.height,
        });
        return encode(image, options.ext, options.quality);
    }

    /** Refuse images larger than `maxSize` on a side: one request must not take the process down. */
    private checkSize(sides: number[]) {
        if (sides.some((side) => side > this.maxSize))
            throw new RangeError(
                `image sides must be at most ${this.maxSize} px`,
            );
    }

    private async loadStyle(
        stylejson: string | StyleSpecification,
    ): Promise<StyleSpecification> {
        if (typeof stylejson !== 'string') return stylejson;
        const cached = this.styles.get(stylejson);
        const now = performance.now();
        if (cached !== undefined && now - cached.loadedAt < STYLE_TTL_MS)
            return cached.style;
        // Styles change upstream: read them past the backing cache (which may never expire them),
        // through the short-lived memory layer only.
        const memoryOnly: Cache = {
            get: async () => undefined,
            set: async () => {},
            memory: this.cache.memory,
        };
        const bytes = await getSource(stylejson, memoryOnly, this.protocols);
        if (bytes === null) throw new Error('style not found');
        const text = new TextDecoder().decode(bytes);
        // An unchanged style keeps its object, and with it its renderer and their decoded resources.
        if (cached !== undefined && cached.text === text) {
            cached.loadedAt = now;
            return cached.style;
        }
        const style: StyleSpecification = JSON.parse(text);
        this.styles.set(stylejson, { text, style, loadedAt: now });
        return style;
    }

    private async renderer(
        stylejson: string | StyleSpecification,
    ): Promise<MapRenderer> {
        const style = await this.loadStyle(stylejson);
        // A style read from a URL is one object until it changes, cheaper to key by than its JSON.
        const key =
            typeof stylejson === 'string'
                ? `url:${idOf(style)}`
                : `json:${JSON.stringify(style)}`;
        let renderer = this.renderers.get(key);
        if (renderer === undefined) {
            renderer = MapRenderer.init(style, {
                concurrency: CONCURRENCY,
                // null = 404 (empty vector tile, missing raster); errors fail the render
                loadResource: async (url) => {
                    const buf = await getSource(
                        url,
                        this.cache,
                        this.protocols,
                    );
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
    type RenderBboxOptions,
    type RenderTileOptions,
    type RenderViewOptions,
    type SupportedFormat,
};
