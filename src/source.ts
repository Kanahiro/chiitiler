/// <reference lib="dom" />
// for using native fetch in TypeScript

import * as fs from 'fs';

import { GetObjectCommand } from '@aws-sdk/client-s3';

import { getS3Client } from './s3.js';
import { Cache, noneCache } from './cache/index.js';

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
        } catch (e) {
            console.log(e);
            return null;
        }
    }

    if (uri.startsWith('mbtiles://')) {
        // uri = mbtiles://path/to/file.mbtiles/{z}/{x}/{y}
        // mbtilesFilepath -> path/to/file.mbtiles
        const mbtilesFilepath = uri.replace('mbtiles://', '').split('/')[0];
        throw new Error('mbtiles is not supported yet');
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
