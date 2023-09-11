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
    const mapOptions = {
        request: function (req, callback) {
            // TODO: better Caching
            if (KV[req.url]) {
                callback(null, { data: Buffer.from(KV[req.url]) });
                return;
            }

            fetch(req.url).then((res) => {
                if (res.status === 200) {
                    res.arrayBuffer().then((data) => {
                        callback(null, { data: Buffer.from(data) });
                        KV[req.url] = data;
                    });
                } else if (res.statusCode == 204) {
                    callback();
                } else {
                    callback();
                }
            });
        },
        ratio: options.tileSize / 512, // 1=512x512: entire globe at zoom 0
    };

    const render = function (z, x, y) {
        const map = new mbgl.Map(mapOptions);
        map.load(style);

        const center = getTileCenter(z, x, y, options.tileSize);
        const renderOptions = {
            zoom: z,
            width: 512,
            height: 512,
            center,
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
