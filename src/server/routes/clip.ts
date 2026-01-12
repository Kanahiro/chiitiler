import { Hono } from 'hono';
import { isSupportedFormat, isValidStylejson } from '../utils.js';
import { getRenderedClip } from '../../render/index.js';
import { Cache } from '../../cache/index.js';

function createClipRouter(options: { cache: Cache }) {
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
				const sharp = await getRenderedClip({
					stylejson: url,
					bbox: [minx, miny, maxx, maxy],
					size,
					cache: options.cache,
					ext,
					quality,
				});

				const buf = await sharp.toBuffer();
				return c.body(buf as Uint8Array<ArrayBuffer>);
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
				const sharp = await getRenderedClip({
					stylejson: style,
					bbox: [minx, miny, maxx, maxy],
					size,
					cache: options.cache,
					ext,
					quality,
				});

				const buf = await sharp.toBuffer();
				return c.body(buf as Uint8Array<ArrayBuffer>);
			} catch (e) {
				console.error(`render error: ${e}`);
				return c.body('failed to render tile', 400);
			}
		});
	return clip;
}

export { createClipRouter };
