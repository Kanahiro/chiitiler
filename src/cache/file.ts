import { createHash, randomUUID } from 'node:crypto';
import {
    mkdir,
    readFile,
    rename,
    stat,
    unlink,
    writeFile,
} from 'node:fs/promises';
import { join } from 'node:path';

import type { Cache, Value } from './index.js';

type fileCacheOptions = {
    dir: string;
    /** Seconds a value is served after it was written; 0 keeps it forever. */
    ttl: number;
};

/**
 * Values as files named by the SHA-256 of their key, written whole under a temporary name and
 * renamed, so processes sharing the directory never read a partial file.
 */
const fileCache: (options: fileCacheOptions) => Cache = ({ dir, ttl }) => {
    const path = (key: string) =>
        join(dir, createHash('sha256').update(key).digest('hex'));
    return {
        name: 'file',
        set: async (key: string, value: Value) => {
            const file = path(key);
            const temporary = `${file}.${randomUUID()}.tmp`;
            try {
                await mkdir(dir, { recursive: true });
                await writeFile(temporary, value);
                await rename(temporary, file);
            } catch (e) {
                console.error(e);
                await unlink(temporary).catch(() => {});
            }
        },
        get: async (key: string): Promise<Value | undefined> => {
            const file = path(key);
            try {
                if (ttl > 0) {
                    const age = Date.now() - (await stat(file)).mtimeMs;
                    if (age > ttl * 1000) {
                        await unlink(file).catch(() => {});
                        return undefined;
                    }
                }
                return await readFile(file);
            } catch {
                // missing, or removed meanwhile
                return undefined;
            }
        },
    };
};

export { fileCache, type fileCacheOptions };
