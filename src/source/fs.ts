import fs from 'node:fs/promises';

import { loadWithDecodedPathFallback } from './path.js';

/** The file's bytes, or null when it does not exist (like HTTP 404); other failures throw. */
async function read(path: string): Promise<Buffer | null> {
    try {
        // devices (file:///dev/zero) and pipes never end: read regular files only
        if (!(await fs.stat(path)).isFile())
            throw new Error(`not a regular file: ${path}`);
        return await fs.readFile(path);
    } catch (e) {
        const code = (e as NodeJS.ErrnoException).code;
        if (code === 'ENOENT' || code === 'ENOTDIR') return null;
        throw e;
    }
}

async function getFilesystemSource(uri: string): Promise<Buffer | null> {
    return loadWithDecodedPathFallback(uri.replace('file://', ''), read);
}

export { getFilesystemSource };
