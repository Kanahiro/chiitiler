import { describe, test, expect } from 'vitest';
import sizeof from 'image-size';
import { readFile } from 'fs/promises';

describe('render test', () => {
	const FIXTURES = [
		{
			tileSize: 256,
			scale: 1,
			fixture: '5-17-11_256_1x.webp',
		},
		{
			tileSize: 256,
			scale: 2,
			fixture: '5-17-11_256_2x.webp',
		},
		{
			tileSize: 512,
			scale: 3,
			fixture: '5-17-11_512_3x.webp',
		},
		{
			tileSize: 1024,
			scale: 4,
			fixture: '5-17-11_1024_4x.webp',
		},
		{
			tileSize: 2048,
			scale: 5,
			fixture: '5-17-11_2048_5x.webp',
		},
	];
	for (const { tileSize, scale, fixture } of FIXTURES) {
		test(`${tileSize}px-x${scale}`, async () => {
			const res = await fetch(
				`http://localhost:3000/tiles/5/17/11.webp?url=https://demotiles.maplibre.org/style.json&tileSize=${tileSize}&scale=${scale}`,
			);
			let buf = new Uint8Array(await res.arrayBuffer());

			// check tile size
			const { width, height } = sizeof(buf);
			expect(width).toBe(tileSize);
			expect(height).toBe(tileSize);

			// compare to pre rendered image
			const res2 = await readFile(`tests/fixtures/${fixture}`);
			expect(Buffer.from(buf).equals(res2)).toBe(true);
		});
	}
});
