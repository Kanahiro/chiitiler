import { memoryCache } from './memory.js';
import { s3Cache } from './s3.js';
import { gcsCache } from './gcs.js';
import { fileCache } from './file.js';

type Value = Buffer;
type Cache = {
    name: string;
    get: (key: string) => Promise<Value | undefined>;
    set: (key: string, value: Value) => Promise<void>;
};

const noneCache: () => Cache = () => ({
    name: 'none',
    get: async () => undefined,
    set: async () => undefined,
});

export { noneCache, memoryCache, s3Cache, gcsCache, fileCache, type Value, type Cache };
