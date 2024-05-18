import { GetObjectCommand } from '@aws-sdk/client-s3';

import { getS3Client } from '../s3.js';

async function getS3Source(uri: string) {
    const s3Client = getS3Client({
        region: process.env.CHIITILER_S3_REGION ?? 'us-east1',
        endpoint: process.env.CHIITILER_S3_ENDPOINT ?? null,
    });
    const bucket = uri.replace('s3://', '').split('/')[0];
    const key = uri.replace(`s3://${bucket}/`, '');
    const cmd = new GetObjectCommand({
        Bucket: bucket,
        Key: key,
    });
    try {
        const obj = await s3Client.send(cmd);
        if (obj.Body === undefined) return null;
        const buf = Buffer.from(await obj.Body.transformToByteArray());
        return buf;
    } catch (e: any) {
        if (e.name !== 'NoSuchKey') console.log(e);
        return null;
    }
}

export { getS3Source };
