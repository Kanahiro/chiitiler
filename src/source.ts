/// <reference lib="dom" />
// for using native fetch in TypeScript

import * as fs from 'fs';

import { GetObjectCommand } from '@aws-sdk/client-s3';
import Database, { Statement } from 'better-sqlite3';
import { unzip } from 'zlib';

import { getS3Client } from './s3.js';
import { Cache, noneCache } from './cache/index.js';

const mbtilesCache: { [key: string]: Statement } = {};

/**
 * retrieve sources from the uri
 * @param uri
 * @param cache {Cache} - Cache Strategy. Affect only for http(s) sources.
 * @returns
 */
async function getSource(
    uri: string,
    cache: Cache = noneCache(),
): Promise<Buffer | null> {
    if (uri.startsWith('file://')) {
        return new Promise((resolve, reject) => {
            fs.readFile(uri.replace('file://', ''), (err, data) => {
                if (err) reject(err);
                resolve(data);
            });
        });
    }

    if (uri.startsWith('s3://')) {
        const s3Client = getS3Client({
            region: process.env.CHIITILER_S3_REGION ?? 'us-east1',
            endpoint: process.env.CHIITILER_S3_ENDPOINT ?? null,
        });
        const bucket = uri.replace('s3://', '').split('/')[0];
        const key = uri.replace(`s3://${bucket}/`, '');
        const cmd = new GetObjectCommand({
            Bucket: bucket,
            Key: key,
        });
        try {
            const obj = await s3Client.send(cmd);
            if (obj.Body === undefined) return null;
            const buf = Buffer.from(await obj.Body.transformToByteArray());
            return buf;
        } catch (e: any) {
            if (e.name !== 'NoSuchKey') console.log(e);
            return null;
        }
    }

    if (uri.startsWith('mbtiles://')) {
        // uri = mbtiles://path/to/file.mbtiles/{z}/{x}/{y}
        const mbtilesFilepath = uri.replace('mbtiles://', '').replace(/\/\d+\/\d+\/\d+$/, '')
        if (mbtilesCache[mbtilesFilepath] === undefined) {
            const db = new Database(mbtilesFilepath, { readonly: true });
            mbtilesCache[mbtilesFilepath] = db.prepare(
                'SELECT tile_data FROM tiles WHERE zoom_level = ? AND tile_column = ? AND tile_row = ?',
            );
        }
        const [z, x, y] = uri.replace(`mbtiles://${mbtilesFilepath}/`, '').split(
            '/',
        );
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

    if (uri.startsWith('pmtiles://')) {
        throw new Error('pmtiles is not supported yet');
    }

    if (uri.startsWith('http://') || uri.startsWith('https://')) {
        // use cache only for http(s) sources
        const val = await cache.get(uri);
        if (val !== undefined) return val; // hit

        // miss
        try {
            const res = await fetch(uri);
            const buf = Buffer.from(await res.arrayBuffer());
            cache.set(uri, buf);
            return buf;
        } catch (e) {
            console.log(e);
            return null;
        }
    }

    return null;
}

export { getSource };
