import * as fs from 'fs';

import { describe, it, expect } from 'vitest';

import { memoryCache } from './memory.js';

describe('memoryCache', () => {
    it('getset', async () => {
        const _fileCache = memoryCache({ ttl: 1000, maxItemCount: 1000 });

        const val = await _fileCache.get('key');
        expect(val).toBe(undefined);

        await _fileCache.set('key', Buffer.from('value'));

        const val2 = await _fileCache.get('key');
        expect(val2).toBeInstanceOf(Buffer);
        expect(val2!.toString()).toBe('value');
    });

    it('ttl', async () => {
        const _fileCache = memoryCache({
            ttl: 1,
            maxItemCount: 1000,
            checkInterval: 1,
        });

        await _fileCache.set('key', Buffer.from('value'));

        const val = await _fileCache.get('key');
        expect(val).toBeInstanceOf(Buffer);

        await new Promise((resolve) => setTimeout(resolve, 1000));

        // expired
        const val2 = await _fileCache.get('key');
        expect(val2).toBe(undefined);
    });
});
