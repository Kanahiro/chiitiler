import { Hono } from 'hono';
import { stream } from 'hono/streaming';
import { isSupportedFormat, isValidStylejson } from '../utils.js';
import { getRenderedImage } from '../../render/index.js';
import { Cache } from '../../cache/index.js';

function isValidCamera({
	zoom,
	lat,
	lon,
	bearing,
	pitch,
}: {
	zoom: number;
	lat: number;
	lon: number;
	bearing: number;
	pitch: number;
}): boolean {
	if (Number.isNaN(lat) || lat < -90 || lat > 90) return false;
	if (Number.isNaN(lon) || lon < -180 || lon > 180) return false;
	if (Number.isNaN(zoom) || zoom < 0 || zoom > 24) return false;
	if (Number.isNaN(bearing) || bearing < 0 || bearing > 360) return false;
	if (Number.isNaN(pitch) || pitch < 0 || pitch > 180) return false;
	return true;
}

function isValidDimensions({
	width,
	height,
}: {
	width: number;
	height: number;
}): boolean {
	return !Number.isNaN(width) && !Number.isNaN(height);
}

function createCameraRouter(options: { cache: Cache; stream: boolean }) {
	const camera = new Hono()
		.get('/:zoom/:lat/:lon/:bearing/:pitch/:dimensions_ext', async (c) => {
			// path params
			const { zoom, lat, lon, bearing, pitch } = c.req.param();

			const [_dimensions, ext] = c.req.param('dimensions_ext').split('.');
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);
			const dimensions = _dimensions.split('x').map(Number);
			const [width, height] = dimensions;

			const _lat = Number(lat);
			const _lon = Number(lon);
			const _zoom = Number(zoom);
			const _bearing = Number(bearing);
			const _pitch = Number(pitch);
			const _height = Number(height);
			const _width = Number(width);

			if (!isValidDimensions({ width: _width, height: _height }))
				return c.body('invalid dimensions', 400);

			if (
				!isValidCamera({
					zoom: _zoom,
					lat: _lat,
					lon: _lon,
					bearing: _bearing,
					pitch: _pitch,
				})
			)
				return c.body('invalid camera', 400);

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
					center: [_lon, _lat],
					zoom: _zoom,
					bearing: _bearing,
					pitch: _pitch,
					height: _height,
					width: _width,
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
		.post('/:zoom/:lat/:lon/:bearing/:pitch/:dimensions_ext', async (c) => {
			// body
			const { style } = await c.req.json();
			if (!isValidStylejson(style)) return c.body('invalid stylejson', 400);

			// path params
			const { zoom, lat, lon, bearing, pitch } = c.req.param();

			const [_dimensions, ext] = c.req.param('dimensions_ext').split('.');
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);
			const dimensions = _dimensions.split('x').map(Number);
			const [width, height] = dimensions;

			const _lat = Number(lat);
			const _lon = Number(lon);
			const _zoom = Number(zoom);
			const _bearing = Number(bearing);
			const _pitch = Number(pitch);
			const _height = Number(height);
			const _width = Number(width);

			if (!isValidDimensions({ width: _width, height: _height }))
				return c.body('invalid dimensions', 400);

			if (
				!isValidCamera({
					zoom: _zoom,
					lat: _lat,
					lon: _lon,
					bearing: _bearing,
					pitch: _pitch,
				})
			)
				return c.body('invalid camera', 400);

			// query params
			const quality = Number(c.req.query('quality') ?? 100);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedImage({
					stylejson: style,
					cache: options.cache,
					ext,
					quality,
					center: [_lon, _lat],
					zoom: _zoom,
					bearing: _bearing,
					pitch: _pitch,
					height: _height,
					width: _width,
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
