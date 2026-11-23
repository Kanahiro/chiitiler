#!/usr/bin/env node
import './threadpool.js'; // first: before anything uses libuv's thread pool

import process from 'node:process';

import { run } from './cli.js';

run(process.argv.slice(2)).catch((error: unknown) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
});
