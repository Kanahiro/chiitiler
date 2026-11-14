import type { S3Client } from '@aws-sdk/client-s3';

type S3Sdk = typeof import('@aws-sdk/client-s3');
type S3Options = {
    region: string;
    endpoint?: string;
    forcePathStyle?: boolean;
};

let sdk: Promise<S3Sdk> | undefined;
/** `@aws-sdk/client-s3`, an optional peer dependency: loaded on first use. */
function loadSdk(): Promise<S3Sdk> {
    sdk ??= import('@aws-sdk/client-s3').catch((e) => {
        sdk = undefined;
        throw new Error(
            's3:// sources and the s3 cache need @aws-sdk/client-s3: install it',
            { cause: e },
        );
    });
    return sdk;
}

// One client per configuration: a cache bucket and s3:// sources may live in different regions
// or behind different endpoints.
const clients = new Map<string, S3Client>();
async function getS3(
    options: S3Options,
): Promise<{ client: S3Client; sdk: S3Sdk }> {
    const s3 = await loadSdk();
    const { region, endpoint, forcePathStyle } = options;
    const key = JSON.stringify([region, endpoint, forcePathStyle]);
    let client = clients.get(key);
    if (client === undefined) {
        client = new s3.S3Client({ region, endpoint, forcePathStyle });
        clients.set(key, client);
    }
    return { client, sdk: s3 };
}

export { getS3 };
