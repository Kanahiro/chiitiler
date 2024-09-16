import { getFilesystemSource } from './fs.js';
import { getHttpSource } from './http.js';
import { getPmtilesSoruce } from './pmtiles.js';
import { getMbtilesSource } from './mbtiles.js';
import { getS3Source } from './s3.js';
import { getCogSource } from './cog.js';
import { Cache, noneCache } from '../cache/index.js';

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
    let data: Buffer | null = null;

    if (uri.startsWith('http://') || uri.startsWith('https://'))
        data = await getHttpSource(uri, cache);
    else if (uri.startsWith('file://')) data = await getFilesystemSource(uri);
    else if (uri.startsWith('s3://')) data = await getS3Source(uri);
    else if (uri.startsWith('mbtiles://')) data = await getMbtilesSource(uri);
    else if (uri.startsWith('pmtiles://'))
        data = await getPmtilesSoruce(uri, cache);
    else if (uri.startsWith('cog://')) data = await getCogSource(uri);
    else return null;

    return data;
}

export { getSource };
