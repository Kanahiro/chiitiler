import sharp from 'sharp';
import type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
// @ts-ignore
import SphericalMercator from '@mapbox/sphericalmercator';
const mercator = new SphericalMercator();
import { LRUCache } from 'lru-cache';

import { renderTile, render } from './render.js';
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
	scale: number;
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
	scale,
}: GetRenderedTileOptions): Promise<sharp.Sharp> {
	const style = await loadStyle(stylejson, cache);

	let pixels: Uint8Array;
	pixels = await renderTile(style, z, x, y, {
		tileSize,
		cache,
		margin,
		scale,
	});

	// hack: tile-margin clip area
	// maplibre-native won't render outer area of meractor
	// so top-end and bottom-end clipped area is special
	const isTopEnd = y === 0;
	const isBottomEnd = y === 2 ** z - 1;
	const topMargin = isTopEnd ? 0 : isBottomEnd ? margin : margin / 2;

	let _sharp: sharp.Sharp;
	if (tileSize === 256 && z === 0 && scale === 1) {
		// hack: when tileSize=256, z=0
		// pixlels will be 512x512 so we need to resize to 256x256
		_sharp = sharp(pixels, {
			raw: {
				width: 512,
				height: 512,
				channels: 4,
			},
		}).resize(256, 256);
	} else if (margin === 0) {
		_sharp = sharp(pixels, {
			raw: {
				width: Math.floor(tileSize / scale) * scale,
				height: Math.floor(tileSize / scale) * scale,
				channels: 4,
			},
		}).resize(tileSize, tileSize);
	} else {
		_sharp = sharp(pixels, {
			raw: {
				width: Math.floor((tileSize + margin) / scale) * scale,
				height: Math.floor((tileSize + margin) / scale) * scale,
				channels: 4,
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
	scale: number,
): {
	zoom: number;
	width: number;
	height: number;
	center: [number, number];
} => {
	// reference: https://github.com/maptiler/tileserver-gl/blob/cc4b8f7954069fd0e1db731ff07f5349f7b9c8cd/src/serve_rendered.js#L346
	// very hacky and it might be wrong
	let zoom = 25;
	const minCorner = mercator.px([bbox[0], bbox[3]], zoom);
	const maxCorner = mercator.px([bbox[2], bbox[1]], zoom);
	const dx = maxCorner[0] - minCorner[0];
	const dy = maxCorner[1] - minCorner[1];

	zoom -= Math.max(Math.log(dx / size), Math.log(dy / size)) / Math.LN2;
	zoom -= Math.log2(scale);
	zoom = Math.max(Math.log(size / 256) / Math.LN2, Math.min(25, zoom)) - 1;

	const width = dx > dy ? size : Math.ceil((dx / dy) * size);
	const height = dx > dy ? Math.ceil((dy / dx) * size) : size;

	const mercCenter: [number, number] = [
		(maxCorner[0] + minCorner[0]) / 2,
		(maxCorner[1] + minCorner[1]) / 2,
	];
	const center = mercator.ll(mercCenter, 25); // latlon

	return { zoom, width, height, center };
};

type GetRenderedBboxOptions = {
	stylejson: string | StyleSpecification;
	bbox: [number, number, number, number];
	size: number;
	cache: Cache;
	ext: SupportedFormat;
	quality: number;
	scale: number;
};

async function getRenderedBbox({
	stylejson,
	bbox,
	size,
	cache,
	ext,
	quality,
	scale,
}: GetRenderedBboxOptions): Promise<sharp.Sharp> {
	const style = await loadStyle(stylejson, cache);

	const { zoom, width, height, center } = calcRenderingParams(
		bbox,
		size,
		scale,
	);

	const pixels = await render(
		style,
		{
			zoom,
			width: width / scale,
			height: height / scale,
			center,
		},
		cache,
		'static',
		scale,
	);

	let _sharp = sharp(pixels, {
		raw: {
			width: Math.floor(width / scale) * scale,
			height: Math.floor(height / scale) * scale,
			channels: 4,
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

export {
	getRenderedTile,
	getRenderedBbox,
	type GetRenderedBboxOptions,
	type GetRenderedTileOptions,
	type SupportedFormat,
};
