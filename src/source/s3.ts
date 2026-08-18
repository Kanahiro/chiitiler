import { GetObjectCommand } from '@aws-sdk/client-s3';

import { getS3Client } from '../s3.js';
import { loadWithDecodedPathFallback } from './path.js';

async function getS3Source(uri: string) {
    const s3Client = getS3Client({
        region: process.env.CHIITILER_S3_REGION ?? 'us-east-1',
        endpoint: process.env.CHIITILER_S3_ENDPOINT,
        forcePathStyle: process.env.CHIITILER_S3_FORCE_PATH_STYLE === 'true',
    });
    const bucket = uri.replace('s3://', '').split('/')[0];
    const key = uri.replace(`s3://${bucket}/`, '');

    return loadWithDecodedPathFallback(key, async (candidate) => {
        const cmd = new GetObjectCommand({
            Bucket: bucket,
            Key: candidate,
        });
        try {
            const obj = await s3Client.send(cmd);
            if (obj.Body === undefined) return null;
            return Buffer.from(await obj.Body.transformToByteArray());
        } catch (e: any) {
            // NoSuchKey: オブジェクトが存在しない。空タイルとして扱わせる
            if (e.name === 'NoSuchKey') return null;
            // それ以外(スロットリング・ネットワークエラー等)は有無が不明なので
            // throwしてレンダリングを失敗させる
            console.log(e);
            throw e;
        }
    });
}

export { getS3Source };
