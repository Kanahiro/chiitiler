import { describe, expect, it } from 'vitest';

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

import { generateTile, tileRange } from './generate.js';
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
            expect(tiles.map((t) => [t.zoom_level, t.tile_column, t.tile_row]))
                .toEqual([
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

    it.each([
        [['--maxzoom', '1'], 'usage'],
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
                kept.prepare('SELECT tile_data FROM tiles WHERE zoom_level = 0').get()
                    ?.tile_data,
            ).toEqual(new Uint8Array([0]));
            kept.close();

            await expect(
                generateTile([...args, '--maxzoom', '0'], noneCache()),
            ).rejects.toThrow('not generated with these options');
            await generateTile([...args, '--maxzoom', '0', '--force'], noneCache());
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
                generateTile(['--style', style, '-o', output, '--maxzoom', '0'], noneCache()),
            ).rejects.toThrow('not generated with these options');
            expect(readFileSync(output, 'utf8')).toBe('mine');
        } finally {
            rmSync(dir, { recursive: true, force: true });
        }
    });
});
