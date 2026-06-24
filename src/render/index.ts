import sharp from 'sharp';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { LRUCache } from 'lru-cache';

import { renderTile, render } from './rasterize.js';
import { mercX, mercY, invMercY } from './mercator.js';
import type { Cache } from '../cache/index.js';
import { getSource } from '../source/index.js';

type GetRenderedTileOptions = {
	stylejson: string | StyleSpecification;
	z: number;
	x: number;
	y: number;
	tileSize: number;
	cache: Cache;
	margin: number;
	ext: SupportedFormat;
	quality: number;
};

type SupportedFormat = 'png' | 'jpeg' | 'jpg' | 'webp';

const styleCache = new LRUCache<string, StyleSpecification>({
	max: 5,
});
async function loadStyle(stylejson: string | StyleSpecification, cache: Cache) {
	let style: StyleSpecification;
	if (typeof stylejson === 'string') {
		// url
		const cachedStyle = styleCache.get(stylejson);
		if (cachedStyle !== undefined) {
			// hit-cache
			style = cachedStyle;
		} else {
			const styleJsonBuf = await getSource(stylejson, cache);
			if (styleJsonBuf === null) {
				throw new Error('style not found');
			}
			style = JSON.parse(styleJsonBuf.toString());
			styleCache.set(stylejson, style);
		}
	} else {
		// as stylejson object
		style = stylejson;
	}
	return style;
}

async function getRenderedTile({
	stylejson,
	z,
	x,
	y,
	tileSize,
	cache,
	margin,
	ext,
	quality,
}: GetRenderedTileOptions): Promise<sharp.Sharp> {
	const style = await loadStyle(stylejson, cache);

	let pixels: Uint8Array;
	pixels = await renderTile(style, z, x, y, {
		tileSize,
		cache,
		margin,
	});

	// hack: tile-margin clip area
	// maplibre-native won't render outer area of meractor
	// so top-end and bottom-end clipped area is special
	const isTopEnd = y === 0;
	const isBottomEnd = y === 2 ** z - 1;
	const topMargin = isTopEnd ? 0 : isBottomEnd ? margin : margin / 2;

	let _sharp: sharp.Sharp;
	if (tileSize === 256 && z === 0) {
		// hack: when tileSize=256, z=0
		// pixlels will be 512x512 so we need to resize to 256x256
		_sharp = sharp(pixels, {
			raw: {
				width: 512,
				height: 512,
				channels: 4,
				// maplibre-native returns premultiplied alpha; tell sharp so it
				// unpremultiplies to straight alpha, otherwise antialiased edges
				// (e.g. white text-halo) come out gray on transparent backgrounds.
				premultiplied: true,
			},
		}).resize(256, 256);
	} else if (margin === 0) {
		_sharp = sharp(pixels, {
			raw: {
				width: tileSize,
				height: tileSize,
				channels: 4,
				premultiplied: true,
			},
		});
	} else {
		_sharp = sharp(pixels, {
			raw: {
				width: tileSize + margin,
				height: tileSize + margin,
				channels: 4,
				premultiplied: true,
			},
		})
			.extract({
				left: margin / 2,
				top: topMargin,
				width: tileSize,
				height: tileSize,
			})
			.resize(tileSize, tileSize);
	}

	switch (ext) {
		case 'png':
			return _sharp.png();
		case 'jpeg':
		case 'jpg':
			return _sharp.jpeg({ quality });
		case 'webp':
			return _sharp.webp({ quality, effort: 0 });
	}
}

const calcRenderingParams = (
	bbox: [number, number, number, number],
	size: number,
): {
	zoom: number;
	width: number;
	height: number;
	center: [number, number];
} => {
	// bbox width/height as fractions of the whole world ([0, 1] mercator)
	const x1 = mercX(bbox[0]);
	const x2 = mercX(bbox[2]);
	const y1 = mercY(bbox[3]); // north -> smaller y
	const y2 = mercY(bbox[1]);
	const dx = x2 - x1;
	const dy = y2 - y1;

	// maplibre-native's world is 512px wide at zoom 0, so at zoom z it is
	// 512 * 2^z px. The zoom that fits the longer side into `size` px solves
	// 512 * 2^z * max(dx, dy) = size.
	const zoom = Math.log2(size / (512 * Math.max(dx, dy)));

	const width = dx >= dy ? size : Math.ceil((dx / dy) * size);
	const height = dx >= dy ? Math.ceil((dy / dx) * size) : size;

	const center: [number, number] = [
		(bbox[0] + bbox[2]) / 2, // longitude is linear in mercator x
		invMercY((y1 + y2) / 2), // latitude: invert the mercator-space midpoint
	];

	return { zoom, width, height, center };
};

type GetRenderedClipOptions = {
	stylejson: string | StyleSpecification;
	bbox: [number, number, number, number];
	size: number;
	cache: Cache;
	ext: SupportedFormat;
	quality: number;
};

async function getRenderedClip({
	stylejson,
	bbox,
	size,
	cache,
	ext,
	quality,
}: GetRenderedClipOptions): Promise<sharp.Sharp> {
	const style = await loadStyle(stylejson, cache);

	const { zoom, width, height, center } = calcRenderingParams(bbox, size);

	const pixels = await render(
		style,
		{
			zoom,
			width,
			height,
			center,
		},
		cache,
		'static',
	);

	const _sharp = sharp(pixels, {
		raw: {
			width,
			height,
			channels: 4,
			// see getRenderedTile: maplibre-native returns premultiplied alpha
			premultiplied: true,
		},
	});

	switch (ext) {
		case 'png':
			return _sharp.png();
		case 'jpeg':
		case 'jpg':
			return _sharp.jpeg({ quality });
		case 'webp':
			return _sharp.webp({ quality, effort: 0 });
	}
}

type GetRenderedCameraOptions = {
	stylejson: string | StyleSpecification;
	cache: Cache;
	ext: SupportedFormat;
	quality: number;
	bearing: number;
	pitch: number;
	zoom: number;
	center: [number, number];
	height: number;
	width: number;
};

async function getRenderedCamera(options: GetRenderedCameraOptions) {
	const style = await loadStyle(options.stylejson, options.cache);

	const pixels = await render(
		style,
		{
			center: options.center,
			height: options.height,
			width: options.width,
			zoom: options.zoom,
			bearing: options.bearing,
			pitch: options.pitch,
		},
		options.cache,
		'static',
	);

	const _sharp = sharp(pixels, {
		raw: {
			width: options.width,
			height: options.height,
			channels: 4,
			// see getRenderedTile: maplibre-native returns premultiplied alpha
			premultiplied: true,
		},
	});

	switch (options.ext) {
		case 'png':
			return _sharp.png();
		case 'jpeg':
		case 'jpg':
			return _sharp.jpeg({ quality: options.quality });
		case 'webp':
			return _sharp.webp({ quality: options.quality, effort: 0 });
	}
}

export {
	getRenderedTile,
	getRenderedClip,
	getRenderedCamera,
	type GetRenderedClipOptions,
	type GetRenderedTileOptions,
	type GetRenderedCameraOptions,
	type SupportedFormat,
};
