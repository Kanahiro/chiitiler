import { Hono } from 'hono';
import { stream } from 'hono/streaming';
import {
	isSupportedFormat,
	isValidCamera,
	isValidDimensions,
	isValidStylejson,
} from './utils.js';
import { getRenderedImage } from '../render/index.js';
import { Cache } from '../cache/index.js';

function createCameraRouter(options: { cache: Cache; stream: boolean }) {
	const camera = new Hono()
		.get('/:camera/:dimensions_ext', async (c) => {
			// path params
			const camera = c.req
				.param('camera')
				.match(/([\d.]+),([\d.]+),([\d.]+)(?:@(\d+)(?:,(\d+))?)?/);
			const [_dimensions, ext] = c.req.param('dimensions_ext').split('.');

			const dimensions = _dimensions.match(/(\d+)x(\d+)?/);

			if (!camera || !isValidCamera(camera))
				return c.body('invalid camera', 400);
			if (!dimensions || !isValidDimensions(dimensions))
				return c.body('invalid dimensions', 400);
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

			const [, _lon, _lat, _zoom, _bearing, _pitch] = camera;
			const [, _width, _height] = dimensions;

			const lat = Number(_lat);
			const lon = Number(_lon);
			const zoom = Number(_zoom);
			const bearing = Number(_bearing ?? 0);
			const pitch = Number(_pitch ?? 0);
			const height = Number(_height);
			const width = Number(_width);

			// query params
			const url = c.req.query('url');
			if (url === undefined) return c.body('url is required', 400);
			const quality = Number(c.req.query('quality') ?? 100);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedImage({
					stylejson: url,
					cache: options.cache,
					ext,
					quality,
					center: [lon, lat],
					zoom,
					bearing,
					pitch,
					height,
					width,
				});

				if (options.stream) {
					// stream mode
					return stream(c, async (stream) => {
						for await (const chunk of sharp) {
							stream.write(chunk);
						}
					});
				} else {
					const buf = await sharp.toBuffer();
					return c.body(buf as Uint8Array<ArrayBuffer>);
				}
			} catch (e) {
				console.error(`render error: ${e}`);
				return c.body('failed to render static image', 400);
			}
		})
		.post('/:camera/:dimensions_ext', async (c) => {
			// body
			const { style } = await c.req.json();
			if (!isValidStylejson(style)) return c.body('invalid stylejson', 400);

			// path params
			const camera = c.req
				.param('camera')
				.match(/([\d.]+),([\d.]+),([\d.]+)(?:@(\d+)(?:,(\d+))?)?/);
			const [_dimensions, ext] = c.req.param('dimensions_ext').split('.');

			const dimensions = _dimensions.match(/(\d+)x(\d+)?/);

			if (!camera || !isValidCamera(camera))
				return c.body('invalid camera', 400);
			if (!dimensions || !isValidDimensions(dimensions))
				return c.body('invalid dimensions', 400);
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

			const [, _lon, _lat, _zoom, _bearing, _pitch] = camera;
			const [, _width, _height] = dimensions;

			const lat = Number(_lat);
			const lon = Number(_lon);
			const zoom = Number(_zoom);
			const bearing = Number(_bearing ?? 0);
			const pitch = Number(_pitch ?? 0);
			const height = Number(_height);
			const width = Number(_width);

			// query params
			const quality = Number(c.req.query('quality') ?? 100);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedImage({
					stylejson: style,
					cache: options.cache,
					ext,
					quality,
					center: [lon, lat],
					zoom,
					bearing,
					pitch,
					height,
					width,
				});

				if (options.stream) {
					// stream mode
					return stream(c, async (stream) => {
						for await (const chunk of sharp) {
							stream.write(chunk);
						}
					});
				} else {
					const buf = await sharp.toBuffer();
					return c.body(buf as Uint8Array<ArrayBuffer>);
				}
			} catch (e) {
				console.error(`render error: ${e}`);
				return c.body('failed to render static image', 400);
			}
		});
	return camera;
}

export { createCameraRouter };
