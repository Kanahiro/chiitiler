import { Cache as FileSystemCache } from 'file-system-cache';

import { type Cache, type Value } from './index.js';

type fileCacheOptions = {
    dir: string;
    ttl: number;
};
const fileCache: (options: fileCacheOptions) => Cache = function (options) {
    const cache = new FileSystemCache({
        basePath: options.dir,
        hash: 'sha1',
        ttl: options.ttl,
    });

    return {
        name: 'file',
        set: async function (key: string, value: Value) {
            await cache.set(key, value.toString('hex'));
        },
        get: async function (key: string): Promise<Value | undefined> {
            const val = await cache.get(key, undefined);
            if (val === undefined) return undefined;
            return Buffer.from(val, 'hex');
        },
    };
};

export { fileCache };
