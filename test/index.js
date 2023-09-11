import sharp from 'sharp';

import { getRenderer } from '../src/tiling.js';
import style from './style.json' assert { type: 'json' };

const writeImage = async function (buffer, filepath, tileSize = 512) {
    const image = sharp(buffer, {
        raw: {
            width: tileSize,
            height: tileSize,
            channels: 4,
        },
    });
    await image.toFile(filepath);
};

async function main() {
    const { render, release } = getRenderer(style);

    const tiling = async (z, x, y) =>
        writeImage(await render(z, x, y), `./${z}-${x}-${y}.png`);

    await tiling(1, 0, 0);
    await tiling(1, 0, 1);
    await tiling(1, 1, 0);
    await tiling(1, 1, 1);

    release();
}

main();
