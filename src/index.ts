import {
	getRenderedClip,
	getRenderedTile,
	getRenderedCamera,
	GetRenderedClipOptions,
	GetRenderedTileOptions,
	GetRenderedCameraOptions,
} from './render/index.js';

export async function getRenderedClipBuffer(
	options: GetRenderedClipOptions,
): Promise<Buffer> {
	const sharp = await getRenderedClip(options);
	return sharp.toBuffer();
}

export { getRenderedClip as getRenderedClipStream };
export async function getRenderedCameraBuffer(
	options: GetRenderedCameraOptions,
): Promise<Buffer> {
	const sharp = await getRenderedCamera(options);
	return sharp.toBuffer();
}

export { getRenderedCamera as getRenderedCameraStream };
export async function getRenderedTileBuffer(
	options: GetRenderedTileOptions,
): Promise<Buffer> {
	const sharp = await getRenderedTile(options);
	return sharp.toBuffer();
}

export { getRenderedTile as getRenderedTileStream };

export * as ChiitilerCache from './cache/index.js';
export { setUserAgent } from './source/userAgent.js';

export type {
	GetRenderedClipOptions,
	GetRenderedTileOptions,
	GetRenderedCameraOptions,
	SupportedFormat,
} from './render/index.js';
export type { Cache } from './cache/index.js';

// re-exported so consumers can reference exactly the style-spec version
// chiitiler is built against, without depending on their own resolution
export type { StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
