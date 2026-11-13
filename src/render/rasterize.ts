import { CameraUpdateMode } from '@maplibre/native-ffi';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { getRenderPool } from './pool.js';
import { invMercX, invMercY } from './mercator.js';
import type { Cache } from '../cache/index.js';

function getTileCenter(z: number, x: number, y: number): [number, number] {
    // tile (x, y) center as a world fraction is (x + 0.5) / 2^z, independent
    // of tile pixel size; convert that back to lon/lat.
    const n = 2 ** z;
    return [invMercX((x + 0.5) / n), invMercY((y + 0.5) / n)];
}

type RenderOptions = {
    zoom: number;
    width: number;
    height: number;
    center: [number, number];
    bearing?: number;
    pitch?: number;
};

async function render(
    style: StyleSpecification,
    { zoom, width, height, center, bearing = 0, pitch = 0 }: RenderOptions,
    cache: Cache,
    mode: 'tile' | 'static',
): Promise<Uint8Array> {
    const pool = await getRenderPool(style, cache, mode);
    const worker = await pool.acquire();
    try {
        if (worker.width !== width || worker.height !== height) {
            await worker.session.resize({ width, height });
            worker.width = width;
            worker.height = height;
        }
        worker.map.discarding.updateCamera({
            mode: CameraUpdateMode.jump,
            camera: {
                center: { longitude: center[0], latitude: center[1] },
                zoom,
                bearing,
                pitch,
            },
        });
        await worker.map.requestStillImage();
        const { data, info } = await worker.session.readTexture();
        const row = width * 4;
        if (info.stride === row) return data;
        // rows may be padded; pack them for sharp's raw input
        const packed = new Uint8Array(row * height);
        for (let y = 0; y < height; y++)
            packed.set(data.subarray(y * info.stride, y * info.stride + row), y * row);
        return packed;
    } finally {
        pool.release(worker);
    }
}

async function renderTile(
    style: StyleSpecification,
    z: number,
    x: number,
    y: number,
    options: { tileSize: number; cache: Cache; margin?: number },
): Promise<Uint8Array> {
    /**
     * zoom(renderingOptions): tileSize=256 -> z-1, 512 -> z, 1024 -> z+1...
     * width, height(renderingOptions): equal to tileSize but:
     * when zoom=0, entire globe is rendered in 512x512
     * even when tilesize=256, entire globe is rendered in "512x512 at zoom=0"
     * so we have to set 512 when tilesize=256 and zoom=0, and adjust ratio
     */
    const renderingParams =
        options.tileSize === 256 && z === 0
            ? {
                  zoom: 0,
                  height: 512,
                  width: 512,
                  ratio: 0.5,
              }
            : {
                  // offset = 128 -> -1, 256 -> 0, 512 -> 1, 1024 -> 2...
                  zoom: z - 1 + Math.log2(options.tileSize / 256),
                  height: options.tileSize,
                  width: options.tileSize,
                  ratio: 1,
              };

    const tileMode =
        options.margin === 0 &&
        (options.tileSize === 256 || options.tileSize === 512); // mode=tile supports only 256 and 512

    const rendered = await render(
        style,
        {
            zoom: renderingParams.zoom,
            width: renderingParams.width + (options.margin ?? 0),
            height: renderingParams.height + (options.margin ?? 0),
            center: getTileCenter(z, x, y),
            bearing: 0,
            pitch: 0,
        },
        options.cache,
        tileMode ? 'tile' : 'static',
    );
    return rendered;
}

export { renderTile, render };
