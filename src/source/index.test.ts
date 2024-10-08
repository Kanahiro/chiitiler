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

    /**
    it('s3://', async () => {
        const uri = 's3://chiitiler/tiles/0/0/0.pbf';
        const data = await getSource(uri);
        expect(data).not.toBeNull();
    });
    */
});
