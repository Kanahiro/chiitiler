import { describe, expect, it } from 'vitest';

import { execFileSync } from 'node:child_process';
import {
    existsSync,
    mkdtempSync,
    readFileSync,
    rmSync,
    writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

import { blocks, generateTile, tileRange } from './generate.js';
import { noneCache } from './cache/index.js';

const WORLD: [number, number, number, number] = [-180, -90, 180, 90];

describe('tileRange', () => {
    it('covers the world', () => {
        expect(tileRange(WORLD, 0)).toEqual([0, 0, 0, 0]);
        expect(tileRange(WORLD, 2)).toEqual([0, 0, 3, 3]);
    });

    it('covers a box', () => {
        // Tokyo
        expect(tileRange([139.7, 35.6, 139.8, 35.7], 10)).toEqual([
            909, 403, 909, 403,
        ]);
    });
});

describe('blocks', () => {
    it('takes every tile once, by blocks of a parent tile', () => {
        const bbox: [number, number, number, number] = [139, 35, 140.5, 36];
        for (const z of [0, 3, 9, 14]) {
            const [minX, minY, maxX, maxY] = tileRange(bbox, z);
            const all = [...blocks(bbox, z, 4)];
            const tiles = all.flat().map(({ x, y }) => `${x}/${y}`);
            expect(new Set(tiles).size).toBe(tiles.length);
            expect(tiles.length).toBe((maxX - minX + 1) * (maxY - minY + 1));
            // each within one tile 4 zooms up (smaller blocks divide it)
            for (const block of all) {
                const parents = block.map(
                    ({ x, y }) => `${Math.floor(x / 16)}/${Math.floor(y / 16)}`,
                );
                expect(new Set(parents).size).toBe(1);
            }
            // enough blocks to share between 4 lanes, where the zoom has the tiles
            if (tiles.length >= 8) expect(all.length).toBeGreaterThanOrEqual(8);
        }
    });
});

function tmp() {
    const dir = mkdtempSync(join(tmpdir(), 'chiitiler-generate-'));
    const style = join(dir, 'style.json');
    writeFileSync(
        style,
        JSON.stringify({
            version: 8,
            sources: {},
            layers: [
                {
                    id: 'bg',
                    type: 'background',
                    paint: { 'background-color': '#f00' },
                },
            ],
        }),
    );
    return { dir, style, output: join(dir, 'out.mbtiles') };
}

describe('generateTile', () => {
    it('writes an MBTiles file', async () => {
        const { dir, style, output } = tmp();
        try {
            await generateTile(
                ['--style', style, '-o', output, '--maxzoom', '1'],
                noneCache(),
            );
            const db = new DatabaseSync(output, { readOnly: true });
            const tiles = db
                .prepare(
                    'SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles ORDER BY 1, 2, 3',
                )
                .all();
            expect(
                tiles.map((t) => [t.zoom_level, t.tile_column, t.tile_row]),
            ).toEqual([
                [0, 0, 0],
                [1, 0, 0],
                [1, 0, 1],
                [1, 1, 0],
                [1, 1, 1],
            ]);
            expect([...(tiles[0].tile_data as Uint8Array).slice(0, 4)]).toEqual(
                [0x89, 0x50, 0x4e, 0x47],
            );
            const meta = Object.fromEntries(
                db
                    .prepare('SELECT name, value FROM metadata')
                    .all()
                    .map((r) => [r.name, r.value]),
            );
            expect(meta).toMatchObject({
                name: 'out',
                format: 'png',
                minzoom: '0',
                maxzoom: '1',
            });
            db.close();
        } finally {
            rmSync(dir, { recursive: true, force: true });
        }
    });

    it('renders tiles of tile-size × pixel-ratio pixels, a pixel ratio of its own to resume', async () => {
        const { dir, style, output } = tmp();
        const args = ['--style', style, '-o', output, '--maxzoom', '0'];
        try {
            await generateTile(
                [...args, '--tile-size', '256', '--pixel-ratio', '2'],
                noneCache(),
            );
            const db = new DatabaseSync(output, { readOnly: true });
            const png = db.prepare('SELECT tile_data FROM tiles').get()
                ?.tile_data as Uint8Array;
            db.close();
            // PNG IHDR: width and height, big-endian, at bytes 16 and 20
            const view = new DataView(png.buffer, png.byteOffset);
            expect([view.getUint32(16), view.getUint32(20)]).toEqual([
                512, 512,
            ]);
            await expect(
                generateTile([...args, '--tile-size', '256'], noneCache()),
            ).rejects.toThrow('not generated with these options');
        } finally {
            rmSync(dir, { recursive: true, force: true });
        }
    });

    it.each([
        [['--maxzoom', '1'], 'usage'],
        [['--pixel-ratio', '0'], '--pixel-ratio must be'],
        [['--pixel-ratio', '1.5'], '--pixel-ratio must be'],
        [['-o', 'x.pmtiles'], 'must be a .mbtiles file'],
        [['--ext', 'gif'], '--ext must be'],
        [['--bbox', '1,2,3'], '--bbox must be'],
        [['--minzoom', '3'], '--maxzoom must be'],
    ])('rejects %j', async (args, error) => {
        const { dir, style, output } = tmp();
        try {
            const base = ['--style', style, '-o', output, '--maxzoom', '1'];
            await expect(
                generateTile(
                    args[0] === '--maxzoom' ? args : [...base, ...args],
                    noneCache(),
                ),
            ).rejects.toThrow(error);
        } finally {
            rmSync(dir, { recursive: true, force: true });
        }
    });

    it('resumes a file of the same options, and replaces it with --force', async () => {
        const { dir, style, output } = tmp();
        const args = ['--style', style, '-o', output, '--maxzoom', '1'];
        const count = () => {
            const db = new DatabaseSync(output, { readOnly: true });
            const rows = db.prepare('SELECT tile_data FROM tiles').all();
            db.close();
            return rows.length;
        };
        try {
            // A failed run keeps its file to resume.
            await expect(
                generateTile(
                    [...args.slice(2), '--style', join(dir, 'missing.json')],
                    noneCache(),
                ),
            ).rejects.toThrow('style not found');
            expect(existsSync(output)).toBe(true);
            await expect(generateTile(args, noneCache())).rejects.toThrow(
                'not generated with these options',
            );
            rmSync(output);

            await generateTile(args, noneCache());
            const db = new DatabaseSync(output);
            db.exec(`
                DELETE FROM tiles WHERE zoom_level = 1 AND tile_column = 0 AND tile_row = 0;
                UPDATE tiles SET tile_data = x'00' WHERE zoom_level = 0;
            `);
            db.close();
            await generateTile(args, noneCache());
            expect(count()).toBe(5);
            const kept = new DatabaseSync(output, { readOnly: true });
            expect(
                kept
                    .prepare('SELECT tile_data FROM tiles WHERE zoom_level = 0')
                    .get()?.tile_data,
            ).toEqual(new Uint8Array([0]));
            kept.close();

            await expect(
                generateTile([...args, '--maxzoom', '0'], noneCache()),
            ).rejects.toThrow('not generated with these options');
            await generateTile(
                [...args, '--maxzoom', '0', '--force'],
                noneCache(),
            );
            expect(count()).toBe(1);
        } finally {
            rmSync(dir, { recursive: true, force: true });
        }
    });

    it('leaves a file it did not write', async () => {
        const { dir, style, output } = tmp();
        try {
            writeFileSync(output, 'mine');
            await expect(
                generateTile(
                    ['--style', style, '-o', output, '--maxzoom', '0'],
                    noneCache(),
                ),
            ).rejects.toThrow('not generated with these options');
            expect(readFileSync(output, 'utf8')).toBe('mine');
        } finally {
            rmSync(dir, { recursive: true, force: true });
        }
    });

    it('draws in renderer processes', () => {
        const { dir, style, output } = tmp();
        // The CLI: renderer processes are cluster workers of its entry.
        const cli = (styleArg: string) =>
            execFileSync(
                process.execPath,
                [
                    '--import',
                    'tsx',
                    'src/main.ts',
                    'generate-tile',
                    '--style',
                    styleArg,
                    '-o',
                    output,
                    '--maxzoom',
                    '3',
                    '--force',
                ],
                {
                    env: { ...process.env, CHIITILER_PROCESSES: '2' },
                    stdio: 'pipe',
                },
            );
        try {
            cli(style);
            const db = new DatabaseSync(output, { readOnly: true });
            const { n } =
                db.prepare('SELECT count(*) AS n FROM tiles').get() ?? {};
            db.close();
            expect(n).toBe(1 + 4 + 16 + 64);
            // A renderer's failure fails the run.
            expect(() => cli(join(dir, 'missing.json'))).toThrow(
                'a renderer process failed',
            );
        } finally {
            rmSync(dir, { recursive: true, force: true });
        }
    });
});
