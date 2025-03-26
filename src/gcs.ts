import { Storage, type StorageOptions } from '@google-cloud/storage';

let storageClient: Storage; // singleton
const getStorageClient = function ({
    projectId,
    keyFilename,
    apiEndpoint
}: {
    projectId?: string;
    keyFilename?: string;
    apiEndpoint?: string;
}) {
    if (storageClient !== undefined) return storageClient;

    const storageOptions: StorageOptions = {
        projectId,
        keyFilename,
        apiEndpoint
    };

    storageClient = new Storage(storageOptions);
    return storageClient;
};

export { getStorageClient };
