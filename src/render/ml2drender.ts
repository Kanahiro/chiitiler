import type {
    FillExtrusionLayerSpecification,
    FillLayerSpecification,
    LayerSpecification,
    StyleSpecification,
} from '@maplibre/maplibre-gl-style-spec';

import { native, type NativeRenderer, type NativeView } from '../native.js';

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
    /** Lossless. */
    toWebp(): Promise<Uint8Array<ArrayBuffer>>;
    /** `quality` 1–100, flattened over black. */
    toJpeg(quality: number): Promise<Uint8Array<ArrayBuffer>>;
}

/** Style Spec layers supported by the 2D renderer. */
export type StyleLayer = Exclude<
    LayerSpecification,
    FillExtrusionLayerSpecification
>;
export type FillExtrusionLayer = FillExtrusionLayerSpecification;

export interface InitOptions {
    /** Load complete resource bytes, or null for a missing resource (HTTP 404). Other failures must reject. */
    loadResource: (url: string) => Promise<Uint8Array | null>;
    /**
     * Decoded resources (tiles and their layer geometry, sprites, glyphs) are kept inside the
     * renderer and reused by later renders; the first render ttlMs or more after they started
     * being kept drops them. 0 disables reuse (and drops each layer's buckets once drawn, for a
     * lower peak), Infinity never expires. Default: 5 minutes.
     */
    cache?: { ttlMs?: number };
    /**
     * Native renderers (each with its own decoded resources) that draw overlapping renders in
     * parallel on libuv's thread pool. Default: 1.
     */
    concurrency?: number;
    /** Replace each fill-extrusion layer. Defaults to a fill preserving its 2D appearance. */
    transformFillExtrusion?: (layer: FillExtrusionLayer) => StyleLayer;
}

/** A native renderer shared by overlapping renders; dropped once retired and idle. */
interface Core {
    renderer: NativeRenderer;
    createdAt: number;
    active: number;
    retired: boolean;
    /** The end of the last operation queued: calls on a renderer must not overlap. */
    queue: Promise<unknown>;
    /** Fetches in flight, shared by overlapping renders, with the newest ticket that awaits each. */
    pending: Map<string, { request: Promise<void>; ticket: number }>;
    /** Tickets of the renders in progress: the renderer keeps the tiles they need. */
    tickets: Set<number>;
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
): string {
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
    return JSON.stringify({ ...style, layers });
}

/** Render a MapLibre Style as a static 2D RGBA image with the Node-API renderer. */
export class MapRenderer {
    /** Cores in use, at most `concurrency` (none kept with a TTL of 0). */
    private cores: Core[] = [];
    private lastTicket = 0;

    private constructor(
        private readonly style: string,
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
        const renderer = new MapRenderer(
            prepareStyle(style, options.transformFillExtrusion),
            options.loadResource,
            ttlMs,
            concurrency,
        );
        // Parse the style now: errors surface here, not at the first render.
        const core = renderer.acquire(
            nativeView(tileView({ z: 0, x: 0, y: 0 })),
        );
        core.active--;
        return renderer;
    }

    /** Fetch required resources and return a new RGBA image of a Web Mercator camera. */
    async renderView(view: View): Promise<RenderedImage> {
        const args = nativeView(view);
        const core = this.acquire(args);
        const ticket = ++this.lastTicket;
        core.tickets.add(ticket);
        try {
            for (let round = 0; ; round++) {
                // Other renders may move the camera while this one awaits resources: point it back
                // and ask what is missing in one operation, and draw in the same one if nothing is.
                const step = await this.exec(core, async () => {
                    core.renderer.setView(args);
                    core.renderer.setTicket(ticket);
                    const urls = await core.renderer.needs();
                    if (urls.length > 0) return { urls };
                    // Tickets increase, so the first in the set is the oldest in progress.
                    const oldest = core.tickets.values().next().value as number;
                    return { data: await core.renderer.render(oldest) };
                });
                if (step.data) return image(step.data, args);
                if (round >= 32)
                    throw new Error('Resource requests did not settle');
                await Promise.all(
                    step.urls.map((url) => this.fetch(core, url, ticket)),
                );
            }
        } finally {
            core.tickets.delete(ticket);
            core.active--;
        }
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
     * Run `op` on `core` after the operations queued before it. A failed operation may leave the
     * renderer inconsistent: later renders use a new core.
     */
    private exec<T>(core: Core, op: () => Promise<T>): Promise<T> {
        const result = core.queue.then(op).catch((error) => {
            this.retire(core);
            throw error;
        });
        core.queue = result.catch(() => {});
        return result;
    }

    /**
     * Fetch `url` once per core and provide it to `core`, which requested it, before it leaves
     * `pending`. It is kept for the newest render awaiting it: while any of them is in progress,
     * the oldest ticket in progress is at most that one.
     */
    private fetch(core: Core, url: string, ticket: number): Promise<void> {
        const shared = core.pending.get(url);
        if (shared) {
            shared.ticket = Math.max(shared.ticket, ticket);
            return shared.request;
        }
        const entry = { ticket, request: Promise.resolve() };
        entry.request = this.loadResource(url)
            .then((bytes) =>
                this.exec(core, async () => {
                    core.renderer.setTicket(entry.ticket);
                    await core.renderer.provide(url, bytes);
                }),
            )
            .finally(() => core.pending.delete(url));
        core.pending.set(url, entry);
        return entry.request;
    }

    /**
     * The least busy core, a new one while all are busy and fewer than `concurrency` exist. Cores
     * older than the cache TTL are replaced. With a TTL of 0, each render gets its own.
     */
    private acquire(view: NativeView): Core {
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
            const renderer = new (native().Renderer)(this.style, view);
            // A core that is not reused drops buckets as soon as they are drawn.
            if (this.ttlMs === 0) renderer.setKeepBuckets(false);
            core = {
                renderer,
                createdAt: now,
                active: 0,
                retired: this.ttlMs === 0,
                queue: Promise.resolve(),
                pending: new Map(),
                tickets: new Set(),
            };
            if (!core.retired) this.cores.push(core);
        }
        core.active++;
        return core;
    }

    /** Stop handing `core` to new renders; it is dropped once its renders end. */
    private retire(core: Core) {
        core.retired = true;
        this.cores = this.cores.filter((c) => c !== core);
    }
}

function image(data: Uint8Array<ArrayBuffer>, view: NativeView): RenderedImage {
    const width = Math.round(view.width * view.pixelRatio);
    const height = Math.round(view.height * view.pixelRatio);
    const n = native();
    return {
        width,
        height,
        data,
        toPng: () => n.encodePng(data, width, height),
        toWebp: () => n.encodeWebp(data, width, height),
        toJpeg: async (quality) => {
            if (!Number.isFinite(quality))
                throw new RangeError('quality must be finite');
            const q = Math.min(100, Math.max(1, Math.round(quality)));
            return n.encodeJpeg(data, width, height, q);
        },
    };
}
