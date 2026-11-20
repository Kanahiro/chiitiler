import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { gzipSync } from 'node:zlib';
import { describe, expect, it } from 'vitest';

import { getMbtilesSource } from './mbtiles.js';

describe('getMbtilesSource', () => {
    const png = Buffer.from([0x89, 0x50, 0x4e, 0x47, 1, 2, 3]);
    const mvt = Buffer.from([0x1a, 1, 2, 3]);

    const mbtiles = (tile: Uint8Array) => {
        const path = join(mkdtempSync(join(tmpdir(), 'mbtiles-')), 't.mbtiles');
        const db = new DatabaseSync(path);
        db.exec(
            'CREATE TABLE tiles (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB)',
        );
        db.prepare('INSERT INTO tiles VALUES (0, 0, 0, ?)').run(tile);
        db.close();
        return `mbtiles://${path}/0/0/0`;
    };

    it('returns raster tiles as they are', async () => {
        expect(await getMbtilesSource(mbtiles(png))).toEqual(png);
    });

    it('decompresses gzip vector tiles', async () => {
        expect(await getMbtilesSource(mbtiles(gzipSync(mvt)))).toEqual(mvt);
    });

    it('throws for a corrupt compressed tile', async () => {
        await expect(
            getMbtilesSource(mbtiles(Buffer.from([0x1f, 0x8b, 0, 0]))),
        ).rejects.toThrow();
    });
});
