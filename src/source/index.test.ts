import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { describe, it, expect, vi } from 'vitest';

import { getSource } from './index.js';

describe('getSource', () => {
    it('file://', async () => {
        const uri = 'file://localdata/style.json';
        const data = await getSource(uri);
        expect(data).not.toBeNull();
    });

    it('file:// with percent-encoded path', async () => {
        // maplibre-gl-native percent-encodes {fontstack} in glyphs URLs
        const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'chiitiler-'));
        const fontDir = path.join(dir, 'Noto Sans Regular');
        fs.mkdirSync(fontDir);
        fs.writeFileSync(path.join(fontDir, '0-255.pbf'), 'glyph');
        try {
            const uri = `file://${dir}/Noto%20Sans%20Regular/0-255.pbf`;
            const data = await getSource(uri);
            expect(data).not.toBeNull();
            expect(data?.toString()).toBe('glyph');
        } finally {
            fs.rmSync(dir, { recursive: true, force: true });
        }
    });

    it('https://', async () => {
        const uri = 'https://demotiles.maplibre.org/style.json';
        const data = await getSource(uri);
        expect(data).not.toBeNull();
    });

    it('mbtiles://', async () => {
        const uri = 'mbtiles://localdata/school.mbtiles/0/0/0';
        const data = await getSource(uri);
        expect(data).not.toBeNull();
    });

    it('pmtiles://', async () => {
        const uri = 'pmtiles://localdata/school.pmtiles/0/0/0';
        const data = await getSource(uri);
        expect(data).not.toBeNull();
    });

    it('cog://', async () => {
        const uri =
            'cog://https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/54/T/WN/2024/9/S2A_54TWN_20240908_0_L2A/TCI.tif/6/32/24';
        const data = await getSource(uri);
        expect(data).not.toBeNull();
    });

    it('invalid uri', async () => {
        const uri = 'invalid://localdata/style.json';
        const data = await getSource(uri);
        expect(data).toBeNull();
    });

    it('single-flight: concurrent requests for the same uri share one fetch', async () => {
        const uri = 'https://demotiles.maplibre.org/style.json';
        const fetchSpy = vi.spyOn(globalThis, 'fetch');
        const before = fetchSpy.mock.calls.length;
        const [a, b, c] = await Promise.all([
            getSource(uri),
            getSource(uri),
            getSource(uri),
        ]);
        const after = fetchSpy.mock.calls.length;
        expect(a).not.toBeNull();
        expect(b).not.toBeNull();
        expect(c).not.toBeNull();
        expect(after - before).toBe(1);
        fetchSpy.mockRestore();
    });

    /**
    it('s3://', async () => {
        const uri = 's3://chiitiler/tiles/0/0/0.pbf';
        const data = await getSource(uri);
        expect(data).not.toBeNull();
    });
    */
});
