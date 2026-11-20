/// <reference lib="dom" />
// for using native fetch in TypeScript

import { type Cache, noneCache } from '../cache/index.js';
import { getUserAgent } from './userAgent.js';

async function getHttpSource(
    uri: string,
    cache: Cache = noneCache(),
): Promise<Uint8Array | null> {
    const val = await cache.get(uri);
    if (val !== undefined) return val; // hit

    // miss
    // the query may hold credentials (API keys, signatures): keep it out of errors
    const where = uri.split('?')[0];
    let res: Response;
    try {
        const userAgent = getUserAgent();
        res = await fetch(uri, {
            headers: userAgent ? { 'User-Agent': userAgent } : undefined,
        });
    } catch (e) {
        // ネットワークエラー: タイルの有無が不明なまま透明タイルを返すと
        // CDN等に欠損タイルがキャッシュされうるので、throwしてレンダリングを失敗させる
        throw new Error(`failed to fetch ${where}`, { cause: e });
    }
    if (res.status >= 500) {
        // 5xx: 一時的な障害でタイルの有無は不明。ネットワークエラーと同様に扱う
        throw new Error(`upstream error ${res.status} for ${where}`);
    }
    if (!res.ok) {
        // 4xx: タイルが存在しない(404など)。空タイルとして扱わせる
        return null;
    }
    // 204/空ボディはキャッシュ汚染を避けるため書き込まずnullを返す
    if (res.status === 204) return null;
    const buf = Buffer.from(await res.arrayBuffer());
    if (buf.length === 0) return null;
    cache.set(uri, buf);
    return buf;
}

export { getHttpSource };
