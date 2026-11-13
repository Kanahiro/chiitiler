import type {
    FillExtrusionLayerSpecification,
    FillLayerSpecification,
    LayerSpecification,
    SourceSpecification,
    StyleSpecification,
} from '@maplibre/maplibre-gl-style-spec';

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
 * A longitude/latitude box in Web Mercator. The image has the box's aspect ratio, `size` on its long side,
 * at the zoom that fits the box into it.
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

/**
 * A CRS on MapLibre GL JS's tile grid for it: what `addProjection` registers, with the CRS as PROJJSON.
 */
export interface Crs {
    /** The name GL JS knows the CRS by (`addProjection`'s `name`, `projection.type` of `toStyle`'s style). */
    name: string;
    /**
     * PROJJSON of a geographic or projected CRS. Supported methods: Transverse Mercator, Mercator, Pseudo Mercator,
     * Lambert Conic Conformal, Polar Stereographic, Lambert Azimuthal Equal Area, Equal Earth and PROJ's
     * Geostationary Satellite, without datum shifts.
     */
    projjson: object;
    tileMatrixSet: TileMatrixSet;
}

/**
 * A box of a projected CRS (see `MapRenderer.projected`). Vector layers (background, fill, line, circle, heatmap,
 * symbol) are drawn in it with projected vertices; raster, hillshade and color-relief are rendered in Web Mercator
 * and warped.
 */
export interface ProjectedBox {
    /** In CRS units (degrees for longlat), always easting/longitude first whatever the CRS's axis order. */
    bbox: [minX: number, minY: number, maxX: number, maxY: number];
    width: number;
    height: number;
    pixelRatio?: number;
    /** Non-negative integer CSS pixels of extra coverage, cropped from the output. Default: 0. */
    margin?: number;
}

/**
 * MapLibre GL JS's `TileMatrixSet` (`addProjection`): tile 0/0/0 is the square of side `extentAtZoom0` whose
 * top-left corner is `origin`, and each zoom splits every tile in four; x grows east, y south.
 */
export interface TileMatrixSet {
    /** CRS coordinates of the top-left corner of tile 0/0/0: [min x, max y]. */
    origin: [number, number];
    /** Width (= height) of tile 0/0/0 in CRS units. */
    extentAtZoom0: number;
}

/** A source of the style in a tile of a CRS's grid (see `ProjectedRenderer.getTile`). */
export interface SourceTile {
    /** A vector, GeoJSON, raster, raster-dem or image source of the style. */
    source: string;
    z: number;
    x: number;
    y: number;
}

/** A `MapRenderer` drawing in a CRS (see `MapRenderer.projected`). */
export interface ProjectedRenderer {
    readonly crs: Crs;
    /** Fetch required resources and return a new RGBA image of `box.bbox` (CRS units). */
    render(box: ProjectedBox): Promise<RenderedImage>;
    /** `render` of tile `z/x/y` of the CRS's grid, `tileSize` CSS pixels (default 256) on a side. */
    renderTile(tile: Tile): Promise<RenderedImage>;
    /**
     * Fetch the data of `tile.source` and return it in tile `z/x/y` of the CRS's grid, for MapLibre GL JS to draw the
     * style in the CRS from (see `toStyle`), not a picture of the map:
     * - vector / GeoJSON: a vector tile (MVT, extent 4096). Layers, features and properties are kept as they are (a
     *   GeoJSON source's layer is named after the source).
     * - raster / image: a 512 px PNG of the source as a raster layer with default paint draws it.
     * - raster-dem: a 512 px PNG of its elevations in the `terrarium` encoding.
     *
     * Source data is fetched at the zoom of the tile drawn at 512 CSS pixels.
     */
    getTile(tile: SourceTile): Promise<Uint8Array<ArrayBuffer>>;
    /**
     * The style for MapLibre GL JS to draw in the CRS (registered with `addProjection` as `crs.name`) from tiles of
     * `getTile`, whose URL templates (`{z}/{x}/{y}` of the CRS's grid) `tiles` gives for each source: vector and
     * GeoJSON sources become vector sources (layers of a GeoJSON source read the layer named after it), raster and
     * image sources raster sources, raster-dem sources terrarium ones, all of 512 px tiles. Layers of other sources
     * (video) are left out. The style is the one given to `init`, fill-extrusion layers included.
     */
    toStyle(options: { tiles: (source: string) => string }): StyleSpecification;
}

export interface RenderedImage {
    width: number;
    height: number;
    /** Row-major, 8-bit RGBA pixels. */
    data: Uint8ClampedArray<ArrayBuffer>;
    /** Encode `data` inside WASM as an owned, lossless RGBA8 PNG. */
    toPng(): Uint8Array<ArrayBuffer>;
    /** Encode `data` inside WASM as an owned, lossless WebP. */
    toWebp(): Uint8Array<ArrayBuffer>;
    /** Encode `data` inside WASM as an owned JPEG of `quality` (1–100), flattened over black. */
    toJpeg(quality: number): Uint8Array<ArrayBuffer>;
}

/** Style Spec layers supported by the 2D renderer. */
export type StyleLayer = Exclude<
    LayerSpecification,
    FillExtrusionLayerSpecification
>;

export type FillExtrusionLayer = FillExtrusionLayerSpecification;
export type { StyleSpecification };

export interface InitOptions {
    /** Load complete resource bytes, or null for a missing resource (HTTP 404). Other failures must reject. */
    loadResource?: (url: string) => Promise<Uint8Array<ArrayBuffer> | null>;
    /** Precompiled WASM (required on Cloudflare Workers). Defaults to the bundled asset in browsers. */
    wasm?: WebAssembly.Module;
    /**
     * Decoded resources (tiles and their layer geometry, sprites, glyphs) are kept inside the
     * renderer and reused by later renders; the first render ttlMs or more after they started
     * being kept drops them. 0 disables reuse, Infinity never expires. Default: 5 minutes.
     * With 0, a render also drops each layer bucket once drawn, which lowers its peak memory
     * (e.g. on Cloudflare Workers, whose 128 MB includes WASM memory, which never shrinks).
     * Fetched bytes are not cached: rely on the HTTP cache, or cache in `loadResource`.
     */
    cache?: { ttlMs?: number };
    /**
     * Run this renderer's renders one at a time with the other exclusive renders on the same WASM
     * instance (renderers of one module with `cache.ttlMs` 0 share it), from the first fetch to the image. Overlapping
     * renders otherwise each hold their decoded resources while they await fetches, so WASM memory
     * grows with their number; exclusive renders need only as much as the largest one. Use it where
     * memory is tight, e.g. on Cloudflare Workers (128 MB per isolate, serving concurrent requests).
     * Default: false.
     */
    exclusive?: boolean;
    /** Replace each fill-extrusion layer. Defaults to a fill preserving its 2D appearance. */
    transformFillExtrusion?: (layer: FillExtrusionLayer) => StyleLayer;
}

interface WasmExports {
    memory: WebAssembly.Memory;
    alloc(len: number): number;
    dealloc(ptr: number, len: number): void;
    out_ptr(): number;
    out_len(): number;
    renderer_new(stylePtr: number, styleLen: number, ...view: ViewArgs): number;
    renderer_set_view(handle: number, ...view: ViewArgs): void;
    renderer_set_projected_view(
        handle: number,
        projjsonPtr: number,
        projjsonLen: number,
        minX: number,
        minY: number,
        maxX: number,
        maxY: number,
        width: number,
        height: number,
        pixelRatio: number,
        margin: number,
    ): number;
    renderer_set_ticket(handle: number, ticket: number): void;
    renderer_set_keep_buckets(handle: number, keep: number): void;
    renderer_needs(handle: number): number;
    renderer_provide(
        handle: number,
        urlPtr: number,
        urlLen: number,
        dataPtr: number,
        dataLen: number,
        found: number,
    ): void;
    renderer_render(handle: number, oldest: number): number;
    renderer_set_export_source(
        handle: number,
        ptr: number,
        len: number,
    ): number;
    renderer_export_tile(handle: number, oldest: number): number;
    encode_png(ptr: number, len: number, width: number, height: number): number;
    encode_webp(
        ptr: number,
        len: number,
        width: number,
        height: number,
    ): number;
    encode_jpeg(
        ptr: number,
        len: number,
        width: number,
        height: number,
        quality: number,
    ): number;
    renderer_free(handle: number): void;
    projection_new(ptr: number, len: number): number;
    projection_forward(handle: number, lng: number, lat: number): number;
    projection_inverse(handle: number, x: number, y: number): number;
    projection_free(handle: number): void;
    xy_ptr(): number;
}

/** A CRS's projection, computed by the renderer itself. */
export interface Projection {
    /** `[lng, lat]` in degrees → `[x, y]` in CRS units; NaN where the CRS has no point (e.g. the far side of a geostationary view). */
    forward(lngLat: number[]): number[];
    /** `[x, y]` → `[lng, lat]` in degrees; NaN outside the projection's domain. */
    inverse(xy: number[]): number[];
}

type ViewArgs = [
    lng: number,
    lat: number,
    zoom: number,
    bearing: number,
    width: number,
    height: number,
    pixelRatio: number,
    margin: number,
];

/** A WASM renderer shared by overlapping renders; freed once retired and idle. */
interface Core {
    handle: number;
    createdAt: number;
    active: number;
    retired: boolean;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();
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

async function fetchResource(
    url: string,
): Promise<Uint8Array<ArrayBuffer> | null> {
    const response = await fetch(url);
    if (!response.ok) {
        await response.body?.cancel();
        if (response.status === 404) return null;
        throw new Error(
            `Resource fetch failed: HTTP ${response.status} ${url}`,
        );
    }
    return new Uint8Array(await response.arrayBuffer());
}

/** The bundled module, compiled once. */
let bundled: Promise<WebAssembly.Module> | undefined;

async function loadWasm(
    module: WebAssembly.Module | undefined,
): Promise<WasmExports> {
    if (!module) {
        // Vite bundles this asset for browsers. Workers supply their imported module and
        // never fetch or compile WASM at runtime. Fetched directly: `loadResource` serves the style.
        if (!bundled) {
            const url = new URL('../wasm/ml2drender_wasm.wasm', import.meta.url)
                .href;
            bundled = fetchResource(url).then((bytes) => {
                if (bytes === null) throw new Error(`WASM not found: ${url}`);
                return WebAssembly.compile(bytes);
            });
            // A failed load is retried by the next call.
            bundled.catch(() => {
                bundled = undefined;
            });
        }
        module = await bundled;
    }
    const instance = await WebAssembly.instantiate(module);
    return instance.exports as unknown as WasmExports;
}

/** Copied output buffer of the last call. */
function output(w: WasmExports): Uint8Array<ArrayBuffer> {
    return new Uint8Array(w.memory.buffer, w.out_ptr(), w.out_len()).slice();
}

function withBytes<T>(
    w: WasmExports,
    bytes: Uint8Array,
    call: (ptr: number, len: number) => T,
): T {
    const ptr = w.alloc(bytes.length);
    new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);
    try {
        return call(ptr, bytes.length);
    } finally {
        w.dealloc(ptr, bytes.length);
    }
}

/**
 * One WASM instance per module (the bundled one under `undefined`), shared by projections and by
 * renderers that keep nothing between renders (`ttlMs` 0): memory freed by one render is reused by
 * the next. (WASM memory never shrinks, and an instance per renderer holds its own until collected;
 * on Cloudflare Workers all of it counts towards the isolate's 128 MB.) A renderer that keeps
 * resources gets its own instance, which goes with it.
 */
const instances = new Map<
    WebAssembly.Module | undefined,
    Promise<WasmExports>
>();
/** Instances a trap (Rust panic or out of memory) left in an unknown state. */
const brokenInstances = new WeakSet<WasmExports>();

function sharedWasm(
    module: WebAssembly.Module | undefined,
): Promise<WasmExports> {
    let instance = instances.get(module);
    if (!instance) {
        const loading = loadWasm(module);
        instances.set(module, loading);
        // A failed load is retried by the next call.
        loading.catch(() => {
            if (instances.get(module) === loading) instances.delete(module);
        });
        instance = loading;
    }
    return instance;
}

/** Later loads get a new instance. */
function discardInstance(
    module: WebAssembly.Module | undefined,
    instance: Promise<WasmExports>,
    w: WasmExports,
) {
    brokenInstances.add(w);
    if (instances.get(module) === instance) instances.delete(module);
}

const freeProjection = new FinalizationRegistry<{
    w: WasmExports;
    handle: number;
}>(({ w, handle }) => w.projection_free(handle));

/**
 * Load a PROJJSON CRS the way `MapRenderer.projected` reads `Crs.projjson`, e.g. to compute
 * tile bounds or to register the CRS with MapLibre's `addProjection`, so both agree on the CRS.
 * Throws for a CRS the renderer does not support.
 */
export async function loadProjection(
    projjson: object,
    options: Pick<InitOptions, 'wasm'> = {},
): Promise<Projection> {
    return (await loadProjectionFactory(options))(projjson);
}

/**
 * Load the WASM once and return a synchronous `loadProjection`, for hosts that must build a
 * projection without awaiting, such as MapLibre's worker `setProjectionHandler`.
 */
export async function loadProjectionFactory(
    options: Pick<InitOptions, 'wasm'> = {},
): Promise<(projjson: object) => Projection> {
    const w = await sharedWasm(options.wasm);
    return (projjson) => {
        if (!projjson || typeof projjson !== 'object')
            throw new TypeError('projjson must be a PROJJSON object');
        const handle = withBytes(
            w,
            encoder.encode(JSON.stringify(projjson)),
            (ptr, len) => w.projection_new(ptr, len),
        );
        if (!handle) throw new Error(decoder.decode(output(w)));
        // A fresh view per call: memory growth detaches earlier ones.
        const xy = () => [...new Float64Array(w.memory.buffer, w.xy_ptr(), 2)];
        // The methods hold `owner`, which frees the handle once collected: callers may keep only them.
        const owner = { handle };
        const projection: Projection = {
            forward: ([lng, lat]) => {
                w.projection_forward(owner.handle, lng, lat);
                return xy();
            },
            inverse: ([x, y]) => {
                w.projection_inverse(owner.handle, x, y);
                return xy();
            },
        };
        freeProjection.register(owner, { w, handle });
        return projection;
    };
}

function viewArgs(view: View): ViewArgs {
    const [lng, lat] = view.center;
    for (const [name, value] of Object.entries({
        lng,
        lat,
        zoom: view.zoom,
        bearing: view.bearing ?? 0,
    })) {
        if (!Number.isFinite(value))
            throw new RangeError(`${name} must be finite`);
    }
    return [
        lng,
        lat,
        view.zoom,
        view.bearing ?? 0,
        ...sizeArgs(view),
        marginArg(view.margin),
    ];
}

/** Width and height (positive uint32 integers) and a finite, positive pixel ratio. */
function sizeArgs({
    width,
    height,
    pixelRatio = 1,
}: {
    width: number;
    height: number;
    pixelRatio?: number;
}): [number, number, number] {
    for (const [name, value] of [
        ['width', width],
        ['height', height],
    ] as const) {
        if (!Number.isInteger(value) || value < 1 || value > 0xffffffff)
            throw new RangeError(`${name} must be a positive uint32 integer`);
    }
    if (!Number.isFinite(pixelRatio) || pixelRatio <= 0)
        throw new RangeError('pixelRatio must be finite and positive');
    return [width, height, pixelRatio];
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
        width: Math.max(1, Math.round(dx * scale)),
        height: Math.max(1, Math.round(dy * scale)),
        pixelRatio,
        margin,
    };
}

function marginArg(value = 0): number {
    const margin = nonnegative(value, 'margin');
    if (!Number.isInteger(margin) || margin > 0xffffffff)
        throw new RangeError('margin must be a uint32 integer');
    return margin;
}

function nonnegative(value: number, name: string): number {
    if (!Number.isFinite(value) || value < 0)
        throw new RangeError(`${name} must be a finite, non-negative number`);
    return value;
}

function flattenFillExtrusion(
    layer: FillExtrusionLayer,
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

function prepareStyle(
    style: StyleSpecification,
    transform: NonNullable<
        InitOptions['transformFillExtrusion']
    > = flattenFillExtrusion,
): Uint8Array {
    if (!style || typeof style !== 'object' || !Array.isArray(style.layers)) {
        throw new Error('Style must contain a layers array');
    }
    const layers = style.layers.map((layer: unknown) => {
        if (
            !layer ||
            typeof layer !== 'object' ||
            !('type' in layer) ||
            layer.type !== 'fill-extrusion'
        )
            return layer;
        const replacement = transform(layer as FillExtrusionLayer);
        if (
            !replacement ||
            typeof replacement.id !== 'string' ||
            !SUPPORTED_LAYERS.has(replacement.type)
        ) {
            throw new Error(
                `Invalid replacement for fill-extrusion layer ${'id' in layer ? String(layer.id) : ''}`,
            );
        }
        return replacement;
    });
    return encoder.encode(JSON.stringify({ ...style, layers }));
}

/** Per instance: the end of the last exclusive render queued (see `InitOptions.exclusive`). */
const exclusiveRenders = new WeakMap<WasmExports, Promise<void>>();

/** Render a MapLibre Style as a static 2D RGBA image. */
export class MapRenderer {
    /** The reused core (none with a TTL of 0). */
    private core?: Core;
    /** A trap (Rust panic or out of memory) left the WASM instance in an unknown state. */
    private broken?: unknown;
    /** Fetches in flight, shared by overlapping renders, with the newest ticket that awaits each. */
    private readonly pending = new Map<
        string,
        { request: Promise<void>; ticket: number }
    >();
    /** Tickets of the renders in progress: WASM keeps the tiles they need (`renderer_set_ticket`). */
    private readonly tickets = new Set<number>();
    private lastTicket = 0;

    private constructor(
        private readonly module: WebAssembly.Module | undefined,
        private readonly instance: Promise<WasmExports>,
        private readonly wasm: WasmExports,
        private readonly style: Uint8Array,
        /** The style as given, for `toStyle`. */
        private readonly original: StyleSpecification,
        private readonly loadResource: NonNullable<InitOptions['loadResource']>,
        private readonly ttlMs: number,
        private readonly exclusive: boolean,
    ) {}

    /** Snapshot a style object and load the renderer. Resource URLs are passed to `loadResource` as written. */
    static async init(
        style: StyleSpecification,
        options: InitOptions = {},
    ): Promise<MapRenderer> {
        const ttlMs = options.cache?.ttlMs ?? DEFAULT_TTL_MS;
        if (ttlMs !== Infinity) nonnegative(ttlMs, 'cache.ttlMs');
        const loadResource = options.loadResource ?? fetchResource;
        const prepared = prepareStyle(style, options.transformFillExtrusion);
        const instance =
            ttlMs === 0 ? sharedWasm(options.wasm) : loadWasm(options.wasm);
        return new MapRenderer(
            options.wasm,
            instance,
            await instance,
            prepared,
            structuredClone(style),
            loadResource,
            ttlMs,
            options.exclusive ?? false,
        );
    }

    /** Fetch required resources and return a new RGBA image of a Web Mercator camera. */
    async renderView(view: View): Promise<RenderedImage> {
        const args = viewArgs(view);
        return this.draw(view, args, (handle) =>
            this.wasm.renderer_set_view(handle, ...args),
        );
    }

    /** `renderView` of an XYZ tile of the Web Mercator grid. */
    async renderTile(tile: Tile): Promise<RenderedImage> {
        return this.renderView(tileView(tile));
    }

    /** `renderView` of a longitude/latitude box, `box.size` CSS pixels on the long side. */
    async render(box: LngLatBox): Promise<RenderedImage> {
        return this.renderView(fitLngLat(box));
    }

    /** This renderer drawing in `crs`, on its tile grid. Resources fetched for one CRS serve the others too. */
    projected(crs: Crs): ProjectedRenderer {
        if (typeof crs.name !== 'string' || !crs.name)
            throw new TypeError('crs.name must be a name');
        if (!crs.projjson || typeof crs.projjson !== 'object')
            throw new TypeError('crs.projjson must be a PROJJSON object');
        const { origin, extentAtZoom0 } = crs.tileMatrixSet;
        if (
            !origin.every(Number.isFinite) ||
            !(extentAtZoom0 > 0 && Number.isFinite(extentAtZoom0))
        ) {
            throw new RangeError(
                'crs.tileMatrixSet must have a finite origin and a finite, positive extentAtZoom0',
            );
        }
        const projjson = encoder.encode(JSON.stringify(crs.projjson));
        // The box of tile z/x/y of the grid.
        const box = ({
            z,
            x,
            y,
        }: {
            z: number;
            x: number;
            y: number;
        }): ProjectedBox['bbox'] => {
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
            const s = extentAtZoom0 / n;
            const [minX, maxY] = [origin[0] + x * s, origin[1] - y * s];
            return [minX, maxY - s, minX + s, maxY];
        };
        const render = (view: ProjectedBox) =>
            this.draw(view, ...this.projectedArgs(projjson, view));
        return {
            crs,
            render,
            renderTile: ({ tileSize = 256, pixelRatio, margin, ...tile }) => {
                if (!Number.isInteger(tileSize) || tileSize < 1)
                    throw new RangeError('tileSize must be a positive integer');
                return render({
                    bbox: box(tile),
                    width: tileSize,
                    height: tileSize,
                    pixelRatio,
                    margin,
                });
            },
            getTile: (tile) => {
                if (typeof tile.source !== 'string' || !tile.source)
                    throw new TypeError('source must be a source id');
                const view = { bbox: box(tile), width: 512, height: 512 };
                return this.run(
                    ...this.projectedArgs(projjson, view),
                    encoder.encode(tile.source),
                    this.wasm.renderer_export_tile,
                );
            },
            toStyle: ({ tiles }) => {
                const style = this.original;
                const sources: Record<string, SourceSpecification> = {};
                for (const [id, source] of Object.entries(style.sources)) {
                    const common = {
                        tiles: [tiles(id)],
                        ...('attribution' in source && source.attribution
                            ? { attribution: source.attribution }
                            : {}),
                    };
                    if (source.type === 'vector' || source.type === 'geojson')
                        sources[id] = { type: 'vector', ...common };
                    else if (
                        source.type === 'raster' ||
                        source.type === 'image'
                    )
                        sources[id] = {
                            type: 'raster',
                            tileSize: 512,
                            ...common,
                        };
                    else if (source.type === 'raster-dem')
                        sources[id] = {
                            type: 'raster-dem',
                            tileSize: 512,
                            encoding: 'terrarium',
                            ...common,
                        };
                }
                const layers = style.layers.flatMap(
                    (layer): LayerSpecification[] => {
                        if (
                            !('source' in layer) ||
                            typeof layer.source !== 'string'
                        )
                            return [layer];
                        if (!sources[layer.source]) return [];
                        return [
                            style.sources[layer.source].type === 'geojson'
                                ? ({
                                      ...layer,
                                      'source-layer': layer.source,
                                  } as LayerSpecification)
                                : layer,
                        ];
                    },
                );
                return structuredClone({
                    ...style,
                    projection: { type: crs.name },
                    sources,
                    layers,
                });
            },
        };
    }

    /** The core arguments and `setView` of a view of the CRS `projjson` (encoded). */
    private projectedArgs(
        projjson: Uint8Array,
        view: ProjectedBox,
    ): [ViewArgs, (handle: number) => void] {
        const margin = marginArg(view.margin);
        const size = sizeArgs(view);
        if (!view.bbox.every(Number.isFinite))
            throw new RangeError('bbox must be finite');
        // The initial camera is replaced before the first `needs`.
        return [
            [0, 0, 0, 0, ...size, 0],
            (handle) => {
                const ok = this.withBytes(projjson, (ptr, len) =>
                    this.wasm.renderer_set_projected_view(
                        handle,
                        ptr,
                        len,
                        ...view.bbox,
                        ...size,
                        margin,
                    ),
                );
                if (!ok) throw new Error(decoder.decode(this.output()));
            },
        ];
    }

    /** The fetch-and-render loop of images; `setView` points a core at the view. */
    private async draw(
        view: View | ProjectedBox,
        args: ViewArgs,
        setView: (handle: number) => void,
    ): Promise<RenderedImage> {
        const w = this.wasm;
        const bytes = await this.run(
            args,
            setView,
            new Uint8Array(),
            w.renderer_render,
        );
        const width = Math.round(view.width * (view.pixelRatio ?? 1));
        const height = Math.round(view.height * (view.pixelRatio ?? 1));
        const data = new Uint8ClampedArray(
            bytes.buffer,
            bytes.byteOffset,
            bytes.byteLength,
        );
        const encode =
            (
                format: (
                    ptr: number,
                    len: number,
                    width: number,
                    height: number,
                ) => number,
            ) =>
            () => {
                if (
                    !this.withBytes(bytes, (ptr, len) =>
                        format(ptr, len, width, height),
                    )
                ) {
                    throw new Error(decoder.decode(this.output()));
                }
                return this.output();
            };
        return {
            width,
            height,
            data,
            toPng: encode(w.encode_png),
            toWebp: encode(w.encode_webp),
            toJpeg: (quality) => {
                if (!Number.isFinite(quality))
                    throw new RangeError('quality must be finite');
                const q = Math.min(100, Math.max(1, Math.round(quality)));
                return encode((ptr, len, width, height) =>
                    w.encode_jpeg(ptr, len, width, height, q),
                )();
            },
        };
    }

    /**
     * The fetch loop of the view `setView` points a core at, for the export source `source` (empty: none), then
     * `finish(handle, oldest)`'s output.
     */
    private async run(
        args: ViewArgs,
        setView: (handle: number) => void,
        source: Uint8Array,
        finish: (handle: number, oldest: number) => number,
    ): Promise<Uint8Array<ArrayBuffer>> {
        let done = () => {};
        if (this.exclusive) {
            const previous = exclusiveRenders.get(this.wasm);
            const mine = new Promise<void>((resolve) => {
                done = resolve;
            });
            exclusiveRenders.set(
                this.wasm,
                (previous ?? Promise.resolve()).then(() => mine),
            );
            await previous;
        }
        try {
            return await this.runOnce(args, setView, source, finish);
        } catch (error) {
            if (error instanceof WebAssembly.RuntimeError) {
                this.broken = error;
                discardInstance(this.module, this.instance, this.wasm);
            }
            throw error;
        } finally {
            done();
        }
    }

    private async runOnce(
        args: ViewArgs,
        setView: (handle: number) => void,
        source: Uint8Array,
        finish: (handle: number, oldest: number) => number,
    ): Promise<Uint8Array<ArrayBuffer>> {
        const w = this.wasm;
        const core = this.acquire(args);
        const ticket = ++this.lastTicket;
        this.tickets.add(ticket);
        try {
            // Other renders may move the camera while this one awaits resources.
            for (let round = 0; ; round++) {
                setView(core.handle);
                if (
                    !this.withBytes(source, (ptr, len) =>
                        w.renderer_set_export_source(core.handle, ptr, len),
                    )
                ) {
                    throw new Error(decoder.decode(this.output()));
                }
                w.renderer_set_ticket(core.handle, ticket);
                const count = w.renderer_needs(core.handle);
                if (count === 0) break;
                if (round >= 32)
                    throw new Error('Resource requests did not settle');
                const urls = decoder.decode(this.output()).split('\n');
                await Promise.all(
                    urls.map((url) => this.fetch(core, url, ticket)),
                );
            }
            // Tickets increase, so the first in the set is the oldest in progress.
            const oldest = this.tickets.values().next().value!;
            if (!finish(core.handle, oldest))
                throw new Error(decoder.decode(this.output()));
            // Own the bytes before another render can overwrite or grow WASM memory.
            return this.output();
        } finally {
            this.tickets.delete(ticket);
            this.release(core);
        }
    }

    /**
     * Fetch `url` once and provide it to `core`, which requested it, before it leaves `pending`. It is
     * kept for the newest render awaiting it: while any of them is in progress, the oldest ticket in
     * progress is at most that one.
     */
    private fetch(core: Core, url: string, ticket: number): Promise<void> {
        const shared = this.pending.get(url);
        if (shared) {
            shared.ticket = Math.max(shared.ticket, ticket);
            return shared.request;
        }
        const entry = { ticket, request: Promise.resolve() };
        entry.request = this.loadResource(url)
            .then((bytes) => {
                // A freed core was retired by the TTL; its successor fetches again.
                if (core.retired && core.active === 0) return;
                this.wasm.renderer_set_ticket(core.handle, entry.ticket);
                this.withBytes(encoder.encode(url), (urlPtr, urlLen) =>
                    this.withBytes(
                        bytes ?? new Uint8Array(),
                        (dataPtr, dataLen) =>
                            this.wasm.renderer_provide(
                                core.handle,
                                urlPtr,
                                urlLen,
                                dataPtr,
                                dataLen,
                                bytes === null ? 0 : 1,
                            ),
                    ),
                );
            })
            .finally(() => this.pending.delete(url));
        this.pending.set(url, entry);
        return entry.request;
    }

    /**
     * The current WASM renderer, replaced once older than the cache TTL. With a TTL of 0, each
     * render gets its own, freed when the render ends.
     */
    private acquire(args: ViewArgs): Core {
        if (this.broken || brokenInstances.has(this.wasm)) {
            throw new Error('The renderer crashed earlier; create a new one', {
                cause: this.broken,
            });
        }
        const now = performance.now();
        if (this.core && now - this.core.createdAt >= this.ttlMs) {
            this.core.retired = true;
            if (this.core.active === 0)
                this.wasm.renderer_free(this.core.handle);
            this.core = undefined;
        }
        let core = this.core;
        if (!core) {
            const handle = this.withBytes(this.style, (ptr, len) =>
                this.wasm.renderer_new(ptr, len, ...args),
            );
            if (!handle) throw new Error(decoder.decode(this.output()));
            // A core that is not reused drops buckets as soon as they are drawn.
            if (this.ttlMs === 0)
                this.wasm.renderer_set_keep_buckets(handle, 0);
            core = {
                handle,
                createdAt: now,
                active: 0,
                retired: this.ttlMs === 0,
            };
            if (!core.retired) this.core = core;
        }
        core.active++;
        return core;
    }

    private release(core: Core) {
        core.active--;
        if (core.retired && core.active === 0)
            this.wasm.renderer_free(core.handle);
    }

    private output(): Uint8Array<ArrayBuffer> {
        return output(this.wasm);
    }

    private withBytes<T>(
        bytes: Uint8Array,
        call: (ptr: number, len: number) => T,
    ): T {
        return withBytes(this.wasm, bytes, call);
    }
}
