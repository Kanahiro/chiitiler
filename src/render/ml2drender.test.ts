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

    it('is called only for unsupported layers; returning the layer keeps it', async () => {
        const seen: string[] = [];
        await MapRenderer.init(style(layers), {
            loadResource,
            transformLayers: (layer: LayerSpecification) => {
                seen.push(layer.id);
                return [layer];
            },
        });
        expect(seen).toEqual(['ext', 'x']);
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
                    : [],
        });
        const { style: prepared } = renderer as unknown as {
            style: { layers: { id: string }[] };
        };
        expect(prepared.layers.map((l) => l.id)).toEqual(['bg', 'a', 'b']);
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
