export { Chiitiler, encode } from './render/index.js';

export * as ChiitilerCache from './cache/index.js';
export { setUserAgent } from './source/userAgent.js';

export type {
    ChiitilerOptions,
    Crs,
    RenderBboxOptions,
    RenderedImage,
    RenderTileOptions,
    RenderViewOptions,
    SupportedFormat,
} from './render/index.js';
export type { Cache, Value } from './cache/index.js';
export type { Protocol, Protocols } from './source/index.js';

/**
 * The style-spec type chiitiler is built against. Prefer it over importing
 * `@maplibre/maplibre-gl-style-spec` directly: another major of that package in your tree
 * is not assignable to it.
 */
export type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
