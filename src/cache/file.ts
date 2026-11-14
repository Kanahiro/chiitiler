import { Cache as FileSystemCache } from 'file-system-cache';

import type { Cache, Value } from './index.js';

type fileCacheOptions = {
    dir: string;
    ttl: number;
};
const fileCache: (options: fileCacheOptions) => Cache = (options) => {
    const cache = new FileSystemCache({
        basePath: options.dir,
        hash: 'sha1',
        ttl: options.ttl,
    });

    return {
        name: 'file',
        set: async (key: string, value: Value) => {
            try {
                await cache.set(
                    key,
                    Buffer.from(
                        value.buffer,
                        value.byteOffset,
                        value.byteLength,
                    ).toString('hex'),
                );
            } catch (e) {
                console.error(e);
            }
        },
        get: async (key: string): Promise<Value | undefined> => {
            try {
                const val = await cache.get(key, undefined);
                if (val === undefined) return undefined;
                return Buffer.from(val, 'hex');
            } catch (e) {
                console.error(e);
                cache.remove(key);
                return undefined;
            }
        },
    };
};

export { fileCache, type fileCacheOptions };
