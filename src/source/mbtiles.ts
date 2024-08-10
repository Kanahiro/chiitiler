import Database, { Statement } from 'better-sqlite3';
import { unzip } from 'zlib';
import { LRUCache } from 'lru-cache';

const mbtilesCache = new LRUCache<string, Statement>({
    max: 20,
});

/**
 * uri = mbtiles://path/to/file.mbtiles/{z}/{x}/{y}
 */
async function getMbtilesSource(uri: string): Promise<Buffer | null> {
    const mbtilesFilepath = uri
        .replace('mbtiles://', '')
        .replace(/\/\d+\/\d+\/\d+$/, '');
    let statement = mbtilesCache.get(mbtilesFilepath);
    if (statement === undefined) {
        const db = new Database(mbtilesFilepath, { readonly: true });
        statement = db.prepare(
            'SELECT tile_data FROM tiles WHERE zoom_level = ? AND tile_column = ? AND tile_row = ?',
        );
        mbtilesCache.set(mbtilesFilepath, statement);
    }
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
