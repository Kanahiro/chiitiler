import process from 'node:process';

import { Command } from 'commander';

import { initServer, type InitServerOptions } from './server/index.js';
import * as caches from './cache/index.js';

function parseCacheStrategy(
    method: 'none' | 'memory' | 'file' | 's3' | 'gcs',
    options: {
        cacheTtl: number;
        memoryCacheMaxItemCount: number;
        fileCacheDir: string;
        s3CacheBucket: string;
        s3Region: string;
        s3Endpoint: string;
        s3ForcePathStyle: boolean;
        gcsCacheBucket: string;
        gcsProjectId: string;
        gcsKeyFilename: string;
    },
) {
    // command-line option
    if (method === 'memory')
        return caches.memoryCache({
            ttl: options.cacheTtl,
            maxItemCount: options.memoryCacheMaxItemCount,
        });
    if (method === 'file')
        return caches.fileCache({
            dir: options.fileCacheDir,
            ttl: options.cacheTtl,
        });
    if (method === 's3')
        return caches.s3Cache({
            bucket: options.s3CacheBucket,
            region: options.s3Region,
            endpoint: options.s3Endpoint,
            forcePathStyle: options.s3ForcePathStyle,
        });
    if (method === 'gcs')
        return caches.gcsCache({
            bucket: options.gcsCacheBucket,
            projectId: options.gcsProjectId,
            keyFilename: options.gcsKeyFilename,
        });

    // command-line is not specified -> try to read from env
    const cacheEnv = process.env.CHIITILER_CACHE_METHOD;
    if (cacheEnv === 'memory')
        return caches.memoryCache({
            ttl: Number(process.env.CHIITILER_CACHE_TTL_SEC ?? '3600'),
            maxItemCount: Number(
                process.env.CHIITILER_MEMORYCACHE_MAXITEMCOUNT ?? '1000',
            ),
        });
    if (cacheEnv === 'file')
        return caches.fileCache({
            dir: process.env.CHIITILER_FILECACHE_DIR ?? './.cache',
            ttl: Number(process.env.CHIITILER_CACHE_TTL_SEC ?? '3600'),
        });
    if (cacheEnv === 's3')
        return caches.s3Cache({
            bucket: process.env.CHIITILER_S3CACHE_BUCKET ?? '',
            region: process.env.CHIITILER_S3_REGION ?? 'us-east1',
            endpoint: process.env.CHIITILER_S3_ENDPOINT,
            forcePathStyle:
                process.env.CHIITILER_S3_FORCE_PATH_STYLE === 'true',
        });
    if (cacheEnv === 'gcs')
        return caches.gcsCache({
            bucket: process.env.CHIITILER_GCS_BUCKET ?? '',
            projectId: process.env.CHIITILER_GCS_PROJECT_ID ?? '',
            keyFilename: process.env.CHIITILER_GCS_KEY_FILENAME ?? '',
        });

    // undefined or invalid
    return caches.noneCache();
}

function parsePort(port: string | undefined) {
    // command-line option
    if (port !== undefined) return Number(port);

    // command-line is not specified -> try to read from env
    const portEnv = process.env.CHIITILER_PORT;
    if (portEnv !== undefined) return Number(portEnv);

    // undefined or invalid
    return 3000;
}

function parseDebug(debug: boolean | undefined) {
    // command-line option
    if (debug) return true;

    // command-line is not specified or false -> try to read from env
    const debugEnv = process.env.CHIITILER_DEBUG;
    if (debugEnv !== undefined) return debugEnv === 'true';

    // undefined or invalid
    return false;
}

function parseStream(stream: boolean | undefined) {
    // command-line option
    if (stream) return true;

    // command-line is not specified or false -> try to read from env
    const streamEnv = process.env.CHIITILER_STREAM_MODE;
    if (streamEnv !== undefined) return streamEnv === 'true';

    // undefined or invalid
    return false;
}

export function createProgram() {
    const program = new Command();
    program
        .command('tile-server')
        .option('-c, --cache <type>', 'cache type', 'none')
        .option('-ctl --cache-ttl', 'cache ttl', '3600')
        .option(
            '-mci --memory-cache-max-item-count',
            'memory cache max item count',
            '1000',
        )
        .option(
            '-fcd --file-cache-dir <dir>',
            'file cache directory',
            './.cache',
        )
        .option(
            '-s3r --s3-region <region-name>',
            's3 bucket region for get/put',
            'us-east1',
        )
        .option(
            '-s3b --s3-cache-bucket <bucket-name>',
            's3 cache bucket name',
            '',
        )
        .option('-s3e --s3-endpoint <url>', 's3 endpoint url', '')
        .option('-3p --s3-force-path-style', 's3 force path style', '')
        .option(
            '-gcsb --gcs-cache-bucket <bucket-name>',
            'gcs cache bucket name',
            '',
        )
        .option(
            '-gcsp --gcs-project-id <project-id>',
            'gcs project id',
            '',
        )
        .option(
            '-gcsk --gcs-key-filename <key-filename>',
            'gcs key filename',
            '',
        )
        .option('-p --port <port>', 'port number')
        .option('-r --stream', 'stream mode')
        .option('-D --debug', 'debug mode')
        .action((options) => {
            const serverOptions: InitServerOptions = {
                cache: parseCacheStrategy(options.cache, {
                    cacheTtl: Number(options.cacheTtl),
                    memoryCacheMaxItemCount: Number(
                        options.memoryCacheMaxItemCount,
                    ),
                    fileCacheDir: options.fileCacheDir,
                    s3CacheBucket: options.s3CacheBucket,
                    s3Region: options.s3Region,
                    s3Endpoint: options.s3Endpoint,
                    s3ForcePathStyle: options.s3ForcePathStyle === 'true',
                    gcsCacheBucket: options.gcsCacheBucket,
                    gcsProjectId: options.gcsProjectId,
                    gcsKeyFilename: options.gcsKeyFilename,
                }),
                port: parsePort(options.port),
                debug: parseDebug(options.debug),
                stream: parseStream(options.stream),
            };

            if (serverOptions.debug) {
                console.log(
                    `running server: http://localhost:${serverOptions.port}`,
                );
                console.log(`cache method: ${serverOptions.cache.name}`);
                console.log(
                    `debug page: http://localhost:${serverOptions.port}/debug`,
                );
                console.log(
                    `editor page: http://localhost:${serverOptions.port}/editor`,
                );
                console.log(
                    'stream mode:',
                    serverOptions.stream ? 'enabled' : 'disabled',
                );
            }

            const { start } = initServer(serverOptions);
            start();
        });
    return program;
}
