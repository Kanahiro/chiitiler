/// <reference lib="dom" />
// for using native fetch in TypeScript

import * as fs from 'fs';
import * as zlib from 'zlib';
// @ts-ignore
import MBTiles from '@mapbox/mbtiles';

async function getSource(uri: string): Promise<Buffer | null> {
    if (uri.startsWith('http://') || uri.startsWith('https://')) {
        const res = await fetch(uri);
        return Buffer.from(await res.arrayBuffer());
    }

    if (uri.startsWith('file://')) {
        return new Promise((resolve, reject) => {
            fs.readFile(uri.replace('file://', ''), (err, data) => {
                if (err) reject(err);
                resolve(data);
            });
        });
    }

    // TODO: better implementation
    if (uri.startsWith('mbtiles://')) {
        // uri=mbtiles://path/to/file.mbtiles/0/0/0

        // extract only path/to/file.mbtiles with regex
        const mbtilesPath = uri
            .replace('mbtiles://', '')
            .replace(/\/\d+\/\d+\/\d+$/, '');
        const [z, x, y] = uri.replace('mbtiles://', '').split('/').slice(-3);

        return new Promise((resolve, reject) => {
            new MBTiles(`${mbtilesPath}?mode=ro`, (err: any, mbtiles: any) => {
                mbtiles.getTile(
                    z,
                    x,
                    y,
                    (err: any, tile: any, headers: any) => {
                        if (err) reject(err);
                        if (tile === undefined) {
                            resolve(null);
                            return;
                        }
                        // un gzip
                        const unzipped = zlib.gunzipSync(tile);
                        resolve(unzipped);
                    },
                );
            });
        });
    }

    return null;
}

export { getSource };
