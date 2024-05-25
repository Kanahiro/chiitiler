import cluster from 'node:cluster';
import { availableParallelism } from 'node:os';
import process from 'node:process';

import { createProgram } from './cli.js';

const program = createProgram();

const numProcesses = (() => {
    if (process.env.CHIITILER_PROCESSES === undefined) return 1;
    const processesEnv = Number(process.env.CHIITILER_PROCESSES);
    if (processesEnv === 0) return availableParallelism();
    else return processesEnv;
})();

if (cluster.isPrimary && numProcesses !== 1) {
    // Fork workers.
    for (let i = 0; i < numProcesses; i++) cluster.fork();
} else {
    program.parse(process.argv);
}
