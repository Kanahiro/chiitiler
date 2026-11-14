import { createHash } from 'node:crypto';
import type { Cache, Value } from './index.js';
import { getStorage } from '../gcs.js';
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

    async function bucketFile(key: string): Promise<File> {
        const storage = await getStorage({
            projectId: options.projectId,
            keyFilename: options.keyFilename,
            apiEndpoint: options.apiEndpoint,
        });
        const name = objectName(key);
        const nameWithPrefix = options.prefix
            ? `${options.prefix}/${name}`
            : name;
        return storage.bucket(options.bucket).file(nameWithPrefix);
    }

    return {
        name: 'gcs',
        set: async (key: string, value: Value) => {
            try {
                const file = await bucketFile(key);
                await file.save(value, {
                    resumable: false,
                });
            } catch (e) {
                console.log(`[error]: ${e}`);
            }
        },
        get: async (key: string): Promise<Value | undefined> => {
            try {
                const file = await bucketFile(key);
                const [content] = await file.download();
                return content;
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
