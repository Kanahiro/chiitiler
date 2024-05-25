import { describe, it, expect } from 'vitest';

import { noneCache } from './index.js';

describe('noneCache', () => {
    it('getset', async () => {
        const cache = noneCache();
        await cache.set('key', Buffer.from('value'));
        expect(await cache.get('key')).toBe(undefined);
    });
});
