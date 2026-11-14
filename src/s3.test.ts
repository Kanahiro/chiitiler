import { describe, it, expect } from 'vitest';
import { S3Client } from '@aws-sdk/client-s3';

import { getS3 } from './s3.js';

describe('getS3', () => {
    it('one client per configuration', async () => {
        const client1 = (await getS3({ region: 'us-east-1' })).client;
        expect(client1).toBeInstanceOf(S3Client);
        const client2 = (await getS3({ region: 'us-east-1' })).client;
        expect(client2).toBe(client1); // one per configuration
        const client3 = (await getS3({ region: 'ap-northeast-1' })).client;
        expect(client3).not.toBe(client1);
        expect((await getS3({ region: 'ap-northeast-1' })).client).toBe(
            client3,
        );
    });
});
