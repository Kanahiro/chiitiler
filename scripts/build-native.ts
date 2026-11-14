// Build the Node-API addon for this machine into native/ml2drender.<platform>-<arch>.node,
// the file src/native.ts loads.
// On Linux, GLIBC=2.34 links against that glibc version with cargo-zigbuild (on PATH, e.g.
// `pip install cargo-zigbuild ziglang`), so the addon loads on any glibc from 2.34 up.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';

const glibc = process.platform === 'linux' ? process.env.GLIBC : undefined;
const triple = { x64: 'x86_64', arm64: 'aarch64' }[process.arch as string] + '-unknown-linux-gnu';
execFileSync(
    'cargo',
    glibc
        ? ['zigbuild', '-p', 'ml2drender-napi', '--profile', 'native', '--target', `${triple}.${glibc}`]
        : ['build', '-p', 'ml2drender-napi', '--profile', 'native'],
    { stdio: 'inherit' },
);
const lib = { darwin: 'libml2drender_napi.dylib', win32: 'ml2drender_napi.dll' }[process.platform as string] ?? 'libml2drender_napi.so';
mkdirSync('native', { recursive: true });
const out = `native/ml2drender.${process.platform}-${process.arch}.node`;
copyFileSync(glibc ? `target/${triple}/native/${lib}` : `target/native/${lib}`, out);
console.log(out);
