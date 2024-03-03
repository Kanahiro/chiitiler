import { S3Client, S3ClientConfig } from '@aws-sdk/client-s3';

let s3Client: S3Client; // singleton
const getS3Client = function (region: string) {
    if (s3Client !== undefined) return s3Client;

    let s3ClientConfig: S3ClientConfig = { region };
    if (process.env.NODE_ENV === 'development') {
        s3ClientConfig = {
            region,
            credentials: {
                accessKeyId: 'minioadmin',
                secretAccessKey: 'minioadmin',
            },
            forcePathStyle: true, // special option for minio
            endpoint: 'http://minio:9000',
        };
    }
    s3Client = new S3Client(s3ClientConfig);
    return s3Client;
};

export { getS3Client };
