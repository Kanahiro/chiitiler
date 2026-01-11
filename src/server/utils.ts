import {
	type StyleSpecification,
	validateStyleMin,
} from '@maplibre/maplibre-gl-style-spec';
import type { SupportedFormat } from '../render/index.js';

function isValidStylejson(stylejson: any): stylejson is StyleSpecification {
	return validateStyleMin(stylejson).length === 0;
}

function isValidCamera([, lon, lat, zoom, bearing, pitch]: string[]) {
	if (Number.isNaN(Number(lat)) || Number(lat) < -90 || Number(lat) > 90)
		return false;
	if (Number.isNaN(Number(lon)) || Number(lon) < -180 || Number(lon) > 180)
		return false;
	if (Number.isNaN(Number(zoom)) || Number(zoom) < 0 || Number(zoom) > 24)
		return false;
	if (
		bearing &&
		(Number.isNaN(Number(bearing)) ||
			Number(bearing) < 0 ||
			Number(bearing) > 360)
	)
		return false;
	if (
		pitch &&
		(Number.isNaN(Number(pitch)) || Number(pitch) < 0 || Number(pitch) > 180)
	)
		return false;
	return true;
}

function isValidDimensions([, width, height]: string[]) {
	return !Number.isNaN(Number(width)) && !Number.isNaN(Number(height));
}

function isSupportedFormat(ext: string | undefined): ext is SupportedFormat {
	return Boolean(ext && ['png', 'jpeg', 'jpg', 'webp'].includes(ext));
}

export {
	isValidStylejson,
	isValidCamera,
	isValidDimensions,
	isSupportedFormat,
};
