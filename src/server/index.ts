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
	type SupportedFormat,
} from '../render/index.js';
import { describeRoute, openAPISpecs } from 'hono-openapi';
import { resolver, validator } from 'hono-openapi/zod';
import { z } from 'zod';

import packagejson from '../../package.json';
import { swaggerUI } from '@hono/swagger-ui';

function isValidStylejson(stylejson: any): stylejson is StyleSpecification {
	return validateStyleMin(stylejson).length === 0;
}

function isValidXyz(x: number, y: number, z: number) {
	if (x < 0 || y < 0 || z < 0) return false;
	if (x >= 2 ** z || y >= 2 ** z) return false;
	return true;
}

function isSupportedFormat(ext: string): ext is SupportedFormat {
	return ['png', 'jpeg', 'webp'].includes(ext);
}

type InitServerOptions = {
	cache: Cache;
	port: number;
	debug: boolean;
	stream: boolean;
};

type InitializedServer = {
	app: Hono;
	start: () => void;
};

function initServer(options: InitServerOptions): InitializedServer {
	const hono = new Hono();
	hono.get('/health', (c) => c.text('OK'));

	if (options.debug) {
		hono.get('/debug', getDebugPage);
		hono.get('/editor', getEditorgPage);

		// openapi
		hono.get(
			'/openapi.json',
			openAPISpecs(hono, {
				documentation: {
					info: {
						title: 'Chiitiler API',
						version: packagejson.version,
						description: 'Chiitiler API',
					},
					servers: [
						{ url: 'http://localhost:3000', description: 'Local Server' },
					],
				},
			}),
		);
		hono.get('/openapi', swaggerUI({ url: '/openapi.json' }));
	}

	hono.get(
		'/tiles/:z/:x/:y_ext',
		describeRoute({
			description: 'Get a tile image',
			tags: ['tiles'],
			responses: {
				200: {
					description: 'tile image',
					content: {
						'image/png': { schema: resolver(z.instanceof(Buffer)) },
						'image/jpeg': { schema: resolver(z.instanceof(Buffer)) },
						'image/webp': { schema: resolver(z.instanceof(Buffer)) },
					},
				},
			},
		}),
		validator(
			'param',
			z.object({
				z: z.string(),
				x: z.string(),
				y_ext: z.string(),
			}),
		),
		validator(
			'query',
			z.object({
				url: z.string(),
				tileSize: z.string().optional(),
				quality: z.string().optional(),
				margin: z.string().optional(),
			}),
		),
		async (c) => {
			let { url, tileSize, quality, margin } = c.req.valid('query');
			const tileSizeNum = Number(tileSize ?? 512);
			const qualityNum = Number(quality ?? 100);
			const marginNum = Number(margin ?? 0);

			const { z, x, y_ext } = c.req.valid('param');
			const [y, ext] = y_ext.split('.');
			const zNum = Number(z);
			const xNum = Number(x);
			const yNum = Number(y);

			if (!isValidXyz(xNum, yNum, zNum)) return c.body('invalid xyz', 400);

			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedTile({
					stylejson: url,
					z: zNum,
					x: xNum,
					y: yNum,
					tileSize: tileSizeNum,
					cache: options.cache,
					margin: marginNum,
					ext,
					quality: qualityNum,
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
		},
	);

	hono.post(
		'/tiles/:z/:x/:y_ext',
		describeRoute({
			description: 'Get a tile image',
			tags: ['tiles'],
			responses: {
				200: {
					description: 'tile image',
					content: {
						'image/png': { schema: resolver(z.instanceof(Buffer)) },
						'image/jpeg': { schema: resolver(z.instanceof(Buffer)) },
						'image/webp': { schema: resolver(z.instanceof(Buffer)) },
					},
				},
			},
		}),
		validator(
			'param',
			z.object({
				z: z.string(),
				x: z.string(),
				y_ext: z.string(),
			}),
		),
		validator(
			'query',
			z.object({
				tileSize: z.string().optional(),
				quality: z.string().optional(),
				margin: z.string().optional(),
			}),
		),
		validator('json', z.object({ style: z.any() })),
		async (c) => {
			// body
			const { style } = c.req.valid('json');
			if (!isValidStylejson(style)) return c.body('invalid stylejson', 400);

			// path params
			const { z, x, y_ext } = c.req.valid('param');
			const [y, ext] = y_ext.split('.');
			const zNum = Number(z);
			const xNum = Number(x);
			const yNum = Number(y);

			if (!isValidXyz(xNum, yNum, zNum)) return c.body('invalid xyz', 400);
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

			// query params
			const { tileSize, quality, margin } = c.req.valid('query');
			const tileSizeNum = Number(tileSize ?? 512);
			const qualityNum = Number(quality ?? 100);
			const marginNum = Number(margin ?? 0);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedTile({
					stylejson: style,
					z: zNum,
					x: xNum,
					y: yNum,
					tileSize: tileSizeNum,
					cache: options.cache,
					margin: marginNum,
					ext,
					quality: qualityNum,
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
		},
	);

	hono.get(
		'/:filename_ext',
		describeRoute({
			description: 'Get a clip image',
			tags: ['clips'],
			responses: {
				200: {
					description: 'clip image',
					content: {
						'image/png': { schema: resolver(z.instanceof(Buffer)) },
						'image/jpeg': { schema: resolver(z.instanceof(Buffer)) },
						'image/webp': { schema: resolver(z.instanceof(Buffer)) },
					},
				},
			},
		}),
		validator(
			'param',
			z.object({
				filename_ext: z.string(),
			}),
		),
		validator(
			'query',
			z.object({
				url: z.string(),
				bbox: z.string(),
				size: z.string().optional(),
				quality: z.string().optional(),
			}),
		),
		async (c) => {
			// path params
			const { filename_ext } = c.req.valid('param');
			const [filename, ext] = filename_ext.split('.');
			if (filename !== 'clip') return c.body('not found', 404);
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

			// query params
			const { url, bbox, size, quality } = c.req.valid('query');
			const [minx, miny, maxx, maxy] = bbox.split(',').map(Number);
			if (minx >= maxx || miny >= maxy) return c.body('invalid bbox', 400);

			const qualityNum = Number(quality ?? 100);
			const sizeNum = Number(size ?? 1024);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedBbox({
					stylejson: url,
					bbox: [minx, miny, maxx, maxy],
					size: sizeNum,
					cache: options.cache,
					ext,
					quality: qualityNum,
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
		},
	);

	hono.post(
		'/:filename_ext',
		describeRoute({
			description: 'Get a clip image',
			tags: ['clips'],
			responses: {
				200: {
					description: 'clip image',
					content: {
						'image/png': { schema: resolver(z.instanceof(Buffer)) },
						'image/jpeg': { schema: resolver(z.instanceof(Buffer)) },
						'image/webp': { schema: resolver(z.instanceof(Buffer)) },
					},
				},
			},
		}),
		validator(
			'param',
			z.object({
				filename_ext: z.string(),
			}),
		),
		validator(
			'query',
			z.object({
				bbox: z.string(),
				size: z.string().optional(),
				quality: z.string().optional(),
			}),
		),
		validator(
			'json',
			z.object({
				style: z.any(),
			}),
		),
		async (c) => {
			// body
			const { style } = c.req.valid('json');
			if (!isValidStylejson(style)) return c.body('invalid stylejson', 400);

			// path params
			const { filename_ext } = c.req.valid('param');
			const [filename, ext] = filename_ext.split('.');
			if (filename !== 'clip') return c.body('not found', 404);
			if (!isSupportedFormat(ext)) return c.body('invalid format', 400);

			// query params
			const { bbox, size, quality } = c.req.valid('query');
			const [minx, miny, maxx, maxy] = bbox.split(',').map(Number);
			if (minx >= maxx || miny >= maxy) return c.body('invalid bbox', 400);

			const qualityNum = Number(quality ?? 100);
			const sizeNum = Number(size ?? 1024);

			c.header('Content-Type', `image/${ext}`);

			try {
				const sharp = await getRenderedBbox({
					stylejson: style,
					bbox: [minx, miny, maxx, maxy],
					size: sizeNum,
					cache: options.cache,
					ext,
					quality: qualityNum,
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
		},
	);

	return {
		app: hono,
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
