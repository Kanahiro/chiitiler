import { initServer, type InitServerOptions } from './server.js';
import { getCache } from './cache/index.js';

const options: InitServerOptions = {
    cache: getCache('file'),
};

const { start } = initServer(options);

start();
