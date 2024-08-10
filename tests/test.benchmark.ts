import { bench, describe } from 'vitest';

describe('warmup', () => {
    bench(
        `warmup chiitiler`,
        async () => {
            await fetch(
                `http://localhost:3000/tiles/0/0/0.png?url=file://localdata/style.json`,
            );
        },
        {
            iterations: 1,
        },
    );
});

describe('render', () => {
    ['png', 'webp', 'jpeg'].forEach(
        (ext) => {
            bench(`render as ${ext}`, async () => {
                await fetch(
                    `http://localhost:3000/tiles/5/28/12.${ext}?url=file://localdata/style.json`,
                );
            });
        },
        {
            iterations: 10,
        },
    );
});

describe('render with margin', () => {
    ['png', 'webp', 'jpeg'].forEach(
        (ext) => {
            bench(`render as ${ext} with margin`, async () => {
                await fetch(
                    `http://localhost:3000/tiles/5/28/12.${ext}?url=file://localdata/style.json&margin=80`,
                );
            });
        },
        {
            iterations: 10,
        },
    );
});

describe('render tileSize=2048', () => {
    ['png', 'webp', 'jpeg'].forEach(
        (ext) => {
            bench(`render as ${ext}: 2048px`, async () => {
                await fetch(
                    `http://localhost:3000/tiles/7/109/54.${ext}?url=file://localdata/style.json&tileSize=2048`,
                );
            });
        },
        {
            iterations: 10,
        },
    );
});
