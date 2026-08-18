import { describe, expect, it, vi } from 'vitest';

import { loadWithDecodedPathFallback } from './path.js';

describe('loadWithDecodedPathFallback', () => {
    it('prefers the decoded form of a percent-encoded path', async () => {
        const load = vi.fn().mockResolvedValue(Buffer.from('decoded'));

        const result = await loadWithDecodedPathFallback('font%20name', load);

        expect(result?.toString()).toBe('decoded');
        expect(load).toHaveBeenCalledOnce();
        expect(load).toHaveBeenCalledWith('font name');
    });

    it('falls back to an existing literal percent-encoded path', async () => {
        const load = vi
            .fn()
            .mockResolvedValueOnce(null)
            .mockResolvedValueOnce(Buffer.from('literal'));

        const result = await loadWithDecodedPathFallback('font%20name', load);

        expect(result?.toString()).toBe('literal');
        expect(load.mock.calls).toEqual([['font name'], ['font%20name']]);
    });

    it('does not retry malformed percent encoding', async () => {
        const load = vi.fn().mockResolvedValue(null);

        await expect(
            loadWithDecodedPathFallback('font%ZZname', load),
        ).resolves.toBeNull();
        expect(load).toHaveBeenCalledOnce();
    });
});
