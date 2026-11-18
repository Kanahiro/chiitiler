// Delete the CRS files of a directory (from generate.py) that ml2drender cannot render.
//     node validate.mjs <dir>
import { readdirSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { Renderer } from '@ml2drender/node';

const dir = process.argv[2];
const style = {
    version: 8,
    sources: {},
    layers: [{ id: 'bg', type: 'background', paint: { 'background-color': '#fff' } }],
};
const renderer = new Renderer(style, { loadResource: async () => null });
const tile = { z: 0, x: 0, y: 0, tileSize: 16, pixelRatio: 1, margin: 0 };
const files = readdirSync(dir).filter((f) => f.endsWith('.json'));
const reasons = new Map();
for (const file of files) {
    const path = join(dir, file);
    try {
        await renderer.projected(JSON.parse(readFileSync(path, 'utf8'))).renderTile(tile);
    } catch (e) {
        rmSync(path);
        reasons.set(e.message, (reasons.get(e.message) ?? 0) + 1);
    }
}
const removed = [...reasons.values()].reduce((a, b) => a + b, 0);
for (const [reason, n] of reasons) console.log(`removed ${n}: ${reason}`);
console.log(`${files.length - removed} of ${files.length} CRSs render`);
