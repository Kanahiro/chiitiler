import * as path from 'path';

import mbgl from '@maplibre/maplibre-gl-native';
import genericPool from 'generic-pool';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { getSource } from '../source/index.js';
import type { Cache } from '../cache/index.js';
import { getTransparentBuffer } from './sharp.js';

const EMPTY_BUFFER = Buffer.alloc(0);

function handleFileExt(uri: string) {
    // extract extension only, take into account query string or hash
    const basename = path.basename(uri).split(/[?#]/)[0];
    const ext = basename.split('.')[1];
    const l = ext.toLowerCase();
    if (l === 'jpeg') return 'jpg';
    return l;
}

// key:value = styleJsonString:Pooled Map Instance
const mapPoolDict: Record<string, genericPool.Pool<mbgl.Map>> = {};
async function getRenderPool(
    style: StyleSpecification,
    cache: Cache,
    mode: 'tile' | 'static',
) {
    const dictKey = JSON.stringify(style);
    if (mapPoolDict[dictKey] === undefined) {
        const pool = genericPool.createPool({
            create: async () => {
                const map = new mbgl.Map({
                    request: async function (req, callback) {
                        const buf = await getSource(req.url, cache);
                        if (buf) {
                            callback(undefined, { data: buf });
                        } else {
                            const trnBuf = await getTransparentBuffer(
                                handleFileExt(req.url),
                            );
                            if (trnBuf) callback(undefined, { data: trnBuf });
                            else callback(undefined, { data: EMPTY_BUFFER });
                        }
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
