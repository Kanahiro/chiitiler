import { Context } from 'hono';

function getDebugPage(c: Context) {
    //demo tile
    const url =
        c.req.query('url') ?? 'https://demotiles.maplibre.org/style.json';
    const margin = Number(c.req.query('margin') ?? 0);
    const quality = Number(c.req.query('quality') ?? 100);
    const tileSize = Number(c.req.query('tileSize') ?? 512);

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
                const tileUrl = window.location.origin + '/tiles/{z}/{x}/{y}.webp?url=${url}&quality=${quality}&margin=${margin}&tileSize=${tileSize}';

                const map = new maplibregl.Map({
                    hash: true,
                    container: 'map', // container id
                    style: {
                        version: 8,
                        sources: {
                            chiitiler: {
                                type: 'raster',
                                tiles: [tileUrl],
                                tileSize: ${tileSize},
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

function getEditorgPage(c: Context) {
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
            <link href="https://cdnjs.cloudflare.com/ajax/libs/jsoneditor/10.0.3/jsoneditor.css" rel="stylesheet" type="text/css">
            <script src="https://cdnjs.cloudflare.com/ajax/libs/jsoneditor/10.0.3/jsoneditor.min.js"></script>
            <style>
                body {
                    margin: 0;
                    padding: 0;
                }
            </style>
        </head>
        <body>
            <div id="map" style="height: 50vh"></div>
            <div id="jsoneditor" style="height: 50vh"></div>
            <script>
                const container = document.getElementById("jsoneditor")
                const editor = new JSONEditor(container, {
                    mode: 'code',
                    onChange: function() {
                        reloadStyle()
                        localStorage.setItem('style', JSON.stringify(editor.get()))
                    }
                })

                const initialStyle = localStorage.getItem('style') ? JSON.parse(localStorage.getItem('style')) : {
                    version: 8,
                    sources: {
                        osm: {
                            type: 'raster',
                            tiles: ['https://a.tile.openstreetmap.org/{z}/{x}/{y}.png'],
                            tileSize: 256,
                        },
                        point: {
                            type: 'geojson',
                            data: {
                                type: 'FeatureCollection',
                                features: [
                                    {
                                        type: 'Feature',
                                        properties: {},
                                        geometry: {
                                            type: 'Point',
                                            coordinates: [140, 40],
                                        },
                                    },
                                ],
                            },
                        },
                    },
                    layers: [
                        {
                            id: 'osm',
                            type: 'raster',
                            source: 'osm',
                            minzoom: 0,
                            maxzoom: 22,
                        },
                        {
                            id: 'geojson',
                            type: 'circle',
                            source: 'point',
                            paint: {
                                'circle-radius': 10,
                                'circle-color': '#f00',
                            },
                        },
                    ],
                };

                editor.set(initialStyle);
                
                maplibregl.addProtocol('post', async (params, abortController) => {
                    const imageUrl = params.url.replace('post://', '');
                    const style = editor.get()
                    const png = await fetch(imageUrl, {
                        method: 'POST',
                        headers: {
                            'Content-Type': 'application/json',
                        },
                        body: JSON.stringify({ style }),
                    }).then((res) => res.arrayBuffer());
                    return { data: png };
                });

                const style = {
                    version: 8,
                    sources: {
                        chiitiler: {
                            type: 'raster',
                            tiles: ['post://' + window.location.origin + '/tiles/{z}/{x}/{y}.png'],
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
                };

                const map = new maplibregl.Map({
                    hash: true,
                    container: 'map', // container id
                    style,
                    center: [0, 0], // starting position [lng, lat]
                    zoom: 1, // starting zoom
                });

                // reload button
                function reloadStyle() {
                    map.setStyle({
                        version: 8,
                        sources: {},
                        layers: [],
                    });
                    map.setStyle(style);
                }
            </script>
        </body>
    </html>`);
}

export { getDebugPage, getEditorgPage };
