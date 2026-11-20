import * as fs from 'node:fs';
import { utimes } from 'node:fs/promises';
import { join } from 'node:path';

import { afterEach, describe, it, expect } from 'vitest';

import { fileCache } from './file.js';

const dir = 'temp-file-cache';

afterEach(() => fs.rmSync(dir, { recursive: true, force: true }));

describe('fileCache', () => {
    it('getset', async () => {
        const cache = fileCache({ dir, ttl: 1000 });
        expect(await cache.get('key')).toBe(undefined);

        await cache.set('key', new Uint8Array([0, 255, 1]));
        expect([...(await cache.get('key'))!]).toEqual([0, 255, 1]);
        // stored as raw bytes, one file per key, nothing left over
        const files = fs.readdirSync(dir);
        expect(files).toHaveLength(1);
        expect(fs.statSync(join(dir, files[0])).size).toBe(3);
    });

    it('expires values older than ttl', async () => {
        const cache = fileCache({ dir, ttl: 10 });
        await cache.set('key', new Uint8Array([1]));
        const [file] = fs.readdirSync(dir);
        const old = new Date(Date.now() - 20_000);
        await utimes(join(dir, file), old, old);
        expect(await cache.get('key')).toBe(undefined);
        expect(fs.readdirSync(dir)).toHaveLength(0);
    });

    it('keeps values forever with ttl 0', async () => {
        const cache = fileCache({ dir, ttl: 0 });
        await cache.set('key', new Uint8Array([1]));
        const [file] = fs.readdirSync(dir);
        const old = new Date(0);
        await utimes(join(dir, file), old, old);
        expect([...(await cache.get('key'))!]).toEqual([1]);
    });
});
