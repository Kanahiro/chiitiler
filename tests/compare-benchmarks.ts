import fs from 'node:fs/promises';
import process from 'node:process';

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

function key(r: Row): string {
    return `${r.scenario}/${r.connections}`;
}

function pct(current: number, base: number): string {
    if (base === 0) return 'n/a';
    const diff = ((current - base) / base) * 100;
    const sign = diff >= 0 ? '+' : '';
    return `${sign}${diff.toFixed(1)}%`;
}

function cell(current: number, base: number): string {
    return `${pct(current, base)} (${base.toFixed(1)} → ${current.toFixed(1)})`;
}

async function main() {
    const [, , baselinePath, currentPath, outputPath] = process.argv;
    if (!baselinePath || !currentPath) {
        process.stderr.write(
            'usage: compare-benchmarks.ts <baseline.json> <current.json> [output.md]\n',
        );
        process.exit(2);
    }

    const [baselineRaw, currentRaw] = await Promise.all([
        fs.readFile(baselinePath, 'utf-8').catch(() => null),
        fs.readFile(currentPath, 'utf-8'),
    ]);
    const current: Row[] = JSON.parse(currentRaw);
    const baseline: Row[] | null = baselineRaw ? JSON.parse(baselineRaw) : null;

    const lines: string[] = [];
    lines.push('## Benchmark');
    lines.push('');

    if (!baseline) {
        lines.push(
            '_Baseline (main) results unavailable — showing current numbers only._',
        );
        lines.push('');
        lines.push(
            '| Scenario | Req/s | p50 (ms) | p90 (ms) | p99 (ms) |',
        );
        lines.push('|---|---:|---:|---:|---:|');
        for (const r of current) {
            lines.push(
                `| ${r.scenario} | ${r.reqPerSec.toFixed(1)} | ${r.latencyP50.toFixed(1)} | ${r.latencyP90.toFixed(1)} | ${r.latencyP99.toFixed(1)} |`,
            );
        }
    } else {
        const baselineMap = new Map(baseline.map((r) => [key(r), r]));
        lines.push(
            'Comparison: `main` → this PR (same runner, same environment).',
        );
        lines.push('');
        lines.push(
            '| Scenario | Req/s (higher is better) | p50 ms (lower is better) | p99 ms (lower is better) |',
        );
        lines.push('|---|---|---|---|');
        for (const r of current) {
            const b = baselineMap.get(key(r));
            if (!b) {
                lines.push(
                    `| ${r.scenario} | _new_ (${r.reqPerSec.toFixed(1)}) | _new_ (${r.latencyP50.toFixed(1)}) | _new_ (${r.latencyP99.toFixed(1)}) |`,
                );
                continue;
            }
            lines.push(
                `| ${r.scenario} | ${cell(r.reqPerSec, b.reqPerSec)} | ${cell(r.latencyP50, b.latencyP50)} | ${cell(r.latencyP99, b.latencyP99)} |`,
            );
        }
    }

    const output = lines.join('\n') + '\n';
    if (outputPath) {
        await fs.writeFile(outputPath, output);
    } else {
        process.stdout.write(output);
    }
}

main().catch((err) => {
    console.error(err);
    process.exit(1);
});
