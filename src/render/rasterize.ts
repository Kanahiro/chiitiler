import type { RenderOptions } from '@maplibre/maplibre-gl-native';
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

async function render(
    style: StyleSpecification,
    renderOptions: RenderOptions,
    cache: Cache,
    mode: 'tile' | 'static',
) {
    const pool = await getRenderPool(style, cache, mode);
    const worker = await pool.acquire();

    const rendered: Promise<Uint8Array> = new Promise((resolve, reject) => {
        worker.render(
            renderOptions,
            function (err: any, buffer: Uint8Array | undefined) {
                pool.release(worker);
                if (err) {
                    reject(err);
                    return;
                }
                if (buffer === undefined) {
                    reject('buffer is undefined');
                    return;
                }
                resolve(buffer);
            },
        );
    });

    return rendered;
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
