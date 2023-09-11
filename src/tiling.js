import mbgl from '@maplibre/maplibre-gl-native';
import SphericalMercator from '@mapbox/sphericalmercator';

function getTileCenter(z, x, y, tileSize = 256) {
    const mercator = new SphericalMercator({
        size: tileSize,
    });
    const px = tileSize / 2 + x * tileSize;
    const py = tileSize / 2 + y * tileSize;
    const tileCenter = mercator.ll([px, py], z);
    return tileCenter;
}

const KV = {};

function getRenderer(style, options = { tileSize: 256 }) {
    const render = function (z, x, y) {
        /**
         * zoom(renderingOptions): tileSize=256 -> z-1, 512 -> z, 1024 -> z+1...
         * width, height(renderingOptions): equal to tileSize but:
         * when zoom=0, entire globe is rendered in 512x512
         * even when tilesize=256, entire globe is rendered in "512x512 at zoom=0"
         * so we have to set 512 when tilesize=256 and zoom=0, and adjust ratio
         */
        const renderingParams =
            options.tileSize === 256 && z === 0
                ? {
                      zoom: 0,
                      height: 512,
                      width: 512,
                      ratio: 0.5,
                  }
                : {
                      zoom: z - 1 + Math.floor(options.tileSize / 512),
                      height: options.tileSize,
                      width: options.tileSize,
                      ratio: 1,
                  };

        const map = new mbgl.Map({
            request: function (req, callback) {
                // TODO: better Caching
                if (KV[req.url]) {
                    callback(null, { data: Buffer.from(KV[req.url]) });
                    return;
                }

                fetch(req.url)
                    .then((res) => {
                        if (res.status === 200) {
                            res.arrayBuffer().then((data) => {
                                callback(null, { data: Buffer.from(data) });
                                KV[req.url] = data;
                            });
                        } else {
                            callback();
                        }
                    })
                    .catch((err) => {
                        callback(err);
                    });
            },
            ratio: renderingParams.ratio,
        });

        map.load(style);

        const renderOptions = {
            zoom: renderingParams.zoom,
            width: renderingParams.width,
            height: renderingParams.height,
            center: getTileCenter(z, x, y, options.tileSize),
        };

        return new Promise((resolve, reject) => {
            map.render(renderOptions, function (err, buffer) {
                if (err) reject(err);
                resolve(buffer);
                map.release();
            });
        });
    };

    return {
        render,
    };
}

export { getRenderer };
