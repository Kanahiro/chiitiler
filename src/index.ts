export {
    getRenderedTile,
    getRenderedClip,
    getRenderedCamera,
} from './render/index.js';

export * as ChiitilerCache from './cache/index.js';
export { setUserAgent } from './source/userAgent.js';

export type {
    GetRenderedClipOptions,
    GetRenderedTileOptions,
    GetRenderedCameraOptions,
    SupportedFormat,
} from './render/index.js';
export type { Cache, Value } from './cache/index.js';
export type { Protocol, Protocols } from './source/index.js';

// re-exported so consumers can reference exactly the style-spec version
// chiitiler is built against, without depending on their own resolution
export type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
