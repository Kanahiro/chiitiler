import { program } from 'commander';

import { initServer, type InitServerOptions } from './server.js';
import { noneCache, memoryCache, fileCache, s3Cache } from './cache/index.js';

function parseCacheStrategy(
    strategy: 'none' | 'memory' | 'file' | 's3',
    options: { fileCacheDir?: string },
) {
    if (strategy === 'memory') return memoryCache();
    if (strategy === 'file')
        return fileCache({ dir: options.fileCacheDir ?? './.cache' });

    if (strategy === 's3') return s3Cache({ bucket: '' });

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
