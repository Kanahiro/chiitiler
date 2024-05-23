import { Context } from 'hono';
import { stream } from 'hono/streaming';
import { type Sharp } from 'sharp';
import { type StyleSpecification } from '@maplibre/maplibre-gl-style-spec';

import { renderTilePipeline, type SupportedFormat } from '../render/index.js';
import { type Cache } from '../cache/index.js';

type TileResponseOptions = { cache: Cache } & (
    | {
          mode: 'url';
          url: string;
      }
    | {
          mode: 'style';
          style: StyleSpecification;
      }
);

function isValidXyz(x: number, y: number, z: number) {
    if (x < 0 || y < 0 || z < 0) return false;
    if (x >= 2 ** z || y >= 2 ** z) return false;
    return true;
}

function isSupportedFormat(ext: string): ext is SupportedFormat {
    return ['png', 'jpeg', 'jpg', 'webp'].includes(ext);
}

async function tileResponse(
    c: Context,
    options: TileResponseOptions,
): Promise<Response> {
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

    let src: string | StyleSpecification;
    if (options.mode === 'url') {
        src = options.url;
    } else {
        src = options.style;
    }

    let pipeline: Sharp;
    try {
        pipeline = await renderTilePipeline({
            stylejson: src,
            z,
            x,
            y,
            tileSize,
            cache: options.cache,
            margin,
            ext,
            quality,
        });
    } catch (e) {
        console.error(`render error: ${e}`);
        return c.body('failed to render tile', 400);
    }

    c.header('Content-Type', `image/${ext}`);
    return stream(c, async (stream) => {
        for await (const chunk of pipeline) {
            stream.write(chunk);
        }
    });
}

export { tileResponse };
