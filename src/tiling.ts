/// <reference lib="dom" />
// for using native fetch in TypeScript

import mbgl from '@maplibre/maplibre-gl-native';
// @ts-ignore
import SphericalMercator from '@mapbox/sphericalmercator';
import type { StyleSpecification } from 'maplibre-gl';
import genericPool from 'generic-pool';

import type { Cache } from './cache/index.js';
import { getSource } from './source.js';

function getTileCenter(z: number, x: number, y: number, tileSize = 256) {
    const mercator = new SphericalMercator({
        size: tileSize,
    });
    const px = tileSize / 2 + x * tileSize;
    const py = tileSize / 2 + y * tileSize;
    const tileCenter = mercator.ll([px, py], z);
    return tileCenter;
}

type GetRendererOptions = {
    tileSize: number;
    cache: Cache;
};

// key:value = styleJsonString:Pooled Map Instance
const mapPoolDict: Record<string, genericPool.Pool<mbgl.Map>> = {};

function getRenderer(
    style: StyleSpecification,
    options: GetRendererOptions,
): {
    render: (
        z: number,
        x: number,
        y: number,
    ) => Promise<Uint8Array | undefined>;
} {
    const render = function (
        z: number,
        x: number,
        y: number,
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

        const styleJson = JSON.stringify(style);
        if (mapPoolDict[styleJson] === undefined) {
            // MapLibre Native Instance can render only ONE Image simultaneously
            // Then we need to create a pool of Instances
            const pool = genericPool.createPool({
                create: async () => {
                    const map = new mbgl.Map({
                        request: function (req, callback) {
                            options.cache.get(req.url).then((val) => {
                                if (val !== undefined) {
                                    // hit
                                    callback(undefined, {
                                        data: val as Buffer,
                                    });
                                    return;
                                }
                                // miss
                                getSource(req.url)
                                    .then((buf) => {
                                        if (buf === null) {
                                            callback();
                                            return;
                                        }
                                        callback(undefined, { data: buf });
                                        options.cache.set(req.url, buf);
                                    })
                                    .catch((err: any) => {
                                        callback(err);
                                    });
                            });
                        },
                        ratio: renderingParams.ratio,
                        // @ts-ignore
                        mode: 'tile',
                    });
                    map.load(style);
                    return map;
                },
                destroy: async (map: mbgl.Map) => {
                    map.release();
                },
            });
            mapPoolDict[styleJson] = pool;
        }

        const renderOptions = {
            zoom: renderingParams.zoom,
            width: renderingParams.width,
            height: renderingParams.height,
            center: getTileCenter(z, x, y, options.tileSize),
        };

        const render: Promise<Uint8Array> = new Promise((resolve, reject) => {
            mapPoolDict[styleJson].acquire().then((map) =>
                map.render(
                    renderOptions,
                    function (err: any, buffer: Uint8Array | undefined) {
                        if (err) {
                            reject(err);
                            return;
                        }
                        if (buffer === undefined) {
                            reject('buffer is undefined');
                            return;
                        }
                        resolve(buffer);
                        mapPoolDict[styleJson].release(map);
                    },
                ),
            );
        });

        return render;
    };

    return {
        render,
    };
}

export { getRenderer };
