import { Hono } from 'hono/quick';
import { stream } from 'hono/streaming';
import { serve } from '@hono/node-server';
import {
	type StyleSpecification,
	validateStyleMin,
} from '@maplibre/maplibre-gl-style-spec';

import { type Cache } from '../cache/index.js';
import { getDebugPage, getEditorgPage } from './debug.js';
import {
    getRenderedTile,
    getRenderedBbox,
    getRenderedImage,
    type SupportedFormat,
} from '../render/index.js';

function isValidStylejson(stylejson: any): stylejson is StyleSpecification {
	return validateStyleMin(stylejson).length === 0;
}

function isValidCamera([, lon, lat, zoom, bearing, pitch]: string[]) {
    if (Number(lat) < -90 || Number(lat) > 90) return false;
    if (Number(lon) < -180 || Number(lon) > 180) return false;
    if (Number(zoom) < 0 || Number(zoom) > 24) return false;
    if (bearing && Number(bearing) < 0 || Number(bearing) > 360) return false;
    if (pitch && Number(pitch) < 0 || Number(pitch) > 180) return false;
    return true;
}

function isValidXyz(x: number, y: number, z: number) {
	if (x < 0 || y < 0 || z < 0) return false;
	if (x >= 2 ** z || y >= 2 ** z) return false;
	return true;
}

function isSupportedFormat(ext: string | undefined): ext is SupportedFormat {
	return Boolean(ext && ['png', 'jpeg', 'jpg', 'webp'].includes(ext));
}

type InitServerOptions = {
	cache: Cache;
	port: number;
	debug: boolean;
	stream: boolean;
};

type InitializedServer = {
	app: Hono;
	tiles: Hono;
	clip: Hono;
	start: () => void;
};

function initServer(options: InitServerOptions): InitializedServer {
	const tiles = new Hono()
		.get('/:z/:x/:y_ext', async (c) => {
			const url = c.req.query('url');
			if (url === undefined) return c.body('url is required', 400);

			// path params
			const z = Number(c.req.param('z'));
			const x = Number(c.req.param('x'));
			let [_y, ext] = c.req.param('y_ext').split('.');
			const y = Number(_y);

			if (!isValidXyz(x, y, z)) return c.body('invalid xyz', 400);
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

			// query params
			const tileSize = Number(c.req.query('tileSize') ?? 512);
			const quality = Number(c.req.query('quality') ?? 100);
			const margin = Number(c.req.query('margin') ?? 0);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedTile({
					stylejson: url,
					z,
					x,
					y,
					tileSize,
					cache: options.cache,
					margin,
					ext,
					quality,
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
					return c.body(buf);
				}
			} catch (e) {
				console.error(`render error: ${e}`);
				return c.body('failed to render tile', 400);
			}
		})
		.post('/:z/:x/:y_ext', async (c) => {
			// body
			const { style } = await c.req.json();
			if (!isValidStylejson(style)) return c.body('invalid stylejson', 400);

			// path params
			const z = Number(c.req.param('z'));
			const x = Number(c.req.param('x'));
			let [_y, ext] = c.req.param('y_ext').split('.');
			const y = Number(_y);

			if (!isValidXyz(x, y, z)) return c.body('invalid xyz', 400);
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

			// query params
			const tileSize = Number(c.req.query('tileSize') ?? 512);
			const quality = Number(c.req.query('quality') ?? 100);
			const margin = Number(c.req.query('margin') ?? 0);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedTile({
					stylejson: style,
					z,
					x,
					y,
					tileSize,
					cache: options.cache,
					margin,
					ext,
					quality,
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
					return c.body(buf);
				}
			} catch (e) {
				console.error(`render error: ${e}`);
				return c.body('failed to render tile', 400);
			}
		});

	const clip = new Hono()
		.get('/:filename_ext', async (c) => {
			// path params
			const [filename, ext] = c.req.param('filename_ext').split('.');
			if (filename !== 'clip') return c.body('not found', 404);
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

			// query params
			const bbox = c.req.query('bbox'); // ?bbox=minx,miny,maxx,maxy
			if (bbox === undefined) return c.body('bbox is required', 400);
			const [minx, miny, maxx, maxy] = bbox.split(',').map(Number);
			if (minx >= maxx || miny >= maxy) return c.body('invalid bbox', 400);

			const url = c.req.query('url');
			if (url === undefined) return c.body('url is required', 400);
			const quality = Number(c.req.query('quality') ?? 100);
			const size = Number(c.req.query('size') ?? 1024);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedBbox({
					stylejson: url,
					bbox: [minx, miny, maxx, maxy],
					size,
					cache: options.cache,
					ext,
					quality,
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
					return c.body(buf);
				}
			} catch (e) {
				console.error(`render error: ${e}`);
				return c.body('failed to render tile', 400);
			}
		})
		.post('/:filename_ext', async (c) => {
			// body
			const { style } = await c.req.json();
			if (!isValidStylejson(style)) return c.body('invalid stylejson', 400);

			// path params
			const [filename, ext] = c.req.param('filename_ext').split('.');
			if (filename !== 'clip') return c.body('not found', 404);
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

			// query params
			const bbox = c.req.query('bbox'); // ?bbox=minx,miny,maxx,maxy
			if (bbox === undefined) return c.body('bbox is required', 400);
			const [minx, miny, maxx, maxy] = bbox.split(',').map(Number);
			if (minx >= maxx || miny >= maxy) return c.body('invalid bbox', 400);

			const quality = Number(c.req.query('quality') ?? 100);
			const size = Number(c.req.query('size') ?? 1024);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedBbox({
					stylejson: style,
					bbox: [minx, miny, maxx, maxy],
					size,
					cache: options.cache,
					ext,
					quality,
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
					return c.body(buf);
				}
			} catch (e) {
				console.error(`render error: ${e}`);
				return c.body('failed to render tile', 400);
			}
		});

    const staticImage = new Hono()
        .get('/:camera/:dimensions_ext', async (c) => {
            // path params
            const camera = c.req.param('camera').match(/([\d.]+),([\d.]+),(\d+)(?:@(\d+)(?:,(\d+))?)?/);
            const [_dimensions, ext] = c.req.param('dimensions_ext').split('.');

            if (!camera || !isValidCamera(camera)) return c.body('invalid camera', 400);
            if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

            const [, _lon, _lat, _zoom, _bearing, _pitch] = camera;
            const [_width, _height] = _dimensions.split('x');

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
                    lat,
                    lon,
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
                    return c.body(buf);
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
            const camera = c.req.param('camera').match(/([\d.]+),([\d.]+),(\d+)(?:@(\d+)(?:,(\d+))?)?/);
            const [_dimensions, ext] = c.req.param('dimensions_ext').split('.');

            if (!camera || !isValidCamera(camera)) return c.body('invalid camera', 400);
            if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

            const [, _lon, _lat, _zoom, _bearing, _pitch] = camera;
            const [_width, _height] = _dimensions.split('x');

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
                    lat,
                    lon,
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
                    return c.body(buf);
                }
            } catch (e) {
                console.error(`render error: ${e}`);
                return c.body('failed to render tile', 400);
            }
        });

	const hono = new Hono();
	if (options.debug) {
		hono.get('/debug', getDebugPage);
		hono.get('/editor', getEditorgPage);
	}
	hono.get('/health', (c) => c.text('OK'));
    hono.route('/static', staticImage);
	hono.route('/tiles', tiles);
	hono.route('/', clip);

	return {
		app: hono,
		tiles,
		clip,
		start: () => {
			const server = serve({ port: options.port, fetch: hono.fetch });
			process.on('SIGINT', () => {
				console.log('shutting down server...');
				server.close();
				process.exit(0);
			});
			process.on('SIGTERM', () => {
				console.log('shutting down server...');
				server.close();
				process.exit(0);
			});
		},
	};
}

export { initServer, type InitServerOptions };
