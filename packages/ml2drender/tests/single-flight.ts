// Overlapping renders on one MapRenderer fetch each URL once.
// Run: node --experimental-transform-types tests/single-flight.ts (after pnpm build:wasm)
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { MapRenderer } from '../src/index.ts';

const wasm = await WebAssembly.compile(readFileSync(new URL('../wasm/ml2drender_wasm.wasm', import.meta.url)));
const style = {
  version: 8 as const,
  sources: { v: { type: 'vector' as const, tiles: ['t/{z}/{x}/{y}'] } },
  layers: [{ id: 'f', type: 'fill' as const, source: 'v', 'source-layer': 'l' }],
};
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const calls = new Map<string, number>();
const renderer = await MapRenderer.init(style, {
  wasm,
  cache: { ttlMs: Infinity },
  loadResource: async (url) => {
    calls.set(url, (calls.get(url) ?? 0) + 1);
    // Per-URL latency, so one render still waits for some tiles while others are done.
    const [, , x, y] = url.split('/').map(Number);
    await sleep(5 + ((x * 7 + y * 13) % 20));
    return null;
  },
});

// Views around the z3 tile corners (1..3, 4): adjacent views share 2 of their 4 tiles, 8 tiles total.
await Promise.all([1, 2, 3, 2, 1, 3].map(async (x, i) => {
  await sleep(i * 4);
  await renderer.renderView({ center: [x * 45 - 180, 0], zoom: 3, width: 200, height: 200 });
}));

assert.ok(calls.size <= 8, `${calls.size} tiles exceed the WASM tile cache`);
assert.deepEqual([...calls].filter(([, n]) => n > 1), []);
console.log(`ok: ${calls.size} URLs fetched once each`);

// Many overlapping renders need far more tiles than the WASM tile cache keeps (8 per source): no
// render drops tiles another one has been given and not drawn yet, so each URL is fetched once.
const many = new Map<string, number>();
const busy = await MapRenderer.init(style, {
  wasm,
  cache: { ttlMs: Infinity },
  loadResource: async (url) => {
    many.set(url, (many.get(url) ?? 0) + 1);
    const [, , x, y] = url.split('/').map(Number);
    await sleep(5 + ((x * 7 + y * 13) % 40));
    return null;
  },
});
await Promise.all(Array.from({ length: 64 }, (_, i) => busy.renderTile({ z: 6, x: i % 8, y: Math.floor(i / 8), margin: 64 })));
assert.ok(many.size > 8, `${many.size} tiles fit the WASM tile cache`);
assert.deepEqual([...many].filter(([, n]) => n > 1), []);
console.log(`ok: ${many.size} URLs fetched once each by 64 overlapping renders`);
