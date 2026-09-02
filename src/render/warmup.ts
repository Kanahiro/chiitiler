import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { getRenderedTile } from './index.js';
import type { Cache } from '../cache/index.js';

// 実運用スタイルが指定されないときのフォールバック。
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
 * Render one z0 tile ahead of the first request so that renderer
 * initialization happens at startup — on AWS Lambda (with Lambda Web
 * Adapter's readiness check) this moves the work into the INIT phase,
 * which runs with a full CPU boost.
 *
 * When styleUrl is given, the style is fetched and its render pool is
 * built, so glyphs/sprite/source fetches for that style are warmed too.
 */
async function prewarm(cache: Cache, styleUrl?: string): Promise<void> {
    const sharp = await getRenderedTile({
        stylejson: styleUrl ?? MINIMAL_STYLE,
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
