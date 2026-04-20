import { getFilesystemSource } from './fs.js';
import { getHttpSource } from './http.js';
import { getPmtilesSource } from './pmtiles.js';
import { getMbtilesSource } from './mbtiles.js';
import { getS3Source } from './s3.js';
import { getGCSSource } from './gcs.js';
import { getCogSource } from './cog.js';
import { Cache, noneCache } from '../cache/index.js';

async function fetchSource(
    uri: string,
    cache: Cache,
): Promise<Buffer | null> {
    if (uri.startsWith('http://') || uri.startsWith('https://'))
        return getHttpSource(uri, cache);
    if (uri.startsWith('file://')) return getFilesystemSource(uri);
    if (uri.startsWith('s3://')) return getS3Source(uri);
    if (uri.startsWith('gs://')) return getGCSSource(uri);
    if (uri.startsWith('mbtiles://')) return getMbtilesSource(uri);
    if (uri.startsWith('pmtiles://')) return getPmtilesSource(uri, cache);
    if (uri.startsWith('cog://')) return getCogSource(uri);
    return null;
}

const inFlight = new Map<string, Promise<Buffer | null>>();

/**
 * retrieve sources from the uri.
 * Concurrent requests for the same uri share a single in-flight fetch
 * (single-flight) to avoid duplicate downloads.
 * @param uri
 * @param cache {Cache} - Cache Strategy. Affect only for http(s) sources.
 * @returns
 */
async function getSource(
    uri: string,
    cache: Cache = noneCache(),
): Promise<Buffer | null> {
    const existing = inFlight.get(uri);
    if (existing !== undefined) return existing;

    const promise = fetchSource(uri, cache).finally(() => {
        inFlight.delete(uri);
    });
    inFlight.set(uri, promise);
    return promise;
}

export { getSource };
