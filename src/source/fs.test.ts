import { describe, expect, it } from 'vitest';

import { getFilesystemSource } from './fs.js';

describe('getFilesystemSource', () => {
    it('returns null for a missing file', async () => {
        expect(await getFilesystemSource('file:///no/such/file')).toBeNull();
    });

    it('refuses devices: /dev/zero never ends and would take all memory', async () => {
        await expect(getFilesystemSource('file:///dev/zero')).rejects.toThrow(
            'not a regular file',
        );
    });

    it('throws for other failures, which are not "missing"', async () => {
        await expect(getFilesystemSource('file:///tmp')).rejects.toThrow();
    });
});
