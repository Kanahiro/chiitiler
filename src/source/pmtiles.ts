import * as fs from 'fs';

import { PMTiles, Source, RangeResponse } from 'pmtiles';
import { GetObjectCommand, S3Client } from '@aws-sdk/client-s3';

import { getS3Client } from '../s3.js';
import { type Cache, noneCache } from '../cache/index.js';

const pmtilesCache: { [key: string]: PMTiles } = {};

class FilesystemSource implements Source {
    filepath: string;
    fileHandle: Promise<fs.promises.FileHandle>;

    constructor(filepath: string) {
        this.filepath = filepath;
        this.fileHandle = fs.promises.open(filepath, 'r');
    }

    getKey() {
        return this.filepath;
    }

    async getBytes(offset: number, length: number): Promise<RangeResponse> {
        const buf = Buffer.alloc(length);
        await (await this.fileHandle).read(buf, 0, length, offset);
        return { data: buf.buffer };
    }
}

class S3Source implements Source {
    bucket: string;
    key: string;
    s3Client: S3Client;

    constructor(bucket: string, key: string) {
        this.bucket = bucket;
        this.key = key;
        this.s3Client = getS3Client({
            region: process.env.CHIITILER_S3_REGION ?? 'us-east1',
            endpoint: process.env.CHIITILER_S3_ENDPOINT ?? null,
        });
    }

    getKey() {
        return `s3://${this.bucket}/${this.key}`;
    }

    async getBytes(offset: number, length: number): Promise<RangeResponse> {
        const cmd = new GetObjectCommand({
            Bucket: this.bucket,
            Key: this.key,
            Range: `bytes=${offset}-${offset + length - 1}`,
        });
        try {
            const obj = await this.s3Client.send(cmd);
            if (obj.Body === undefined) return { data: Buffer.alloc(0).buffer };
            const buf = Buffer.from(await obj.Body.transformToByteArray());
            return { data: buf.buffer };
        } catch (e: any) {
            if (e.name !== 'NoSuchKey') console.log(e);
            return { data: Buffer.alloc(0).buffer };
        }
    }
}

/**
 * uri = pmtiles://path/to/file.pmtiles/{z}/{x}/{y}
 * uri = pmtiles://http://url/to/file.pmtiles/{z}/{x}/{y}
 * uri = pmtiles://s3://bucket/key/to/file.pmtiles/{z}/{x}/{y}
 */
async function getPmtilesSoruce(
    uri: string,
    cache: Cache = noneCache(),
): Promise<Buffer | null> {
    const pmtilesUri = uri
        .replace('pmtiles://', '')
        .replace(/\/\d+\/\d+\/\d+$/, '');

    const isHttpSource =
        pmtilesUri.startsWith('http://') || pmtilesUri.startsWith('https://');
    if (isHttpSource) {
        const val = await cache.get(uri);
        if (val !== undefined) return val; // hit
        if (!pmtilesCache[pmtilesUri])
            pmtilesCache[pmtilesUri] = new PMTiles(pmtilesUri);
    } else if (pmtilesUri.startsWith('s3://')) {
        if (!pmtilesCache[pmtilesUri]) {
            const bucket = pmtilesUri.replace('s3://', '').split('/')[0];
            const key = pmtilesUri.replace(`s3://${bucket}/`, '');
            const s3Source = new S3Source(bucket, key);
            pmtilesCache[pmtilesUri] = new PMTiles(s3Source);
        }
    } else {
        // local filesystem
        const fileSource = new FilesystemSource(pmtilesUri);
        pmtilesCache[pmtilesUri] = new PMTiles(fileSource);
    }

    const [z, x, y] = uri.replace(`pmtiles://${pmtilesUri}/`, '').split('/');
    const tile = await pmtilesCache[pmtilesUri].getZxy(
        Number(z),
        Number(x),
        Number(y),
    );

    if (!tile) return null;

    const buf = Buffer.from(tile.data);
    if (isHttpSource) cache.set(uri, buf);
    return buf;
}

export { getPmtilesSoruce };
