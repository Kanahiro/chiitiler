import * as fs from 'fs';

import { describe, it, expect } from 'vitest';

import { fileCache } from './file.js';

describe('fileCache', () => {
    it('getset', async () => {
        if (fs.existsSync('temp')) fs.rmSync('temp', { recursive: true });

        const _fileCache = fileCache({ dir: 'temp', ttl: 1000 });

        const val = await _fileCache.get('key');
        expect(val).toBe(undefined);

        await _fileCache.set('key', Buffer.from('value'));

        const val2 = await _fileCache.get('key');
        expect(val2).toBeInstanceOf(Buffer);
        expect(val2!.toString()).toBe('value');

        fs.rmSync('temp', { recursive: true });
    });
});
