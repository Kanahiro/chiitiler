import {
	type StyleSpecification,
	validateStyleMin,
} from '@maplibre/maplibre-gl-style-spec';
import type { SupportedFormat } from '../render/index.js';

function isValidStylejson(stylejson: any): stylejson is StyleSpecification {
	return validateStyleMin(stylejson).length === 0;
}

function isSupportedFormat(ext: string | undefined): ext is SupportedFormat {
	return Boolean(ext && ['png', 'jpeg', 'jpg', 'webp'].includes(ext));
}

export { isValidStylejson, isSupportedFormat };
