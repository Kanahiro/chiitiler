import { describe, expect, it } from 'vitest';

import { mkdtempSync, rmSync, writeFileSync, existsSync } from 'node:fs';
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

    it('keeps an existing file and removes a failed one', async () => {
        const { dir, output } = tmp();
        try {
            writeFileSync(output, 'mine');
            await expect(
                generateTile(['--style', 'x', '-o', output, '--maxzoom', '0'], noneCache()),
            ).rejects.toThrow('already exists');
            rmSync(output);
            await expect(
                generateTile(
                    ['--style', join(dir, 'missing.json'), '-o', output, '--maxzoom', '0'],
                    noneCache(),
                ),
            ).rejects.toThrow('style not found');
            expect(existsSync(output)).toBe(false);
        } finally {
            rmSync(dir, { recursive: true, force: true });
        }
    });
});
