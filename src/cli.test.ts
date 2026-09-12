import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import { createProgram } from './cli.js';
import * as server from './server/index.js';
import { prewarm } from './render/warmup.js';
import * as caches from './cache/index.js';

vi.mock('@maplibre/maplibre-gl-native', () => ({}));
vi.mock('./render/warmup.js', () => ({
    prewarm: vi.fn().mockResolvedValue(undefined),
}));

beforeEach(() => {
    vi.mocked(prewarm).mockReset().mockResolvedValue(undefined);
    vi.stubEnv('CHIITILER_PREWARM', undefined);
    vi.stubEnv('CHIITILER_FRONT_CACHE_TTL_SEC', undefined);
    vi.stubEnv('CHIITILER_FRONT_CACHE_MAX_BYTES', undefined);
});

afterEach(() => {
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
});

describe('run chiitiler', () => {
    it('applies front-cache TTL and byte limits from the environment', async () => {
        vi.stubEnv('CHIITILER_CACHE_METHOD', 'file');
        vi.stubEnv('CHIITILER_FRONT_CACHE_TTL_SEC', '2');
        vi.stubEnv('CHIITILER_FRONT_CACHE_MAX_BYTES', '6');
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
        await createProgram().parseAsync(['node', 'cli.js', 'tile-server']);
        await options.cache.set('a', Buffer.from('aaa'));
        await options.cache.set('b', Buffer.from('bbbb'));
        expect(await options.cache.get('a')).toBeUndefined();
        expect(await options.cache.get('b')).toEqual(Buffer.from('bbbb'));
        now += 2001;
        expect(await options.cache.get('b')).toBeUndefined();
        expect(backing.get.mock.calls).toEqual([['a'], ['b']]);
    });

    it.each([
        ['CHIITILER_FRONT_CACHE_TTL_SEC', '0', 'ttlSeconds'],
        ['CHIITILER_FRONT_CACHE_TTL_SEC', '-1', 'ttlSeconds'],
        ['CHIITILER_FRONT_CACHE_TTL_SEC', 'invalid', 'ttlSeconds'],
        ['CHIITILER_FRONT_CACHE_TTL_SEC', 'Infinity', 'ttlSeconds'],
        ['CHIITILER_FRONT_CACHE_MAX_BYTES', '', 'maxBytes'],
        ['CHIITILER_FRONT_CACHE_MAX_BYTES', '0', 'maxBytes'],
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
        { env: undefined, flags: [], enabled: true },
        { env: 'true', flags: [], enabled: true },
        { env: 'false', flags: [], enabled: false },
        { env: undefined, flags: ['--no-prewarm'], enabled: false },
        { env: 'true', flags: ['--no-prewarm'], enabled: false },
        { env: 'false', flags: ['--no-prewarm'], enabled: false },
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
