import { describe, it, expect } from 'vitest';
import { Storage } from '@google-cloud/storage';

import { getStorageClient } from './gcs.js';

describe('getStorageClient', () => {
    it('getStorageClient', async () => {
        const client1 = getStorageClient({});
        expect(client1).toBeInstanceOf(Storage);
        const client2 = getStorageClient({});
        expect(client2).toBe(client1); // one per configuration
        const client3 = getStorageClient({ projectId: 'test-project' });
        expect(client3).not.toBe(client1);
        expect(getStorageClient({ projectId: 'test-project' })).toBe(client3);
    });
});
