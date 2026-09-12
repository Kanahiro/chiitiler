import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export type Row = {
    scenario: string; connections: number; reqPerSec: number;
    latencyP50: number; latencyP90: number; latencyP99: number;
    errors: number; non2xx: number; timeouts: number; requests: number;
};
type Report = {
    metadata: { baseline: string; current: string; rounds: number; duration: number; warmup: number; node: string; platform: string; arch: string; cpu?: string; cpus: number };
    samples: { round: number; side: 'baseline' | 'current'; success: boolean; rows: unknown }[];
};
const key = (r: Row) => `${r.scenario}/${r.connections}`;
function median(values: number[]): number {
    const sorted = [...values].sort((a, b) => a - b);
    const mid = Math.floor(sorted.length / 2);
    return sorted.length % 2 ? sorted[mid]! : (sorted[mid - 1]! + sorted[mid]!) / 2;
}
function validateRows(rows: unknown): asserts rows is Row[] {
    if (!Array.isArray(rows) || !rows.length) throw new Error('Missing scenario results');
    const keys = new Set<string>();
    for (const r of rows) {
        if (!r || typeof r.scenario !== 'string' || !r.scenario ||
            !Number.isInteger(r.connections) || r.connections < 1 ||
            !['reqPerSec', 'latencyP50', 'latencyP90', 'latencyP99', 'requests', 'errors', 'non2xx', 'timeouts'].every((k) => typeof r[k] === 'number' && Number.isFinite(r[k]) && r[k] >= 0) ||
            r.errors !== 0 || r.non2xx !== 0 || r.timeouts !== 0 || r.requests === 0 || r.reqPerSec <= 0) {
            throw new Error('Errors, invalid metrics, or zero responses in a scenario');
        }
        if (keys.has(key(r))) throw new Error('Duplicate scenario');
        keys.add(key(r));
    }
}

export function compare(report: Report): { valid: boolean; markdown: string } {
    try {
        const m = report.metadata;
        if (!Number.isInteger(m.rounds) || m.rounds < 2 || report.samples.length !== m.rounds * 2) throw new Error('Incomplete measurement pairs');
        const pairs: { baseline: Row[]; current: Row[] }[] = [];
        let expected = '';
        for (let round = 1; round <= m.rounds; round++) {
            const pair = {} as { baseline: Row[]; current: Row[] };
            for (const side of ['baseline', 'current'] as const) {
                const found = report.samples.filter((s) => s.round === round && s.side === side);
                if (found.length !== 1 || !found[0]!.success) throw new Error('Missing, duplicate, or failed measurement');
                const rows = found[0]!.rows;
                validateRows(rows);
                const keys = rows.map(key).sort().join('\n');
                if (expected && keys !== expected) throw new Error('Scenario mismatch');
                expected = keys;
                pair[side] = rows;
            }
            pairs.push(pair);
        }
        const lines = ['## Benchmark', '',
            `Baseline \`${m.baseline}\` → current \`${m.current}\`. ${m.rounds} pairs, alternating execution order.`,
            `${m.duration}s measurement + ${m.warmup}s warmup per scenario. Node ${m.node}; ${m.platform}/${m.arch}; ${m.cpu ?? 'unknown CPU'} (${m.cpus} logical CPUs).`, '',
            'Values are medians across runs. Δ is the median of paired percentage changes; range is the observed min…max, not a confidence interval. p99 is diagnostic only and omitted if any run has fewer than 100 responses. Minimum responses is the smallest per-run count on each side. No automatic performance gate.', '',
            '| Scenario | Req/s base → current | Paired Req/s Δ (range) | Faster pairs | Min responses base → current | p50 ms base → current | p99 ms base → current |',
            '|---|---:|---:|---:|---:|---:|---:|'];
        for (const scenario of pairs[0]!.baseline) {
            const base = pairs.map((p) => p.baseline.find((r) => key(r) === key(scenario))!);
            const current = pairs.map((p) => p.current.find((r) => key(r) === key(scenario))!);
            const deltas = base.map((b, i) => 100 * (current[i]!.reqPerSec / b.reqPerSec - 1));
            const values = (k: 'reqPerSec' | 'latencyP50' | 'latencyP99') => `${median(base.map((r) => r[k])).toFixed(1)} → ${median(current.map((r) => r[k])).toFixed(1)}`;
            const baseCount = Math.min(...base.map((r) => r.requests));
            const currentCount = Math.min(...current.map((r) => r.requests));
            const p99 = Math.min(baseCount, currentCount) < 100 ? 'n/a (<100 responses)' : values('latencyP99');
            lines.push(`| ${scenario.scenario.replaceAll('|', '\\|')} | ${values('reqPerSec')} | ${median(deltas).toFixed(1)}% (${Math.min(...deltas).toFixed(1)}…${Math.max(...deltas).toFixed(1)}%) | ${deltas.filter((d) => d > 0).length}/${m.rounds} | ${baseCount} → ${currentCount} | ${values('latencyP50')} | ${p99} |`);
        }
        lines.push('', 'Shared local fixture, one server process, source cache=none. Measures warm repeated-tile rendering and encoding; excludes startup, remote I/O and representative map navigation. A/A runs measure noise; small differences within that noise should not be treated as regressions.', '');
        return { valid: true, markdown: lines.join('\n') };
    } catch (error) {
        return { valid: false, markdown: `## Benchmark\n\n**Comparison unavailable:** ${error instanceof Error ? error.message : String(error)}. See raw artifacts and job logs.\n` };
    }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
    const [, , input, output] = process.argv;
    if (!input) throw new Error('usage: compare-benchmarks.ts <results.json> [output.md]');
    const result = compare(JSON.parse(await fs.readFile(input, 'utf8')));
    if (output) await fs.writeFile(output, result.markdown);
    else process.stdout.write(result.markdown);
    if (!result.valid) process.exitCode = 1;
}
