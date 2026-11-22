import type {
    FillExtrusionLayerSpecification,
    FillLayerSpecification,
    LayerSpecification,
    StyleSpecification,
} from '@maplibre/maplibre-gl-style-spec';

import {
    encodeJpeg,
    encodePng,
    encodeWebp,
    type ProjectedRenderer,
    Renderer,
    type View as NativeView,
} from '@ml2drender/node';

/**
 * A CRS, with a tile grid for it. Chiitiler's own type (ml2drender's is 0.x and may change): the
 * renderer's is checked against it where it is passed.
 */
export interface Crs {
    /**
     * PROJJSON of a geographic or projected CRS (or of a bound or compound CRS of one). Most
     * projection methods are supported (see `@ml2drender/node`), without datum shifts.
     */
    projjson: object;
    /**
     * The CRS's tile grid, which a CRS does not have by itself. Required to render tiles and
     * views (whose zoom is the grid's); a box renders without it.
     */
    tileMatrixSet?: QuadTileMatrixSet | OgcTileMatrixSet;
}

/**
 * MapLibre GL JS's grid (`addProjection`): tile 0/0/0 is the square of side `extentAtZoom0` whose
 * top-left corner is `origin`, and each zoom splits every tile in four; x grows east, y south.
 */
export interface QuadTileMatrixSet {
    /** CRS coordinates of the top-left corner of tile 0/0/0: [min x, max y]. */
    origin: [number, number];
    /** Width (= height) of tile 0/0/0 in CRS units. */
    extentAtZoom0: number;
}

/** An OGC 2D TileMatrixSet (2.0, JSON); members not listed here are not read. */
export interface OgcTileMatrixSet {
    /** The CRS's axes in order; when the first is Lat, Lon, N or E, it orders each `pointOfOrigin`. */
    orderedAxes?: string[];
    /** The zoom levels, from the coarsest (`z` is the index). */
    tileMatrices: {
        /** CRS units per pixel. */
        cellSize: number;
        /** The corner rows and columns count from, in the CRS's axis order. */
        pointOfOrigin: [number, number];
        cornerOfOrigin?: 'topLeft' | 'bottomLeft';
        tileWidth: number;
        tileHeight: number;
        matrixWidth: number;
        matrixHeight: number;
    }[];
}

/** A Web Mercator camera. */
export interface View {
    center: [lng: number, lat: number];
    zoom: number;
    bearing?: number;
    width: number;
    height: number;
    pixelRatio?: number;
    /** Non-negative integer CSS pixels of extra coverage, cropped from the output. Default: 0. */
    margin?: number;
}

/** An XYZ tile of the Web Mercator grid (`tileSize` CSS pixels square). */
export interface Tile {
    z: number;
    x: number;
    y: number;
    /** CSS pixels. Default: 256. */
    tileSize?: number;
    pixelRatio?: number;
    /** Non-negative integer CSS pixels of extra coverage, cropped from the output. Default: 0. */
    margin?: number;
}

/**
 * A longitude/latitude box in Web Mercator. The image has the box's aspect ratio, `size` on its long side
 * (the short side rounded up), at the zoom that fits the box into it.
 */
export interface LngLatBox {
    /** Degrees; `west > east` crosses the antimeridian. Latitudes are clamped to Web Mercator's ±85.0511°. */
    bbox: [west: number, south: number, east: number, north: number];
    /** Positive integer CSS pixels of the image's long side. */
    size: number;
    pixelRatio?: number;
    /** Non-negative integer CSS pixels of extra coverage, cropped from the output. Default: 0. */
    margin?: number;
}

export interface RenderedImage {
    width: number;
    height: number;
    /** Row-major, 8-bit RGBA pixels. */
    data: Uint8Array<ArrayBuffer>;
    /** Encoders run on libuv's thread pool. */
    toPng(): Promise<Uint8Array<ArrayBuffer>>;
    /**
     * Lossy at `quality` below 100 (alpha stays lossless) with `@napi-rs/image` installed;
     * lossless otherwise.
     */
    toWebp(quality: number): Promise<Uint8Array<ArrayBuffer>>;
    /** `quality` 1–100, flattened over black. */
    toJpeg(quality: number): Promise<Uint8Array<ArrayBuffer>>;
    /** `quality` 0–100, full (4:4:4) chroma. Throws without `@napi-rs/image`. */
    toAvif(quality: number): Promise<Uint8Array<ArrayBuffer>>;
}

/** Style Spec layers supported by the 2D renderer. */
export type StyleLayer = Exclude<
    LayerSpecification,
    FillExtrusionLayerSpecification
>;

export interface InitOptions {
    /** Load complete resource bytes, or null for a missing resource (HTTP 404). Other failures must reject. */
    loadResource: (url: string) => Promise<Uint8Array | null>;
    /**
     * Decoded resources (tiles and their layer geometry, sprites, glyphs) are kept inside the
     * renderer and reused by later renders; the first render ttlMs or more after they started
     * being kept drops them. 0 disables reuse, Infinity never expires. Default: 5 minutes.
     */
    cache?: { ttlMs?: number };
    /**
     * Native renderers (each with its own decoded resources) that draw overlapping renders in
     * parallel on libuv's thread pool. Default: 1.
     */
    concurrency?: number;
    /**
     * Called for every style layer, in order; the layer is replaced with the returned layers
     * (`[]` drops it, several stand in for it). Each returned layer must be the given layer itself
     * or one of a type the 2D renderer supports (all but fill-extrusion).
     *
     * Default: a fill-extrusion becomes a fill keeping its other properties (id, source, filter,
     * zoom range, …) and visibility, whose `fill-color`, `fill-opacity`, `fill-pattern`, `fill-translate` and
     * `fill-translate-anchor` (and their transitions) are the `fill-extrusion-*` ones; height and
     * base are dropped. Every other layer is kept as is.
     */
    transformLayers?: (layer: LayerSpecification) => LayerSpecification[];
}

/** A renderer shared by overlapping renders (they take turns on it); dropped once retired and idle. */
interface Core {
    renderer: Renderer;
    createdAt: number;
    active: number;
}

const DEFAULT_TTL_MS = 5 * 60 * 1000;
const SUPPORTED_LAYERS = new Set<string>([
    'background',
    'fill',
    'line',
    'circle',
    'raster',
    'symbol',
    'hillshade',
    'color-relief',
    'heatmap',
] satisfies StyleLayer['type'][]);

function nativeView(view: View): NativeView {
    const [lng, lat] = view.center;
    const bearing = view.bearing ?? 0;
    for (const [name, value] of Object.entries({
        lng,
        lat,
        zoom: view.zoom,
        bearing,
    })) {
        if (!Number.isFinite(value))
            throw new RangeError(`${name} must be finite`);
    }
    const { width, height, pixelRatio = 1, margin = 0 } = view;
    for (const [name, value] of [
        ['width', width],
        ['height', height],
    ] as const) {
        if (!Number.isInteger(value) || value < 1 || value > 0xffffffff)
            throw new RangeError(`${name} must be a positive uint32 integer`);
    }
    if (!Number.isFinite(pixelRatio) || pixelRatio <= 0)
        throw new RangeError('pixelRatio must be finite and positive');
    if (!Number.isInteger(margin) || margin < 0 || margin > 0xffffffff)
        throw new RangeError('margin must be a uint32 integer');
    return {
        lng,
        lat,
        zoom: view.zoom,
        bearing,
        width,
        height,
        pixelRatio,
        margin,
    };
}

function tileView({ z, x, y, tileSize = 256, pixelRatio, margin }: Tile): View {
    const n = 2 ** z;
    if (!Number.isInteger(z) || z < 0 || z > 30)
        throw new RangeError('z must be an integer in 0..=30');
    if (
        !Number.isInteger(x) ||
        !Number.isInteger(y) ||
        x < 0 ||
        y < 0 ||
        x >= n ||
        y >= n
    )
        throw new RangeError('x and y must be integers in 0..2^z');
    if (!Number.isInteger(tileSize) || tileSize < 1)
        throw new RangeError('tileSize must be a positive integer');
    return {
        center: [
            ((x + 0.5) / n) * 360 - 180,
            (Math.atan(Math.sinh(Math.PI * (1 - (2 * (y + 0.5)) / n))) * 180) /
                Math.PI,
        ],
        // MapLibre's zoom puts the world in 512 CSS pixels at zoom 0.
        zoom: z + Math.log2(tileSize / 512),
        width: tileSize,
        height: tileSize,
        pixelRatio,
        margin,
    };
}

const MAX_LAT = 85.0511287798066;

function fitLngLat({
    bbox: [west, south, east, north],
    size,
    pixelRatio,
    margin,
}: LngLatBox): View {
    if (!Number.isInteger(size) || size < 1 || size > 0xffffffff)
        throw new RangeError('size must be a positive uint32 integer');
    if (east < west) east += 360;
    [south, north] = [south, north].map((lat) =>
        Math.max(-MAX_LAT, Math.min(MAX_LAT, lat)),
    );
    if (!(east > west && north > south))
        throw new RangeError(
            'bbox must be [west, south, east, north] with a non-empty area',
        );
    // Web Mercator y in world units (0 at the north edge, 1 at the south edge).
    const my = (lat: number) =>
        (1 -
            Math.log(Math.tan(Math.PI / 4 + (lat * Math.PI) / 360)) / Math.PI) /
        2;
    const [y0, y1] = [my(north), my(south)];
    const [dx, dy] = [(east - west) / 360, y1 - y0];
    // CSS pixels per world: the long side spans `size`.
    const scale = size / Math.max(dx, dy);
    const lng = (west + east) / 2;
    return {
        center: [
            lng > 180 ? lng - 360 : lng,
            (Math.atan(Math.sinh(Math.PI * (1 - (y0 + y1)))) * 180) / Math.PI,
        ],
        zoom: Math.log2(scale / 512),
        // The long side is `size`; the short one is rounded up (as chiitiler 1.x did).
        width: dx >= dy ? size : Math.max(1, Math.ceil((dx / dy) * size)),
        height: dx >= dy ? Math.max(1, Math.ceil((dy / dx) * size)) : size,
        pixelRatio,
        margin,
    };
}

function flattenFillExtrusion(
    layer: FillExtrusionLayerSpecification,
): FillLayerSpecification {
    const { type, paint, layout, ...base } = layer;
    // Shared paint properties have the same defaults; leaving absent values unset
    // preserves them without duplicating Style Spec defaults. Expressions stay intact.
    return {
        ...base,
        type: 'fill',
        layout: { visibility: layout?.visibility },
        paint: {
            'fill-color': paint?.['fill-extrusion-color'],
            'fill-opacity': paint?.['fill-extrusion-opacity'],
            'fill-pattern': paint?.['fill-extrusion-pattern'],
            'fill-translate': paint?.['fill-extrusion-translate'],
            'fill-translate-anchor': paint?.['fill-extrusion-translate-anchor'],
            'fill-color-transition': paint?.['fill-extrusion-color-transition'],
            'fill-opacity-transition':
                paint?.['fill-extrusion-opacity-transition'],
            'fill-pattern-transition':
                paint?.['fill-extrusion-pattern-transition'],
            'fill-translate-transition':
                paint?.['fill-extrusion-translate-transition'],
        },
    };
}

/** `transformLayers`' default: a fill-extrusion is flattened into a fill, other layers are kept. */
export function defaultTransformLayers(
    layer: LayerSpecification,
): LayerSpecification[] {
    return [
        layer.type === 'fill-extrusion' ? flattenFillExtrusion(layer) : layer,
    ];
}

function prepareStyle(
    style: StyleSpecification,
    transform: NonNullable<
        InitOptions['transformLayers']
    > = defaultTransformLayers,
): StyleSpecification {
    if (!style || typeof style !== 'object' || !Array.isArray(style.layers)) {
        throw new Error('Style must contain a layers array');
    }
    const layers = style.layers.flatMap((layer: unknown) => {
        if (!layer || typeof layer !== 'object' || !('type' in layer))
            return [layer];
        const replacements = transform(layer as LayerSpecification);
        if (
            !Array.isArray(replacements) ||
            !replacements.every(
                (r) =>
                    r === layer ||
                    (r &&
                        typeof r.id === 'string' &&
                        SUPPORTED_LAYERS.has(r.type)),
            )
        ) {
            throw new Error(
                `Invalid replacement for ${String(layer.type)} layer ${'id' in layer ? String(layer.id) : ''}`,
            );
        }
        return replacements;
    });
    return { ...style, layers } as StyleSpecification;
}

/** Render a MapLibre Style as a static 2D RGBA image with the Node-API renderer. */
export class MapRenderer {
    /** Renderers in use, at most `concurrency` (none kept with a TTL of 0). */
    private cores: Core[] = [];

    private constructor(
        private readonly style: StyleSpecification,
        private readonly loadResource: InitOptions['loadResource'],
        private readonly ttlMs: number,
        private readonly concurrency: number,
    ) {}

    /** Snapshot a style object. Resource URLs are passed to `loadResource` as written. */
    static async init(
        style: StyleSpecification,
        options: InitOptions,
    ): Promise<MapRenderer> {
        const ttlMs = options.cache?.ttlMs ?? DEFAULT_TTL_MS;
        if (ttlMs !== Infinity && !(Number.isFinite(ttlMs) && ttlMs >= 0))
            throw new RangeError(
                'cache.ttlMs must be a finite, non-negative number',
            );
        const concurrency = options.concurrency ?? 1;
        if (!Number.isInteger(concurrency) || concurrency < 1)
            throw new RangeError('concurrency must be a positive integer');
        return new MapRenderer(
            prepareStyle(style, options.transformLayers),
            options.loadResource,
            ttlMs,
            concurrency,
        );
    }

    /** Fetch required resources and return a new RGBA image of a Web Mercator camera. */
    async renderView(view: View): Promise<RenderedImage> {
        const args = nativeView(view);
        const data = await this.draw((r) => r.renderView(args));
        return image(data, Math.round(args.width * args.pixelRatio));
    }

    /** `renderView` of an XYZ tile of the Web Mercator grid. */
    async renderTile(tile: Tile): Promise<RenderedImage> {
        return this.renderView(tileView(tile));
    }

    /** `renderView` of a longitude/latitude box, `box.size` CSS pixels on the long side. */
    async render(box: LngLatBox): Promise<RenderedImage> {
        return this.renderView(fitLngLat(box));
    }

    /**
     * Render in `crs`: a box in CRS units (easting first, y up), a tile of its grid, or a camera
     * (center in degrees, zoom of its grid). Invalid CRSs, grids and tiles reject with a `RangeError`.
     */
    async renderProjected(
        crs: Crs,
        target: { tile: Tile } | { box: LngLatBox } | { view: View },
    ): Promise<RenderedImage> {
        let render: (r: ProjectedRenderer) => Promise<Uint8Array>;
        /** Device pixels across, from the RGBA pixel count. */
        let width: (pixels: number) => number;
        if ('tile' in target) {
            const { tileSize = 256, pixelRatio = 1, margin = 0 } = target.tile;
            const tile = { ...target.tile, tileSize, pixelRatio, margin };
            render = (r) => r.renderTile(tile);
            width = () => Math.round(tileSize * pixelRatio);
        } else if ('box' in target) {
            const { bbox, size, pixelRatio = 1, margin = 0 } = target.box;
            const [minX, minY, maxX, maxY] = bbox;
            const long = Math.round(size * pixelRatio);
            render = (r) => r.render({ bbox, size, pixelRatio, margin });
            // `size` is the long side; the other follows the box's aspect ratio
            width = (pixels) =>
                maxX - minX >= maxY - minY ? long : pixels / long;
        } else {
            const args = nativeView(target.view);
            render = (r) => r.renderView(args);
            width = () => Math.round(args.width * args.pixelRatio);
        }
        const data = await this.draw((r) => drawProjected(r, crs, render));
        return image(data, width(data.length / 4));
    }

    /** Draw with a renderer; one whose render failed is not reused. */
    private async draw(
        render: (renderer: Renderer) => Promise<Uint8Array>,
    ): Promise<Bytes> {
        const core = this.acquire();
        try {
            return (await render(core.renderer)) as Bytes;
        } catch (error) {
            // A failed render may leave the renderer inconsistent: later renders use a new one.
            this.retire(core);
            throw error;
        } finally {
            core.active--;
        }
    }

    /**
     * The least busy renderer, a new one while all are busy and fewer than `concurrency` exist.
     * Renderers older than the cache TTL are replaced. With a TTL of 0, each render gets its own.
     */
    private acquire(): Core {
        const now = performance.now();
        for (const core of this.cores) {
            if (now - core.createdAt >= this.ttlMs) this.retire(core);
        }
        let core = this.cores.reduce<Core | undefined>(
            (best, c) =>
                best === undefined || c.active < best.active ? c : best,
            undefined,
        );
        if (
            !core ||
            (core.active > 0 && this.cores.length < this.concurrency)
        ) {
            core = {
                renderer: new Renderer(this.style, {
                    loadResource: this.loadResource,
                }),
                createdAt: now,
                active: 0,
            };
            if (this.ttlMs !== 0) this.cores.push(core);
        }
        core.active++;
        return core;
    }

    /** Stop handing `core` to new renders; it is dropped once its renders end. */
    private retire(core: Core) {
        this.cores = this.cores.filter((c) => c !== core);
    }
}

// The addon returns ArrayBuffer-backed bytes; its types only say Uint8Array.
type Bytes = Uint8Array<ArrayBuffer>;

/** `render` in `crs`; errors of its CRS, grid or tile ("unsupported: …" from the addon) as `RangeError`s. */
async function drawProjected(
    renderer: Renderer,
    crs: Crs,
    render: (r: ProjectedRenderer) => Promise<Uint8Array>,
): Promise<Uint8Array> {
    try {
        return await render(renderer.projected(crs));
    } catch (error) {
        if (
            error instanceof TypeError ||
            (error instanceof Error && error.message.startsWith('unsupported:'))
        )
            throw new RangeError(error.message);
        throw error;
    }
}

type NapiImage = typeof import('@napi-rs/image');
let napiImage: Promise<NapiImage | null> | undefined;
/** `@napi-rs/image`, an optional peer dependency for lossy WebP and AVIF: loaded on first use, null if absent. */
function loadNapiImage(): Promise<NapiImage | null> {
    napiImage ??= import('@napi-rs/image').catch(() => null);
    return napiImage;
}
let warnedLossless = false;

function checkQuality(quality: number): void {
    if (!Number.isFinite(quality))
        throw new RangeError('quality must be finite');
}

/** `width` device pixels wide; the height follows from the bytes (tiles of a grid need not be square). */
export function image(data: Bytes, width: number): RenderedImage {
    const height = data.length / 4 / width;
    return {
        width,
        height,
        data,
        toPng: () => encodePng(data, width, height) as Promise<Bytes>,
        toWebp: async (quality) => {
            checkQuality(quality);
            const napi = quality < 100 ? await loadNapiImage() : null;
            if (!napi) {
                if (quality < 100 && !warnedLossless) {
                    warnedLossless = true;
                    console.warn(
                        'lossy WebP needs @napi-rs/image: install it. Encoding lossless WebP instead.',
                    );
                }
                return encodeWebp(data, width, height) as Promise<Bytes>;
            }
            const webp = await napi.Transformer.fromRgbaPixels(
                data,
                width,
                height,
            ).webp(Math.max(0, quality));
            return webp as Bytes;
        },
        toJpeg: async (quality) => {
            checkQuality(quality);
            const q = Math.min(100, Math.max(1, Math.round(quality)));
            return encodeJpeg(data, width, height, q) as Promise<Bytes>;
        },
        toAvif: async (quality) => {
            checkQuality(quality);
            const napi = await loadNapiImage();
            if (!napi) throw new Error('AVIF needs @napi-rs/image: install it');
            // rav1e's fastest preset: the default (4) takes 20–30 times longer for a 20% smaller
            // tile. Full chroma (the default): unlike lossy WebP's 4:2:0, it keeps thin colored
            // lines sharp, and beats WebP at the same size from mid quality on. One thread, as
            // encodes already run in parallel on libuv's thread pool.
            const avif = await napi.Transformer.fromRgbaPixels(
                data,
                width,
                height,
            ).avif({
                quality: Math.min(100, Math.max(0, quality)),
                speed: 10,
                threads: 1,
            });
            return avif as Bytes;
        },
    };
}
