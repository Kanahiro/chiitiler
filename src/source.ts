/// <reference lib="dom" />
// for using native fetch in TypeScript

import * as fs from 'fs';

import { GetObjectCommand } from '@aws-sdk/client-s3';

import { getS3Client } from './s3.js';

async function getSource(uri: string): Promise<Buffer | null> {
    if (uri.startsWith('file:///')) {
        return new Promise((resolve, reject) => {
            fs.readFile(uri.replace('file:///', ''), (err, data) => {
                if (err) reject(err);
                resolve(data);
            });
        });
    }

    if (uri.startsWith('s3://')) {
        const s3Client = getS3Client(process.env.CHIITILER_S3_REGION);
        const bucket = uri.replace('s3://', '').split('/')[0];
        const key = uri.replace(`s3://${bucket}/`, '');
        const cmd = new GetObjectCommand({
            Bucket: bucket,
            Key: key,
        });
        try {
            console.log(uri);
            const obj = await s3Client.send(cmd);
            if (obj.Body === undefined) return null;
            const buf = Buffer.from(await obj.Body.transformToByteArray());
            console.log(buf);
            return buf;
        } catch (e) {
            console.log(e);
            return null;
        }
    }

    if (uri.startsWith('http://') || uri.startsWith('https://')) {
        const res = await fetch(uri);
        return Buffer.from(await res.arrayBuffer());
    }

    return null;
}

export { getSource };
