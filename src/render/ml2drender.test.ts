import type { LayerSpecification } from '@maplibre/maplibre-gl-style-spec';
import { describe, expect, it } from 'vitest';
import { MapRenderer } from './ml2drender.js';

const style = (layers: unknown[]) =>
    ({ version: 8, sources: {}, layers }) as never;
const loadResource = async () => null;

describe('transformLayers', () => {
    const layers = [
        { id: 'bg', type: 'background' },
        { id: 'ext', type: 'fill-extrusion', source: 's' },
        { id: 'x', type: 'future-type' },
    ];

    it('is called for every layer; returning the layer keeps it', async () => {
        const seen: string[] = [];
        await MapRenderer.init(style(layers), {
            loadResource,
            transformLayers: (layer: LayerSpecification) => {
                seen.push(layer.id);
                return [layer];
            },
        });
        expect(seen).toEqual(['bg', 'ext', 'x']);
    });

    it('replaces a layer with none or several', async () => {
        const renderer = await MapRenderer.init(style(layers), {
            loadResource,
            transformLayers: (layer) =>
                layer.type === 'fill-extrusion'
                    ? [
                          { id: 'a', type: 'background' },
                          { id: 'b', type: 'background' },
                      ]
                    : layer.type === 'background'
                      ? [layer]
                      : [],
        });
        const { style: prepared } = renderer as unknown as {
            style: { layers: { id: string }[] };
        };
        expect(prepared.layers.map((l) => l.id)).toEqual(['bg', 'a', 'b']);
    });

    it('flattens fill-extrusion by default', async () => {
        const renderer = await MapRenderer.init(
            style([
                {
                    id: 'ext',
                    type: 'fill-extrusion',
                    source: 's',
                    paint: {
                        'fill-extrusion-color': 'red',
                        'fill-extrusion-height': 10,
                    },
                },
            ]),
            { loadResource },
        );
        const { style: prepared } = renderer as unknown as {
            style: { layers: { type: string; paint: object }[] };
        };
        expect(prepared.layers[0].type).toBe('fill');
        expect(prepared.layers[0].paint).toMatchObject({ 'fill-color': 'red' });
        expect(prepared.layers[0].paint).not.toHaveProperty(
            'fill-extrusion-height',
        );
    });

    it('rejects a replacement of an unsupported type', async () => {
        await expect(
            MapRenderer.init(style(layers), {
                loadResource,
                transformLayers: (layer) => [{ ...layer, id: 'y' }],
            }),
        ).rejects.toThrow('Invalid replacement for fill-extrusion layer ext');
    });
});
