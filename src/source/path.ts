/**
 * Load a percent-decoded path first, then retry its literal form when the
 * resource is absent. MapLibre Native encodes glyph font stacks before passing
 * their URLs to the source resolver, while object stores treat keys literally.
 */
async function loadWithDecodedPathFallback<T>(
    path: string,
    load: (path: string) => Promise<T | null>,
): Promise<T | null> {
    let decodedPath: string;
    try {
        decodedPath = decodeURIComponent(path);
    } catch {
        return load(path);
    }

    if (decodedPath === path) return load(path);

    const decodedData = await load(decodedPath);
    if (decodedData !== null) return decodedData;
    return load(path);
}

export { loadWithDecodedPathFallback };
