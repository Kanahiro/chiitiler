import { getRenderer } from '../src/index.js';
import style from './style.json' assert { type: 'json' };

async function main() {
    const { render, release } = getRenderer(style);
    await render(1, 0, 0);
    await render(1, 0, 1);
    await render(1, 1, 0);
    await render(1, 1, 1);
    release();
}

main();
