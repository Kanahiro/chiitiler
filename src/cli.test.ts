import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

import { createProgram } from './cli.js';
import * as server from './server/index.js';
import { prewarm } from './render/warmup.js';

vi.mock('@maplibre/maplibre-gl-native', () => ({}));
vi.mock('./render/warmup.js', () => ({
    prewarm: vi.fn().mockResolvedValue(undefined),
}));

beforeEach(() => {
    vi.mocked(prewarm).mockReset().mockResolvedValue(undefined);
    vi.stubEnv('CHIITILER_PREWARM', undefined);
});

afterEach(() => {
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
});

describe('run chiitiler', () => {
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
        { env: 'false', flags: ['--prewarm'], enabled: true },
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
        await program.parseAsync(['node', 'cli.js', 'tile-server', '--prewarm']); // prettier-ignore
        expect(prewarm).toHaveBeenCalled();
        expect(start).toHaveBeenCalled();
    });
});
