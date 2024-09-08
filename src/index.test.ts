import { describe, it, expect } from 'vitest';

import {
    getRenderedBboxBuffer,
    getRenderedBboxStream,
    getRenderedTileBuffer,
    getRenderedTileStream,
    ChiitilerCache,
} from './index.js';

describe('library mode', () => {
    // testing actual behavior of rendering methods is difficult...
    it('members are exported', async () => {
        expect(getRenderedBboxBuffer).toBeDefined();
        expect(getRenderedBboxStream).toBeDefined();
        expect(getRenderedTileBuffer).toBeDefined();
        expect(getRenderedTileStream).toBeDefined();

        // actual behaviors are tested in cache module
        expect(ChiitilerCache.fileCache).toBeDefined();
        expect(ChiitilerCache.memoryCache).toBeDefined();
        expect(ChiitilerCache.noneCache).toBeDefined();
        expect(ChiitilerCache.s3Cache).toBeDefined();
    });
});
