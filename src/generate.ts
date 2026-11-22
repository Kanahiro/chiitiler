import cluster, { type Worker } from 'node:cluster';
import { existsSync, rmSync } from 'node:fs';
import { basename, extname, resolve } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { pathToFileURL } from 'node:url';
import process from 'node:process';
import { parseArgs } from 'node:util';

import type { Cache } from './cache/index.js';
import { getSource, type Protocols } from './source/index.js';
import {
    Chiitiler,
    CONCURRENCY,
    encode,
    type RenderTileOptions,
    type SupportedFormat,
} from './render/index.js';

type Bbox = [number, number, number, number];

const MAX_LAT = 85.0511287798066;
const WORLD: Bbox = [-180, -MAX_LAT, 180, MAX_LAT];
const FORMATS = ['png', 'jpeg', 'jpg', 'webp', 'avif'];
/** Side of a block of tiles rendered by one renderer: the children of a tile 4 zooms up. */
const BLOCK = 16;
/**
 * Renderers per thread of the pool: a renderer waits for each source it fetches, and remote ones take
 * long enough to leave a renderer per thread mostly waiting.
 */
const LANES = CONCURRENCY * 4;

type Tile = { z: number; x: number; y: number; row: number };
type Written = [z: number, x: number, row: number, data: Uint8Array];
/** From a renderer process: 'next' for a block, a tile to write, or an http(s) URL to load. */
type Message = 'next' | Written | { load: string };
/** To a renderer process: a block (empty: no more), 'stop', or a URL it asked for. */
type Reply =
    | Tile[]
    | 'stop'
    | { url: string; bytes?: Uint8Array | null; error?: string };

const USAGE = `usage: chiitiler generate-tile --style <url|path> --output <file.mbtiles> --maxzoom <z>
    [--minzoom 0] [--bbox minLon,minLat,maxLon,maxLat] [--ext png] [--tile-size 512] [--quality 100] [--margin 0] [--force]`;

/** The XYZ tiles of zoom `z` covering `bbox`: [minX, minY, maxX, maxY]. */
function tileRange([w, s, e, n]: Bbox, z: number): Bbox {
    const size = 2 ** z;
    const clamp = (v: number) => Math.min(size - 1, Math.max(0, Math.floor(v)));
    const x = (lon: number) => clamp(((lon + 180) / 360) * size);
    const y = (lat: number) => {
        const r = (Math.max(-MAX_LAT, Math.min(MAX_LAT, lat)) * Math.PI) / 180;
        return clamp(
            ((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) *
                size,
        );
    };
    return [x(w), y(n), x(e), y(s)];
}

/**
 * The tiles of zoom `z` in `bbox` by blocks of up to BLOCK × BLOCK (the children of one tile), smaller
 * where a zoom has too few blocks to keep `lanes` busy.
 */
function* blocks(bbox: Bbox, z: number, lanes: number) {
    const [minX, minY, maxX, maxY] = tileRange(bbox, z);
    const count = (side: number) =>
        (Math.floor(maxX / side) - Math.floor(minX / side) + 1) *
        (Math.floor(maxY / side) - Math.floor(minY / side) + 1);
    let side = BLOCK;
    while (side > 1 && count(side) < 2 * lanes) side /= 2;
    for (let bx = minX - (minX % side); bx <= maxX; bx += side)
        for (let by = minY - (minY % side); by <= maxY; by += side) {
            const block = [];
            const lastX = Math.min(bx + side - 1, maxX);
            const lastY = Math.min(by + side - 1, maxY);
            for (let x = Math.max(bx, minX); x <= lastX; x++)
                for (let y = Math.max(by, minY); y <= lastY; y++)
                    block.push({ x, y });
            yield block;
        }
}

function int(name: string, value: string, min: number, max = Infinity) {
    const n = Number(value);
    if (!Number.isSafeInteger(n) || n < min || n > max)
        throw new Error(`--${name} must be an integer from ${min} to ${max}`);
    return n;
}

function readArgs(args: string[]) {
    const { values } = parseArgs({
        args,
        options: {
            style: { type: 'string' },
            output: { type: 'string', short: 'o' },
            minzoom: { type: 'string', default: '0' },
            maxzoom: { type: 'string' },
            bbox: { type: 'string' },
            ext: { type: 'string', default: 'png' },
            'tile-size': { type: 'string', default: '512' },
            quality: { type: 'string', default: '100' },
            margin: { type: 'string', default: '0' },
            force: { type: 'boolean', default: false },
        },
    });
    const { style, output, maxzoom } = values;
    if (style === undefined || output === undefined || maxzoom === undefined)
        throw new Error(USAGE);
    // ponytail: MBTiles only; a directory or .pmtiles output would branch on the extension here.
    if (extname(output) !== '.mbtiles')
        throw new Error(`--output must be a .mbtiles file (got ${output})`);
    if (!FORMATS.includes(values.ext))
        throw new Error(`--ext must be one of ${FORMATS.join(', ')}`);
    const bbox = (values.bbox?.split(',').map(Number) ?? WORLD) as Bbox;
    if (bbox.length !== 4 || bbox.some(Number.isNaN))
        throw new Error('--bbox must be minLon,minLat,maxLon,maxLat');
    const minzoom = int('minzoom', values.minzoom, 0, 30);
    return {
        // A path without a scheme is a local file.
        stylejson: style.includes('://')
            ? style
            : pathToFileURL(resolve(style)).href,
        output,
        force: values.force,
        bbox,
        minzoom,
        maxzoom: int('maxzoom', maxzoom, minzoom, 30),
        ext: values.ext as SupportedFormat,
        tileSize: int('tile-size', values['tile-size'], 1),
        quality: int('quality', values.quality, 1, 100),
        margin: int('margin', values.margin, 0),
    };
}

/**
 * Draw blocks from `next` until it has none, a renderer per lane (LANES of them), and hand
 * each tile to `write`. A lane is fed whole blocks: neighboring tiles share source tiles (all
 * 16×16 of a block overzoomed from one), which its renderer then decodes once, not every renderer.
 */
async function drawLanes(
    cache: Cache,
    { render, ext, quality }: ReturnType<typeof drawOptions>,
    /** The next block; none (empty) when done. */
    next: () => Promise<Tile[]> | Tile[],
    write: (...tile: Written) => void,
    stopping: () => boolean,
    protocols?: Protocols,
) {
    await Promise.all(
        Array.from({ length: LANES }, async () => {
            // No requests to guard against here: any size the machine can draw.
            const chiitiler = new Chiitiler({
                cache,
                protocols,
                maxSize: Infinity,
                concurrency: 1,
            });
            while (!stopping()) {
                const block = await next();
                if (block.length === 0) return;
                const tiles = block.values();
                // Two in flight: one tile's sources load while the other draws.
                await Promise.all(
                    [0, 1].map(async () => {
                        for (const { z, x, y, row } of tiles) {
                            if (stopping()) return;
                            const image = await chiitiler.renderTile({
                                ...render,
                                z,
                                x,
                                y,
                            });
                            write(z, x, row, await encode(image, ext, quality));
                        }
                    }),
                );
            }
        }),
    );
}

function drawOptions(args: string[]) {
    const { output, force, bbox, minzoom, maxzoom, ext, quality, ...render } =
        readArgs(args);
    return {
        render: render satisfies Omit<RenderTileOptions, 'z' | 'x' | 'y'>,
        ext,
        quality,
    };
}

/**
 * A renderer process of `generateTile` (a cluster worker, run with its arguments): blocks come
 * from the primary, tiles go back to it to write. So do http(s) sources: the primary fetches each
 * once for all its renderers.
 */
async function drawForPrimary(args: string[], cache: Cache) {
    let stopping = false;
    // The primary stops its renderers, after the tiles in flight; a terminal's ^C reaches all.
    const ignore = () => {};
    process.on('SIGINT', ignore).on('SIGTERM', ignore);
    const waiting: ((block: Tile[]) => void)[] = [];
    const loads = new Map<string, PromiseWithResolvers<Uint8Array | null>>();
    const receive = (m: Reply) => {
        if (m === 'stop') stopping = true;
        else if (Array.isArray(m)) waiting.shift()?.(m);
        else {
            const load = loads.get(m.url);
            loads.delete(m.url);
            if (m.error === undefined) load?.resolve(m.bytes ?? null);
            else load?.reject(new Error(m.error));
        }
    };
    process.on('message', receive);
    const send = (m: Message) => process.send?.(m);
    // getSource takes one load of a URL at a time
    const load = (url: string) => {
        const pending = Promise.withResolvers<Uint8Array | null>();
        loads.set(url, pending);
        send({ load: url });
        return pending.promise;
    };
    try {
        await drawLanes(
            cache,
            drawOptions(args),
            () =>
                new Promise((resolve) => {
                    waiting.push(resolve);
                    send('next');
                }),
            (...tile) => send(tile),
            () => stopping,
            { http: load, https: load },
        );
    } finally {
        process.off('message', receive);
        // Not process.disconnect(): a worker exits 0 on that, a failed one too.
        cluster.worker?.disconnect();
    }
}

/**
 * Render the tiles of a style into an MBTiles file, without a server. An existing file of the same
 * options is resumed: its tiles are kept and skipped. `--force` starts over.
 *
 * With `processes` above 1, renderer processes (cluster workers run with `args`) draw, and this
 * one writes: one process's thread pool and JS thread bound how many cheap tiles it can draw.
 */
async function generateTile(args: string[], cache: Cache, processes = 1) {
    if (cluster.isWorker) return drawForPrimary(args, cache);
    const { output, force, ...options } = readArgs(args);
    const { bbox, minzoom, maxzoom, ext } = options;

    if (force)
        for (const file of [output, `${output}-wal`, `${output}-shm`])
            rmSync(file, { force: true });
    const resume = existsSync(output);
    const db = new DatabaseSync(output);
    // Handled, not left to the default: node as PID 1 (Docker) ignores signals it has no handler
    // for. The tiles in flight finish and the file closes; a second signal quits at once.
    let stopping = false;
    const renderers: Worker[] = [];
    const stop = () => {
        if (stopping) process.exit(130);
        stopping = true;
        for (const r of renderers) if (r.isConnected()) r.send('stop');
        console.error('stopping after the tiles in flight (again to quit now)');
    };
    process.on('SIGINT', stop).on('SIGTERM', stop);
    try {
        const key = JSON.stringify(options);
        if (resume) {
            let stored: unknown;
            try {
                stored = db
                    .prepare(
                        "SELECT value FROM metadata WHERE name = 'chiitiler'",
                    )
                    .get()?.value;
            } catch {} // not an MBTiles file
            if (stored !== key)
                throw new Error(
                    `${output} exists and was not generated with these options: --force to replace it`,
                );
        } else {
            db.exec(`
                BEGIN;
                CREATE TABLE metadata (name TEXT, value TEXT);
                CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);
                CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);
            `);
            const meta = db.prepare('INSERT INTO metadata VALUES (?, ?)');
            for (const [name, value] of Object.entries({
                name: basename(output, '.mbtiles'),
                format: ext === 'jpeg' ? 'jpg' : ext,
                type: 'baselayer',
                bounds: bbox.join(','),
                minzoom,
                maxzoom,
                chiitiler: key, // the options, to resume with
            }))
                meta.run(name, String(value));
            db.exec('COMMIT');
        }

        // Commits block the JS thread every render waits on: tiles commit once a second, without
        // fsync and without WAL (which writes every tile twice). A stopped or failed run keeps what it
        // wrote; a killed one loses its last second; a power loss may corrupt the file (--force).
        db.exec(
            'PRAGMA journal_mode = DELETE; PRAGMA synchronous = OFF; BEGIN',
        );
        let committed = performance.now();

        const exists = db.prepare(
            'SELECT 1 FROM tiles WHERE zoom_level = ? AND tile_column = ? AND tile_row = ?',
        );
        const insert = db.prepare('INSERT INTO tiles VALUES (?, ?, ?, ?)');
        let skipped = 0;
        function* queued() {
            for (let z = minzoom; z <= maxzoom; z++)
                for (const block of blocks(bbox, z, LANES)) {
                    const todo = [];
                    for (const { x, y } of block) {
                        const row = 2 ** z - 1 - y; // MBTiles rows are TMS
                        if (exists.get(z, x, row)) skipped++;
                        else todo.push({ z, x, y, row });
                    }
                    if (todo.length > 0) yield todo;
                }
        }

        const queue = queued();
        const next = () => (stopping ? [] : (queue.next().value ?? []));
        let done = 0;
        const write = (...tile: Written) => {
            insert.run(...tile);
            if (performance.now() - committed > 1000) {
                db.exec('COMMIT; BEGIN');
                committed = performance.now();
            }
            if (++done % 1000 === 0) console.error(`${done} tiles`);
        };
        if (processes === 1)
            await drawLanes(
                cache,
                drawOptions(args),
                next,
                write,
                () => stopping,
            );
        else {
            // Workers run this file's entry with these arguments, each with its share of the
            // thread pool (UV_THREADPOOL_SIZE, as the server's). Sources they ask for go through
            // this process's cache, the style's past the backing one (as Chiitiler reads it).
            const styleCache: Cache = {
                get: async () => undefined,
                set: async () => {},
                front: cache.front,
            };
            const load = (renderer: Worker, url: string) =>
                getSource(url, url === options.stylejson ? styleCache : cache)
                    .then(
                        (bytes) => ({ url, bytes }),
                        (error: Error) => ({ url, error: error.message }),
                    )
                    .then((reply: Reply) => {
                        if (renderer.isConnected()) renderer.send(reply);
                    });
            cluster.setupPrimary({ serialization: 'advanced' });
            const threads = String(
                Math.max(1, Math.floor(CONCURRENCY / processes)),
            );
            for (let i = 0; i < processes; i++)
                renderers.push(cluster.fork({ UV_THREADPOOL_SIZE: threads }));
            try {
                await Promise.all(
                    renderers.map(
                        (renderer) =>
                            new Promise<void>((resolve, reject) =>
                                renderer
                                    .on('message', (m: Message) => {
                                        if (m === 'next') renderer.send(next());
                                        else if (Array.isArray(m)) write(...m);
                                        else load(renderer, m.load);
                                    })
                                    .on('exit', (code, signal) => {
                                        if (code === 0) resolve();
                                        else
                                            reject(
                                                new Error(
                                                    `a renderer process failed (${signal ?? `exit code ${code}`})`,
                                                ),
                                            );
                                    }),
                            ),
                    ),
                );
            } finally {
                for (const r of renderers) {
                    r.removeAllListeners('message'); // the file closes
                    if (!r.isDead()) r.kill();
                }
            }
        }
        db.exec('COMMIT');
        console.error(
            `${done} tiles written to ${output}${skipped ? ` (${skipped} already there)` : ''}${stopping ? ': stopped, run again to resume' : ''}`,
        );
        if (stopping) process.exitCode = 130;
    } finally {
        process.off('SIGINT', stop).off('SIGTERM', stop);
        if (db.isTransaction) db.exec('COMMIT');
        db.close();
    }
}

export { blocks, generateTile, tileRange };
