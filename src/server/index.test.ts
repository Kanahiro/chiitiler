import { describe, it, expect, vi } from 'vitest';

import { initServer } from './index.js';
import * as cache from '../cache/index.js';

describe('initServer', () => {
    it('init', async () => {
        const { app } = await initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
            stream: false,
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
            stream: false,
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
            stream: false,
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
            stream: false,
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
            stream: false,
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
            '/camera/10/67.89/123.45/0/0/1024x1024.png',
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
            stream: false,
        });

        const res = await app.request(
            '/camera/10/67.89/123.45/0/0/1024x1024.png',
        );
        expect(res.status).toBe(400);
        expect(await res.text()).toBe('url is required');

        const res2 = await app.request(
            '/camera/10/67.89/123.45/0/0/1024x1024.gif?url=file://localdata/style.json',
        );
        expect(res2.status).toBe(400);
        expect(await res2.text()).toBe('invalid format');

        const res3 = await app.request(
            '/camera/10/67.89/1234.5/0/0/1024x1024.png?url=file://localdata/style.json',
        );
        expect(res3.status).toBe(400);
        expect(await res3.text()).toBe('invalid camera');

        const res4 = await app.request(
            '/camera/10/67.89/123.45/0/0/1024xno.png?url=file://localdata/style.json',
        );
        expect(res4.status).toBe(400);
        expect(await res4.text()).toBe('invalid dimensions');

        const res5 = await app.request(
            '/camera/10/67.89/123.45/0/45/1024x1024.png?url=file://localdata/style.json',
        );
        expect(res5.status).toBe(400);
        expect(await res5.text()).toBe('pitch is not supported');
    });

    it('renders an inline style', async () => {
        const { app } = await initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
            stream: false,
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
            ['/camera/2/0/0/30/0/64x32.png', 'image/png', [0x89, 0x50]],
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
});
