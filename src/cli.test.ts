import { describe, it, expect, vi } from 'vitest';

import { createProgram } from './cli.js';
import * as server from './server/index.js';

vi.mock('@maplibre/maplibre-gl-native', () => ({}));

describe('run chiitiler', () => {
    it('parse options1', async () => {
        let options: server.InitServerOptions | undefined;

        vi.spyOn(server, 'initServer').mockImplementation(
            (opts: server.InitServerOptions) => {
                options = opts;
                return {
                    app: {} as any,
                    tiles: {} as any,
                    bbox: {} as any,
                    start: vi.fn(),
                };
            },
        );

        const program = createProgram();
        program.parse(['node', 'cli.js', 'tile-server']);
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
                    bbox: {} as any,
                    start: vi.fn(),
                };
            },
        );

        const program = createProgram();
        program.parse(['node', 'cli.js', 'tile-server', '-D', '-c', 'memory', '-p', '8989']); // prettier-ignore
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
                    bbox: {} as any,
                    start: vi.fn(),
                };
            },
        );

        const program = createProgram();
        program.parse(['node', 'cli.js', 'tile-server', '-c', 's3']);
        expect(options!.cache.name).toBe('s3');
    });
});
