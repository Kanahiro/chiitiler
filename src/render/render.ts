/// <reference lib="dom" />
// for using native fetch in TypeScript

// @ts-ignore
import SphericalMercator from '@mapbox/sphericalmercator';
import { getRenderPool } from './pool.js';
import type { Cache } from '../cache/index.js';

function getTileCenter(z: number, x: number, y: number, tileSize = 256) {
    const mercator = new SphericalMercator({
        size: tileSize,
    });
    const px = tileSize / 2 + x * tileSize;
    const py = tileSize / 2 + y * tileSize;
    const tileCenter = mercator.ll([px, py], z);
    return tileCenter;
}

function render(
    styleUrl: string,
    zoom: number,
    width: number,
    height: number,
    center: [number, number],
    pitch: number,
    bearing: number,
    rotation: number,
    cache: Cache,
    mode: 'tile' | 'static' = 'tile',
) {
    const renderOptions = {
        zoom,
        width,
        height,
        center,
        pitch,
        bearing,
        rotation,
    };

    const rendered: Promise<Uint8Array> = new Promise((resolve, reject) => {
        getRenderPool(styleUrl, cache, mode).then((pool) => {
            pool.acquire().then((map) =>
                map.render(
                    renderOptions,
                    function (err: any, buffer: Uint8Array | undefined) {
                        pool.release(map);
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
                ),
            );
        })
    });

    return rendered;
}

async function renderTile(
    styleUrl: string,
    z: number,
    x: number,
    y: number,
    options: { tileSize: number; cache: Cache },
): Promise<Uint8Array | undefined> {
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
                zoom: z - 1 + Math.floor(options.tileSize / 512),
                height: options.tileSize,
                width: options.tileSize,
                ratio: 1,
            };

    const rendered = await render(
        styleUrl,
        renderingParams.zoom,
        renderingParams.width,
        renderingParams.height,
        getTileCenter(z, x, y, options.tileSize),
        0,
        0,
        0,
        options.cache,
        'tile',
    );
    return rendered;
}

export { renderTile, render };
