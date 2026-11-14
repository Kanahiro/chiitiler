import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import cluster from 'node:cluster';
import { availableParallelism } from 'node:os';

import { readConfig, run } from './cli.js';
import * as server from './server/index.js';
import * as caches from './cache/index.js';

vi.mock('node:cluster', () => ({
    default: {
        get isPrimary() {
            return true;
        },
        fork: vi.fn(),
    },
}));

beforeEach(() => {
    vi.mocked(cluster.fork).mockReset();
});

afterEach(() => {
    vi.restoreAllMocks();
});

/** Run with `env`, returning the options the server was started with (if it was). */
async function start(env: Record<string, string>, args: string[] = []) {
    let options: server.InitServerOptions | undefined;
    vi.spyOn(server, 'initServer').mockImplementation((opts) => {
        options = opts;
        return { app: {} as any, start: vi.fn() };
    });
    await run(args, env);
    return options;
}

const backing = (name: string) => ({
    name,
    get: vi.fn().mockResolvedValue(undefined),
    set: vi.fn().mockResolvedValue(undefined),
});

describe('configuration', () => {
    it('defaults', () => {
        const config = readConfig({});
        expect(config.cache.name).toBe('none');
        expect(config.port).toBe(3000);
        expect(config.debug).toBe(false);
        expect(config.processes).toBe(1);
        expect(config.maxSize).toBe(4096);
        expect(config.cacheControl).toBeUndefined();
        expect(config.corsOrigins).toBeUndefined();
    });

    it('reads every option from the environment', () => {
        const config = readConfig({
            CHIITILER_CACHE_METHOD: 'memory',
            CHIITILER_PORT: '8989',
            CHIITILER_DEBUG: 'true',
            CHIITILER_PROCESSES: '2',
            CHIITILER_MAX_SIZE: '2048',
            CHIITILER_CACHE_CONTROL: 'public, max-age=60',
            CHIITILER_CORS: 'https://a.example, https://b.example',
        });
        expect(config.cache.name).toBe('memory');
        expect(config.port).toBe(8989);
        expect(config.debug).toBe(true);
        expect(config.processes).toBe(2);
        expect(config.maxSize).toBe(2048);
        expect(config.cacheControl).toBe('public, max-age=60');
        expect(config.corsOrigins).toEqual([
            'https://a.example',
            'https://b.example',
        ]);
    });

    it('treats empty values as unset', () => {
        const config = readConfig({
            CHIITILER_PORT: '',
            CHIITILER_CACHE_METHOD: ' ',
            CHIITILER_CORS: '',
        });
        expect(config.port).toBe(3000);
        expect(config.cache.name).toBe('none');
        expect(config.corsOrigins).toBeUndefined();
    });

    it.each([
        ['CHIITILER_CACHE_METHOD', 'redis', 'CHIITILER_CACHE_METHOD must be'],
        ['CHIITILER_DEBUG', 'yes', 'CHIITILER_DEBUG must be true or false'],
        ['CHIITILER_PORT', '-1', 'CHIITILER_PORT must be an integer'],
        ['CHIITILER_PORT', 'http', 'CHIITILER_PORT must be an integer'],
        ['CHIITILER_PROCESSES', '1.5', 'CHIITILER_PROCESSES must be'],
        ['CHIITILER_PROCESSES', '-1', 'CHIITILER_PROCESSES must be'],
        ['CHIITILER_MAX_SIZE', '0', 'CHIITILER_MAX_SIZE must be'],
        ['CHIITILER_CACHE_TTL_SEC', 'x', 'CHIITILER_CACHE_TTL_SEC must be'],
    ])('rejects %s=%s', (name, value, error) => {
        expect(() => readConfig({ [name]: value })).toThrow(error);
    });

    it.each([
        ['s3', 'CHIITILER_S3CACHE_BUCKET'],
        ['gcs', 'CHIITILER_GCS_CACHE_BUCKET'],
    ])('requires a bucket for the %s cache', (method, name) => {
        expect(() => readConfig({ CHIITILER_CACHE_METHOD: method })).toThrow(
            `${name} is required`,
        );
    });
});

describe('run chiitiler', () => {
    it('starts without arguments', async () => {
        expect(await start({})).toBeDefined();
    });

    it.each([['tile-server'], ['--port', '3000'], ['-D']])(
        'rejects command-line arguments: %j',
        async (...args) => {
            const init = vi.spyOn(server, 'initServer');
            await expect(run(args, {})).rejects.toThrow(
                'chiitiler takes no command-line arguments',
            );
            expect(init).not.toHaveBeenCalled();
        },
    );

    it('applies front-cache limits', async () => {
        let now = 1;
        vi.spyOn(performance, 'now').mockImplementation(() => now);
        const file = backing('file');
        vi.spyOn(caches, 'fileCache').mockReturnValue(file);
        const options = await start({
            CHIITILER_CACHE_METHOD: 'file',
            CHIITILER_FRONT_CACHE_TTL_SEC: '2',
            CHIITILER_FRONT_CACHE_MAX_BYTES: '6',
        });
        await options!.cache.set('a', Buffer.from('aaa'));
        await options!.cache.set('b', Buffer.from('bbbb'));
        expect(await options!.cache.get('a')).toBeUndefined();
        expect(await options!.cache.get('b')).toEqual(Buffer.from('bbbb'));
        now += 2001;
        expect(await options!.cache.get('b')).toBeUndefined();
        expect(file.get.mock.calls).toEqual([['a'], ['b']]);
    });

    it.each([
        ['CHIITILER_FRONT_CACHE_TTL_SEC', '-1', 'ttlSeconds'],
        ['CHIITILER_FRONT_CACHE_TTL_SEC', 'invalid', 'must be a number'],
        ['CHIITILER_FRONT_CACHE_TTL_SEC', 'Infinity', 'ttlSeconds'],
        ['CHIITILER_FRONT_CACHE_MAX_BYTES', '1.5', 'maxBytes'],
        ['CHIITILER_FRONT_CACHE_MAX_BYTES', 'invalid', 'must be a number'],
    ])('rejects invalid %s=%s before startup', async (name, value, error) => {
        vi.spyOn(caches, 'fileCache').mockReturnValue(backing('file'));
        const init = vi.spyOn(server, 'initServer');
        await expect(
            run([], { CHIITILER_CACHE_METHOD: 'file', [name]: value }),
        ).rejects.toThrow(error);
        expect(init).not.toHaveBeenCalled();
    });

    it.each([
        'CHIITILER_FRONT_CACHE_TTL_SEC',
        'CHIITILER_FRONT_CACHE_MAX_BYTES',
    ])('disables the front cache when %s is zero', async (name) => {
        const file = backing('file');
        vi.spyOn(caches, 'fileCache').mockReturnValue(file);
        const options = await start({
            CHIITILER_CACHE_METHOD: 'file',
            [name]: '0',
        });
        expect(options!.cache).toBe(file);
    });

    it.each([
        ['file', 1],
        ['s3', 1],
        ['gcs', 1],
        ['none', 2],
        ['memory', 2],
    ] as const)(
        'front cache for %s: backing reads=%i',
        async (method, reads) => {
            const factory =
                method === 'none' ? 'noneCache' : (`${method}Cache` as const);
            const cache = backing(method);
            cache.get.mockResolvedValue(Buffer.from('value'));
            vi.spyOn(caches, factory).mockReturnValue(cache);
            const options = await start({
                CHIITILER_CACHE_METHOD: method,
                CHIITILER_S3CACHE_BUCKET: 'bucket',
                CHIITILER_GCS_CACHE_BUCKET: 'bucket',
            });
            await options!.cache.get('key');
            await options!.cache.get('key');
            expect(cache.get).toHaveBeenCalledTimes(reads);
        },
    );

    it.each([
        { env: undefined, count: 1 },
        { env: '2', count: 2 },
        { env: '0', count: availableParallelism() },
    ])('processes=$env forks $count', async ({ env, count }) => {
        vi.spyOn(cluster, 'isPrimary', 'get').mockReturnValue(true);
        const fork = vi.spyOn(cluster, 'fork').mockReturnValue({} as any);
        const options = await start(
            env === undefined ? {} : { CHIITILER_PROCESSES: env },
        );
        expect(fork).toHaveBeenCalledTimes(count === 1 ? 0 : count);
        expect(options === undefined).toBe(count !== 1);
    });

    it('starts a cluster worker without forking again', async () => {
        vi.spyOn(cluster, 'isPrimary', 'get').mockReturnValue(false);
        const fork = vi.spyOn(cluster, 'fork').mockReturnValue({} as any);
        const options = await start({ CHIITILER_PROCESSES: '2' });
        expect(fork).not.toHaveBeenCalled();
        expect(options).toBeDefined();
    });
});
