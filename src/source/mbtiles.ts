import { DatabaseSync, StatementSync } from 'node:sqlite';
import { unzip } from 'zlib';
import { LRUCache } from 'lru-cache';

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
        const statement = db.prepare(
            'SELECT tile_data FROM tiles WHERE zoom_level = ? AND tile_column = ? AND tile_row = ?',
        );
        entry = { db, statement };
        mbtilesCache.set(mbtilesFilepath, entry);
    }
    const { statement } = entry;
    const [z, x, y] = uri
        .replace(`mbtiles://${mbtilesFilepath}/`, '')
        .split('/');
    const ty = Math.pow(2, Number(z)) - 1 - Number(y);

    const row = statement.get(z, x, ty);

    if (!row) return null;

    const unzipped = await new Promise<Buffer | null>((resolve, _) => {
        unzip((row as any).tile_data, (err, buffer) => {
            if (err) {
                console.error(`[ERROR]: ${err}`);
                resolve(null);
            } else resolve(buffer);
        });
    });
    return unzipped;
}

export { getMbtilesSource };
