// @ts-ignore
import SphericalMercator from '@mapbox/sphericalmercator';
import type { RenderOptions } from '@maplibre/maplibre-gl-native';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { getRenderPool } from './pool.js';
import type { Cache } from '../cache/index.js';

function getTileCenter(z: number, x: number, y: number, tileSize = 256) {
	const mercator = new SphericalMercator({
		size: tileSize,
	});
	const px = tileSize / 2 + x * tileSize;
	const py = tileSize / 2 + y * tileSize;
	const tileCenter = mercator.ll([px, py], z);
	return tileCenter;
}

async function render(
	style: StyleSpecification,
	renderOptions: RenderOptions,
	cache: Cache,
	mode: 'tile' | 'static',
	scale: number,
) {
	const pool = await getRenderPool(style, cache, mode, scale);
	const worker = await pool.acquire();

	const rendered: Promise<Uint8Array> = new Promise((resolve, reject) => {
		worker.render(
			renderOptions,
			function (err: any, buffer: Uint8Array | undefined) {
				pool.release(worker);
				if (err) {
					reject(err);
					return;
				}
				if (buffer === undefined) {
					reject('buffer is undefined');
					return;
				}
				resolve(buffer);
			},
		);
	});

	return rendered;
}

async function renderTile(
	style: StyleSpecification,
	z: number,
	x: number,
	y: number,
	options: { tileSize: number; cache: Cache; margin: number; scale: number },
): Promise<Uint8Array> {
	/**
	 * zoom(renderingOptions): tileSize=256 -> z-1, 512 -> z, 1024 -> z+1...
	 * width, height(renderingOptions): equal to tileSize but:
	 * when zoom=0, entire globe is rendered in 512x512
	 * even when tilesize=256, entire globe is rendered in "512x512 at zoom=0"
	 * so we have to set 512 when tilesize=256 and zoom=0, and adjust ratio
	 */
	const renderingParams =
		options.tileSize === 256 && z === 0 && options.scale === 1
			? {
					zoom: 0,
					height: 512,
					width: 512,
			  }
			: {
					zoom:
						z -
						1 +
						Math.log2(options.tileSize / 256) - // tileSize offset = 128 -> -1, 256 -> 0, 512 -> 1, 1024 -> 2...
						Math.log2(options.scale), // scale offset = x1=0, x2=1, x4=2, x8=3...
					height: options.tileSize,
					width: options.tileSize,
			  };

	const tileMode =
		options.margin === 0 &&
		(options.tileSize === 256 || options.tileSize === 512); // mode=tile supports only 256 and 512

	const rendered = await render(
		style,
		{
			zoom: renderingParams.zoom,
			width: Math.floor(
				(renderingParams.width + (options.margin ?? 0)) / options.scale,
			),
			height: Math.floor(
				(renderingParams.height + (options.margin ?? 0)) / options.scale,
			),
			center: getTileCenter(z, x, y, options.tileSize),
			bearing: 0,
			pitch: 0,
		},
		options.cache,
		tileMode ? 'tile' : 'static',
		options.scale,
	);
	return rendered;
}

export { renderTile, render };
