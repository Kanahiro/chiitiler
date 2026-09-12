import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import cluster from 'node:cluster';
import { availableParallelism } from 'node:os';

import { createProgram } from './cli.js';
import * as server from './server/index.js';
import { prewarm } from './render/warmup.js';
import * as caches from './cache/index.js';

vi.mock('@maplibre/maplibre-gl-native', () => ({}));
vi.mock('node:cluster', () => ({
    default: { get isPrimary() { return true; }, fork: vi.fn() },
}));
vi.mock('./render/warmup.js', () => ({
    prewarm: vi.fn().mockResolvedValue(undefined),
}));

beforeEach(() => {
    vi.mocked(cluster.fork).mockReset();
    vi.mocked(prewarm).mockReset().mockResolvedValue(undefined);
    vi.stubEnv('CHIITILER_PREWARM', undefined);
    vi.stubEnv('CHIITILER_PROCESSES', undefined);
    vi.stubEnv('CHIITILER_FRONT_CACHE_TTL_SEC', undefined);
    vi.stubEnv('CHIITILER_FRONT_CACHE_MAX_BYTES', undefined);
});

afterEach(() => {
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
});

describe('run chiitiler', () => {
    it.each([false, true])('applies front-cache limits with CLI override=%s', async (useFlags) => {
        vi.stubEnv('CHIITILER_CACHE_METHOD', 'file');
        vi.stubEnv('CHIITILER_FRONT_CACHE_TTL_SEC', useFlags ? 'invalid' : '2');
        vi.stubEnv('CHIITILER_FRONT_CACHE_MAX_BYTES', useFlags ? 'invalid' : '6');
        let now = 1;
        vi.spyOn(performance, 'now').mockImplementation(() => now);
        const backing = {
            name: 'file',
            get: vi.fn().mockResolvedValue(undefined),
            set: vi.fn().mockResolvedValue(undefined),
        };
        vi.spyOn(caches, 'fileCache').mockReturnValue(backing);
        let options!: server.InitServerOptions;
        vi.spyOn(server, 'initServer').mockImplementation((opts) => {
            options = opts;
            return { app: {} as any, start: vi.fn() };
        });
        await createProgram().parseAsync(['node', 'cli.js', 'tile-server',
            ...(useFlags ? ['--front-cache-ttl-sec', '2', '--front-cache-max-bytes', '6'] : []),
        ]);
        await options.cache.set('a', Buffer.from('aaa'));
        await options.cache.set('b', Buffer.from('bbbb'));
        expect(await options.cache.get('a')).toBeUndefined();
        expect(await options.cache.get('b')).toEqual(Buffer.from('bbbb'));
        now += 2001;
        expect(await options.cache.get('b')).toBeUndefined();
        expect(backing.get.mock.calls).toEqual([['a'], ['b']]);
    });

    it.each([
        ['CHIITILER_FRONT_CACHE_TTL_SEC', '-1', 'ttlSeconds'],
        ['CHIITILER_FRONT_CACHE_TTL_SEC', 'invalid', 'ttlSeconds'],
        ['CHIITILER_FRONT_CACHE_TTL_SEC', 'Infinity', 'ttlSeconds'],
        ['CHIITILER_FRONT_CACHE_MAX_BYTES', '', 'maxBytes'],
        ['CHIITILER_FRONT_CACHE_MAX_BYTES', '1.5', 'maxBytes'],
        ['CHIITILER_FRONT_CACHE_MAX_BYTES', 'invalid', 'maxBytes'],
    ])('rejects invalid %s=%s before startup', async (env, value, error) => {
        vi.stubEnv(env, value);
        vi.spyOn(caches, 'fileCache').mockReturnValue({
            name: 'file', get: vi.fn(), set: vi.fn(),
        });
        const init = vi.spyOn(server, 'initServer');
        await expect(createProgram().parseAsync([
            'node', 'cli.js', 'tile-server', '-c', 'file',
        ])).rejects.toThrow(error);
        expect(init).not.toHaveBeenCalled();
        expect(prewarm).not.toHaveBeenCalled();
    });

    it.each([
        ['--front-cache-ttl-sec', '-1', 'ttlSeconds'],
        ['--front-cache-ttl-sec', 'invalid', 'ttlSeconds'],
        ['--front-cache-max-bytes', '1.5', 'maxBytes'],
        ['--front-cache-max-bytes', '', 'maxBytes'],
    ])('rejects invalid %s=%s', async (flag, value, error) => {
        vi.spyOn(caches, 'fileCache').mockReturnValue({ name: 'file', get: vi.fn(), set: vi.fn() });
        const init = vi.spyOn(server, 'initServer');
        await expect(createProgram().parseAsync([
            'node', 'cli.js', 'tile-server', '-c', 'file', flag, value,
        ])).rejects.toThrow(error);
        expect(init).not.toHaveBeenCalled();
    });

    it.each(['--front-cache-ttl-sec', '--front-cache-max-bytes'])(
        'disables the front cache with %s=0 despite the environment', async (flag) => {
            vi.stubEnv('CHIITILER_FRONT_CACHE_TTL_SEC', '10');
            vi.stubEnv('CHIITILER_FRONT_CACHE_MAX_BYTES', '1024');
            const backing = { name: 'file', get: vi.fn(), set: vi.fn() };
            vi.spyOn(caches, 'fileCache').mockReturnValue(backing);
            const init = vi.spyOn(server, 'initServer').mockReturnValue({ app: {} as any, start: vi.fn() });
            await createProgram().parseAsync(['node', 'cli.js', 'tile-server', '-c', 'file', flag, '0']);
            expect(init.mock.calls[0]![0].cache).toBe(backing);
        },
    );

    it.each(['CHIITILER_FRONT_CACHE_TTL_SEC', 'CHIITILER_FRONT_CACHE_MAX_BYTES'])(
        'disables the front cache when %s is zero', async (env) => {
            vi.stubEnv(env, '0');
            const backing = { name: 'file', get: vi.fn(), set: vi.fn() };
            vi.spyOn(caches, 'fileCache').mockReturnValue(backing);
            let options!: server.InitServerOptions;
            vi.spyOn(server, 'initServer').mockImplementation((opts) => {
                options = opts;
                return { app: {} as any, start: vi.fn() };
            });
            await createProgram().parseAsync(['node', 'cli.js', 'tile-server', '-c', 'file']);
            expect(options.cache).toBe(backing);
        },
    );

    it.each(['file', 's3', 'gcs', 'none', 'memory'] as const)(
        'adds the memory front cache only for I/O-backed caches: %s',
        async (method) => {
            const factory = method === 'none' ? 'noneCache' : `${method}Cache` as const;
            const backing = {
                name: method,
                get: vi.fn().mockResolvedValue(Buffer.from('value')),
                set: vi.fn().mockResolvedValue(undefined),
            };
            vi.spyOn(caches, factory).mockReturnValue(backing);
            vi.stubEnv('CHIITILER_CACHE_METHOD', undefined);
            let options!: server.InitServerOptions;
            vi.spyOn(server, 'initServer').mockImplementation((opts) => {
                options = opts;
                return { app: {} as any, start: vi.fn() };
            });
            await createProgram().parseAsync(['node', 'cli.js', 'tile-server', '-c', method]);
            await options.cache.get('key');
            await options.cache.get('key');
            expect(backing.get).toHaveBeenCalledTimes(
                ['file', 's3', 'gcs'].includes(method) ? 1 : 2,
            );
        },
    );

    it.each([
        { env: undefined, flags: [], count: 1 },
        { env: '2', flags: [], count: 2 },
        { env: '0', flags: [], count: availableParallelism() },
        { env: '2', flags: ['--processes', '1'], count: 1 },
        { env: 'invalid', flags: ['--processes', '3'], count: 3 },
        { env: '2', flags: ['--processes', '0'], count: availableParallelism() },
    ])('processes env=$env flags=$flags', async ({ env, flags, count }) => {
        vi.stubEnv('CHIITILER_PROCESSES', env);
        vi.spyOn(cluster, 'isPrimary', 'get').mockReturnValue(true);
        const fork = vi.spyOn(cluster, 'fork').mockReturnValue({} as any);
        const init = vi.spyOn(server, 'initServer').mockReturnValue({ app: {} as any, start: vi.fn() });
        await createProgram().parseAsync(['node', 'cli.js', 'tile-server', ...flags]);
        expect(fork).toHaveBeenCalledTimes(count === 1 ? 0 : count);
        expect(init).toHaveBeenCalledTimes(count === 1 ? 1 : 0);
        expect(prewarm).toHaveBeenCalledTimes(count === 1 ? 1 : 0);
    });

    it('starts a cluster worker without forking again', async () => {
        vi.spyOn(cluster, 'isPrimary', 'get').mockReturnValue(false);
        const fork = vi.spyOn(cluster, 'fork').mockReturnValue({} as any);
        const init = vi.spyOn(server, 'initServer').mockReturnValue({ app: {} as any, start: vi.fn() });
        await createProgram().parseAsync(['node', 'cli.js', 'tile-server', '--processes', '2']);
        expect(fork).not.toHaveBeenCalled();
        expect(init).toHaveBeenCalledOnce();
    });

    it.each(['-1', '1.5', '', 'invalid', 'Infinity'])('rejects invalid process count %s', async (value) => {
        const fork = vi.spyOn(cluster, 'fork');
        const program = createProgram().commands[0]!.exitOverride().configureOutput({ writeErr: () => {} });
        await expect(program.parseAsync(['node', 'cli.js', '--processes', value]))
            .rejects.toThrow('processes must be a non-negative safe integer');
        expect(fork).not.toHaveBeenCalled();
    });

    it('rejects the removed --no-prewarm flag', async () => {
        const program = createProgram().commands[0]!.exitOverride().configureOutput({ writeErr: () => {} });
        await expect(program.parseAsync(['node', 'cli.js', '--no-prewarm']))
            .rejects.toThrow("unknown option '--no-prewarm'");
    });

    it('parse options1', async () => {
        let options: server.InitServerOptions | undefined;

        vi.spyOn(server, 'initServer').mockImplementation(
            (opts: server.InitServerOptions) => {
                options = opts;
                return {
                    app: {} as any,
                    tiles: {} as any,
                    clip: {} as any,
                    start: vi.fn(),
                };
            },
        );

        const program = createProgram();
        await program.parseAsync(['node', 'cli.js', 'tile-server']);
        expect(options!.cache.name).toBe('none');
        expect(options!.port).toBe(3000);
        expect(options!.debug).toBe(false);
    });

    it('parse options2', async () => {
        let options: server.InitServerOptions | undefined;

        vi.spyOn(server, 'initServer').mockImplementation(
            (opts: server.InitServerOptions) => {
                options = opts;
                return {
                    app: {} as any,
                    tiles: {} as any,
                    clip: {} as any,
                    start: vi.fn(),
                };
            },
        );

        const program = createProgram();
        await program.parseAsync(['node', 'cli.js', 'tile-server', '-D', '-c', 'memory', '-p', '8989']); // prettier-ignore
        expect(options!.cache.name).toBe('memory');
        expect(options!.port).toBe(8989);
        expect(options!.debug).toBe(true);
    });

    it('parse options3', async () => {
        let options: server.InitServerOptions | undefined;

        vi.spyOn(server, 'initServer').mockImplementation(
            (opts: server.InitServerOptions) => {
                options = opts;
                return {
                    app: {} as any,
                    tiles: {} as any,
                    clip: {} as any,
                    start: vi.fn(),
                };
            },
        );

        const program = createProgram();
        await program.parseAsync(['node', 'cli.js', 'tile-server', '-c', 's3']);
        expect(options!.cache.name).toBe('s3');
    });

    it.each([
        { env: 'false', flags: ['--prewarm'], enabled: true },
        { env: 'false', flags: ['--prewarm', 'true'], enabled: true },
        { env: undefined, flags: [], enabled: true },
        { env: 'true', flags: [], enabled: true },
        { env: 'false', flags: [], enabled: false },
        { env: undefined, flags: ['--prewarm', 'false'], enabled: false },
        { env: 'true', flags: ['--prewarm', 'false'], enabled: false },
        { env: 'false', flags: ['--prewarm', 'false'], enabled: false },
    ])('prewarm env=$env flags=$flags enabled=$enabled', async ({ env, flags, enabled }) => {
        vi.stubEnv('CHIITILER_PREWARM', env);
        const start = vi.fn();
        vi.spyOn(server, 'initServer').mockImplementation(() => ({
            app: {} as any,
            start,
        }));

        const program = createProgram();
        await program.parseAsync(['node', 'cli.js', 'tile-server', ...flags]);
        expect(prewarm).toHaveBeenCalledTimes(enabled ? 1 : 0);
        expect(start).toHaveBeenCalled();
    });

    it('prewarm runs before the server starts', async () => {
        const callOrder: string[] = [];
        vi.mocked(prewarm).mockImplementation(async () => {
            callOrder.push('prewarm');
        });
        vi.spyOn(server, 'initServer').mockImplementation(() => ({
            app: {} as any,
            start: () => {
                callOrder.push('start');
            },
        }));

        const program = createProgram();
        await program.parseAsync(['node', 'cli.js', 'tile-server']);
        expect(callOrder).toEqual(['prewarm', 'start']);
    });

    it('prewarm failure does not prevent startup', async () => {
        vi.mocked(prewarm).mockRejectedValue(new Error('boom'));
        const start = vi.fn();
        vi.spyOn(server, 'initServer').mockImplementation(() => ({
            app: {} as any,
            start,
        }));

        const program = createProgram();
        await program.parseAsync(['node', 'cli.js', 'tile-server']);
        expect(prewarm).toHaveBeenCalled();
        expect(start).toHaveBeenCalled();
    });
});
