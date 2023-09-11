import { Hono } from 'hono';
import { serve } from '@hono/node-server';
import sharp from 'sharp';

import { getRenderer } from './tiling.js';

const hono = new Hono();

let style;

hono.get('/tiles/:z/:x/:y', async (c) => {
    // path params
    const z = Number(c.req.param('z'));
    const x = Number(c.req.param('x'));
    let [y, ext] = c.req.param('y').split('.');
    y = Number(y);

    if (['png', 'jpg', 'webp'].indexOf(ext) === -1) {
        return c.body('Invalid extension', 400);
    }

    // query params
    const tileSize = c.req.query('tileSize') ?? 512;
    const useSymbol = c.req.query('useSymbol') ?? false;
    const url = c.req.query('url') ?? null;

    if (style === null) {
        return c.body('style is required', 400);
    }

    if (!style) {
        style = await (await fetch(url)).json();
    }

    if (!useSymbol) {
        // TODO: symbol-layerが厄介。タイルの切れ目で文字が切れたりする
        style = {
            ...style,
            layers: style.layers.filter((layer) => layer.type !== 'symbol'),
        };
    }

    const { render } = getRenderer(style, { tileSize });
    const buffer = await render(z, x, y);

    const image = sharp(buffer, {
        raw: {
            width: tileSize,
            height: tileSize,
            channels: 4,
        },
    });

    let imgBuf;
    if (ext === 'jpg') {
        imgBuf = await image.jpeg({ quality: 20 }).toBuffer();
    } else if (ext === 'webp') {
        imgBuf = await image.webp({ quality: 100 }).toBuffer();
    } else {
        imgBuf = await image.png().toBuffer();
    }

    return c.body(imgBuf, 200, {
        'Content-Type': `image/${ext}`,
    });
});

hono.get('/debug', (c) => {
    return c.html(`<!-- show tile in MapLibre GL JS-->

  <!DOCTYPE html>
  <html>
      <head>
          <meta charset="utf-8" />
          <title>MapLibre GL JS</title>
          <!-- maplibre gl js-->
          <script src="https://unpkg.com/maplibre-gl@3.3.1/dist/maplibre-gl.js"></script>
          <link
              rel="stylesheet"
              href="https://unpkg.com/maplibre-gl@3.3.1/dist/maplibre-gl.css"
          />
          <style>
              body {
                  margin: 0;
                  padding: 0;
              }
              #map {
                  position: absolute;
                  top: 0;
                  bottom: 0;
                  width: 100%;
              }
          </style>
      </head>
      <body>
          <div id="map" style="height: 100vh"></div>
          <script>
              const map = new maplibregl.Map({
                  hash: true,
                  container: 'map', // container id
                  style: {
                      version: 8,
                      sources: {
                          chiitiler: {
                              type: 'raster',
                              tiles: [
                                  'http://localhost:3000/tiles/{z}/{x}/{y}.png?url=https://tile.openstreetmap.jp/styles/maptiler-toner-ja/style.json',
                              ],
                          },
                      },
                      layers: [
                          {
                              id: 'chiitiler',
                              type: 'raster',
                              source: 'chiitiler',
                              minzoom: 0,
                              maxzoom: 22,
                          },
                      ],
                  },
                  center: [0, 0], // starting position [lng, lat]
                  zoom: 1, // starting zoom
              });
          </script>
      </body>
  </html>
  `);
});

serve({ port: 3000, fetch: hono.fetch });
