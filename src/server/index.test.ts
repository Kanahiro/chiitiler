import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, it, expect, vi } from 'vitest';

import { initServer } from './index.js';
import * as cache from '../cache/index.js';

describe('initServer', () => {
    it('init', async () => {
        const { app } = await initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
        });
        const res = await app.request('/health');
        expect(res.status).toBe(200);
        expect(await res.text()).toBe('OK');

        const debug = await app.request('/debug');
        expect(debug.status).toBe(404);

        const editor = await app.request('/editor');
        expect(editor.status).toBe(404);
    });

    it('init with debug', async () => {
        const { app } = await initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: true,
        });
        const res = await app.request('/health');
        expect(res.status).toBe(200);
        expect(await res.text()).toBe('OK');

        const debug = await app.request('/debug');
        expect(debug.status).toBe(200);

        const editor = await app.request('/editor');
        expect(editor.status).toBe(200);
    });

    it('tiles', async () => {
        const { app } = await initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
        });

        const res = await app.request('/tiles/0/0/0.png');
        expect(res.status).toBe(400);
        expect(await res.text()).toBe('url is required');

        const res2 = await app.request(
            '/tiles/0/0/0.gif?url=file://localdata/style.json',
        );
        expect(res2.status).toBe(400);
        expect(await res2.text()).toBe('invalid format');

        const res3 = await app.request(
            '/tiles/0/10/0.webp?url=file://localdata/style.json',
        );
        expect(res3.status).toBe(400);
        expect(await res3.text()).toBe('invalid xyz');

        // rendering will be tested in integration test
    });

    it('bbox', async () => {
        const { app } = await initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
        });

        const res = await app.request('/clip.png?bbox=0,1,2,3');
        expect(res.status).toBe(400);
        expect(await res.text()).toBe('url is required');

        const res2 = await app.request(
            '/clip.gif?bbox=0,1,2,3&url=file://localdata/style.json',
        );
        expect(res2.status).toBe(400); // invalid format will not be handled in /clip

        const res3 = await app.request(
            'clip.png?bbox=0,0,0,0&url=file://localdata/style.json',
        );
        expect(res3.status).toBe(400);
        expect(await res3.text()).toBe('invalid bbox');

        // rendering will be tested in integration test
    });

    it('POST with missing or invalid body returns 400', async () => {
        const { app } = await initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
        });

        const jsonPost = (path: string, body: string) =>
            app.request(path, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body,
            });

        for (const path of [
            '/tiles/0/0/0.png',
            '/clip.png?bbox=0,1,2,3',
            '/camera/10/67.89/123.45/0/1024x1024.png',
        ]) {
            // empty object body - missing style key
            const res1 = await jsonPost(path, '{}');
            expect(res1.status).toBe(400);
            expect(await res1.text()).toBe('invalid stylejson');

            // malformed JSON body
            const res2 = await jsonPost(path, 'not-json');
            expect(res2.status).toBe(400);
            expect(await res2.text()).toBe('invalid stylejson');

            // null style
            const res3 = await jsonPost(path, '{"style":null}');
            expect(res3.status).toBe(400);
            expect(await res3.text()).toBe('invalid stylejson');
        }
    });

    it('camera', async () => {
        const { app } = await initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
        });

        const res = await app.request(
            '/camera/10/67.89/123.45/0/1024x1024.png',
        );
        expect(res.status).toBe(400);
        expect(await res.text()).toBe('url is required');

        const res2 = await app.request(
            '/camera/10/67.89/123.45/0/1024x1024.gif?url=file://localdata/style.json',
        );
        expect(res2.status).toBe(400);
        expect(await res2.text()).toBe('invalid format');

        const res3 = await app.request(
            '/camera/10/67.89/1234.5/0/1024x1024.png?url=file://localdata/style.json',
        );
        expect(res3.status).toBe(400);
        expect(await res3.text()).toBe('invalid camera');

        const res4 = await app.request(
            '/camera/10/67.89/123.45/0/1024xno.png?url=file://localdata/style.json',
        );
        expect(res4.status).toBe(400);
        expect(await res4.text()).toBe('invalid dimensions');
    });

    it('renders an inline style', async () => {
        const { app } = await initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
        });
        const style = {
            version: 8,
            sources: {},
            layers: [
                {
                    id: 'bg',
                    type: 'background',
                    paint: { 'background-color': '#ff0000' },
                },
            ],
        };
        for (const [path, type, magic] of [
            ['/tiles/1/0/0.png?tileSize=256', 'image/png', [0x89, 0x50]],
            ['/tiles/1/0/0.jpg', 'image/jpeg', [0xff, 0xd8]],
            ['/clip.webp?bbox=0,0,10,10&size=64', 'image/webp', [0x52, 0x49]],
            ['/camera/2/0/0/30/64x32.png', 'image/png', [0x89, 0x50]],
        ] as const) {
            const res = await app.request(path, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ style }),
            });
            expect(res.status, path).toBe(200);
            expect(res.headers.get('content-type')).toBe(type);
            const bytes = new Uint8Array(await res.arrayBuffer());
            expect([...bytes.slice(0, 2)]).toEqual(magic);
        }
    });

    it('limits image sizes and request bodies', async () => {
        const { app } = initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
            maxSize: 1000,
        });
        const style = { version: 8, sources: {}, layers: [] };
        const post = (path: string, body: string) =>
            app.request(path, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body,
            });
        for (const path of [
            '/camera/1/0/0/0/1001x10.png',
            '/clip.png?bbox=0,0,1,1&size=1001',
            '/tiles/0/0/0.png?tileSize=1000&margin=2',
        ]) {
            const res = await post(path, JSON.stringify({ style }));
            expect(res.status, path).toBe(400);
            expect(await res.text()).toBe(
                'image sides must be at most 1000 px',
            );
        }
        const ok = await post(
            '/camera/1/0/0/0/1000x10.png',
            JSON.stringify({ style }),
        );
        expect(ok.status).toBe(200);

        const big = await post(
            '/tiles/0/0/0.png',
            JSON.stringify({ style, padding: 'x'.repeat(17 * 1024 * 1024) }),
        );
        expect(big.status).toBe(413);
    });

    it('sets caching and CORS headers', async () => {
        const plain = initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
        }).app;
        const { app } = initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
            cacheControl: 'public, max-age=60',
            corsOrigins: ['*'],
        });
        const dir = mkdtempSync(join(tmpdir(), 'chiitiler-'));
        const style = join(dir, 'style.json');
        writeFileSync(
            style,
            JSON.stringify({ version: 8, sources: {}, layers: [] }),
        );
        const path = `/tiles/0/0/0.png?url=${encodeURIComponent(`file://${style}`)}`;
        try {
            const none = await plain.request(path);
            expect(none.status).toBe(200);
            expect(none.headers.get('cache-control')).toBeNull();
            expect(none.headers.get('access-control-allow-origin')).toBeNull();

            const res = await app.request(path, {
                headers: { Origin: 'https://example.com' },
            });
            expect(res.headers.get('cache-control')).toBe('public, max-age=60');
            expect(res.headers.get('access-control-allow-origin')).toBe('*');
            const tag = res.headers.get('etag');
            expect(tag).toBeTruthy();
            const again = await app.request(path, {
                headers: { 'If-None-Match': tag! },
            });
            expect(again.status).toBe(304);
            expect(again.headers.get('cache-control')).toBe(
                'public, max-age=60',
            );
        } finally {
            rmSync(dir, { recursive: true });
        }

        // errors are not marked cacheable
        const bad = await app.request('/tiles/0/0/0.gif?url=x');
        expect(bad.status).toBe(400);
        expect(bad.headers.get('cache-control')).toBeNull();
        expect((await app.request('/other.png?url=x')).status).toBe(404);
    });

    it('keeps the event loop free while rendering', async () => {
        const { app } = initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
        });
        // ~15,000 circles: about 100 ms of drawing per tile
        const url = `file://${process.cwd()}/tests/fixtures/bench-style.json`;
        const path = `/tiles/5/28/12.png?url=${encodeURIComponent(url)}`;
        expect((await app.request(path)).status).toBe(200); // load resources
        let last = performance.now();
        let longest = 0;
        const timer = setInterval(() => {
            const now = performance.now();
            longest = Math.max(longest, now - last);
            last = now;
        }, 1);
        const renders = [0, 1, 2].map((i) =>
            app.request(`${path}&margin=${2 * i}`),
        );
        for (const res of await Promise.all(renders))
            expect(res.status).toBe(200);
        clearInterval(timer);
        expect(longest).toBeLessThan(50);
    });
});
