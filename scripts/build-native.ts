// Build the Node-API addon for this machine into native/ml2drender.<platform>-<arch>.node,
// the file src/native.ts loads.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';

execFileSync('cargo', ['build', '-p', 'ml2drender-napi', '--profile', 'native'], { stdio: 'inherit' });
const lib = { darwin: 'libml2drender_napi.dylib', win32: 'ml2drender_napi.dll' }[process.platform as string] ?? 'libml2drender_napi.so';
mkdirSync('native', { recursive: true });
const out = `native/ml2drender.${process.platform}-${process.arch}.node`;
copyFileSync(`target/native/${lib}`, out);
console.log(out);
