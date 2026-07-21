/// <reference lib="dom" />
// for using native fetch in TypeScript

import { type Cache, noneCache } from '../cache/index.js';
import { getUserAgent } from './userAgent.js';

async function getHttpSource(
    uri: string,
    cache: Cache = noneCache(),
): Promise<Buffer | null> {
    // use cache only for http(s) sources
    const val = await cache.get(uri);
    if (val !== undefined) return val; // hit

    // miss
    try {
        const res = await fetch(uri, {
            headers: { 'User-Agent': getUserAgent() },
        });
        if (!res.ok) {
            console.log(`failed to fetch ${uri}`);
            return null;
        }
        // 204/空ボディはキャッシュ汚染を避けるため書き込まずnullを返す
        if (res.status === 204) return null;
        const buf = Buffer.from(await res.arrayBuffer());
        if (buf.length === 0) return null;
        cache.set(uri, buf);
        return buf;
    } catch (e) {
        console.error(`[ERROR] ${e}`);
        return null;
    }
}

export { getHttpSource };
