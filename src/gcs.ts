import { Storage } from '@google-cloud/storage';

type StorageOptions = {
    projectId?: string;
    keyFilename?: string;
    apiEndpoint?: string;
};

// One client per configuration (see getS3Client).
const clients = new Map<string, Storage>();
const getStorageClient = (options: StorageOptions) => {
    const { projectId, keyFilename, apiEndpoint } = options;
    const key = JSON.stringify([projectId, keyFilename, apiEndpoint]);
    let client = clients.get(key);
    if (client === undefined) {
        client = new Storage({ projectId, keyFilename, apiEndpoint });
        clients.set(key, client);
    }
    return client;
};

export { getStorageClient };
