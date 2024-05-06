import mbgl from '@maplibre/maplibre-gl-native';
import type { StyleSpecification } from 'maplibre-gl';
import genericPool from 'generic-pool';

import { getSource } from '../source.js';
import type { Cache } from '../cache/index.js';

// key:value = styleJsonString:Pooled Map Instance
const mapPoolDict: Record<string, genericPool.Pool<mbgl.Map>> = {};
async function getRenderPool(
    styleUrl: string,
    cache: Cache,
    mode: 'tile' | 'static',
) {
    const dictKey = `${styleUrl}-${mode}`;
    if (mapPoolDict[dictKey] === undefined) {
        const styleJsonBuf = await getSource(styleUrl, cache);
        if (styleJsonBuf === null) throw new Error('style not found');
        const style = JSON.parse(styleJsonBuf.toString()) as StyleSpecification;
        const pool = genericPool.createPool({
            create: async () => {
                const map = new mbgl.Map({
                    request: function (req, callback) {
                        getSource(req.url, cache)
                            .then((buf) => {
                                if (buf === null) {
                                    callback();
                                    return;
                                }
                                callback(undefined, { data: buf });
                            })
                            .catch((err: any) => {
                                callback(err);
                            });
                    },
                    ratio: 1,
                    // @ts-ignore
                    mode,
                });
                map.load(style);
                return map;
            },
            destroy: async (map: mbgl.Map) => {
                map.release();
            },
        });
        mapPoolDict[dictKey] = pool;
    }
    return mapPoolDict[dictKey];
}

export { getRenderPool };
