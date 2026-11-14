import { createHash } from 'node:crypto';

import type { Cache, Value } from './index.js';
import { getS3 } from '../s3.js';

type S3CacheOptions = {
    bucket: string;
    region: string;
    endpoint?: string;
    forcePathStyle?: boolean;
};
function s3Cache(options: S3CacheOptions): Cache {
    const s3 = () =>
        getS3({
            region: options.region,
            endpoint: options.endpoint,
            forcePathStyle: options.forcePathStyle,
        });
    return {
        name: 's3',
        set: async (key: string, value: Value) => {
            try {
                const { client, sdk } = await s3();
                const cmd = new sdk.PutObjectCommand({
                    Bucket: options.bucket,
                    Key: objectName(key),
                    Body: value,
                });
                await client.send(cmd);
            } catch (e) {
                console.log(`[error]: ${e}`);
            }
        },
        get: async (key: string): Promise<Value | undefined> => {
            try {
                const { client, sdk } = await s3();
                const cmd = new sdk.GetObjectCommand({
                    Bucket: options.bucket,
                    Key: objectName(key),
                });
                const obj = await client.send(cmd);
                if (obj.Body === undefined) return undefined;
                return await obj.Body.transformToByteArray();
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

export { s3Cache, type S3CacheOptions };
