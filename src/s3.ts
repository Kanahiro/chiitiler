import { S3Client } from '@aws-sdk/client-s3';

type S3Options = {
    region: string;
    endpoint?: string;
    forcePathStyle?: boolean;
};

// One client per configuration: a cache bucket and s3:// sources may live in different regions
// or behind different endpoints.
const clients = new Map<string, S3Client>();
const getS3Client = (options: S3Options) => {
    const { region, endpoint, forcePathStyle } = options;
    const key = JSON.stringify([region, endpoint, forcePathStyle]);
    let client = clients.get(key);
    if (client === undefined) {
        client = new S3Client({ region, endpoint, forcePathStyle });
        clients.set(key, client);
    }
    return client;
};

export { getS3Client };
