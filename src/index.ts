import {
    getRenderedClip,
    getRenderedTile,
    getRenderedCamera,
} from './render/index.js';

/** Render a slippy-map tile and encode it as `ext`. */
export const getRenderedTileBuffer = getRenderedTile;
/** Render a lon/lat bounding box and encode it as `ext`. */
export const getRenderedClipBuffer = getRenderedClip;
/** Render a free camera (pitch 0 only) and encode it as `ext`. */
export const getRenderedCameraBuffer = getRenderedCamera;

export * as ChiitilerCache from './cache/index.js';
export { setUserAgent } from './source/userAgent.js';
export { prewarm } from './render/warmup.js';

export type {
    GetRenderedClipOptions,
    GetRenderedTileOptions,
    GetRenderedCameraOptions,
    SupportedFormat,
} from './render/index.js';
export type { Cache } from './cache/index.js';

// re-exported so consumers can reference exactly the style-spec version
// chiitiler is built against, without depending on their own resolution
export type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
