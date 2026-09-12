import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { compare } from './compare-benchmarks.js';

const rounds = Number(process.env.CHIITILER_BENCH_ROUNDS ?? 5);
if (!Number.isInteger(rounds) || rounds < 2) throw new Error('ROUNDS must be an integer >= 2');
const output = path.resolve('benchmark-results');
await fs.mkdir(output, { recursive: true });
const baseline = process.env.CHIITILER_BENCH_BASELINE;
const current = process.env.CHIITILER_BENCH_CURRENT;
if (!baseline || !current) throw new Error('BENCH_BASELINE and BENCH_CURRENT checkout paths are required');
const report = {
    metadata: {
        baseline: process.env.CHIITILER_BENCH_BASE_SHA ?? 'local',
        current: process.env.CHIITILER_BENCH_CURRENT_SHA ?? 'local',
        node: process.version, platform: os.platform(), arch: os.arch(), cpu: os.cpus()[0]?.model,
        cpus: os.cpus().length, date: new Date().toISOString(),
        duration: Number(process.env.CHIITILER_BENCH_DURATION ?? 10),
        warmup: Number(process.env.CHIITILER_BENCH_WARMUP ?? 3), rounds,
    },
    samples: [] as { round: number; side: 'baseline' | 'current'; rows: unknown; success: boolean }[],
};
try {
    for (let round = 1; round <= rounds; round++) {
        const order = round % 2 ? ['baseline', 'current'] as const : ['current', 'baseline'] as const;
        for (const side of order) {
            console.error(`Pair ${round}/${rounds}: ${side}`);
            const file = path.join(output, `${round}-${side}.json`);
            await fs.rm(file, { force: true });
            const success = await new Promise<boolean>((resolve, reject) => {
                const child = spawn(process.execPath, ['--import', 'tsx', 'tests/benchmark.ts'], {
                    env: { ...process.env, CHIITILER_BENCH_TARGET: side === 'baseline' ? baseline : current,
                        CHIITILER_BENCH_OUTPUT: file, GITHUB_STEP_SUMMARY: '', CHIITILER_BENCH_MARKDOWN: '' },
                    stdio: 'inherit',
                });
                child.once('error', reject);
                child.once('exit', (code) => resolve(code === 0));
            });
            const rows = await fs.readFile(file, 'utf8').then(JSON.parse).catch(() => null);
            report.samples.push({ round, side, rows, success });
            if (!success) throw new Error(`Measurement failed: pair ${round}, ${side}`);
        }
    }
} catch (error) {
    console.error(error);
    process.exitCode = 1;
} finally {
    await fs.writeFile(path.join(output, 'results.json'), JSON.stringify(report, null, 2) + '\n');
    const result = compare(report);
    await fs.writeFile(path.join(output, 'benchmark.md'), result.markdown);
    if (process.env.GITHUB_STEP_SUMMARY) await fs.appendFile(process.env.GITHUB_STEP_SUMMARY, result.markdown);
    if (!result.valid) process.exitCode = 1;
}
