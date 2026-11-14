import process from 'node:process';
import cluster from 'node:cluster';
import { availableParallelism } from 'node:os';

import { initServer, type InitServerOptions } from './server/index.js';
import * as caches from './cache/index.js';

type Env = Record<string, string | undefined>;

/** The value of `name`; empty means unset. */
function read(env: Env, name: string): string | undefined {
    const value = env[name]?.trim();
    return value === '' ? undefined : value;
}

function integer(env: Env, name: string, fallback: number, min: number) {
    const value = read(env, name);
    if (value === undefined) return fallback;
    const n = Number(value);
    if (!Number.isSafeInteger(n) || n < min)
        throw new Error(`${name} must be an integer of at least ${min}`);
    return n;
}

function number(env: Env, name: string): number | undefined {
    const value = read(env, name);
    if (value === undefined) return undefined;
    const n = Number(value);
    if (Number.isNaN(n)) throw new Error(`${name} must be a number`);
    return n;
}

function boolean(env: Env, name: string): boolean {
    const value = read(env, name);
    if (value === undefined || value === 'false') return false;
    if (value === 'true') return true;
    throw new Error(`${name} must be true or false`);
}

function required(env: Env, name: string, method: string): string {
    const value = read(env, name);
    if (value === undefined)
        throw new Error(
            `${name} is required with CHIITILER_CACHE_METHOD=${method}`,
        );
    return value;
}

function readCache(env: Env): caches.Cache {
    const method = read(env, 'CHIITILER_CACHE_METHOD') ?? 'none';
    const ttl = integer(env, 'CHIITILER_CACHE_TTL_SEC', 3600, 0);
    let cache: caches.Cache;
    switch (method) {
        case 'none':
            return caches.noneCache();
        case 'memory':
            // already in-process: no memory layer in front of it
            return caches.memoryCache({
                ttl,
                maxItemCount: integer(
                    env,
                    'CHIITILER_MEMORYCACHE_MAXITEMCOUNT',
                    1000,
                    1,
                ),
            });
        case 'file':
            cache = caches.fileCache({
                dir: read(env, 'CHIITILER_FILECACHE_DIR') ?? './.cache',
                ttl,
            });
            break;
        case 's3':
            cache = caches.s3Cache({
                bucket: required(env, 'CHIITILER_S3CACHE_BUCKET', method),
                region: read(env, 'CHIITILER_S3_REGION') ?? 'us-east-1',
                endpoint: read(env, 'CHIITILER_S3_ENDPOINT'),
                forcePathStyle: boolean(env, 'CHIITILER_S3_FORCE_PATH_STYLE'),
            });
            break;
        case 'gcs':
            cache = caches.gcsCache({
                bucket: required(env, 'CHIITILER_GCS_CACHE_BUCKET', method),
                prefix: read(env, 'CHIITILER_GCS_CACHE_PREFIX'),
                projectId: read(env, 'CHIITILER_GCS_PROJECT_ID'),
                keyFilename: read(env, 'CHIITILER_GCS_KEY_FILENAME'),
                apiEndpoint: read(env, 'CHIITILER_GCS_API_ENDPOINT'),
            });
            break;
        default:
            throw new Error(
                `CHIITILER_CACHE_METHOD must be none, memory, file, s3 or gcs (got ${method})`,
            );
    }
    return caches.withMemoryCache(cache, {
        ttlSeconds: number(env, 'CHIITILER_FRONT_CACHE_TTL_SEC'),
        maxBytes: number(env, 'CHIITILER_FRONT_CACHE_MAX_BYTES'),
    });
}

type Config = InitServerOptions & { processes: number };

/**
 * The server's configuration, from CHIITILER_* environment variables (empty values count as
 * unset). Throws for an invalid value, before anything starts.
 */
function readConfig(env: Env = process.env): Config {
    const processes = integer(env, 'CHIITILER_PROCESSES', 1, 0);
    return {
        cache: readCache(env),
        port: integer(env, 'CHIITILER_PORT', 3000, 0),
        debug: boolean(env, 'CHIITILER_DEBUG'),
        processes: processes === 0 ? availableParallelism() : processes,
        maxSize: integer(env, 'CHIITILER_MAX_SIZE', 4096, 1),
        cacheControl: read(env, 'CHIITILER_CACHE_CONTROL'),
        corsOrigins: read(env, 'CHIITILER_CORS')
            ?.split(',')
            .map((origin) => origin.trim())
            .filter(Boolean),
    };
}

/**
 * Start the tile server configured by environment variables. It takes no command-line options;
 * `tile-server`, the 1.x command, is accepted and ignored.
 */
async function run(args: string[], env: Env = process.env) {
    const rest = args[0] === 'tile-server' ? args.slice(1) : args;
    if (rest.length > 0)
        throw new Error(
            `chiitiler takes no command-line options (got ${rest.join(' ')}): configure it with CHIITILER_* environment variables`,
        );
    const { processes, ...options } = readConfig(env);
    if (cluster.isPrimary && processes !== 1) {
        // Workers inherit the environment, and initialize their own renderer and cache.
        for (let i = 0; i < processes; i++) cluster.fork();
        return;
    }
    if (options.debug) {
        console.log(`running server: http://localhost:${options.port}`);
        console.log(`cache method: ${options.cache.name}`);
        console.log(`debug page: http://localhost:${options.port}/debug`);
        console.log(`editor page: http://localhost:${options.port}/editor`);
    }
    // CHIITILER_USER_AGENT is read by source/userAgent.ts.
    initServer(options).start();
}

export { readConfig, run };
