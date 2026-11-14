import { createHash } from 'node:crypto';
import type { Cache, Value } from './index.js';
import { getStorageClient } from '../gcs.js';
import type { File } from '@google-cloud/storage';

type GCSCacheOptions = {
    bucket: string;
    prefix?: string;
    projectId?: string;
    keyFilename?: string;
    apiEndpoint?: string;
};

function gcsCache(options: GCSCacheOptions): Cache {
    if (options.prefix?.endsWith('/')) {
        options.prefix = options.prefix.slice(0, -1);
    }

    const storageClient = getStorageClient({
        projectId: options.projectId,
        keyFilename: options.keyFilename,
        apiEndpoint: options.apiEndpoint,
    });
    const bucket = storageClient.bucket(options.bucket);

    function bucketFile(key: string): File {
        const name = objectName(key);
        const nameWithPrefix = options.prefix
            ? `${options.prefix}/${name}`
            : name;
        return bucket.file(nameWithPrefix);
    }

    return {
        name: 'gcs',
        set: async (key: string, value: Value) => {
            const file = bucketFile(key);

            try {
                await file.save(value, {
                    resumable: false,
                });
            } catch (e) {
                console.log(`[error]: ${e}`);
            }
        },
        get: async (key: string): Promise<Value | undefined> => {
            const file = bucketFile(key);

            try {
                const [content] = await file.download();
                return Buffer.from(content);
            } catch {
                // miss or any error
                return undefined;
            }
        },
    };
}

/**
 * The object name for a URL: its SHA-256. (Escaping characters mapped distinct URLs, e.g.
 * `a.pbf?x=1` and `a-pbf-x-1`, to the same object.)
 */
function objectName(url: string) {
    return createHash('sha256').update(url).digest('hex');
}

export { gcsCache, type GCSCacheOptions };
