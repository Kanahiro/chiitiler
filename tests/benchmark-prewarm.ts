import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
import fs from 'node:fs/promises';
import { get } from 'node:http';
import { createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';

import { imageSize } from 'image-size';

const runs = Number(process.env.CHIITILER_BENCH_RUNS ?? 20);
if (!Number.isInteger(runs) || runs < 1) throw new Error('RUNS must be a positive integer');
const style = pathToFileURL(path.resolve('tests/fixtures/bench-style.json')).href;
const tilePath = `/tiles/5/28/12.png?url=${encodeURIComponent(style)}`;

// A new connection for every request avoids client connection-pool differences.
// Timing includes the entire response body, not just headers.
function request(url: string): Promise<Buffer> {
    return new Promise((resolve, reject) => {
        const req = get(url, { agent: false }, (res) => {
            const chunks: Buffer[] = [];
            res.on('data', (chunk: Buffer) => chunks.push(chunk));
            res.on('error', reject);
            res.on('end', () => {
                const body = Buffer.concat(chunks);
                if (res.statusCode !== 200) reject(new Error(`HTTP ${res.statusCode}: ${body}`));
                else resolve(body);
            });
        });
        req.setTimeout(30_000, () => req.destroy(new Error('request timed out')));
        req.on('error', reject);
    });
}

type Sample = {
    round: number;
    prewarm: boolean;
    startupMs: number;
    firstMs: number;
    secondMs: number;
    startupThroughFirstMs: number;
    prewarmMs: number | null;
    bytes: number;
    sha256: string;
};

async function measure(round: number, prewarm: boolean): Promise<Sample> {
    const probe = createServer().listen(0, '127.0.0.1');
    await once(probe, 'listening');
    const port = (probe.address() as { port: number }).port;
    await new Promise<void>((resolve) => probe.close(() => resolve()));
    const base = `http://127.0.0.1:${port}`;
    const started = performance.now();
    // Invoke Node directly so termination always reaches the server itself.
    const child = spawn(process.execPath, ['--import', 'tsx', 'src/main.ts', 'tile-server'], {
        env: {
            ...process.env,
            CHIITILER_PORT: String(port),
            CHIITILER_PROCESSES: '1',
            CHIITILER_CACHE_METHOD: 'none',
            CHIITILER_PREWARM: String(prewarm),
        },
        stdio: ['ignore', 'pipe', 'pipe'],
    });
    let log = '';
    let spawnError: Error | undefined;
    child.on('error', (error) => { spawnError = error; });
    child.stdout.on('data', (data) => { log += data; });
    child.stderr.on('data', (data) => { log += data; });
    try {
        while (true) {
            if (spawnError) throw spawnError;
            if (child.exitCode !== null || child.signalCode !== null) throw new Error(`server exited: ${log}`);
            if (performance.now() - started > 60_000) throw new Error(`startup timed out: ${log}`);
            try {
                await request(`${base}/health`);
                break;
            } catch { await delay(10); }
        }
        const startupMs = performance.now() - started;
        if (prewarm && (!/prewarm done in \d+ms/.test(log) || log.includes('prewarm failed'))) {
            throw new Error(`prewarm did not succeed: ${log}`);
        }
        const firstStart = performance.now();
        const first = await request(base + tilePath);
        const firstEnd = performance.now();
        const secondStart = performance.now();
        const second = await request(base + tilePath);
        const secondMs = performance.now() - secondStart;
        const dimensions = imageSize(first);
        if (dimensions.type !== 'png' || dimensions.width !== 512 || dimensions.height !== 512 || !first.equals(second)) {
            throw new Error('invalid or inconsistent rendered images');
        }
        return {
            round, prewarm, startupMs, firstMs: firstEnd - firstStart, secondMs,
            startupThroughFirstMs: firstEnd - started,
            prewarmMs: prewarm ? Number(log.match(/prewarm done in (\d+)ms/)![1]) : null,
            bytes: first.length,
            sha256: createHash('sha256').update(first).digest('hex'),
        };
    } finally {
        if (child.pid && child.exitCode === null && child.signalCode === null) {
            const exited = once(child, 'exit');
            child.kill('SIGTERM');
            const timer = setTimeout(() => child.kill('SIGKILL'), 5_000);
            try { await exited; } finally { clearTimeout(timer); }
        }
    }
}

function median(values: number[]): number {
    const sorted = [...values].sort((a, b) => a - b);
    const mid = Math.floor(sorted.length / 2);
    return sorted.length % 2 ? sorted[mid]! : (sorted[mid - 1]! + sorted[mid]!) / 2;
}

async function main() {
    await fs.access('localdata/tiles/5/28/12.pbf');
    const samples: Sample[] = [];
    for (let round = 1; round <= runs; round++) {
        // Alternate pair order to reduce drift from OS caches and machine load.
        for (const enabled of round % 2 ? [false, true] : [true, false]) {
            const sample = await measure(round, enabled);
            samples.push(sample);
            console.error(`${round}/${runs} prewarm=${enabled}: first=${sample.firstMs.toFixed(1)}ms`);
        }
    }
    if (new Set(samples.map((s) => s.sha256)).size !== 1) throw new Error('output differs between runs');
    const off = samples.filter((s) => !s.prewarm);
    const on = samples.filter((s) => s.prewarm);
    const lines = [
        '# Prewarm benchmark', '',
        `${runs} fresh processes per mode; alternating pair order. Node ${process.version}, ${os.platform()} ${os.arch()}, ${os.cpus()[0]?.model}.`,
        'Local vector fixture, PNG 512×512, one process, cache=none. Readiness uses /health only; no tile requests before measurement. HTTP timings include full body. Startup includes Node/tsx loading and readiness polling (10 ms interval).', '',
        '| Median (ms) | off | on |', '|---|---:|---:|',
    ];
    for (const key of ['startupMs', 'firstMs', 'secondMs', 'startupThroughFirstMs'] as const) {
        lines.push(`| ${key} | ${median(off.map((s) => s[key])).toFixed(1)} | ${median(on.map((s) => s[key])).toFixed(1)} |`);
    }
    const improvements = off.map((s) => s.firstMs - on.find((w) => w.round === s.round)!.firstMs);
    lines.push('', `First-request median reduction: ${(100 * (1 - median(on.map((s) => s.firstMs)) / median(off.map((s) => s.firstMs)))).toFixed(1)}%.`,
        `Paired improvement median: ${median(improvements).toFixed(1)} ms; faster with prewarm in ${improvements.filter((v) => v > 0).length}/${runs} pairs.`,
        `Prewarm duration median: ${median(on.map((s) => s.prewarmMs!)).toFixed(1)} ms. All ${samples.length * 2} tile responses are HTTP 200, 512×512 PNG and byte-identical (${samples[0]!.bytes} bytes).`, '',
        'These are process-cold measurements with shared OS caches, not machine-cold or AWS Lambda measurements. The fixture has no glyphs/sprites or remote downloads.', '');
    const markdown = lines.join('\n');
    console.log(markdown);
    if (process.env.CHIITILER_BENCH_OUTPUT) await fs.writeFile(process.env.CHIITILER_BENCH_OUTPUT, JSON.stringify({ date: new Date().toISOString(), node: process.version, platform: os.platform(), arch: os.arch(), cpu: os.cpus()[0]?.model, samples }, null, 2) + '\n');
    if (process.env.CHIITILER_BENCH_MARKDOWN) await fs.writeFile(process.env.CHIITILER_BENCH_MARKDOWN, markdown);
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
