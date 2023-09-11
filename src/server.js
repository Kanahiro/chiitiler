import { Hono } from 'hono';
import { serve } from '@hono/node-server';
import sharp from 'sharp';

import { getRenderer } from './tiling.js';

import style from './style.json' assert { type: 'json' };
const { render } = getRenderer(style);

const hono = new Hono();

hono.get('/tiles/:z/:x/:y/test10.png', async (c) => {
    const { z, x, y } = c.req.param();
    const buffer = await render(z, x, y);

    const image = sharp(buffer, {
        raw: {
            width: 256,
            height: 256,
            channels: 4,
        },
    });

    return c.body(await image.png().toBuffer(), 200, {
        'Content-Type': 'image/png',
    });
});

hono.get('/index.html', (c) => {
    return c.html(`<!-- show tile in MapLibre GL JS-->

  <!DOCTYPE html>
  <html>
      <head>
          <meta charset="utf-8" />
          <title>MapLibre GL JS</title>
          <meta
              name="viewport"
              content="initial-scale=1,maximum  -scale=1,user-scalable=no"
          />
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
                  container: 'map', // container id
                  style: {
                      version: 8,
                      sources: {
                          chiitiler: {
                              type: 'raster',
                              tiles: [
                                  'http://localhost:3000/tiles/{z}/{x}/{y}/test10.png',
                              ],
                              tileSize: 256,
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
