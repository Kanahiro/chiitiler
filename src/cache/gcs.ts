import { type Cache, type Value } from './index.js';
import { getStorageClient } from '../gcs.js';

type GCSCacheOptions = {
    bucket: string;
    projectId?: string;
    keyFilename?: string;
};

function gcsCache(options: GCSCacheOptions): Cache {
    const storageClient = getStorageClient({
        projectId: options.projectId,
        keyFilename: options.keyFilename,
    });
    const bucket = storageClient.bucket(options.bucket);

    return {
        name: 'gcs',
        set: async function (key: string, value: Value) {
            try {
                const file = bucket.file(escapeFileName(key));
                await file.save(value);
            } catch (e) {
                console.log(`[error]: ${e}`);
            }
        },
        get: async function (key: string): Promise<Value | undefined> {
            const file = bucket.file(escapeFileName(key));
            try {
                const [buffer] = await file.download();
                return buffer;
            } catch (e) {
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
