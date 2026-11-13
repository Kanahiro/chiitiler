import { readFile } from 'node:fs/promises';

// The only Node-specific part of rendering: read the bundled WASM. Kept at the
// top of src/ so the relative path holds for src/, dist/ and build/ alike.
let module: Promise<WebAssembly.Module> | undefined;

/** ml2drender's WASM module, compiled once per process. */
export function loadWasmModule(): Promise<WebAssembly.Module> {
    module ??= readFile(
        new URL('../wasm/ml2drender_wasm.wasm', import.meta.url),
    ).then((bytes) => WebAssembly.compile(bytes));
    return module;
}
