import Database, { Statement } from 'better-sqlite3';
import { unzip } from 'zlib';

const mbtilesCache: { [key: string]: Statement } = {};

/**
 * uri = mbtiles://path/to/file.mbtiles/{z}/{x}/{y}
 */
async function getMbtilesSource(uri: string): Promise<Buffer | null> {
    const mbtilesFilepath = uri
        .replace('mbtiles://', '')
        .replace(/\/\d+\/\d+\/\d+$/, '');
    if (mbtilesCache[mbtilesFilepath] === undefined) {
        const db = new Database(mbtilesFilepath, { readonly: true });
        mbtilesCache[mbtilesFilepath] = db.prepare(
            'SELECT tile_data FROM tiles WHERE zoom_level = ? AND tile_column = ? AND tile_row = ?',
        );
    }
    const [z, x, y] = uri
        .replace(`mbtiles://${mbtilesFilepath}/`, '')
        .split('/');
    const ty = Math.pow(2, Number(z)) - 1 - Number(y);

    const row = mbtilesCache[mbtilesFilepath].get(z, x, ty);

    if (!row) return null;

    const unzipped = await new Promise<Buffer>((resolve, reject) => {
        unzip((row as any).tile_data, (err, buffer) => {
            if (err) reject(err);
            resolve(buffer);
        });
    });
    return unzipped;
}

export { getMbtilesSource };
