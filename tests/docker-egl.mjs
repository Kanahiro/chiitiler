// Run against the built runtime image (no network or display server required):
// docker run --rm --network none -v "$PWD/tests/docker-egl.mjs:/app/docker-egl.mjs:ro" chiitiler:egl node /app/docker-egl.mjs
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { access, writeFile } from 'node:fs/promises';
import { setTimeout as delay } from 'node:timers/promises';
import sharp from 'sharp';

assert.equal(process.env.DISPLAY, undefined);
for (const path of ['/usr/bin/Xvfb', '/bin/sh', '/usr/bin/apt']) {
    await assert.rejects(access(path), { code: 'ENOENT' });
}
await access('/etc/ssl/certs/ca-certificates.crt');
const style = {
    version: 8,
    sources: { point: { type: 'geojson', data: { type: 'Point', coordinates: [0, 0] } } },
    layers: [
        { id: 'background', type: 'background', paint: { 'background-color': '#ffffff' } },
        { id: 'point', type: 'circle', source: 'point', paint: { 'circle-color': '#ff0000', 'circle-radius': 32 } },
    ],
};
await writeFile('/tmp/egl-style.json', JSON.stringify(style));
let log = '';
const server = spawn(process.execPath, ['/app/build/main.cjs', 'tile-server'], {
    env: { ...process.env, CHIITILER_PORT: '3000', CHIITILER_PROCESSES: '1', CHIITILER_CACHE_METHOD: 'none', CHIITILER_PREWARM: 'true' },
    stdio: ['ignore', 'pipe', 'pipe'],
});
server.stdout.on('data', chunk => { log += chunk; });
server.stderr.on('data', chunk => { log += chunk; });
const exited = once(server, 'exit');
try {
    const deadline = Date.now() + 30000;
    while (true) {
        assert.equal(server.exitCode, null, log);
        assert.ok(Date.now() < deadline, `Server did not become ready: ${log}`);
        try {
            if ((await fetch('http://127.0.0.1:3000/health')).ok) break;
        } catch { /* Wait for the server to listen. */ }
        await delay(50);
    }
    for (const size of [512, 1024]) {
        for (const format of ['png', 'webp', 'jpeg']) {
            const response = await fetch(`http://127.0.0.1:3000/tiles/0/0/0.${format}?tileSize=${size}&url=file:///tmp/egl-style.json`);
            await checkImage(response, size, false);
            console.log(`GET ${size}px ${format}: OK`);
        }
        const response = await fetch(`http://127.0.0.1:3000/tiles/0/0/0.png?tileSize=${size}`, {
            method: 'POST', headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ style: { ...style, layers: style.layers.slice(1) } }),
        });
        await checkImage(response, size, true);
        console.log(`POST ${size}px transparent PNG: OK`);
    }
    assert.ok(log.includes('prewarm done') && !log.includes('prewarm failed'), log);
    console.log('Prewarm and all 8 renders passed without Xvfb or DISPLAY.');
} finally {
    if (server.exitCode === null) server.kill('SIGTERM');
    const timeout = setTimeout(() => server.kill('SIGKILL'), 5000);
    await exited;
    clearTimeout(timeout);
}

async function checkImage(response, size, transparent) {
    const bytes = Buffer.from(await response.arrayBuffer());
    assert.equal(response.status, 200, bytes.toString());
    const { data, info } = await sharp(bytes).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
    assert.equal(info.width, size);
    assert.equal(info.height, size);
    const center = ((size / 2) * size + size / 2) * 4;
    // JPEG/WebP may alter colors slightly; a blank image must still fail.
    assert.ok(data[center] > 240 && data[center + 1] < 15 && data[center + 2] < 15);
    assert.deepEqual([...data.subarray(0, 4)], transparent ? [0, 0, 0, 0] : [255, 255, 255, 255]);
}
