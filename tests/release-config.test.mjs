import { test } from 'node:test';
import assert from 'node:assert/strict';
import { releaseConfig } from '../scripts/release-config.mjs';

for (const [version, flag, channel] of [
    ['2.0.0-pre.0', true, 'next'],
    ['2.0.0-pre.0', false, 'next'],
    ['2.0.0', true, 'next'],
    ['2.0.0', false, 'latest'],
]) {
    test(`release ${version}, prerelease=${flag}`, () => {
        assert.deepEqual(releaseConfig(version, 'release', { release: { tag_name: `v${version}`, prerelease: flag } }), {
            image_tag: `v${version}`, npm_tag: channel, publish_latest: String(channel === 'latest'),
        });
    });
}
test('unprefixed release tags are preserved', () => {
    assert.equal(releaseConfig('2.0.0', 'release', { release: { tag_name: '2.0.0' } }).image_tag, '2.0.0');
});
test('a release cannot publish a different package version', () => {
    assert.throws(() => releaseConfig('2.0.0-pre.0', 'release', { release: { tag_name: 'v1.24.2' } }), /must match/);
});
for (const channel of ['next', 'latest']) {
    test(`manual stable version on ${channel}`, () => {
        assert.equal(releaseConfig('2.0.0', 'workflow_dispatch', { inputs: { channel } }).publish_latest, String(channel === 'latest'));
    });
}
test('manual prerelease uses next and rejects latest', () => {
    assert.equal(releaseConfig('2.0.0-pre.0', 'workflow_dispatch', { inputs: { channel: 'next' } }).npm_tag, 'next');
    assert.throws(() => releaseConfig('2.0.0-pre.0', 'workflow_dispatch', { inputs: { channel: 'latest' } }), /cannot/);
});
test('missing or unknown manual channels fail closed', () => {
    for (const channel of [undefined, 'invalid']) {
        assert.throws(() => releaseConfig('2.0.0', 'workflow_dispatch', { inputs: { channel } }), /Select/);
    }
});
