import {
    getRenderedBbox,
    getRenderedTile,
    getRenderedImage,
    GetRenderedBboxOptions,
    GetRenderedTileOptions,
    GetRenderedImageOptions,
} from './render/index.js';

export async function getRenderedBboxBuffer(
    options: GetRenderedBboxOptions,
): Promise<Buffer> {
    const sharp = await getRenderedBbox(options);
    return sharp.toBuffer();
}

export { getRenderedBbox as getRenderedBboxStream };

export async function getRenderedImageBuffer(
    options: GetRenderedImageOptions,
): Promise<Buffer> {
    const sharp = await getRenderedImage(options);
    return sharp.toBuffer();
}

export { getRenderedImage as getRenderedImageStream };

export async function getRenderedTileBuffer(
    options: GetRenderedTileOptions,
): Promise<Buffer> {
    const sharp = await getRenderedTile(options);
    return sharp.toBuffer();
}

export { getRenderedTile as getRenderedTileStream };

export * as ChiitilerCache from './cache/index.js';
