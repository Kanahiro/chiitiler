import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { getRenderedTile } from './index.js';
import type { Cache } from '../cache/index.js';

// スタイル固有のリソース(タイル/glyphs/sprite)は温まらないが、
// EGL/GLコンテキスト・Xvfb接続・sharp(libvips)・JITの初期化は共通で効く。
const MINIMAL_STYLE: StyleSpecification = {
    version: 8,
    sources: {},
    layers: [
        {
            id: 'background',
            type: 'background',
            paint: { 'background-color': '#ffffff' },
        },
    ],
};

/**
 * Render and PNG-encode a minimal tile to warm shared renderer initialization.
 * Await once per rendering process during application startup, before accepting
 * requests. This does not preload style-specific tiles, glyphs, or sprites.
 * Resolves after encoding finishes; rejects if rendering or encoding fails.
 */
async function prewarm(cache: Cache): Promise<void> {
    const sharp = await getRenderedTile({
        stylejson: MINIMAL_STYLE,
        z: 0,
        x: 0,
        y: 0,
        tileSize: 512,
        cache,
        margin: 0,
        ext: 'png',
        quality: 100,
    });
    await sharp.toBuffer(); // force the actual encode
}

export { prewarm };
