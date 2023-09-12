// TODO: implement

import { type Cache, type Value } from './index.js';

type S3CacheOptions = {
    bucket: string;
};
const s3Cache: (options: S3CacheOptions) => Cache = function () {
    return {
        set: async function (key: string, value: Value) {},
        get: async function (key: string): Promise<Value | undefined> {
            return undefined;
        },
    };
};

export { s3Cache };
