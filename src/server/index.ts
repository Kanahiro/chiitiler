import { Hono } from 'hono/quick';
import { serve } from '@hono/node-server';

import { type Cache } from '../cache/index.js';
import { getDebugPage, getEditorPage } from './debug.js';

import { createClipRouter } from './routes/clip.js';
import { createTilesRouter } from './routes/tiles.js';
import { createCameraRouter } from './routes/camera.js';

type InitServerOptions = {
	cache: Cache;
	port: number;
	debug: boolean;
};

type InitializedServer = {
	app: Hono;
	start: () => void;
};

function initServer(options: InitServerOptions): InitializedServer {
	const hono = new Hono();
	if (options.debug) {
		hono.get('/debug', getDebugPage);
		hono.get('/editor', getEditorPage);
	}
	hono.get('/health', (c) => c.text('OK'));
	hono.route(
		'/camera',
		createCameraRouter({
			cache: options.cache,
		}),
	);
	hono.route(
		'/tiles',
		createTilesRouter({
			cache: options.cache,
		}),
	);
	hono.route(
		'/',
		createClipRouter({
			cache: options.cache,
		}),
	);

	return {
		app: hono,
		start: () => {
			const server = serve({ port: options.port, fetch: hono.fetch });
			process.on('SIGINT', () => {
				console.log('shutting down server...');
				server.close();
				process.exit(0);
			});
			process.on('SIGTERM', () => {
				console.log('shutting down server...');
				server.close();
				process.exit(0);
			});
		},
	};
}

export { initServer, type InitServerOptions };
