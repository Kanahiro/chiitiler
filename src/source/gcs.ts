import { getStorageClient } from '../gcs.js';
import { loadWithDecodedPathFallback } from './path.js';

async function getGCSSource(uri: string) {
    const storageClient = getStorageClient({
        projectId: process.env.CHIITILER_GCS_PROJECT_ID,
        keyFilename: process.env.CHIITILER_GCS_KEY_FILENAME,
        apiEndpoint: process.env.CHIITILER_GCS_API_ENDPOINT,
    });
    const bucket = uri.replace('gs://', '').split('/')[0];
    const path = uri.replace(`gs://${bucket}/`, '');

    return loadWithDecodedPathFallback(path, async (candidate) => {
        try {
            const file = storageClient.bucket(bucket).file(candidate);
            const [buffer] = await file.download();
            return buffer;
        } catch (e: any) {
            // 404: オブジェクトが存在しない。空タイルとして扱わせる
            if (e.code === 404) return null;
            // それ以外は有無が不明なのでthrowしてレンダリングを失敗させる
            console.log(e);
            throw e;
        }
    });
}

export { getGCSSource };
