// Tile-rendering throughput over HTTP, measured with autocannon.
//
//   pnpm test:benchmark               this checkout only
//   pnpm test:benchmark <checkout>    paired comparison: <checkout> (baseline) vs this one
//
// Each checkout's own src/ and node_modules serve one process with the source cache
// off; fixtures (tests/fixtures/bench-style.json, local files only) come from this
// checkout, so both sides render the same thing with no network I/O.
// Both servers run side by side and every scenario is measured back to back,
// alternating which side goes first each round, so runner drift hits both equally.
// Shared CI runners are noisy: a Δ of a few % is noise, so rerun before trusting it.
//
// Env: CHIITILER_BENCH_DURATION seconds per measurement (default 5),
//      CHIITILER_BENCH_ROUNDS (default 3), CHIITILER_BENCH_WARMUP seconds (default 2).
// Markdown goes to stdout, progress to stderr. A baseline that cannot start
// (e.g. too old for this harness) is reported, and this checkout is measured alone.

import { type ChildProcess, spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { createRequire } from 'node:module';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import autocannon from 'autocannon';

const DURATION = Number(process.env.CHIITILER_BENCH_DURATION ?? 5);
const ROUNDS = Number(process.env.CHIITILER_BENCH_ROUNDS ?? 3);
const WARMUP = Number(process.env.CHIITILER_BENCH_WARMUP ?? 2);

const style = `url=${encodeURIComponent(pathToFileURL(path.resolve('tests/fixtures/bench-style.json')).href)}`;
const scenarios = [
    { name: 'png', path: `/tiles/5/28/12.png?${style}`, connections: 1 },
    { name: 'png c=10', path: `/tiles/5/28/12.png?${style}`, connections: 10 },
    { name: 'webp', path: `/tiles/5/28/12.webp?${style}`, connections: 1 },
    { name: 'jpeg', path: `/tiles/5/28/12.jpeg?${style}`, connections: 1 },
    {
        name: 'png tileSize=1024',
        path: `/tiles/5/28/12.png?${style}&tileSize=1024`,
        connections: 1,
    },
];

type Server = { label: string; url: string; proc: ChildProcess };
type Sample = { reqPerSec: number; p50: number };

const log = (msg: string) => process.stderr.write(`${msg}\n`);
const median = (xs: number[]) => {
    const s = [...xs].sort((a, b) => a - b);
    const m = s.length >> 1;
    return s.length % 2 ? s[m]! : (s[m - 1]! + s[m]!) / 2;
};

function freePort(): Promise<number> {
    return new Promise((resolve, reject) => {
        const srv = createServer()
            .once('error', reject)
            .listen(0, '127.0.0.1', () => {
                const { port } = srv.address() as { port: number };
                srv.close(() => resolve(port));
            });
    });
}

async function start(label: string, dir: string): Promise<Server> {
    const checkout = path.resolve(dir);
    const port = await freePort();
    // tsx and src/ from the target checkout; cwd stays here so fixture paths match.
    const tsx = createRequire(path.join(checkout, 'package.json')).resolve(
        'tsx',
    );
    const proc = spawn(
        process.execPath,
        [
            '--import',
            pathToFileURL(tsx).href,
            path.join(checkout, 'src/main.ts'),
        ],
        {
            env: {
                ...process.env,
                CHIITILER_PORT: String(port),
                CHIITILER_CACHE_METHOD: 'none',
                CHIITILER_PROCESSES: '1',
            },
            stdio: ['ignore', 'ignore', 'inherit'],
        },
    );
    const server = { label, url: `http://127.0.0.1:${port}`, proc };
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline && proc.exitCode === null) {
        const ok = await fetch(`${server.url}/health`)
            .then((r) => r.ok)
            .catch(() => false);
        if (ok) return server;
        await new Promise((r) => setTimeout(r, 250));
    }
    proc.kill('SIGKILL');
    throw new Error(`${label} server did not start`);
}

async function measure(
    server: Server,
    scenario: (typeof scenarios)[number],
    duration: number,
): Promise<Sample> {
    const r = await autocannon({
        url: server.url + scenario.path,
        connections: scenario.connections,
        duration,
        timeout: 30,
    });
    if (r.errors || r.timeouts || r.non2xx || !r.requests.total)
        throw new Error(
            `${server.label} ${scenario.name}: ${r.errors} errors, ${r.timeouts} timeouts, ${r.non2xx} non-2xx, ${r.requests.total} responses`,
        );
    return { reqPerSec: r.requests.average, p50: r.latency.p50 };
}

// The served image must be the real thing, not an error page.
async function preflight(server: Server) {
    for (const s of scenarios) {
        const res = await fetch(server.url + s.path);
        const type = res.headers.get('content-type') ?? '';
        if (!res.ok || !type.startsWith('image/'))
            throw new Error(
                `${server.label} ${s.name}: HTTP ${res.status} ${type}`,
            );
        await measure(server, s, WARMUP);
    }
}

async function main() {
    if (![DURATION, ROUNDS, WARMUP].every((n) => n > 0))
        throw new Error('DURATION, ROUNDS and WARMUP must be positive');
    const servers: Server[] = [];
    const notes: string[] = [];
    try {
        const baseDir = process.argv[2];
        if (baseDir) {
            try {
                const base = await start('baseline', baseDir);
                servers.push(base);
                await preflight(base);
            } catch (e) {
                for (const s of servers.splice(0)) s.proc.kill('SIGKILL');
                notes.push(
                    `Baseline could not be measured (${(e as Error).message}); this checkout only.`,
                );
                log(notes.at(-1)!);
            }
        }
        const current = await start('current', '.');
        servers.push(current);
        await preflight(current);

        // samples[scenario][server][round]
        const samples = scenarios.map(() => servers.map((): Sample[] => []));
        for (let round = 0; round < ROUNDS; round++) {
            for (const [i, s] of scenarios.entries()) {
                const order = round % 2 ? [...servers].reverse() : servers;
                for (const server of order) {
                    log(
                        `round ${round + 1}/${ROUNDS} ${s.name} ${server.label}`,
                    );
                    samples[i]![servers.indexOf(server)]!.push(
                        await measure(server, s, DURATION),
                    );
                }
            }
        }

        const head = `${ROUNDS} rounds × ${DURATION}s per scenario, ${process.version}, source cache off, one process.`;
        let table: string[];
        if (servers.length === 2) {
            table = [
                '| Scenario | Req/s baseline → current | Δ median (min…max) | p50 ms baseline → current |',
                '|---|---:|---:|---:|',
                ...scenarios.map((s, i) => {
                    const [b, c] = samples[i]!;
                    const deltas = b!.map(
                        (x, r) => 100 * (c![r]!.reqPerSec / x.reqPerSec - 1),
                    );
                    const pct = (n: number) =>
                        `${n >= 0 ? '+' : ''}${n.toFixed(1)}%`;
                    const med = (xs: Sample[], k: keyof Sample) =>
                        median(xs.map((x) => x[k])).toFixed(1);
                    return `| ${s.name} | ${med(b!, 'reqPerSec')} → ${med(c!, 'reqPerSec')} | ${pct(median(deltas))} (${pct(Math.min(...deltas))}…${pct(Math.max(...deltas))}) | ${med(b!, 'p50')} → ${med(c!, 'p50')} |`;
                }),
            ];
        } else {
            table = [
                '| Scenario | Req/s | p50 ms |',
                '|---|---:|---:|',
                ...scenarios.map((s, i) => {
                    const [c] = samples[i]!;
                    return `| ${s.name} | ${median(c!.map((x) => x.reqPerSec)).toFixed(1)} | ${median(c!.map((x) => x.p50)).toFixed(1)} |`;
                }),
            ];
        }
        process.stdout.write(
            ['## Benchmark', '', head, ...notes, '', ...table, ''].join('\n'),
        );
    } finally {
        for (const s of servers) s.proc.kill('SIGKILL');
    }
}

main().catch((e) => {
    console.error(e);
    process.exit(1);
});
