import assert from 'node:assert/strict';
import { test } from 'node:test';
import { compare } from './compare-benchmarks.js';

function fixture() {
    const row = { scenario: 'png c=1', connections: 1, reqPerSec: 100, latencyP50: 10, latencyP90: 15, latencyP99: 20, errors: 0, non2xx: 0, timeouts: 0, requests: 1000 };
    return {
        metadata: { baseline: 'aaa', current: 'bbb', rounds: 2, duration: 10, warmup: 3, node: 'v24', platform: 'linux', arch: 'x64', cpus: 4 },
        samples: [
            { round: 1, side: 'baseline' as const, success: true, rows: [{ ...row }] },
            { round: 1, side: 'current' as const, success: true, rows: [{ ...row, reqPerSec: 120 }] },
            { round: 2, side: 'current' as const, success: true, rows: [{ ...row, reqPerSec: 180 }] },
            { round: 2, side: 'baseline' as const, success: true, rows: [{ ...row, reqPerSec: 200 }] },
        ],
    };
}
test('uses paired deltas rather than the ratio of aggregate medians', () => {
    const result = compare(fixture());
    assert.equal(result.valid, true);
    assert.match(result.markdown, /5\.0% \(-10\.0…20\.0%\)/);
    assert.match(result.markdown, /1\/2/);
});
for (const field of ['errors', 'non2xx', 'timeouts'] as const) {
    test(`rejects ${field} even when the process reported success`, () => {
        const data = fixture();
        data.samples[0]!.rows[0]![field] = 1;
        assert.equal(compare(data).valid, false);
    });
}
for (const mutation of [
    (d: ReturnType<typeof fixture>) => { d.samples.pop(); },
    (d: ReturnType<typeof fixture>) => { d.samples[0]!.success = false; },
    (d: ReturnType<typeof fixture>) => { d.samples[0]!.rows[0]!.scenario = 'other'; },
    (d: ReturnType<typeof fixture>) => { d.samples[0]!.rows[0]!.requests = 99; },
    (d: ReturnType<typeof fixture>) => { d.samples[0]!.rows[0]!.reqPerSec = NaN; },
    (d: ReturnType<typeof fixture>) => { d.samples[0]!.rows.push({ ...d.samples[0]!.rows[0]! }); },
    (d: ReturnType<typeof fixture>) => { d.samples[0] = d.samples[1]!; },
]) {
    test('rejects incomplete or incompatible measurements', () => {
        const data = fixture();
        mutation(data);
        const result = compare(data);
        assert.equal(result.valid, false);
        assert.doesNotMatch(result.markdown, /Req\/s/);
    });
}
