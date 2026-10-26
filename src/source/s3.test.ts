import fs from 'node:fs';
import path from 'node:path';

import { GetObjectCommand } from '@aws-sdk/client-s3';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { send } = vi.hoisted(() => ({ send: vi.fn() }));

vi.mock('../s3.js', () => ({
    getS3Client: () => ({ send }),
}));

import { getS3Source } from './s3.js';
import { getSource } from './index.js';
import { type Cache, withMemoryCache } from '../cache/index.js';

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

describe('s3 sources use only the memory front cache', () => {
    const backing = {
        name: 'spy',
        get: vi.fn(async () => undefined),
        set: vi.fn(async () => undefined),
    } satisfies Cache;

    beforeEach(() => {
        send.mockReset();
        backing.get.mockClear();
        backing.set.mockClear();
    });

    afterEach(() => {
        // object storage is never copied into the backing cache
        expect(backing.get).not.toHaveBeenCalled();
        expect(backing.set).not.toHaveBeenCalled();
    });

    it('s3:// hits the cache on the second request', async () => {
        send.mockResolvedValue({
            Body: {
                transformToByteArray: async () =>
                    Uint8Array.from(Buffer.from('sprite')),
            },
        });
        const cache = withMemoryCache(backing);

        const a = await getSource('s3://assets/sprite.png', cache);
        const b = await getSource('s3://assets/sprite.png', cache);

        expect(a?.toString()).toBe('sprite');
        expect(b?.toString()).toBe('sprite');
        expect(send).toHaveBeenCalledOnce();
    });

    it('pmtiles://s3:// hits the cache on the second request', async () => {
        const archive = fs.readFileSync(
            path.join(process.cwd(), 'localdata/school.pmtiles'),
        );
        send.mockImplementation(async (cmd: GetObjectCommand) => {
            const [start, end] = /bytes=(\d+)-(\d+)/
                .exec(cmd.input.Range!)!
                .slice(1)
                .map(Number);
            return {
                Body: {
                    transformToByteArray: async () =>
                        Uint8Array.from(archive.subarray(start, end + 1)),
                },
            };
        });
        const cache = withMemoryCache(backing);
        const uri = 'pmtiles://s3://tiles/school.pmtiles/0/0/0';

        const a = await getSource(uri, cache);
        const calls = send.mock.calls.length;
        const b = await getSource(uri, cache);

        expect(a).not.toBeNull();
        expect(b?.equals(a!)).toBe(true);
        expect(send).toHaveBeenCalledTimes(calls);
    });
});
