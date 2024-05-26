import { describe, it, expect } from 'vitest';
import { S3Client } from '@aws-sdk/client-s3';

import { getS3Client } from './s3.js';

describe('getS3Client', () => {
  it('getS3Client', async () => {
    const client1 = getS3Client({ region: 'us-east-1', endpoint: null });
    expect(client1).toBeInstanceOf(S3Client);
    const client2 = getS3Client({ region: 'us-east-1', endpoint: null });
    expect(client2).toBe(client1); // singleton
    const client3 = getS3Client({ region: 'ap-northeast-1', endpoint: null });
    expect(client3).toBe(client1); // even when different options...
  });
});