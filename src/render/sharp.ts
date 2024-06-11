import sharp from 'sharp';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { renderTile } from './rasterize.js';
import type { Cache } from '../cache/index.js';
import { getSource } from '../source/index.js';

type RenderTilePipelineOptions = {
    style: StyleSpecification;
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
function isSupportedFormat(ext: string): ext is SupportedFormat {
    return ['png', 'jpeg', 'jpg', 'webp'].includes(ext);
}

async function getRenderedTileBuffer({
    style,
    z,
    x,
    y,
    tileSize,
    cache,
    margin,
    ext,
    quality,
}: RenderTilePipelineOptions): Promise<Buffer> {
    let pixels: Uint8Array;
    pixels = await renderTile(style, z, x, y, {
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
    } else if (margin === 0) {
        _sharp = sharp(pixels, {
            raw: {
                width: tileSize,
                height: tileSize,
                channels: 4,
            },
        });
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

    let buf: Buffer;
    switch (ext) {
        case 'png':
            buf = await _sharp.png().toBuffer();
            break;
        case 'jpeg':
        case 'jpg':
            buf = await _sharp.jpeg({ quality }).toBuffer();
            break;
        case 'webp':
            buf = await _sharp.webp({ quality, effort: 0 }).toBuffer();
            break;
    }
    return buf;
}

const transparent = sharp({
    create: {
        width: 1,
        height: 1,
        channels: 4,
        background: { r: 1, g: 134, b: 160, alpha: 0 },
    },
});

const png = transparent.png().toBuffer();
const webp = transparent.webp().toBuffer();
const jpeg = transparent.jpeg().toBuffer();
const TRANSPARENT_BUFFER = {
    png,
    webp,
    jpeg,
    jpg: jpeg,
} as const;

async function getTransparentBuffer(ext: string): Promise<Buffer | null> {
    if (!isSupportedFormat(ext)) return null;
    else return TRANSPARENT_BUFFER[ext];
}

export {
    isSupportedFormat,
    getTransparentBuffer,
    getRenderedTileBuffer,
    type SupportedFormat,
};
