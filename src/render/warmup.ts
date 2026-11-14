import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { getRenderedTile } from './index.js';
import type { Cache } from '../cache/index.js';

// Style-specific resources (tiles, glyphs, sprites) stay cold; this compiles
// the native renderer and runs the render and PNG-encode paths once.
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
    await getRenderedTile({
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
}

export { prewarm };
