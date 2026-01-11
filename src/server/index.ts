import { Hono } from 'hono/quick';
import { serve } from '@hono/node-server';

import { type Cache } from '../cache/index.js';
import { getDebugPage, getEditorgPage } from './debug.js';

import { createClipRouter } from './routes/clip.js';
import { createTilesRouter } from './routes/tiles.js';
import { createCameraRouter } from './routes/camera.js';

type InitServerOptions = {
	cache: Cache;
	port: number;
	debug: boolean;
	stream: boolean;
};

type InitializedServer = {
	app: Hono;
	start: () => void;
};

function initServer(options: InitServerOptions): InitializedServer {
	const hono = new Hono();
	if (options.debug) {
		hono.get('/debug', getDebugPage);
		hono.get('/editor', getEditorgPage);
	}
	hono.get('/health', (c) => c.text('OK'));
	hono.route(
		'/static',
		createCameraRouter({
			cache: options.cache,
			stream: options.stream,
		}),
	);
	hono.route(
		'/tiles',
		createTilesRouter({
			cache: options.cache,
			stream: options.stream,
		}),
	);
	hono.route(
		'/',
		createClipRouter({
			cache: options.cache,
			stream: options.stream,
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
