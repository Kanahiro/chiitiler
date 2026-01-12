import { describe, it, expect, vi } from 'vitest';

import { initServer } from './index.js';
import * as cache from '../cache/index.js';

vi.mock('@maplibre/maplibre-gl-native', () => ({}));

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

    it('camera', async () => {
        const { app } = await initServer({
            port: 8989,
            cache: cache.noneCache(),
            debug: false,
            stream: false,
        });

        const res = await app.request('/camera/10/67.89/123.45/0/0/1024x1024.png');
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

        // rendering will be tested in integration test
    });
});
