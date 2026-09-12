import process from 'node:process';

import { createProgram } from './cli.js';

createProgram().parseAsync(process.argv).catch((error: unknown) => {
    console.error(error);
    process.exitCode = 1;
});
