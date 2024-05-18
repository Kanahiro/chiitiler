import fs from 'fs';

function getFilesystemSource(uri: string) {
    return new Promise<Buffer>((resolve, reject) => {
        fs.readFile(uri.replace('file://', ''), (err, data) => {
            if (err) reject(err);
            resolve(data);
        });
    });
}

export { getFilesystemSource };
