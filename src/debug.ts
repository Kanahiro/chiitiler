import { Context } from 'hono';
import { StyleSpecification } from 'maplibre-gl';

function getDebugPage(c: Context) {
    //demo tile
    const url =
        c.req.query('url') ?? 'https://demotiles.maplibre.org/style.json';

    // show tile in MapLibre GL JS
    return c.html(`<!DOCTYPE html>
    <html>
        <head>
            <meta charset="utf-8" />
            <title>MapLibre GL JS</title>
            <!-- maplibre gl js-->
            <script src="https://unpkg.com/maplibre-gl@^4.0/dist/maplibre-gl.js"></script>
            <link
                rel="stylesheet"
                href="https://unpkg.com/maplibre-gl@^4.0/dist/maplibre-gl.css"
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
                // hostname
                const tileUrl = window.location.origin + '/tiles/{z}/{x}/{y}.webp?url=${url}';

                const map = new maplibregl.Map({
                    hash: true,
                    container: 'map', // container id
                    style: {
                        version: 8,
                        sources: {
                            chiitiler: {
                                type: 'raster',
                                tiles: [tileUrl],
                            }
                        },
                        layers: [
                            {
                                id: 'chiitiler',
                                type: 'raster',
                                source: 'chiitiler',
                                minzoom: 0,
                                maxzoom: 22,
                            }
                        ],
                    },
                    center: [0, 0], // starting position [lng, lat]
                    zoom: 1, // starting zoom
                });
            </script>
        </body>
    </html>`);
}

export { getDebugPage };
