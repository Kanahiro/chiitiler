import { getS3 } from '../s3.js';
import { loadWithDecodedPathFallback } from './path.js';

async function getS3Source(uri: string) {
    const { client, sdk } = await getS3({
        region: process.env.CHIITILER_S3_REGION ?? 'us-east-1',
        endpoint: process.env.CHIITILER_S3_ENDPOINT,
        forcePathStyle: process.env.CHIITILER_S3_FORCE_PATH_STYLE === 'true',
    });
    const bucket = uri.replace('s3://', '').split('/')[0];
    const key = uri.replace(`s3://${bucket}/`, '');

    return loadWithDecodedPathFallback(key, async (candidate) => {
        const cmd = new sdk.GetObjectCommand({
            Bucket: bucket,
            Key: candidate,
        });
        try {
            const obj = await client.send(cmd);
            if (obj.Body === undefined) return null;
            return await obj.Body.transformToByteArray();
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
