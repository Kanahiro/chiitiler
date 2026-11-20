import { availableParallelism } from 'node:os';
import process from 'node:process';

// libuv sizes its thread pool, where renders and encodes run, from this on first use, so main.ts
// imports this first. Unset: one thread per CPU, the total split among cluster workers (cli.ts).
process.env.UV_THREADPOOL_SIZE ??= String(availableParallelism());
