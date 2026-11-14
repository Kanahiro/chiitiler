import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
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
};

/** Where resources come from: what a loaded style or renderer is cached for besides the style. */
type Resources = { cache: Cache; protocols?: Protocols };

const ids = new WeakMap<object, number>();
let lastId = 0;
/** A key for the cache and protocols objects (by identity) of `resources`. */
function resourcesKey({ cache, protocols }: Resources): string {
    const id = (o: object | undefined) => {
        if (o === undefined) return 0;
        let n = ids.get(o);
        if (n === undefined) {
            n = ++lastId;
            ids.set(o, n);
        }
        return n;
    };
    return `${id(cache)}/${id(protocols)}`;
}

const styleCache = new LRUCache<string, StyleSpecification>({
    max: 5,
});
async function loadStyle(
    stylejson: string | StyleSpecification,
    resources: Resources,
) {
    if (typeof stylejson !== 'string') return stylejson;
    const key = `${resourcesKey(resources)} ${stylejson}`;
    const cachedStyle = styleCache.get(key);
    if (cachedStyle !== undefined) return cachedStyle;
    const styleJsonBuf = await getSource(
        stylejson,
        resources.cache,
        resources.protocols,
    );
    if (styleJsonBuf === null) throw new Error('style not found');
    const style: StyleSpecification = JSON.parse(
        new TextDecoder().decode(styleJsonBuf),
    );
    styleCache.set(key, style);
    return style;
}

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
    // A renderer fetches through the cache and protocols it was made with. A URL is cheaper
    // to key by than the style it points to.
    const key = `${resourcesKey(resources)} ${
        typeof stylejson === 'string'
            ? `url:${stylejson}`
            : `json:${JSON.stringify(style)}`
    }`;
    let renderer = rendererCache.get(key);
    if (renderer === undefined) {
        renderer = MapRenderer.init(style, {
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
): Uint8Array<ArrayBuffer> {
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
}: GetRenderedTileOptions): Promise<Uint8Array<ArrayBuffer>> {
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

async function getRenderedClip({
    stylejson,
    bbox,
    size,
    cache,
    protocols,
    ext,
    quality,
}: GetRenderedClipOptions): Promise<Uint8Array<ArrayBuffer>> {
    const renderer = await getRenderer(stylejson, { cache, protocols });
    const image = await renderer.render({ bbox, size });
    return encode(image, ext, quality);
}

async function getRenderedCamera(
    options: GetRenderedCameraOptions,
): Promise<Uint8Array<ArrayBuffer>> {
    if (options.pitch !== 0) throw new RangeError('pitch is not supported');
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
