import {
    PutObjectCommand,
    S3Client,
    S3ClientConfig,
    GetObjectCommand,
} from '@aws-sdk/client-s3';

import { type Cache, type Value } from './index.js';
import { escapeFileName } from './utils.js';

type S3CacheOptions = {
    bucket: string;
    region: string;
};
function s3Cache(options: S3CacheOptions): Cache {
    const s3Client = getS3Client(options);
    return {
        name: 's3',
        set: async function (key: string, value: Value) {
            const cmd = new PutObjectCommand({
                Bucket: options.bucket,
                Key: escapeFileName(key),
                Body: value,
            });
            await s3Client.send(cmd);
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

const getS3Client = function (options: S3CacheOptions) {
    let s3ClientConfig: S3ClientConfig = { region: options.region };
    if (process.env.NODE_ENV === 'development') {
        s3ClientConfig = {
            region: options.region,
            credentials: {
                accessKeyId: 'minioadmin',
                secretAccessKey: 'minioadmin',
            },
            forcePathStyle: true, // special option for minio
            endpoint: 'http://localhost:9000',
        };
    }
    const s3Client = new S3Client(s3ClientConfig);
    return s3Client;
};

export { s3Cache };
