import { spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';

import autocannon from 'autocannon';

const PORT = Number(process.env.CHIITILER_BENCH_PORT ?? 3030);
const BASE = `http://127.0.0.1:${PORT}`;
const DURATION = Number(process.env.CHIITILER_BENCH_DURATION ?? 10);
const STYLE_URL = `file://${path.resolve('tests/fixtures/bench-style.json')}`;
const STYLE_QS = `url=${encodeURIComponent(STYLE_URL)}`;

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
    return spawn('npx', ['tsx', 'src/main.ts', 'tile-server'], {
        env: {
            ...process.env,
            CHIITILER_PORT: String(PORT),
            CHIITILER_CACHE_METHOD: 'none',
            CHIITILER_PROCESSES: '1',
        },
        stdio: 'ignore',
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
