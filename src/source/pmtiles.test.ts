import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

import { describe, it, expect, beforeAll, afterAll } from 'vitest';

import { getPmtilesSource } from './pmtiles.js';
import { memoryCache } from '../cache/memory.js';

// Serve localdata/school.pmtiles over HTTP with byte-range support (pmtiles
// requires it) and a switchable failure mode, so we can exercise how the
// pmtiles source reacts to upstream 5xx.
const archive = fs.readFileSync(
    path.join(process.cwd(), 'localdata/school.pmtiles'),
);
let mode: 'ok' | '500' | '404' = 'ok';
let server: http.Server;
let port = 0;

beforeAll(async () => {
    server = http.createServer((req, res) => {
        if (mode === '500') {
            res.statusCode = 500;
            res.end('upstream error');
            return;
        }
        if (mode === '404') {
            res.statusCode = 404;
            res.end('not found');
            return;
        }
        const range = req.headers.range;
        if (range) {
            const m = /bytes=(\d+)-(\d+)/.exec(range)!;
            const start = Number(m[1]);
            const end = Number(m[2]);
            const chunk = archive.subarray(start, end + 1);
            res.statusCode = 206;
            res.setHeader('Content-Range', `bytes ${start}-${end}/${archive.length}`);
            res.setHeader('Content-Length', String(chunk.length));
            res.setHeader('Accept-Ranges', 'bytes');
            res.end(chunk);
            return;
        }
        res.setHeader('Content-Length', String(archive.length));
        res.setHeader('Accept-Ranges', 'bytes');
        res.end(archive);
    });
    await new Promise<void>((resolve) =>
        server.listen(0, '127.0.0.1', () => {
            port = (server.address() as import('node:net').AddressInfo).port;
            resolve();
        }),
    );
});

afterAll(() => {
    server.close();
});

// A distinct archive URL per test keeps the module-level pmtilesCache from
// leaking a resolved archive object between cases.
const uri = (tag: string, zxy: string) =>
    `pmtiles://http://127.0.0.1:${port}/school.pmtiles?t=${tag}/${zxy}`;

describe('getPmtilesSource (http)', () => {
    it('returns a tile that exists in the archive', async () => {
        mode = 'ok';
        const cache = memoryCache({ ttl: 60, maxItemCount: 10 });
        const buf = await getPmtilesSource(uri('exists', '6/57/23'), cache);
        expect(buf).not.toBeNull();
        expect(buf!.length).toBeGreaterThan(0);
    });

    it('returns null for a tile absent from the archive (empty tile)', async () => {
        mode = 'ok';
        const cache = memoryCache({ ttl: 60, maxItemCount: 10 });
        const buf = await getPmtilesSource(uri('sparse', '14/1/1'), cache);
        expect(buf).toBeNull();
    });

    it('throws on upstream 5xx instead of returning an empty tile', async () => {
        mode = '500';
        const cache = memoryCache({ ttl: 60, maxItemCount: 10 });
        await expect(
            getPmtilesSource(uri('err5xx', '6/57/23'), cache),
        ).rejects.toThrow();
    });

    it('recovers after a transient header-load failure (no sticky poisoning)', async () => {
        const cache = memoryCache({ ttl: 60, maxItemCount: 10 });
        // first request loads the archive header while upstream is failing
        mode = '500';
        await expect(
            getPmtilesSource(uri('recover', '6/57/23'), cache),
        ).rejects.toThrow();

        // upstream recovers; the failed archive object must not be cached, so
        // the next request rebuilds it and succeeds
        mode = 'ok';
        const buf = await getPmtilesSource(uri('recover', '6/57/23'), cache);
        expect(buf).not.toBeNull();
        expect(buf!.length).toBeGreaterThan(0);
    });
});
