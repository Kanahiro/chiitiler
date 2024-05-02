import mbgl from '@maplibre/maplibre-gl-native';
import type { StyleSpecification } from 'maplibre-gl';
import genericPool from 'generic-pool';
import hash from 'object-hash';

import { getSource } from '../source.js';
import type { Cache } from '../cache/index.js';

// key:value = styleJsonString:Pooled Map Instance
const mapPoolDict: Record<string, genericPool.Pool<mbgl.Map>> = {};
function getRenderPool(
    style: StyleSpecification,
    cache: Cache,
    mode: 'tile' | 'static',
) {
    const _hash = hash(style);
    const dictKey = `${_hash}-${mode}`;
    if (mapPoolDict[dictKey] === undefined) {
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
