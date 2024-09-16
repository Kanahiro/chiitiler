import * as path from 'path';

import mbgl from '@maplibre/maplibre-gl-native';
import { Pool } from 'lightning-pool';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { LRUCache } from 'lru-cache';

import { getSource } from '../source/index.js';
import type { Cache } from '../cache/index.js';

const EMPTY_BUFFER = Buffer.alloc(0);
const TRANSPARENT_BUFFER: Record<string, Buffer> = {
    // 1x1 transparent images
    png: Buffer.from(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4//8/AwAI/AL+p5qgoAAAAABJRU5ErkJggg==',
        'base64',
    ),
    webp: Buffer.from(
        'UklGRkAAAABXRUJQVlA4WAoAAAAQAAAAAAAAAAAAQUxQSAIAAAAAAFZQOCAYAAAAMAEAnQEqAQABAAFAJiWkAANwAP789AAA',
        'base64',
    ),
    jpeg: Buffer.from(
        '/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAABAAEDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD3+iiigD//2Q==',
    ),
};

function handleFileExt(uri: string) {
    if (uri.startsWith('cog://')) return 'png'; // cog:// always returns as png

    // extract extension only, take into account query string or hash
    const basename = path.basename(uri).split(/[?#]/)[0];
    const ext = basename.split('.').pop();
    if (ext === undefined) return null;
    const l = ext.toLowerCase();
    if (l === 'jpeg') return 'jpg';
    return l;
}

const mapPoolCache = new LRUCache<string, Pool<mbgl.Map>>({
    max: 10,
    dispose: (pool, key) => {
        pool.close();
    },
});
async function getRenderPool(
    style: StyleSpecification,
    cache: Cache,
    mode: 'tile' | 'static',
) {
    const cacheKey = JSON.stringify(style);

    const pool = mapPoolCache.get(cacheKey);
    if (pool !== undefined) return pool;

    const newPool = new Pool({
        create: () => {
            const map = new mbgl.Map({
                request: function (req, callback) {
                    const ext = handleFileExt(req.url);
                    getSource(req.url, cache)
                        .then((buf) => {
                            if (buf) {
                                callback(undefined, { data: buf });
                            } else if (ext && TRANSPARENT_BUFFER[ext])
                                callback(undefined, {
                                    data: TRANSPARENT_BUFFER[ext],
                                });
                            else callback(undefined, { data: EMPTY_BUFFER });
                        })
                        .catch(() => {
                            if (ext && TRANSPARENT_BUFFER[ext])
                                callback(undefined, {
                                    data: TRANSPARENT_BUFFER[ext],
                                });
                            else callback(undefined, { data: EMPTY_BUFFER });
                        });
                },
                ratio: 1,
                // @ts-ignore
                mode,
            });
            map.load(style);
            return map;
        },
        destroy: (map: mbgl.Map) => {
            map.release();
        },
    });
    mapPoolCache.set(cacheKey, newPool);
    return newPool;
}

export { getRenderPool };
