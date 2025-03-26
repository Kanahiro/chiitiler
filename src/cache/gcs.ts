import { type Cache, type Value } from './index.js';
import { getStorageClient } from '../gcs.js';
import { File } from '@google-cloud/storage';

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
        const name = escapeFileName(key);
        const nameWithPrefix = options.prefix ? `${options.prefix}/${name}` : name;
        return bucket.file(nameWithPrefix);
    }

    return {
        name: 'gcs',
        set: async function (key: string, value: Value) {
            const file = bucketFile(key);

            try {
                await file.save(value, {
                    resumable: false,
                });
            } catch (e) {
                console.log(`[error]: ${e}`);
            }
        },
        get: async function (key: string): Promise<Value | undefined> {
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

function escapeFileName(url: string) {
    return url
        .replace(/\//g, '_') // replace slashes with underscores
        .replace(/\?/g, '-') // replace question marks with dashes
        .replace(/&/g, '-') // replace ampersands with dashes
        .replace(/=/g, '-') // replace equals signs with dashes
        .replace(/%/g, '-') // replace percent signs with dashes
        .replace(/#/g, '-') // replace hash signs with dashes
        .replace(/:/g, '-') // replace colons with dashes
        .replace(/\+/g, '-') // replace plus signs with dashes
        .replace(/ /g, '-') // replace spaces with dashes
        .replace(/</g, '-') // replace less than signs with dashes
        .replace(/>/g, '-') // replace greater than signs with dashes
        .replace(/\*/g, '-') // replace asterisks with dashes
        .replace(/\|/g, '-') // replace vertical bars with dashes
        .replace(/"/g, '-') // replace double quotes with dashes
        .replace(/'/g, '-') // replace single quotes with dashes
        .replace(/\?/g, '-') // replace question marks with dashes
        .replace(/\./g, '-') // replace dots with dashes
        .replace(/,/g, '-') // replace commas with dashes
        .replace(/;/g, '-') // replace semicolons with dashes
        .replace(/\\/g, '-'); // replace backslashes with dashes
}

export { gcsCache };
