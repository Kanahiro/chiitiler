import type { Server } from 'node:http';

import { Hono } from 'hono/quick';
import { cors } from 'hono/cors';
import { serve } from '@hono/node-server';

import type { Cache } from '../cache/index.js';
import { getDebugPage, getEditorPage } from './debug.js';
import { registerRoutes } from './routes.js';

type InitServerOptions = {
    cache: Cache;
    port: number;
    debug: boolean;
    /** The largest image side in pixels. Default: 4096. */
    maxSize?: number;
    /** `Cache-Control` of rendered images (e.g. `public, max-age=86400`). Default: none. */
    cacheControl?: string;
    /** Origins allowed by CORS: `*`, or a list. Default: none (no CORS headers). */
    corsOrigins?: string[];
};

type InitializedServer = {
    app: Hono;
    start: () => void;
};

/** How long a shutdown waits for requests in progress. */
const SHUTDOWN_TIMEOUT_MS = 10_000;

function initServer(options: InitServerOptions): InitializedServer {
    const hono = new Hono();
    if (options.corsOrigins?.length) {
        const [first] = options.corsOrigins;
        hono.use(
            '*',
            cors({ origin: first === '*' ? '*' : options.corsOrigins }),
        );
    }
    if (options.debug) {
        hono.get('/debug', getDebugPage);
        hono.get('/editor', getEditorPage);
    }
    hono.get('/health', (c) => c.text('OK'));
    registerRoutes(hono, {
        cache: options.cache,
        maxSize: options.maxSize,
        cacheControl: options.cacheControl,
    });

    return {
        app: hono,
        start: () => {
            const server = serve({
                port: options.port,
                fetch: hono.fetch,
            }) as Server;
            // Stop accepting connections, let requests in progress finish, then exit.
            const shutdown = () => {
                console.log('shutting down server...');
                server.close(() => process.exit(0));
                server.closeIdleConnections();
                setTimeout(() => process.exit(1), SHUTDOWN_TIMEOUT_MS).unref();
            };
            process.once('SIGINT', shutdown);
            process.once('SIGTERM', shutdown);
        },
    };
}

export { initServer, type InitServerOptions };
