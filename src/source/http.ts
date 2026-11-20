/// <reference lib="dom" />
// for using native fetch in TypeScript

import { type Cache, noneCache } from '../cache/index.js';
import { getUserAgent } from './userAgent.js';

/**
 * How long a fetch may take, body included. Renderers fetch inside their render queue: one upstream
 * that never answers would otherwise stall every render queued behind it (undici waits 300 s).
 */
const FETCH_TIMEOUT_MS = 30_000;

/**
 * The largest response read. A style can point at any URL: an endless or huge body must fail the
 * render, not exhaust memory.
 */
const MAX_BODY_BYTES = 64 * 1024 * 1024;

/** Statuses meaning the resource does not exist. */
const MISSING = new Set([403, 404, 410]);

async function readBody(res: Response): Promise<Buffer> {
    const chunks: Uint8Array[] = [];
    let size = 0;
    // throwing out of the loop cancels the body
    for await (const chunk of res.body ?? []) {
        size += chunk.byteLength;
        if (size > MAX_BODY_BYTES)
            throw new Error(`response larger than ${MAX_BODY_BYTES} bytes`);
        chunks.push(chunk);
    }
    return Buffer.concat(chunks, size);
}

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
            signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
        });
    } catch (e) {
        // ネットワークエラー: タイルの有無が不明なまま透明タイルを返すと
        // CDN等に欠損タイルがキャッシュされうるので、throwしてレンダリングを失敗させる
        throw new Error(`failed to fetch ${where}`, { cause: e });
    }
    // タイルが存在しない: 空タイルとして扱わせる。403はS3等のバケットが
    // 存在しないオブジェクトに返すもの(一覧権限がない場合)
    if (MISSING.has(res.status)) {
        await res.body?.cancel(); // release the connection
        return null;
    }
    if (!res.ok) {
        await res.body?.cancel();
        // それ以外(5xx・429・401・408など)は一時的な障害や設定ミスでタイルの有無が不明。
        // 空タイルを返すとCDN等に欠損タイルがキャッシュされうるので、throwしてレンダリングを失敗させる
        throw new Error(`upstream error ${res.status} for ${where}`);
    }
    // 204/空ボディはキャッシュ汚染を避けるため書き込まずnullを返す
    if (res.status === 204) return null;
    let buf: Buffer;
    try {
        buf = await readBody(res);
    } catch (e) {
        // the body broke off, timed out or was too large
        throw new Error(`failed to fetch ${where}`, { cause: e });
    }
    if (buf.length === 0) return null;
    cache.set(uri, buf);
    return buf;
}

export { FETCH_TIMEOUT_MS, MAX_BODY_BYTES, getHttpSource };
