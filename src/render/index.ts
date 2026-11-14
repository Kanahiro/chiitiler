import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { LRUCache } from 'lru-cache';

import { MapRenderer, type RenderedImage } from './ml2drender.js';
import { loadWasmModule } from '../wasm.js';
import type { Cache } from '../cache/index.js';
import { getSource } from '../source/index.js';

type SupportedFormat = 'png' | 'jpeg' | 'jpg' | 'webp';

type GetRenderedTileOptions = {
    stylejson: string | StyleSpecification;
    z: number;
    x: number;
    y: number;
    tileSize: number;
    cache: Cache;
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
    ext: SupportedFormat;
    quality: number;
};

type GetRenderedCameraOptions = {
    stylejson: string | StyleSpecification;
    cache: Cache;
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

const styleCache = new LRUCache<string, StyleSpecification>({
    max: 5,
});
async function loadStyle(stylejson: string | StyleSpecification, cache: Cache) {
    if (typeof stylejson !== 'string') return stylejson;
    const cachedStyle = styleCache.get(stylejson);
    if (cachedStyle !== undefined) return cachedStyle;
    const styleJsonBuf = await getSource(stylejson, cache);
    if (styleJsonBuf === null) throw new Error('style not found');
    const style: StyleSpecification = JSON.parse(
        new TextDecoder().decode(styleJsonBuf),
    );
    styleCache.set(stylejson, style);
    return style;
}

// Each renderer owns a WASM instance and keeps decoded tiles, glyphs and
// sprites for a few minutes (ml2drender's default), so styles are reused.
const rendererCache = new LRUCache<string, Promise<MapRenderer>>({
    max: 10,
});
async function getRenderer(
    stylejson: string | StyleSpecification,
    cache: Cache,
): Promise<MapRenderer> {
    const style = await loadStyle(stylejson, cache);
    // a URL is cheaper to key by than the style it points to
    const key =
        typeof stylejson === 'string'
            ? `url:${stylejson}`
            : `json:${JSON.stringify(style)}`;
    let renderer = rendererCache.get(key);
    if (renderer === undefined) {
        renderer = loadWasmModule().then((wasm) =>
            MapRenderer.init(style, {
                wasm,
                // null = 404 (empty vector tile, missing raster); errors fail the render
                loadResource: async (url) => {
                    const buf = await getSource(url, cache);
                    return buf === null || buf.length === 0
                        ? null
                        : (buf as Uint8Array<ArrayBuffer>);
                },
            }),
        );
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
    margin,
    ext,
    quality,
}: GetRenderedTileOptions): Promise<Uint8Array<ArrayBuffer>> {
    const renderer = await getRenderer(stylejson, cache);
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
    ext,
    quality,
}: GetRenderedClipOptions): Promise<Uint8Array<ArrayBuffer>> {
    const renderer = await getRenderer(stylejson, cache);
    const image = await renderer.render({ bbox, size });
    return encode(image, ext, quality);
}

async function getRenderedCamera(
    options: GetRenderedCameraOptions,
): Promise<Uint8Array<ArrayBuffer>> {
    if (options.pitch !== 0) throw new RangeError('pitch is not supported');
    const renderer = await getRenderer(options.stylejson, options.cache);
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
