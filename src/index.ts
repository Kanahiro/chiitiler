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
