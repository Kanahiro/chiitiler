import { type Cache, noneCache } from '../cache/index.js';

async function getHttpSource(
    uri: string,
    cache: Cache = noneCache(),
): Promise<Buffer | null> {
    // use cache only for http(s) sources
    const val = await cache.get(uri);
    if (val !== undefined) return val; // hit

    // miss
    try {
        const res = await fetch(uri);
        if (!res.ok) {
            console.log(`failed to fetch ${uri}`);
            return null;
        }
        const buf = Buffer.from(await res.arrayBuffer());
        cache.set(uri, buf);
        return buf;
    } catch (e) {
        console.log(e);
        return null;
    }
}

export { getHttpSource };
