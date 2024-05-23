import * as path from 'path';

import mbgl from '@maplibre/maplibre-gl-native';
import genericPool from 'generic-pool';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { hasher } from 'node-object-hash';
const _hasher = hasher();

import { getSource } from '../source/index.js';
import type { Cache } from '../cache/index.js';

const EMPTY_BUFFER = Buffer.alloc(0);
const TRANSPARENT_BUFFER: Record<string, Buffer> = {
    png: Buffer.from(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
        'base64',
    ),
    webp: Buffer.from(
        'UklGRiYAAABXRUJQVlA4TA0AAAAvAAAAEAcQERGIiP4HAA==',
        'base64',
    ),
    jpeg: Buffer.from(
        '/9j/4AAQSkZJRgABAQEAYABgAAD/4QA6RXhpZgAATU0AKgAAAAgAA1IBAAABTAAKAAAABwAAAABkAQMAAAABAAAA+gEBAAMAAAABAAEAAKACAAQAAAABAAABPKADAAQAAAABAAAA+AAAAABIAAAAAQAAAEgAAAABAAOgAQADAAAAAQABAACgAgAEAAAAAQAAAGmHBAABAAAAAQAAAGgAAAAAAElFQyBvU3BhLjEgNjIgTWFjaW50b3NoKFBsdWdpbnMgTWFj...',
    ),
};

function handleFileType(uri: string) {
    // extract extension only, take into account query string or hash
    const basename = path.basename(uri).split(/[?#]/)[0];
    const ext = basename.split('.').pop();
    if (ext === undefined) return null;
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
    const dictKey = _hasher.hash(style);
    if (mapPoolDict[dictKey] === undefined) {
        const pool = genericPool.createPool({
            create: async () => {
                const map = new mbgl.Map({
                    request: function (req, callback) {
                        getSource(req.url, cache)
                            .then((buf) => {
                                const ext = handleFileType(req.url);
                                if (buf) {
                                    callback(undefined, { data: buf });
                                } else if (ext && TRANSPARENT_BUFFER[ext])
                                    callback(undefined, {
                                        data: TRANSPARENT_BUFFER[ext],
                                    });
                                else
                                    callback(undefined, { data: EMPTY_BUFFER });
                            })
                            .catch(() => {
                                const ext = handleFileType(req.url);
                                if (ext && TRANSPARENT_BUFFER[ext])
                                    callback(undefined, {
                                        data: TRANSPARENT_BUFFER[ext],
                                    });
                                else
                                    callback(undefined, { data: EMPTY_BUFFER });
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
