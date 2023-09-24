import * as fs from 'fs';
import * as path from 'path';
import { type Cache, type Value } from './index.js';
import { escapeFileName } from './utils.js';

type fileCacheOptions = {
    dir: string;
};
const fileCache: (options: fileCacheOptions) => Cache = function (options) {
    // mkdir
    if (!fs.existsSync(options.dir)) fs.mkdirSync(options.dir);
    return {
        name: 'file',
        set: async function (key: string, value: Value) {
            // write file
            return new Promise((resolve, reject) => {
                fs.writeFile(
                    path.join(options.dir, escapeFileName(`${key}`)),
                    value,
                    (err: any) => {
                        if (err) reject(err);
                        resolve();
                    },
                );
            });
        },
        get: async function (key: string): Promise<Value | undefined> {
            // read file
            return new Promise((resolve, reject) => {
                // check exist
                if (
                    !fs.existsSync(
                        path.join(options.dir, escapeFileName(`${key}`)),
                    )
                )
                    return resolve(undefined);

                fs.readFile(
                    path.join(options.dir, escapeFileName(`${key}`)),
                    (err: any, data: Buffer) => {
                        if (err) reject(err);
                        resolve(data);
                    },
                );
            });
        },
    };
};

export { fileCache };
