import fs from 'node:fs/promises';

import { loadWithDecodedPathFallback } from './path.js';

async function getFilesystemSource(uri: string): Promise<Buffer | null> {
    const path = uri.replace('file://', '');
    const data = await loadWithDecodedPathFallback(path, (candidate) =>
        fs.readFile(candidate).catch(() => null),
    );
    if (data !== null) return data;

    console.error(`[ERROR]: failed to read ${path}`);
    return null;
}

export { getFilesystemSource };
