import * as fs from 'fs';

import {
    PMTiles,
    FetchSource,
    SharedPromiseCache,
    type Source,
    type RangeResponse,
} from 'pmtiles';
import type { GetObjectCommandOutput } from '@aws-sdk/client-s3';
import { LRUCache } from 'lru-cache';

import { getS3, s3Timeout } from '../s3.js';
import { type Cache, noneCache } from '../cache/index.js';
import { getUserAgent } from './userAgent.js';
import { FETCH_TIMEOUT_MS } from './http.js';

const pmtilesCache = new LRUCache<string, PMTiles>({
    max: 50,
});

class FilesystemSource implements Source {
    filepath: string;

    constructor(filepath: string) {
        this.filepath = filepath;
    }

    getKey() {
        return this.filepath;
    }

    /** Opened per read: an archive evicted from the cache leaves no file handle open. */
    async getBytes(offset: number, length: number): Promise<RangeResponse> {
        const file = await fs.promises.open(this.filepath, 'r');
        try {
            const buf = Buffer.alloc(length);
            const { bytesRead } = await file.read(buf, 0, length, offset);
            // fewer bytes at the end of the file (e.g. a header read from a small archive)
            return { data: buf.buffer.slice(0, bytesRead) };
        } finally {
            await file.close();
        }
    }
}

class S3Source implements Source {
    bucket: string;
    key: string;

    constructor(bucket: string, key: string) {
        this.bucket = bucket;
        this.key = key;
    }

    getKey() {
        return `s3://${this.bucket}/${this.key}`;
    }

    async getBytes(
        offset: number,
        length: number,
        signal?: AbortSignal,
        etag?: string,
    ): Promise<RangeResponse> {
        const { client, sdk } = await getS3({
            region: process.env.CHIITILER_S3_REGION ?? 'us-east-1',
            endpoint: process.env.CHIITILER_S3_ENDPOINT,
            forcePathStyle:
                process.env.CHIITILER_S3_FORCE_PATH_STYLE === 'true',
        });
        let resp: GetObjectCommandOutput;
        try {
            resp = await client.send(
                new sdk.GetObjectCommand({
                    Bucket: this.bucket,
                    Key: this.key,
                    Range: 'bytes=' + offset + '-' + (offset + length - 1),
                    IfMatch: etag,
                }),
                { abortSignal: s3Timeout() },
            );
        } catch (e: unknown) {
            if (
                e instanceof Error &&
                (e as Error).name === 'PreconditionFailed'
            ) {
                throw new Error('etag mismatch');
            }
            throw e;
        }

        const arr = await resp.Body?.transformToByteArray();

        if (!arr) throw Error('Failed to read S3 response body');

        return {
            data: arr.buffer as ArrayBuffer,
            etag: resp.ETag,
            expires: resp.Expires?.toISOString(),
            cacheControl: resp.CacheControl,
        };
    }
}

/**
 * uri = pmtiles://path/to/file.pmtiles/{z}/{x}/{y}
 * uri = pmtiles://http://url/to/file.pmtiles/{z}/{x}/{y}
 * uri = pmtiles://s3://bucket/key/to/file.pmtiles/{z}/{x}/{y}
 */
async function getPmtilesSource(
    uri: string,
    cache: Cache = noneCache(),
): Promise<Uint8Array | null> {
    const pmtilesUri = uri
        .replace('pmtiles://', '')
        .replace(/\/\d+\/\d+\/\d+$/, '');

    let pmtiles: PMTiles | undefined = pmtilesCache.get(pmtilesUri);

    const isHttpSource =
        pmtilesUri.startsWith('http://') || pmtilesUri.startsWith('https://');
    if (isHttpSource) {
        const val = await cache.get(uri);
        if (val !== undefined) return val; // hit
        // Another request may have created the archive while cache.get awaited.
        pmtiles = pmtilesCache.get(pmtilesUri);

        if (pmtiles === undefined) {
            const userAgent = getUserAgent();
            const headers = new Headers();
            if (userAgent) headers.set('User-Agent', userAgent);
            const fetchSource = new FetchSource(pmtilesUri, headers);
            pmtiles = new PMTiles(fetchSource, new SharedPromiseCache());
            pmtilesCache.set(pmtilesUri, pmtiles);
        }
    } else if (pmtilesUri.startsWith('s3://')) {
        if (pmtiles === undefined) {
            const bucket = pmtilesUri.replace('s3://', '').split('/')[0];
            const key = pmtilesUri.replace(`s3://${bucket}/`, '');
            const s3Source = new S3Source(bucket, key);
            pmtiles = new PMTiles(s3Source, new SharedPromiseCache());
            pmtilesCache.set(pmtilesUri, pmtiles);
        }
    } else {
        if (pmtiles === undefined) {
            const fileSource = new FilesystemSource(pmtilesUri);
            pmtiles = new PMTiles(fileSource, new SharedPromiseCache());
            pmtilesCache.set(pmtilesUri, pmtiles);
        }
    }

    const [z, x, y] = uri.replace(`pmtiles://${pmtilesUri}/`, '').split('/');
    const tile = await pmtiles
        .getZxy(
            Number(z),
            Number(x),
            Number(y),
            isHttpSource ? AbortSignal.timeout(FETCH_TIMEOUT_MS) : undefined,
        )
        .catch((err) => {
            // SharedPromiseCache retains rejected metadata promises. Rebuild after a
            // failure, without evicting a replacement created by a newer request.
            if (pmtilesCache.get(pmtilesUri) === pmtiles)
                pmtilesCache.delete(pmtilesUri);
            throw err;
        });

    if (!tile) return null;

    const buf = Buffer.from(tile.data);
    if (isHttpSource) cache.set(uri, buf);
    return buf;
}

export { getPmtilesSource };
