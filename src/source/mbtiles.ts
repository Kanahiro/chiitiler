import { DatabaseSync, type StatementSync } from 'node:sqlite';
import { promisify } from 'node:util';
import { unzip } from 'node:zlib';
import { LRUCache } from 'lru-cache';

const unzipAsync = promisify(unzip);

// A StatementSync is only valid while its DatabaseSync is alive, and node:sqlite
// does not guarantee the statement keeps the database from being GC'd. Cache the
// database alongside the statement so both share the entry's lifetime.
const mbtilesCache = new LRUCache<
    string,
    { db: DatabaseSync; statement: StatementSync }
>({
    max: 20,
    dispose: ({ db }) => db.close(),
});

/**
 * uri = mbtiles://path/to/file.mbtiles/{z}/{x}/{y}
 */
async function getMbtilesSource(uri: string): Promise<Buffer | null> {
    const mbtilesFilepath = uri
        .replace('mbtiles://', '')
        .replace(/\/\d+\/\d+\/\d+$/, '');
    let entry = mbtilesCache.get(mbtilesFilepath);
    if (entry === undefined) {
        const db = new DatabaseSync(mbtilesFilepath, { readOnly: true });
        let statement: StatementSync;
        try {
            statement = db.prepare(
                'SELECT tile_data FROM tiles WHERE zoom_level = ? AND tile_column = ? AND tile_row = ?',
            );
        } catch (e) {
            // not an MBTiles file: it is not cached, close it here
            db.close();
            throw e;
        }
        entry = { db, statement };
        mbtilesCache.set(mbtilesFilepath, entry);
    }
    const { statement } = entry;
    const [z, x, y] = uri
        .replace(`mbtiles://${mbtilesFilepath}/`, '')
        .split('/');
    const ty = 2 ** Number(z) - 1 - Number(y);

    const row = statement.get(z, x, ty);

    if (!row) return null;

    const data = (row as { tile_data: Uint8Array }).tile_data;
    // Vector tiles are gzip (1f 8b) or zlib (78) compressed; raster tiles (PNG, JPEG, WebP) are not.
    // A corrupt compressed tile throws: the render fails rather than drawing a hole.
    const compressed =
        (data[0] === 0x1f && data[1] === 0x8b) || data[0] === 0x78;
    return compressed ? unzipAsync(data) : Buffer.from(data);
}

export { getMbtilesSource };
