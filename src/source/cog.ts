/// <reference lib="dom" />
// for using native fetch in TypeScript

import { renderTile } from 'higuruma';

async function getCogSource(uri: string): Promise<Buffer | null> {
    const cogPath = uri.replace('cog://', '').replace(/\/\d+\/\d+\/\d+$/, '');
    const [z, x, y] = uri.replace(`cog://${cogPath}/`, '').split('/');

    try {
        const tile = await renderTile(cogPath, Number(z), Number(x), Number(y));
        const buf = Buffer.from(tile);
        if (buf.byteLength === 129) return null; // empty tile
        return buf;
    } catch (e) {
        console.error(`[ERROR] ${e}`);
        return null;
    }
}

export { getCogSource };
