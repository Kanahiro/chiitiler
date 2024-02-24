/// <reference lib="dom" />
// for using native fetch in TypeScript

import * as fs from 'fs';

async function getSource(uri: string): Promise<Buffer | null> {
    if (uri.startsWith('http://') || uri.startsWith('https://')) {
        const res = await fetch(uri);
        return Buffer.from(await res.arrayBuffer());
    }

    if (uri.startsWith('file:///')) {
        return new Promise((resolve, reject) => {
            fs.readFile(uri.replace('file:///', ''), (err, data) => {
                if (err) reject(err);
                resolve(data);
            });
        });
    }

    return null;
}

export { getSource };
