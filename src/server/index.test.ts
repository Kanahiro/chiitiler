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
});
