import { memoryCache } from './memory.js';
import { s3Cache } from './s3.js';
import { fileCache } from './file.js';

type Value = Buffer;
type Strategy = 'none' | 'memory' | 'file' | 's3';
type GetCacheOptions = {
    's3:bucket'?: string;
    'file:dir'?: string;
};
type Cache = {
    get: (key: string) => Promise<Value | undefined>;
    set: (key: string, value: Value) => Promise<void>;
};

const getCache = function (
    strategy: Strategy,
    options: GetCacheOptions = {},
): Cache {
    if (strategy === 'none') {
        return {
            get: async () => undefined,
            set: async () => undefined,
        };
    } else if (strategy === 'memory') {
        return memoryCache();
    } else if (strategy === 'file') {
        return fileCache({ dir: options['file:dir'] ?? './.cache' });
    } else if (strategy === 's3') {
        return s3Cache({ bucket: options['s3:bucket'] ?? '' });
    } else {
        throw new Error(`Invalid cache strategy: ${strategy}`);
    }
};

export {
    getCache,
    type Value,
    type Cache,
    type Strategy,
    type GetCacheOptions,
};
