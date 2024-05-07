import { PutObjectCommand, GetObjectCommand } from '@aws-sdk/client-s3';

import { type Cache, type Value } from './index.js';
import { escapeFileName } from './utils.js';
import { getS3Client } from '../s3.js';

type S3CacheOptions = {
    bucket: string;
    region: string;
    endpoint: string | null;
};
function s3Cache(options: S3CacheOptions): Cache {
    const s3Client = getS3Client({
        region: options.region,
        endpoint: options.endpoint,
    });
    return {
        name: 's3',
        set: async function (key: string, value: Value) {
            try {
                const cmd = new PutObjectCommand({
                    Bucket: options.bucket,
                    Key: escapeFileName(key),
                    Body: value,
                });
                await s3Client.send(cmd);
            } catch (e) {
                console.log(`[error]: ${e}`);
            }
        },
        get: async function (key: string): Promise<Value | undefined> {
            const cmd = new GetObjectCommand({
                Bucket: options.bucket,
                Key: escapeFileName(key),
            });
            try {
                const obj = await s3Client.send(cmd);
                if (obj.Body === undefined) return undefined;
                const buf = Buffer.from(await obj.Body.transformToByteArray());
                return buf;
            } catch {
                // miss or any error
                return undefined;
            }
        },
    };
}

export { s3Cache };
