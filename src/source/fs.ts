import fs from 'node:fs/promises';

async function getFilesystemSource(uri: string): Promise<Buffer | null> {
    const path = uri.replace('file://', '');
    const data = await fs.readFile(path).catch(() => null);
    if (data !== null) return data;

    // maplibre-gl-native percent-encodes {fontstack} in glyphs URLs
    // (e.g. "Noto Sans Regular" -> "Noto%20Sans%20Regular"),
    // so retry with the percent-decoded path
    let decodedPath: string;
    try {
        decodedPath = decodeURIComponent(path);
    } catch {
        decodedPath = path;
    }
    if (decodedPath !== path) {
        const decodedData = await fs.readFile(decodedPath).catch(() => null);
        if (decodedData !== null) return decodedData;
    }

    console.error(`[ERROR]: failed to read ${path}`);
    return null;
}

export { getFilesystemSource };
