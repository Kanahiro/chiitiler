import mbgl from '@maplibre/maplibre-gl-native';
import { Pool } from 'lightning-pool';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { LRUCache } from 'lru-cache';

import { getSource } from '../source/index.js';
import type { Cache } from '../cache/index.js';

// mbglはdataプロパティが無いレスポンスを「データなしタイル」として正常に扱う。
// 型定義はdataを必須としているが、空タイルを表現するには渡さないのが正解。
const EMPTY_RESPONSE = {} as { data: Buffer };

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
                    getSource(req.url, cache)
                        .then((buf) => {
                            if (buf !== null && buf.length > 0) {
                                callback(undefined, { data: buf });
                            } else {
                                // 204/空ボディ/404: 空タイルとして扱わせる
                                callback(undefined, EMPTY_RESPONSE);
                            }
                        })
                        .catch((err) => {
                            // 5xx/ネットワークエラー: 空タイルとして描画すると
                            // 欠損タイルがCDN等にキャッシュされうるので、
                            // エラーを渡してレンダリング自体を失敗させる
                            callback(
                                err instanceof Error
                                    ? err
                                    : new Error(String(err)),
                            );
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
