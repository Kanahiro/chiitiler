import { getStorageClient } from '../gcs.js';

async function getGCSSource(uri: string) {
    const storageClient = getStorageClient({
        projectId: process.env.CHIITILER_GCS_PROJECT_ID,
        keyFilename: process.env.CHIITILER_GCS_KEY_FILENAME,
    });
    const bucket = uri.replace('gs://', '').split('/')[0];
    const path = uri.replace(`gs://${bucket}/`, '');

    try {
        const file = storageClient.bucket(bucket).file(path);
        const [buffer] = await file.download();
        return buffer;
    } catch (e: any) {
        if (e.code !== 404) console.log(e);
        return null;
    }
}

export { getGCSSource };
