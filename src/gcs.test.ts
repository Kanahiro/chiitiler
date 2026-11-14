import { describe, it, expect } from 'vitest';
import { Storage } from '@google-cloud/storage';

import { getStorage } from './gcs.js';

describe('getStorage', () => {
    it('one client per configuration', async () => {
        const client1 = await getStorage({});
        expect(client1).toBeInstanceOf(Storage);
        const client2 = await getStorage({});
        expect(client2).toBe(client1); // one per configuration
        const client3 = await getStorage({ projectId: 'test-project' });
        expect(client3).not.toBe(client1);
        expect(await getStorage({ projectId: 'test-project' })).toBe(client3);
    });
});
