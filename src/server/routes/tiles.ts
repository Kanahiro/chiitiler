import { Hono } from 'hono';
import { isSupportedFormat, isValidStylejson } from '../utils.js';
import { getRenderedTile } from '../../render/index.js';
import { Cache } from '../../cache/index.js';

function isValidXyz(x: number, y: number, z: number) {
	if (x < 0 || y < 0 || z < 0) return false;
	if (x >= 2 ** z || y >= 2 ** z) return false;
	return true;
}

function createTilesRouter(options: { cache: Cache }) {
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

				const buf = await sharp.toBuffer();
				return c.body(buf as Uint8Array<ArrayBuffer>);
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

				const buf = await sharp.toBuffer();
				return c.body(buf as Uint8Array<ArrayBuffer>);
			} catch (e) {
				console.error(`render error: ${e}`);
				return c.body('failed to render tile', 400);
			}
		});
	return tiles;
}

export { createTilesRouter };
