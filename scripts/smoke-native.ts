// Load this platform's addon from native/ and render a tile with it (CI, after build-native.ts).
import { native } from '../src/native.ts';

const n = native();
const style = JSON.stringify({ version: 8, sources: {}, layers: [{ id: 'bg', type: 'background', paint: { 'background-color': '#ff0000' } }] });
const r = new n.Renderer(style, { lng: 0, lat: 0, zoom: 0, bearing: 0, width: 8, height: 8, pixelRatio: 1, margin: 0 });
if (r.needs().length !== 0) throw new Error('a style without sources needs nothing');
const rgba = r.render(0);
if (rgba.length !== 8 * 8 * 4 || rgba[0] !== 255 || rgba[1] !== 0 || rgba[3] !== 255) throw new Error(`unexpected pixels ${rgba.slice(0, 4)}`);
const png = n.encodePng(rgba, 8, 8);
if (png[1] !== 0x50) throw new Error('not a PNG');
console.log(`ok: ${process.platform}-${process.arch}`);
