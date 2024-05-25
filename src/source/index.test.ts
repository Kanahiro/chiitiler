import { describe, it, expect, vi } from 'vitest';

import { getSource } from './index.js';

describe('getSource', () => {
    it('file://', async () => {
        const uri = 'file://localdata/style.json';
        const data = await getSource(uri);
        expect(data).not.toBeNull();
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

    it('invalid uri', async () => {
        const uri = 'invalid://localdata/style.json';
        const data = await getSource(uri);
        expect(data).toBeNull();
    });

    // s3://
});
