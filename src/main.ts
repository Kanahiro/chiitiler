import { program } from 'commander';

import { initServer, type InitServerOptions } from './server.js';
import { noneCache, memoryCache, fileCache, s3Cache } from './cache/index.js';

function parseCacheStrategy(
    method: 'none' | 'memory' | 'file' | 's3',
    options: { fileCacheDir?: string; s3CacheBucket?: string },
) {
    // command-line option
    if (method === 'memory') return memoryCache();
    if (method === 'file')
        return fileCache({ dir: options.fileCacheDir ?? './.cache' });
    if (method === 's3')
        return s3Cache({ bucket: options.s3CacheBucket ?? '' });

    // command-line is not specified -> try to read from env
    const cacheEnv = process.env.CHIITILER_CACHE_METHOD;
    if (cacheEnv === 'memory') return memoryCache();
    if (cacheEnv === 'file')
        return fileCache({
            dir: process.env.CHIITILER_CACHE_FILECACHE_DIR ?? './.cache',
        });
    if (cacheEnv === 's3')
        return s3Cache({ bucket: process.env.CHIITILER_CACHE_S3BUCKET ?? '' });

    // undefined or invalid
    return noneCache();
}

function parsePort(port: string) {
    // command-line option
    if (port !== undefined) return Number(port);

    // command-line is not specified -> try to read from env
    const portEnv = process.env.CHIITILER_PORT;
    if (portEnv !== undefined) return Number(portEnv);

    // undefined or invalid
    return 3000;
}

function parseDebug(debug: boolean) {
    // command-line option
    if (debug) return true;

    // command-line is not specified or false -> try to read from env
    const debugEnv = process.env.CHIITILER_DEBUG;
    if (debugEnv !== undefined) return debugEnv === 'true';

    // undefined or invalid
    return false;
}

program
    .command('tile-server')
    .option('-c, --cache <type>', 'cache type', 'none')
    .option('-cd --file-cache-dir <dir>', 'file cache directory', './.cache')
    .option('-p --port <port>', 'port number', '3000')
    .option('-D --debug', 'debug mode')
    .action((options) => {
        const serverOptions: InitServerOptions = {
            cache: parseCacheStrategy(options.cache, options),
            port: parsePort(options.port),
            debug: parseDebug(options.debug),
        };

        console.log(`running server: http://localhost:${serverOptions.port}`);
        console.log(`cache method: ${serverOptions.cache}`);
        if (serverOptions.debug)
            console.log(
                `debug page: http://localhost:${serverOptions.port}/debug`,
            );

        const { start } = initServer(serverOptions);
        start();
    });

program.parse(process.argv);
