import type { FillLayerSpecification, LayerSpecification, StyleSpecification } from '@maplibre/maplibre-gl-style-spec';
import { loadProjection, loadProjectionFactory, MapRenderer, type FillExtrusionLayer, type InitOptions, type Projection, type StyleLayer } from '../src/index';
import wasm from 'ml2drender/renderer.wasm';

async function pngResponse() {
  const style: StyleSpecification = await (await fetch('https://example.com/style.json')).json();
  const renderer = await MapRenderer.init(style, { wasm });
  const png: Uint8Array<ArrayBuffer> = (await renderer.renderView({ center: [0, 0], zoom: 0, width: 256, height: 256 })).toPng();
  return new Response(png, { headers: { 'Content-Type': 'image/png' } });
}

const utm = { type: 'ProjectedCRS', name: 'WGS 84 / UTM zone 54N' /* ... */ };
// The CRS as registered with GL JS's `addProjection`, on the same tile grid.
const crs = { name: 'EPSG:32654', projjson: utm, tileMatrixSet: { origin: [-2000000, 6000000] as [number, number], extentAtZoom0: 4000000 } };

async function projectedPngs(renderer: MapRenderer): Promise<Uint8Array<ArrayBuffer>[]> {
  const utmRenderer = renderer.projected(crs);
  const box = await utmRenderer.render({ bbox: [300000, 3900000, 400000, 4000000], width: 512, height: 512 });
  const tile = await utmRenderer.renderTile({ z: 3, x: 4, y: 2, tileSize: 512 });
  return [box.toPng(), tile.toWebp()];
}

async function sourceTile(renderer: MapRenderer): Promise<Response> {
  const mvt = await renderer.projected(crs).getTile({ source: 'openmaptiles', z: 3, x: 4, y: 2 });
  // @ts-expect-error A source's tile is addressed by its tile index, not a box.
  await renderer.projected(crs).getTile({ source: 'openmaptiles', bbox: [300000, 3900000, 400000, 4000000], width: 512, height: 512 });
  return new Response(mvt, { headers: { 'Content-Type': 'application/vnd.mapbox-vector-tile' } });
}

function glStyle(renderer: MapRenderer): StyleSpecification {
  // The style for GL JS, from tiles a Worker serves with `getTile`.
  return renderer.projected(crs).toStyle({ tiles: (source) => `https://example.com/${source}/{z}/{x}/{y}` });
}

async function mercatorPngs(renderer: MapRenderer): Promise<Uint8Array<ArrayBuffer>[]> {
  const tile = await renderer.renderTile({ z: 3, x: 7, y: 3 });
  const tokyo = await renderer.render({ bbox: [139.5, 35.5, 140, 36], size: 1024 });
  return [tile.toPng(), tokyo.toPng()];
}

async function wrongPath(renderer: MapRenderer) {
  // @ts-expect-error A CRS box renders with `projected(crs).render`.
  await renderer.renderView({ bbox: [300000, 3900000, 400000, 4000000], width: 512, height: 512 });
  // @ts-expect-error A Mercator camera renders with `renderView`.
  await renderer.projected(crs).render({ center: [0, 0], zoom: 0, width: 256, height: 256 });
  // @ts-expect-error A CRS is PROJJSON, not a PROJ string.
  renderer.projected({ ...crs, projjson: '+proj=utm +zone=54 +ellps=GRS80' });
}

async function utmEasting(): Promise<number> {
  const projection: Projection = await loadProjection(utm, { wasm });
  const lngLat: number[] = projection.inverse(projection.forward([139.76, 35.68]));
  // @ts-expect-error `loadProjection` takes PROJJSON, not a PROJ string.
  await loadProjection('+proj=utm +zone=54 +ellps=GRS80');
  const create: (projjson: object) => Projection = await loadProjectionFactory({ wasm });
  create(utm).forward([139.76, 35.68]);
  return lngLat ? lngLat[0] : NaN;
}

async function webpBytes(renderer: MapRenderer): Promise<Uint8Array<ArrayBuffer>> {
  return (await renderer.renderView({ center: [0, 0], zoom: 0, width: 256, height: 256 })).toWebp();
}

const transform: NonNullable<InitOptions['transformFillExtrusion']> = (layer) => ({
  id: layer.id,
  type: 'fill',
  source: layer.source,
  'source-layer': layer['source-layer'],
  minzoom: layer.minzoom,
  maxzoom: layer.maxzoom,
  filter: layer.filter,
  layout: { visibility: layer.layout?.visibility },
  paint: { 'fill-color': layer.paint?.['fill-extrusion-color'] ?? '#888888' },
});

const extrusion: FillExtrusionLayer = {
  id: 'buildings', type: 'fill-extrusion', source: 'buildings',
  paint: { 'fill-extrusion-height': ['get', 'height'] },
};
const replacement: LayerSpecification = transform(extrusion);
const fill: FillLayerSpecification = { id: 'fill', type: 'fill', source: 'buildings' };
const supported: StyleLayer = fill;

// @ts-expect-error A fill layer requires a source.
const missingSource: StyleLayer = { id: 'fill', type: 'fill' };
// @ts-expect-error Fill paint must use Style Spec property names.
const misspelledPaint: StyleLayer = { id: 'fill', type: 'fill', source: 'buildings', paint: { 'fill-colour': '#888888' } };
// @ts-expect-error Fill color cannot be a number.
const invalidColor: StyleLayer = { id: 'fill', type: 'fill', source: 'buildings', paint: { 'fill-color': 123 } };
// @ts-expect-error The replacement must be a supported 2D layer.
const unsupportedTransform: InitOptions = { transformFillExtrusion: (layer) => layer };

const customResourceLoader: InitOptions = {
  loadResource: async (url) => new Uint8Array(await (await fetch(url)).arrayBuffer()),
};
const missingResourceLoader: InitOptions = { loadResource: async () => null };

MapRenderer.init({ version: 8, sources: {}, layers: [] }, { wasm });
// @ts-expect-error The style must be an object; fetch a style URL first.
MapRenderer.init('https://example.com/style.json', { wasm });
// @ts-expect-error Style Spec version must be 8.
MapRenderer.init({ version: 7, sources: {}, layers: [] });
