// Trim maplibre-style-spec reference/v8.json to the layout/paint property fields the renderer
// needs. Usage: node scripts/trim-spec.ts path/to/v8.json > crates/core/src/style/spec.json
import {readFileSync} from 'node:fs';

const KEEP = ['type', 'value', 'length', 'default', 'values', 'property-type', 'expression', 'tokens'];
const LAYERS = ['background', 'fill', 'line', 'circle', 'raster', 'symbol', 'hillshade', 'color-relief', 'heatmap'];

const v8 = JSON.parse(readFileSync(process.argv[2], 'utf8'));
const out: Record<string, Record<string, Record<string, unknown>>> = {};
for (const layer of LAYERS) {
    for (const kind of ['layout', 'paint']) {
        const props: Record<string, Record<string, unknown>> = {};
        for (const [name, spec] of Object.entries<Record<string, unknown>>(v8[`${kind}_${layer}`] ?? {})) {
            const trimmed: Record<string, unknown> = {};
            for (const k of KEEP) {
                if (!(k in spec)) continue;
                trimmed[k] = k === 'values' ? Object.keys(spec[k] as object) : spec[k];
            }
            const expr = trimmed.expression as {interpolated?: boolean; parameters?: string[]} | undefined;
            if (expr) trimmed.expression = {interpolated: expr.interpolated, parameters: expr.parameters};
            props[name] = trimmed;
        }
        out[`${kind}_${layer}`] = props;
    }
}
process.stdout.write(`${JSON.stringify(out)}\n`);
