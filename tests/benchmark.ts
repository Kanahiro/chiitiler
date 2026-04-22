import { spawn, type ChildProcess } from 'node:child_process';
import { createServer } from 'node:net';
import fs from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';

import autocannon from 'autocannon';

const DURATION = Number(process.env.CHIITILER_BENCH_DURATION ?? 10);
const STYLE_URL = `file://${path.resolve('tests/fixtures/bench-style.json')}`;
const STYLE_QS = `url=${encodeURIComponent(STYLE_URL)}`;

function isPortFree(port: number): Promise<boolean> {
    return new Promise((resolve) => {
        const tester = createServer()
            .once('error', () => resolve(false))
            .once('listening', () => {
                tester.close(() => resolve(true));
            })
            .listen(port, '127.0.0.1');
    });
}

async function pickPort(): Promise<number> {
    const configured = process.env.CHIITILER_BENCH_PORT;
    if (configured) {
        const p = Number(configured);
        if (!(await isPortFree(p))) {
            throw new Error(
                `CHIITILER_BENCH_PORT=${p} is already in use. Free it or unset the env.`,
            );
        }
        return p;
    }
    for (let p = 3030; p <= 3049; p++) {
        if (await isPortFree(p)) return p;
    }
    throw new Error('no free port in 3030-3049');
}

let PORT: number;
let BASE: string;

type Scenario = {
    name: string;
    url: string;
    connections: number;
};

const scenarios: Scenario[] = [
    {
        name: 'png c=1',
        url: `/tiles/5/28/12.png?${STYLE_QS}`,
        connections: 1,
    },
    {
        name: 'png c=10',
        url: `/tiles/5/28/12.png?${STYLE_QS}`,
        connections: 10,
    },
    {
        name: 'webp c=1',
        url: `/tiles/5/28/12.webp?${STYLE_QS}`,
        connections: 1,
    },
    {
        name: 'jpeg c=1',
        url: `/tiles/5/28/12.jpeg?${STYLE_QS}`,
        connections: 1,
    },
    {
        name: 'png tileSize=1024 c=1',
        url: `/tiles/5/28/12.png?${STYLE_QS}&tileSize=1024`,
        connections: 1,
    },
];

async function waitForReady(timeoutMs = 60_000): Promise<void> {
    const start = Date.now();
    while (Date.now() - start < timeoutMs) {
        try {
            const res = await fetch(`${BASE}/health`);
            if (res.ok) return;
        } catch {
            // server not up yet
        }
        await new Promise((r) => setTimeout(r, 250));
    }
    throw new Error(`server did not become ready within ${timeoutMs}ms`);
}

function startServer(): ChildProcess {
    // Spawn the tsx binary directly instead of going through `npx`, so a
    // SIGTERM to `server.kill()` hits the actual node process (otherwise
    // it only hits the npx wrapper and the real server is orphaned,
    // holding the port).
    const tsxBin = path.resolve('node_modules/.bin/tsx');
    return spawn(tsxBin, ['src/main.ts', 'tile-server'], {
        env: {
            ...process.env,
            CHIITILER_PORT: String(PORT),
            CHIITILER_CACHE_METHOD: 'none',
            CHIITILER_PROCESSES: '1',
        },
        stdio: ['ignore', 'inherit', 'inherit'],
    });
}

async function stopServer(server: ChildProcess): Promise<void> {
    if (server.exitCode !== null) return;
    server.kill('SIGTERM');
    await new Promise<void>((resolve) => {
        const timer = setTimeout(() => {
            server.kill('SIGKILL');
            resolve();
        }, 5_000);
        server.once('exit', () => {
            clearTimeout(timer);
            resolve();
        });
    });
    // give the OS a beat to release the port before the next run
    await new Promise((r) => setTimeout(r, 250));
}

type Row = {
    scenario: string;
    connections: number;
    reqPerSec: number;
    latencyP50: number;
    latencyP90: number;
    latencyP99: number;
    errors: number;
    non2xx: number;
};

function toMarkdown(rows: Row[]): string {
    const header = [
        '| Scenario | Conns | Req/s (mean) | p50 (ms) | p90 (ms) | p99 (ms) | Errors | non-2xx |',
        '|---|---:|---:|---:|---:|---:|---:|---:|',
    ];
    const body = rows.map(
        (r) =>
            `| ${r.scenario} | ${r.connections} | ${r.reqPerSec.toFixed(1)} | ${r.latencyP50.toFixed(1)} | ${r.latencyP90.toFixed(1)} | ${r.latencyP99.toFixed(1)} | ${r.errors} | ${r.non2xx} |`,
    );
    return [...header, ...body].join('\n');
}

async function main() {
    PORT = await pickPort();
    BASE = `http://127.0.0.1:${PORT}`;
    process.stderr.write(`using port ${PORT}\n`);
    const server = startServer();
    const rows: Row[] = [];
    try {
        await waitForReady();
        const warmRes = await fetch(BASE + scenarios[0].url);
        if (!warmRes.ok) {
            throw new Error(
                `warmup request failed: HTTP ${warmRes.status} ${await warmRes.text()}`,
            );
        }
        await warmRes.arrayBuffer();

        for (const scenario of scenarios) {
            process.stderr.write(`  running: ${scenario.name}\n`);
            const result = await autocannon({
                url: BASE + scenario.url,
                connections: scenario.connections,
                duration: DURATION,
                timeout: 30,
            });
            rows.push({
                scenario: scenario.name,
                connections: scenario.connections,
                reqPerSec: result.requests.average,
                latencyP50: result.latency.p50,
                latencyP90: result.latency.p90,
                latencyP99: result.latency.p99,
                errors: result.errors,
                non2xx: result.non2xx,
            });
        }
    } finally {
        await stopServer(server);
    }

    const md = toMarkdown(rows);
    process.stdout.write(`\n${md}\n`);

    if (process.env.GITHUB_STEP_SUMMARY) {
        await fs.appendFile(
            process.env.GITHUB_STEP_SUMMARY,
            `## Benchmark\n\n${md}\n`,
        );
    }
    if (process.env.CHIITILER_BENCH_OUTPUT) {
        await fs.writeFile(
            process.env.CHIITILER_BENCH_OUTPUT,
            JSON.stringify(rows, null, 2),
        );
    }
    if (process.env.CHIITILER_BENCH_MARKDOWN) {
        const preamble =
            '## Benchmark\n\n' +
            'autocannon against single-process `tile-server` ' +
            '(`CHIITILER_CACHE_METHOD=none`). ' +
            `Duration: ${DURATION}s per scenario.\n\n`;
        await fs.writeFile(
            process.env.CHIITILER_BENCH_MARKDOWN,
            preamble + md + '\n',
        );
    }

    const hadErrors = rows.some((r) => r.errors > 0 || r.non2xx > 0);
    if (hadErrors) {
        process.stderr.write(
            '\nERROR: benchmark produced errors / non-2xx responses\n',
        );
        process.exit(1);
    }
}

main().catch((err) => {
    console.error(err);
    process.exit(1);
});
