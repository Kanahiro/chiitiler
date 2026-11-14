import { createRequire } from 'node:module';

/** A Web Mercator camera (see `ml2drender::View`). */
export type NativeView = {
    lng: number;
    lat: number;
    zoom: number;
    bearing: number;
    width: number;
    height: number;
    pixelRatio: number;
    margin: number;
};

/** The renderer of `crates/napi`. */
export interface NativeRenderer {
    setView(view: NativeView): void;
    setTicket(ticket: number): void;
    setKeepBuckets(keep: boolean): void;
    needs(): string[];
    provide(url: string, bytes: Uint8Array | null): void;
    render(oldest: number): Uint8Array<ArrayBuffer>;
}

export interface Native {
    Renderer: new (style: string, view: NativeView) => NativeRenderer;
    encodePng(
        rgba: Uint8Array,
        width: number,
        height: number,
    ): Uint8Array<ArrayBuffer>;
    encodeWebp(
        rgba: Uint8Array,
        width: number,
        height: number,
    ): Uint8Array<ArrayBuffer>;
    encodeJpeg(
        rgba: Uint8Array,
        width: number,
        height: number,
        quality: number,
    ): Uint8Array<ArrayBuffer>;
}

let loaded: Native | undefined;

/**
 * The Node-API addon for this platform, loaded on first use. Kept at the top of src/ so the
 * relative path holds for src/, dist/ and build/ alike (`pnpm build:native` builds it).
 */
export function native(): Native {
    if (loaded === undefined) {
        const file = `../native/ml2drender.${process.platform}-${process.arch}.node`;
        try {
            loaded = createRequire(import.meta.url)(file) as Native;
        } catch (cause) {
            throw new Error(
                `chiitiler has no renderer for ${process.platform}-${process.arch} (${file}); build one with \`pnpm build:native\``,
                { cause },
            );
        }
    }
    return loaded;
}
