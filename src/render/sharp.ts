import sharp from 'sharp';

import { renderTile } from './rasterize.js';
import type { Cache } from '../cache/index.js';

type RenderTilePipelineOptions = {
    url: string;
    z: number;
    x: number;
    y: number;
    tileSize: number;
    cache: Cache;
    margin: number;
    ext: SupportedFormat;
    quality: number;
};

type SupportedFormat = 'png' | 'jpeg' | 'jpg' | 'webp';

async function renderTilePipeline({
    url,
    z,
    x,
    y,
    tileSize,
    cache,
    margin,
    ext,
    quality,
}: RenderTilePipelineOptions) {
    let pixels: Uint8Array;
    pixels = await renderTile(url, z, x, y, {
        tileSize,
        cache,
        margin,
    });

    // hack: tile-margin clip area
    // maplibre-native won't render outer area of meractor
    // so top-end and bottom-end clipped area is special
    const isTopEnd = y === 0;
    const isBottomEnd = y === 2 ** z - 1;
    const topMargin = isTopEnd ? 0 : isBottomEnd ? margin : margin / 2;

    let _sharp: sharp.Sharp;
    if (tileSize === 256 && z === 0) {
        // hack: when tileSize=256, z=0
        // pixlels will be 512x512 so we need to resize to 256x256
        _sharp = sharp(pixels, {
            raw: {
                width: 512,
                height: 512,
                channels: 4,
            },
        }).resize(256, 256);
    } else {
        _sharp = sharp(pixels, {
            raw: {
                width: tileSize + margin,
                height: tileSize + margin,
                channels: 4,
            },
        })
            .extract({
                left: margin / 2,
                top: topMargin,
                width: tileSize,
                height: tileSize,
            })
            .resize(tileSize, tileSize);
    }

    let pipeline: sharp.Sharp;
    switch (ext) {
        case 'png':
            pipeline = _sharp.png();
            break;
        case 'jpeg':
        case 'jpg':
            pipeline = _sharp.jpeg({ quality });
            break;
        case 'webp':
            pipeline = _sharp.webp({ quality, effort: 0 });
            break;
        default:
            throw new Error('unsupported format');
    }
    return pipeline;
}

export { renderTilePipeline, type SupportedFormat };
