import { beforeEach, describe, expect, it, vi } from 'vitest';

const { download, file } = vi.hoisted(() => {
    const download = vi.fn();
    return {
        download,
        file: vi.fn(() => ({ download })),
    };
});

vi.mock('../gcs.js', () => ({
    getStorageClient: () => ({
        bucket: () => ({ file }),
    }),
}));

import { getGCSSource } from './gcs.js';

describe('getGCSSource', () => {
    beforeEach(() => {
        download.mockReset();
        file.mockClear();
    });

    it('loads a percent-encoded path using its decoded form first', async () => {
        download.mockResolvedValueOnce([Buffer.from('glyph')]);

        const result = await getGCSSource(
            'gs://fonts/Noto%20Sans%20Regular%2CArial/0-255.pbf',
        );

        expect(result?.toString()).toBe('glyph');
        expect(file).toHaveBeenCalledOnce();
        expect(file).toHaveBeenCalledWith(
            'Noto Sans Regular,Arial/0-255.pbf',
        );
    });

    it('does not hide errors other than a missing object', async () => {
        const error = new Error('network failure');
        download.mockRejectedValueOnce(error);

        await expect(getGCSSource('gs://fonts/glyph.pbf')).rejects.toBe(error);
        expect(download).toHaveBeenCalledTimes(1);
    });
});
