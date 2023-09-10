import mbgl from '@maplibre/maplibre-gl-native';
import sharp from 'sharp';
import SphericalMercator from '@mapbox/sphericalmercator';
const mercator = new SphericalMercator();

function getTileCenter(z, x, y) {
    const tileCenter = mercator.ll(
        [
            ((x + 0.5) / (1 << z)) * (256 << z),
            ((y + 0.5) / (1 << z)) * (256 << z),
        ],
        z,
    );
    return tileCenter;
}

function getRenderer(style, options = { tileSize: 512 }) {
    const mapOptions = {
        mode: 'tile',
        request: function (req, callback) {
            // TODO: Caching
            fetch(req.url).then((res) => {
                res.arrayBuffer().then((data) => {
                    callback(null, { data: Buffer.from(data) });
                });
            });
        },
        ratio: options.tileSize / 512, // 1=512x512: entire globe at zoom 0
    };

    const map = new mbgl.Map(mapOptions);
    map.load(style);

    const render = function (z, x, y) {
        const center = getTileCenter(z, x, y);
        const renderOptions = {
            zoom: z,
            width: 512,
            height: 512,
            center,
        };

        return new Promise((resolve, reject) => {
            map.render(renderOptions, function (err, buffer) {
                if (err) reject(err);
                //map.release();
                console.log(buffer);
                const image = sharp(buffer, {
                    raw: {
                        width: options.tileSize,
                        height: options.tileSize,
                        channels: 4,
                    },
                });
                image.toFile(`${z}-${x}-${y}.png`, function (err) {
                    if (err) reject(err);
                    resolve();
                });
            });
        });
    };

    const release = function () {
        map.release();
    };

    return {
        render,
        release,
    };
}

export { getRenderer };
