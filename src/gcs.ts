import { addAbortSignal } from 'node:stream';
import type { File, Storage } from '@google-cloud/storage';

type StorageOptions = {
    projectId?: string;
    keyFilename?: string;
    apiEndpoint?: string;
};

let sdk: Promise<typeof import('@google-cloud/storage')> | undefined;

// One client per configuration (see getS3).
const clients = new Map<string, Storage>();
/** A client of `@google-cloud/storage`, an optional peer dependency loaded on first use. */
async function getStorage(options: StorageOptions): Promise<Storage> {
    sdk ??= import('@google-cloud/storage').catch((e) => {
        sdk = undefined;
        throw new Error(
            'gs:// sources and the gcs cache need @google-cloud/storage: install it',
            { cause: e },
        );
    });
    const { Storage } = await sdk;
    const { projectId, keyFilename, apiEndpoint } = options;
    const key = JSON.stringify([projectId, keyFilename, apiEndpoint]);
    let client = clients.get(key);
    if (client === undefined) {
        client = new Storage({ projectId, keyFilename, apiEndpoint });
        clients.set(key, client);
    }
    return client;
}

/**
 * The bytes of `file`, failing after 30 s (body included). Renders wait on sources, and a stalled
 * bucket must not stall them: the client's `timeout` does not apply to downloads.
 */
async function download(file: File): Promise<Buffer> {
    // destroys the stream when it fires (toArray's own signal is only read between chunks)
    const stream = addAbortSignal(
        AbortSignal.timeout(30_000),
        file.createReadStream(),
    );
    const chunks = await stream.toArray();
    return Buffer.concat(chunks);
}

export { download, getStorage };
