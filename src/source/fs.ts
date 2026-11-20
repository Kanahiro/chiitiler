import fs from 'node:fs/promises';

import { loadWithDecodedPathFallback } from './path.js';

async function getFilesystemSource(uri: string): Promise<Buffer | null> {
    const path = uri.replace('file://', '');
    // a missing file is empty, like HTTP 404
    return loadWithDecodedPathFallback(path, (candidate) =>
        fs.readFile(candidate).catch(() => null),
    );
}

export { getFilesystemSource };
