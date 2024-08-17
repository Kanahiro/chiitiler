import { S3Client, S3ClientConfig } from '@aws-sdk/client-s3';

let s3Client: S3Client; // singleton
const getS3Client = function ({
    region,
    endpoint,
    forcePathStyle,
}: {
    region: string;
    endpoint?: string;
    forcePathStyle?: boolean;
}) {
    if (s3Client !== undefined) return s3Client;

    const s3ClientConfig: S3ClientConfig = {
        region,
        endpoint,
        forcePathStyle,
    };

    s3Client = new S3Client(s3ClientConfig);
    return s3Client;
};

export { getS3Client };
