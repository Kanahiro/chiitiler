import fs from 'fs';

function getFilesystemSource(uri: string) {
    return new Promise<Buffer | null>((resolve, _) => {
        fs.readFile(uri.replace('file://', ''), (err, data) => {
            if (err) {
                console.error(`[ERROR]: ${err}`);
                resolve(null);
            } else {
                resolve(data);
            }
        });
    });
}

export { getFilesystemSource };
