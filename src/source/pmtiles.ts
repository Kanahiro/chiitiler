import * as fs from 'fs';

import { PMTiles, Source, RangeResponse } from 'pmtiles';

import { type Cache, noneCache } from '../cache/index.js';

const pmtilesCache: { [key: string]: PMTiles } = {};

class PmtilesNodejsFileSource implements Source {
    filepath: string;
    fileHandle: Promise<fs.promises.FileHandle>;

    constructor(filepath: string) {
        this.filepath = filepath;
        this.fileHandle = fs.promises.open(filepath, 'r');
    }

    getKey() {
        return this.filepath;
    }

    async getBytes(offset: number, length: number): Promise<RangeResponse> {
        const buf = Buffer.alloc(length);
        await (await this.fileHandle).read(buf, 0, length, offset);
        return { data: buf.buffer };
    }
}

/**
 * uri = pmtiles://path/to/file.pmtiles/{z}/{x}/{y}
 * uri = pmtiles://http://url/to/file.pmtiles/{z}/{x}/{y}
 * uri = pmtiles://s3://bucket/key/to/file.pmtiles/{z}/{x}/{y}
 */
async function getPmtilesSoruce(
    uri: string,
    cache: Cache = noneCache(),
): Promise<Buffer | null> {
    // uri = pmtiles://path/to/file.pmtiles/{z}/{x}/{y}
    const pmtilesFilepath = uri
        .replace('pmtiles://', '')
        .replace(/\/\d+\/\d+\/\d+$/, '');

    const isRemoteData =
        pmtilesFilepath.startsWith('http://') ||
        pmtilesFilepath.startsWith('https://');
    if (isRemoteData) {
        // use cache only for http(s) sources
        const val = await cache.get(uri);
        if (val !== undefined) return val; // hit
    }

    if (!pmtilesCache[pmtilesFilepath]) {
        if (isRemoteData) {
            // remote file
            pmtilesCache[pmtilesFilepath] = new PMTiles(pmtilesFilepath);
        } else {
            // local file
            const fileSource = new PmtilesNodejsFileSource(pmtilesFilepath);
            pmtilesCache[pmtilesFilepath] = new PMTiles(fileSource);
        }
    }

    const [z, x, y] = uri
        .replace(`pmtiles://${pmtilesFilepath}/`, '')
        .split('/');
    const tile = await pmtilesCache[pmtilesFilepath].getZxy(
        Number(z),
        Number(x),
        Number(y),
    );
    if (!tile) return null;

    const buf = Buffer.from(tile.data);
    if (isRemoteData) cache.set(uri, buf);
    return buf;
}

export { getPmtilesSoruce };
