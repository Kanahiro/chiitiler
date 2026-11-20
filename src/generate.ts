import { existsSync, rmSync } from 'node:fs';
import { basename, extname, resolve } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

import type { Cache } from './cache/index.js';
import { Chiitiler, type SupportedFormat } from './render/index.js';

type Bbox = [number, number, number, number];

const MAX_LAT = 85.0511287798066;
const WORLD: Bbox = [-180, -MAX_LAT, 180, MAX_LAT];
const FORMATS = ['png', 'jpeg', 'jpg', 'webp', 'avif'];
/** Tiles in flight: the renderer draws 4 at a time, the rest wait fetched and ready. */
const IN_FLIGHT = 8;

const USAGE = `usage: chiitiler generate-tile --style <url|path> --output <file.mbtiles> --maxzoom <z>
    [--minzoom 0] [--bbox minLon,minLat,maxLon,maxLat] [--ext png] [--tile-size 512] [--quality 100] [--margin 0]`;

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
        },
    });
    const { style, output, maxzoom } = values;
    if (style === undefined || output === undefined || maxzoom === undefined)
        throw new Error(USAGE);
    // ponytail: MBTiles only; a directory or .pmtiles output would branch on the extension here.
    if (extname(output) !== '.mbtiles')
        throw new Error(`--output must be a .mbtiles file (got ${output})`);
    if (existsSync(output)) throw new Error(`${output} already exists`);
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
        bbox,
        minzoom,
        maxzoom: int('maxzoom', maxzoom, minzoom, 30),
        ext: values.ext as SupportedFormat,
        tileSize: int('tile-size', values['tile-size'], 1),
        quality: int('quality', values.quality, 1, 100),
        margin: int('margin', values.margin, 0),
    };
}

/** Render the tiles of a style into an MBTiles file, without a server. */
async function generateTile(args: string[], cache: Cache) {
    const { output, bbox, minzoom, maxzoom, ...render } = readArgs(args);
    // No requests to guard against here: any size the machine can draw.
    const chiitiler = new Chiitiler({ cache, maxSize: Infinity });

    function* tiles() {
        for (let z = minzoom; z <= maxzoom; z++) {
            const [minX, minY, maxX, maxY] = tileRange(bbox, z);
            for (let x = minX; x <= maxX; x++)
                for (let y = minY; y <= maxY; y++) yield { z, x, y };
        }
    }

    const db = new DatabaseSync(output);
    try {
        db.exec(`
            CREATE TABLE metadata (name TEXT, value TEXT);
            CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB);
            CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);
            BEGIN;
        `);
        const meta = db.prepare('INSERT INTO metadata VALUES (?, ?)');
        for (const [name, value] of Object.entries({
            name: basename(output, '.mbtiles'),
            format: render.ext === 'jpeg' ? 'jpg' : render.ext,
            type: 'baselayer',
            bounds: bbox.join(','),
            minzoom,
            maxzoom,
        }))
            meta.run(name, String(value));

        const insert = db.prepare('INSERT INTO tiles VALUES (?, ?, ?, ?)');
        const queue = tiles();
        let done = 0;
        // Workers share one iterator, so each tile is taken once.
        await Promise.all(
            Array.from({ length: IN_FLIGHT }, async () => {
                for (const { z, x, y } of queue) {
                    const data = await chiitiler.renderTile({
                        ...render,
                        z,
                        x,
                        y,
                    });
                    insert.run(z, x, 2 ** z - 1 - y, data); // MBTiles rows are TMS
                    if (++done % 1000 === 0) console.error(`${done} tiles`);
                }
            }),
        );
        db.exec('COMMIT');
        db.close();
        console.error(`${done} tiles written to ${output}`);
    } catch (error) {
        db.close();
        rmSync(output, { force: true });
        throw error;
    }
}

export { generateTile, tileRange };
