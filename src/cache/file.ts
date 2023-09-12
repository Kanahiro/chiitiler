import * as fs from 'fs';
import * as path from 'path';
import { type Cache, type Value } from './index.js';

function escapeFileName(url: string) {
    return url
        .replace(/https?:\/\//, '') // remove protocol
        .replace(/\//g, '_') // replace slashes with underscores
        .replace(/\?/g, '-') // replace question marks with dashes
        .replace(/&/g, '-') // replace ampersands with dashes
        .replace(/=/g, '-') // replace equals signs with dashes
        .replace(/%/g, '-') // replace percent signs with dashes
        .replace(/#/g, '-') // replace hash signs with dashes
        .replace(/:/g, '-') // replace colons with dashes
        .replace(/\+/g, '-') // replace plus signs with dashes
        .replace(/ /g, '-') // replace spaces with dashes
        .replace(/</g, '-') // replace less than signs with dashes
        .replace(/>/g, '-') // replace greater than signs with dashes
        .replace(/\*/g, '-') // replace asterisks with dashes
        .replace(/\|/g, '-') // replace vertical bars with dashes
        .replace(/"/g, '-') // replace double quotes with dashes
        .replace(/'/g, '-') // replace single quotes with dashes
        .replace(/\?/g, '-') // replace question marks with dashes
        .replace(/\./g, '-') // replace dots with dashes
        .replace(/,/g, '-') // replace commas with dashes
        .replace(/;/g, '-') // replace semicolons with dashes
        .replace(/\\/g, '-'); // replace backslashes with dashes
}

type fileCacheOptions = {
    dir: string;
};
const fileCache: (options: fileCacheOptions) => Cache = function (options) {
    return {
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
