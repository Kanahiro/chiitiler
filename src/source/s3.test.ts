import { GetObjectCommand } from '@aws-sdk/client-s3';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const { send } = vi.hoisted(() => ({ send: vi.fn() }));

vi.mock('../s3.js', () => ({
    getS3Client: () => ({ send }),
}));

import { getS3Source } from './s3.js';

describe('getS3Source', () => {
    beforeEach(() => {
        send.mockReset();
    });

    it('loads a percent-encoded key using its decoded form first', async () => {
        send.mockResolvedValueOnce({
            Body: {
                transformToByteArray: async () =>
                    Uint8Array.from(Buffer.from('glyph')),
            },
        });

        const result = await getS3Source(
            's3://fonts/Noto%20Sans%20Regular%2CArial/0-255.pbf',
        );

        expect(result?.toString()).toBe('glyph');
        expect(send).toHaveBeenCalledOnce();
        expect((send.mock.calls[0][0] as GetObjectCommand).input).toEqual({
            Bucket: 'fonts',
            Key: 'Noto Sans Regular,Arial/0-255.pbf',
        });
    });

    it('does not hide errors other than a missing key', async () => {
        const error = new Error('network failure');
        send.mockRejectedValueOnce(error);

        await expect(getS3Source('s3://fonts/glyph.pbf')).rejects.toBe(error);
        expect(send).toHaveBeenCalledTimes(1);
    });
});
