import { program } from 'commander';

import { initServer, type InitServerOptions } from './server.js';
import { noneCache, memoryCache, fileCache, s3Cache } from './cache/index.js';

function parseCacheStrategy(
    method: 'none' | 'memory' | 'file' | 's3',
    options: { fileCacheDir?: string },
) {
    // command-line option
    if (method === 'memory') return memoryCache();
    if (method === 'file')
        return fileCache({ dir: options.fileCacheDir ?? './.cache' });

    if (method === 's3') return s3Cache({ bucket: '' });

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

program
    .command('tile-server')
    .option('-c, --cache <type>', 'cache type', 'none')
    .option('-cd --file-cache-dir <dir>', 'file cache directory', './.cache')
    .option('-p --port <port>', 'port number', '3000')
    .option('-D --debug', 'debug mode')
    .action((options) => {
        const serverOptions: InitServerOptions = {
            cache: parseCacheStrategy(options.cache, options),
            port: Number(options.port),
            debug: options.debug,
        };

        console.log(`running server: http://localhost:${options.port}`);
        if (options.debug)
            console.log(`debug page: http://localhost:${options.port}/debug`);

        const { start } = initServer(serverOptions);
        start();
    });

program.parse(process.argv);
