//! Sans-IO MapLibre style renderer (2D, tile-based). See CLAUDE.md for scope and constraints.
//!
//! Host loop: `new` → while `needs()` is non-empty { fetch each; `provide` } → `render`.
//! Minimal use: `Renderer::new(style, View::tile(z, x, y, 512))` → one XYZ tile image.

mod hash;
pub mod expression;
pub(crate) mod geojson;
mod image;
mod sprite;
mod mvt;
mod projection;
mod render;
mod style;
mod supercluster;
mod symbol;
mod tile;
pub use projection::Projection;
pub mod reproject;

use serde_json::Value as J;
use hash::HashMap;
use std::collections::BTreeSet;
use std::rc::Rc;
use tile::{RenderTile, TileId, Transform};

#[derive(Debug)]
pub enum Error {
    Style(String),
    ImageEncoding(String),
    /// `render` called while `needs()` is non-empty.
    NotReady,
    Unsupported(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Style(m) => write!(f, "style error: {m}"),
            Error::ImageEncoding(m) => write!(f, "image encoding error: {m}"),
            Error::NotReady => write!(f, "render called with outstanding needs()"),
            Error::Unsupported(m) => write!(f, "unsupported: {m}"),
        }
    }
}

/// A resource the host must fetch and hand back via [`Renderer::provide`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Resource {
    /// URL as written in the style (or derived from it, e.g. a tile URL from a template).
    pub url: String,
}

/// What to render: a Web Mercator camera plus output size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// `[lng, lat]` in degrees.
    pub center: [f64; 2],
    /// MapLibre zoom (world is `512 * 2^zoom` CSS pixels wide).
    pub zoom: f64,
    /// Degrees, clockwise.
    pub bearing: f64,
    /// CSS pixels; the output is `round(width*pixel_ratio)` × `round(height*pixel_ratio)`.
    pub width: u32,
    pub height: u32,
    pub pixel_ratio: f64,
    /// CSS pixels rendered around the view before cropping to the requested output size.
    pub margin: u32,
}

impl View {
    /// The XYZ tile `z/x/y` rendered as a `tile_size`-pixel square image.
    pub fn tile(z: u8, x: u32, y: u32, tile_size: u32) -> Self {
        let n = (1u64 << z) as f64;
        let lng = (x as f64 + 0.5) / n * 360.0 - 180.0;
        let lat = tile::mercator_lat((y as f64 + 0.5) / n);
        Self {
            center: [lng, lat],
            zoom: z as f64 + (tile_size as f64 / 512.0).log2(),
            bearing: 0.0,
            width: tile_size,
            height: tile_size,
            pixel_ratio: 1.0,
            margin: 0,
        }
    }
}

struct TileJson {
    tiles: Vec<String>,
    minzoom: u8,
    maxzoom: u8,
    tms: bool,
    bounds: Option<[f64; 4]>,
    tile_size: f64,
    /// raster-dem `encoding`.
    encoding: Option<String>,
}

enum Data {
    /// Waiting for a TileJSON / GeoJSON document at this URL.
    Pending(String),
    Failed,
    Tiled(TileJson),
    GeoJson(geojson::GeoJsonIndex),
    /// Image source: URL, corner coordinates (tl, tr, br, bl) and the decoded image.
    Image { url: String, coordinates: [[f64; 2]; 4], texture: Option<Option<Rc<image::Texture>>> },
}

#[derive(Clone)]
enum TileData {
    Vector(Rc<Vec<mvt::Layer>>),
    Raster(Rc<image::Texture>),
    Dem(Rc<render::dem::Dem>),
}

struct Source {
    ty: String,
    json: J,
    data: Data,
    /// Decoded tiles; `None` when the host could not fetch (or decode) it.
    tiles: HashMap<TileId, Option<TileData>>,
    /// GeoJSON tiles and overzoom slices shared by glyph discovery and all style layers.
    /// Include the data tile so fallback parents cannot share the wrong slice.
    vector_layers: std::cell::RefCell<HashMap<(TileId, TileId), Rc<Vec<mvt::Layer>>>>,
    /// Layer buckets by (data tile, canonical tile, overscaled zoom), kept across views.
    buckets: std::cell::RefCell<HashMap<(TileId, TileId, u8), Rc<render::TileBuckets>>>,
    /// The latest `Renderer::clock` each data tile (GeoJSON: canonical tile) was needed or
    /// provided at.
    last_used: std::cell::RefCell<HashMap<TileId, u64>>,
    /// Whether tiles keep every source layer, not only the style's (once exported).
    all_layers: bool,
}

/// Records that `id` is needed at `clock`: the latest clock wins, whatever the call order.
fn stamp(last_used: &mut HashMap<TileId, u64>, id: TileId, clock: u64) {
    let t = last_used.entry(id).or_insert(clock);
    *t = (*t).max(clock);
}

const ZOOM_LEVELS_TO_OVERSCALE: u8 = 4;
// ponytail: a tile count, not bytes. Dense low-zoom tiles cost far more; bound by size if
// memory becomes the constraint.
const MAX_CACHED_TILES: usize = 8;

/// Lossless PNG of row-major RGBA8 pixels, e.g. the output of [`Renderer::render`].
pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    let mut encode = || -> Result<(), png::EncodingError> {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // Encoding is on the request's CPU budget; favor speed over minimum file size. `Fastest` is
        // `Fast`'s deflate with the Up filter instead of trying all five per row: about 3 times faster
        // in WASM, with files within ±10% (smaller for map tiles).
        encoder.set_compression(png::Compression::Fastest);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
        writer.finish()
    };
    encode().map_err(|e| Error::ImageEncoding(e.to_string()))?;
    Ok(bytes)
}

/// Lossless WebP of row-major RGBA8 pixels, e.g. the output of [`Renderer::render`].
pub fn encode_webp(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, Error> {
    // The encoder panics (a WASM trap) on a size mismatch instead of returning an error.
    if rgba.len() as u64 != width as u64 * height as u64 * 4 {
        return Err(Error::ImageEncoding(format!("{} bytes is not {width}x{height} RGBA", rgba.len())));
    }
    let mut bytes = Vec::new();
    image_webp::WebPEncoder::new(&mut bytes)
        .encode(rgba, width, height, image_webp::ColorType::Rgba8)
        .map_err(|e| Error::ImageEncoding(e.to_string()))?;
    Ok(bytes)
}

/// Baseline JPEG of row-major RGBA8 pixels at `quality` (1–100), flattened over black, with 4:2:0
/// chroma subsampling at every quality (libjpeg's and sharp's default; the encoder's own keeps full
/// chroma from quality 90, at about twice the encoding time and 1.5 times the size).
pub fn encode_jpeg(rgba: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>, Error> {
    let (Ok(w), Ok(h)) = (u16::try_from(width), u16::try_from(height)) else {
        return Err(Error::ImageEncoding(format!("{width}x{height} exceeds JPEG's 65535 px limit")));
    };
    if rgba.len() as u64 != width as u64 * height as u64 * 4 {
        return Err(Error::ImageEncoding(format!("{} bytes is not {width}x{height} RGBA", rgba.len())));
    }
    let rgb: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] as u32;
            [0, 1, 2].map(|i| ((p[i] as u32 * a + 127) / 255) as u8)
        })
        .collect();
    let mut bytes = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut bytes, quality.clamp(1, 100));
    encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::F_2_2);
    encoder
        .encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| Error::ImageEncoding(e.to_string()))?;
    Ok(bytes)
}

pub struct Renderer {
    style: style::Style,
    transform: Transform,
    sources: HashMap<String, Source>,
    sprites: sprite::Sprites,
    /// The style's `sprite`, to pick the sheet again when the pixel ratio changes.
    sprite: J,
    glyphs: symbol::glyphs::Glyphs,
    /// Glyph range URLs returned by `needs()`, with the code points to keep from each, so
    /// `provide()` need not re-evaluate text.
    glyph_urls: std::cell::RefCell<HashMap<String, symbol::glyphs::RangeNeed>>,
    /// The code points of the text by font stack. They depend on source data and camera, not
    /// on downloaded glyphs.
    glyph_dependencies: std::cell::RefCell<Option<symbol::glyphs::Dependencies>>,
    /// Tile URLs returned by `needs()` and not yet provided, with the (source, tile) they fill.
    /// `provide` matches against these, so a view change in between does not lose them.
    requested: std::cell::RefCell<HashMap<String, Vec<(String, TileId)>>>,
    /// Advances with every view change, or set by `set_ticket`; orders tiles for eviction.
    clock: u64,
    view: View,
    /// MapLibre map option `maxZoom` (default 22).
    max_zoom: f64,
    /// MapLibre map option `crossSourceCollisions` (default true).
    cross_source_collisions: bool,
    /// Set by `set_projected_view`: `view` is then the Mercator render of its rasters and DEMs,
    /// warped into it.
    projected: Option<(reproject::ProjectedView, Rc<reproject::Warp>)>,
    /// Whether fill and line buckets are kept for later renders (see `set_keep_buckets`).
    keep_buckets: bool,
    /// Set by `set_export_source`: the source `needs` and `export_tile` are about.
    export: Option<String>,
}

fn tilejson_from(json: &J, overrides: &J) -> TileJson {
    let get = |k: &str| overrides.get(k).filter(|v| !v.is_null()).or_else(|| json.get(k));
    TileJson {
        tiles: get("tiles").and_then(J::as_array).map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default(),
        minzoom: get("minzoom").and_then(J::as_f64).unwrap_or(0.0) as u8,
        maxzoom: get("maxzoom").and_then(J::as_f64).unwrap_or(22.0) as u8,
        tms: get("scheme").and_then(J::as_str) == Some("tms"),
        bounds: get("bounds").and_then(J::as_array).and_then(|b| Some([b.first()?.as_f64()?, b.get(1)?.as_f64()?, b.get(2)?.as_f64()?, b.get(3)?.as_f64()?])),
        // Like MapLibre's `loadTileJSON`, `tileSize` and `encoding` may come from the TileJSON
        // too (the style's source options win).
        tile_size: get("tileSize").and_then(J::as_f64).unwrap_or(512.0),
        encoding: get("encoding").and_then(J::as_str).map(String::from),
    }
}

/// `clusterProperties`: `{name: [reduce operator or expression, map expression]}`.
fn cluster_properties(props: &J) -> supercluster::ClusterProperties {
    let mut out = Vec::new();
    for (key, v) in props.as_object().into_iter().flatten() {
        let (Some(op), Some(map)) = (v.get(0), v.get(1)) else { continue };
        let reduce = match op {
            J::String(op) => serde_json::json!([op, ["accumulated"], ["get", key]]),
            e => e.clone(),
        };
        let parse = |e: &J| expression::create_expression(e, &J::Null, None).ok();
        if let (Some(m), Some(r)) = (parse(map), parse(&reduce)) {
            out.push((key.as_str().into(), m, r));
        }
    }
    out
}

/// The source `filter`: features of a FeatureCollection are kept where it evaluates to true
/// (at zoom 0, against the GeoJSON properties and id).
fn filter_geojson(data: &J, filter: &J) -> Option<J> {
    if data["type"] != "FeatureCollection" || !(filter.is_boolean() || filter.as_array().is_some_and(|a| !a.is_empty())) {
        return None;
    }
    let e = expression::create_expression(filter, &serde_json::json!({"type": "boolean"}), None).ok()?;
    let keep = |f: &J| {
        let properties = match expression::Value::from_json(&f["properties"]) {
            expression::Value::Object(o) => o,
            _ => Default::default(),
        };
        let feature = expression::Feature {
            id: f.get("id").filter(|v| !v.is_null()).map(expression::Value::from_json),
            properties: expression::Properties::Object(properties),
            ..Default::default()
        };
        let mut ctx = expression::EvalCtx { globals: expression::Globals { zoom: 0.0, ..Default::default() }, feature: Some(&feature), ..Default::default() };
        matches!(e.evaluate(&mut ctx), expression::Value::Bool(true))
    };
    let features: Vec<J> = data["features"].as_array().into_iter().flatten().filter(|f| keep(f)).cloned().collect();
    Some(serde_json::json!({"type": "FeatureCollection", "features": features}))
}

fn geojson_index(data: &J, src: &J) -> geojson::GeoJsonIndex {
    let filtered = filter_geojson(data, &src["filter"]);
    let data = filtered.as_ref().unwrap_or(data);
    let scale = geojson::EXTENT / 512.0;
    let maxzoom = src["maxzoom"].as_f64().unwrap_or(18.0);
    let cluster = src["cluster"].as_bool().unwrap_or(false).then(|| supercluster::Options {
        // `clusterMaxZoom` (0 counts as unset), else one below the source maxzoom.
        max_zoom: src["clusterMaxZoom"].as_f64().filter(|&z| z != 0.0).map_or(maxzoom - 1.0, expression::value::js_round).clamp(0.0, 30.0) as u8,
        min_points: src["clusterMinPoints"].as_f64().filter(|&v| v != 0.0).unwrap_or(2.0).max(2.0) as u32,
        radius: src["clusterRadius"].as_f64().filter(|&v| v != 0.0).unwrap_or(50.0) * scale,
        extent: geojson::EXTENT,
        generate_id: src["generateId"].as_bool().unwrap_or(false),
        properties: cluster_properties(&src["clusterProperties"]),
    });
    geojson::GeoJsonIndex::new(
        data,
        geojson::Options {
            cluster,
            maxzoom: maxzoom as u8,
            buffer: src["buffer"].as_f64().unwrap_or(128.0) * scale,
            tolerance: src["tolerance"].as_f64().unwrap_or(0.375) * scale,
            generate_id: src["generateId"].as_bool().unwrap_or(false),
            promote_id: src["promoteId"].as_str().map(String::from),
            line_metrics: src["lineMetrics"].as_bool().unwrap_or(false),
        },
    )
}

/// `getCoordinatesCenterTileID`: the tile an image source is drawn in, and its corners in
/// that tile's units (rounded, like MapLibre).
fn image_tile(coordinates: &[[f64; 2]; 4]) -> (TileId, [[f64; 2]; 4]) {
    let m = coordinates.map(|c| [tile::mercator_x(c[0]), tile::mercator_y(c[1])]);
    let (min_x, max_x) = (m.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min), m.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max));
    let (min_y, max_y) = (m.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min), m.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max));
    let d = (max_x - min_x).max(max_y - min_y);
    let z = (-d.ln() / std::f64::consts::LN_2).floor().max(0.0) as u8;
    let n = (1u64 << z) as f64;
    let id = TileId { z, x: ((min_x + max_x) / 2.0 * n).floor() as u32, y: ((min_y + max_y) / 2.0 * n).floor() as u32 };
    let round = expression::value::js_round;
    let corners = m.map(|p| [round((p[0] * n - id.x as f64) * render::EXTENT), round((p[1] * n - id.y as f64) * render::EXTENT)]);
    (id, corners)
}

/// The image warp for the corners (`calculateImageWarp` with warp 'auto').
fn image_warp(c: &[[f64; 2]; 4]) -> [f64; 3] {
    let [tl, tr, br, bl] = *c;
    if tl[0] + br[0] == tr[0] + bl[0] && tl[1] + br[1] == tr[1] + bl[1] {
        return [0.0, 0.0, 1.0];
    }
    let sum = [tl[0] - tr[0] + br[0] - bl[0], tl[1] - tr[1] + br[1] - bl[1]];
    let (rx, ry, dx, dy) = (tr[0] - br[0], tr[1] - br[1], bl[0] - br[0], bl[1] - br[1]);
    let det = rx * dy - dx * ry;
    let px = (sum[0] * dy - dx * sum[1]) / det;
    let py = (rx * sum[1] - sum[0] * ry) / det;
    let dens = [1.0, 1.0 + px, 1.0 + px + py, 1.0 + py];
    let fore = dens.iter().cloned().fold(f64::NEG_INFINITY, f64::max) / dens.iter().cloned().fold(f64::INFINITY, f64::min);
    let blend = ((1.0 - 4.0 / fore) / (1.0 - 4.0 / 512.0)).max(0.0);
    if !(1.0..=512.0).contains(&fore) || blend >= 1.0 {
        return [0.0, 0.0, 1.0];
    }
    [px, py, blend]
}

/// The union of polygon rings (MVT orientation: outer rings positive in y-down coordinates, holes
/// negative): its polygons, each an outer ring and its holes, closed, in the same orientation.
fn union(rings: Vec<Vec<[f64; 2]>>) -> Vec<Vec<Vec<[f64; 2]>>> {
    use i_overlay::core::fill_rule::FillRule;
    use i_overlay::float::simplify::SimplifyShape;
    let open: Vec<Vec<[f64; 2]>> = rings
        .into_iter()
        .map(|mut r| {
            if r.len() > 1 && r[0] == r[r.len() - 1] {
                r.pop();
            }
            r
        })
        .collect();
    let mut shapes = open.simplify_shape(FillRule::NonZero);
    for r in shapes.iter_mut().flatten() {
        r.push(r[0]);
    }
    shapes
}

impl Renderer {
    /// The style's own `center`/`zoom`/`bearing` are ignored; `view` decides the camera.
    pub fn new(style_json: &str, view: View) -> Result<Self, Error> {
        let mut json: J = serde_json::from_str(style_json).map_err(|e| Error::Style(e.to_string()))?;
        let mut sources = HashMap::default();
        // Moved out of the style, which keeps only what layers need; an inline GeoJSON `data`
        // (which can be large) is dropped once indexed.
        let sources_json = match json.get_mut("sources").map(J::take) {
            Some(J::Object(o)) => o,
            _ => Default::default(),
        };
        for (id, mut src) in sources_json {
            let inline = src.as_object_mut().and_then(|o| o.shift_remove("data")).unwrap_or(J::Null);
            let ty = src["type"].as_str().unwrap_or("").to_string();
            let data = match ty.as_str() {
                "vector" | "raster" | "raster-dem" => match src["url"].as_str() {
                    Some(url) if src.get("tiles").is_none() => Data::Pending(url.to_string()),
                    _ => Data::Tiled(tilejson_from(&src, &J::Null)),
                },
                "geojson" => match inline {
                    J::String(url) => Data::Pending(url),
                    data => Data::GeoJson(geojson_index(&data, &src)),
                },
                "image" => {
                    let c = |i: usize| [src["coordinates"][i][0].as_f64().unwrap_or(0.0), src["coordinates"][i][1].as_f64().unwrap_or(0.0)];
                    Data::Image { url: src["url"].as_str().unwrap_or("").to_string(), coordinates: [c(0), c(1), c(2), c(3)], texture: None }
                }
                _ => Data::Failed,
            };
            sources.insert(id, Source { ty, json: src, data, tiles: HashMap::default(), vector_layers: Default::default(), buckets: Default::default(), last_used: Default::default(), all_layers: false });
        }
        let sprites = sprite::Sprites::new(&json["sprite"], view.pixel_ratio);
        let glyphs = symbol::glyphs::Glyphs::new(json["glyphs"].as_str());
        let sprite = json["sprite"].clone();
        let mut r = Self {
            style: style::Style::parse(json),
            transform: Transform::new([0.0; 2], 0.0, 0.0, 1.0, 1.0, 1.0),
            sources,
            sprites,
            sprite,
            glyphs,
            glyph_urls: Default::default(),
            glyph_dependencies: Default::default(),
            requested: Default::default(),
            clock: 0,
            view,
            max_zoom: 22.0,
            cross_source_collisions: true,
            projected: None,
            keep_buckets: true,
            export: None,
        };
        r.set_max_zoom(22.0);
        Ok(r)
    }

    /// MapLibre's `maxZoom` map option: clamps the camera zoom and bounds tile slicing.
    pub fn set_max_zoom(&mut self, max_zoom: f64) {
        *self.glyph_dependencies.get_mut() = None;
        self.max_zoom = max_zoom;
        let v = self.view;
        let zoom = v.zoom.clamp(tile::MIN_ZOOM, max_zoom);
        self.transform = Transform::new(v.center, zoom, v.bearing, v.width as f64, v.height as f64, v.pixel_ratio);
        self.transform.set_margin(v.margin);
    }

    /// Moves the camera. Fetched resources and the layer buckets built from them are kept (up
    /// to `MAX_CACHED_TILES` per source), so one renderer can draw many views of its style.
    /// Call `needs()` again afterwards. A new pixel ratio refetches the sprite.
    pub fn set_view(&mut self, view: View) {
        self.projected = None;
        if view == self.view {
            return;
        }
        if view.pixel_ratio != self.view.pixel_ratio && !self.sprite.is_null() {
            self.sprites = sprite::Sprites::new(&self.sprite, view.pixel_ratio);
        }
        self.view = view;
        self.clock += 1;
        self.set_max_zoom(self.max_zoom);
    }

    /// For hosts running overlapping renders on one renderer (each one's `set_view`, `needs`,
    /// `provide` rounds interleaved with the others'): tiles needed or provided from now on are
    /// kept for render `ticket`. Give each render an increasing ticket, set it before each of its
    /// calls (after `set_view`, which advances the clock itself), and draw with
    /// `render_retaining` the oldest ticket still in progress, so that no render drops tiles
    /// another one still has to draw (which it would fetch again).
    /// With `false`, a render drops each fill and line bucket once the last layer using it is
    /// drawn, instead of keeping it for later renders: a renderer that draws one view needs less
    /// memory (WASM memory never shrinks, so its peak is what a host must fit).
    pub fn set_keep_buckets(&mut self, keep: bool) {
        self.keep_buckets = keep;
    }

    pub fn set_ticket(&mut self, ticket: u64) {
        self.clock = ticket;
    }

    /// Renders `view`, a box in a projected CRS, instead of a Mercator camera. Layers drawn from
    /// vertices (background, fill, line, circle, heatmap, symbol) are drawn in the projected output
    /// directly; rasters and DEMs are rendered in Mercator (like `set_view`) and the picture is
    /// warped. Call `needs()` again afterwards.
    pub fn set_projected_view(&mut self, view: &reproject::ProjectedView) -> Result<(), Error> {
        if self.projected.as_ref().is_some_and(|(v, _)| v == view) {
            return Ok(());
        }
        let (warp, mercator) = reproject::Warp::new(view)?;
        if let Some(mercator) = mercator {
            self.set_view(mercator);
        }
        self.projected = Some((view.clone(), Rc::new(warp)));
        Ok(())
    }

    /// With a source, `needs()` asks for the data (only) that `export_tile` takes from it; with
    /// `None`, for what `render` draws. A vector source's tiles keep all their layers from then on.
    pub fn set_export_source(&mut self, source: Option<&str>) -> Result<(), Error> {
        if let Some(id) = source {
            let src = self.sources.get_mut(id).filter(|s| matches!(s.ty.as_str(), "vector" | "geojson" | "raster" | "raster-dem" | "image"));
            let src = src.ok_or_else(|| Error::Unsupported(format!("no source {id:?} to export")))?;
            if src.ty == "vector" && !src.all_layers {
                // Decoded with the style's layers only.
                src.all_layers = true;
                src.tiles.clear();
                src.vector_layers.get_mut().clear();
                *self.glyph_dependencies.get_mut() = None;
            }
        }
        self.export = source.map(String::from);
        Ok(())
    }

    /// The export source (`set_export_source`) in the projected view, as the source type of the same
    /// data on the output's grid takes it:
    /// - vector / GeoJSON: a vector tile (MVT, extent 4096) of its layers as they are (a GeoJSON
    ///   source's named after it), projected;
    /// - raster / image: a PNG of the output's size, the source drawn by a raster layer with default
    ///   paint;
    /// - raster-dem: a PNG of the output's size, its elevations (interpolated bilinearly) in the
    ///   `terrarium` encoding.
    ///
    /// Keeps the cached tiles of tickets `oldest` and later, like `render_retaining`.
    pub fn export_tile(&mut self, oldest: u64) -> Result<Vec<u8>, Error> {
        let Some(id) = self.export.clone() else { return Err(Error::Unsupported("no export source set".into())) };
        let Some((_, warp)) = self.projected.clone() else { return Err(Error::Unsupported("export needs a projected view".into())) };
        if !self.needs().is_empty() {
            return Err(Error::NotReady);
        }
        self.evict(oldest.min(self.clock));
        let src = &self.sources[&id];
        let (w, h) = (warp.size[0] - 2 * warp.margin, warp.size[1] - 2 * warp.margin);
        match src.ty.as_str() {
            "vector" | "geojson" if warp.is_empty() => Ok(vec![]),
            "vector" | "geojson" => Ok(self.export_vector(&id, src, &warp)),
            _ if warp.is_empty() => encode_png(&vec![0; w * h * 4], w as u32, h as u32),
            "raster-dem" => {
                let rgba = crop(self.export_dem(src, &warp), warp.size[0], warp.margin);
                encode_png(&rgba, w as u32, h as u32)
            }
            _ => {
                // The Mercator picture of the source, warped.
                let layer = style::Style::parse(serde_json::json!({"layers": [{"id": id, "type": "raster", "source": id}]})).layers.remove(0);
                let t = &self.transform;
                let (mw, mh) = ((t.width * t.pixel_ratio).round() as usize, (t.height * t.pixel_ratio).round() as usize);
                let mut picture = render::raster::Target::new(mw, mh);
                self.draw_raster(&mut picture, &layer, &self.ctx(), src, mw, mh);
                let mut out = render::raster::Target::new(warp.size[0], warp.size[1]);
                warp.composite(&mut picture, t, &mut out);
                encode_png(&crop(out.into_unpremultiplied(), warp.size[0], warp.margin), w as u32, h as u32)
            }
        }
    }

    /// Terrarium-encoded RGBA of the raster-dem source `src` at each pixel of `warp` (margins
    /// included); elevation 0 off the map.
    fn export_dem(&self, src: &Source, warp: &reproject::Warp) -> Vec<u8> {
        let resolved = self.resolve(src).0;
        let dems = self.backfilled_dems(src, &resolved);
        // Highest zoom first, for the fallback parents of missing tiles.
        let mut order: Vec<usize> = (0..resolved.len()).filter(|&i| dems[i].is_some()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(resolved[i].0.canonical.z));
        let mut rgba = Vec::with_capacity(warp.size[0] * warp.size[1] * 4);
        let mut last = 0;
        for m in warp.pixel_world() {
            let mut elevation = 0.0;
            if m[0].is_finite() {
                // Usually the tile of the previous pixel.
                let found = std::iter::once(last).chain(0..order.len()).filter(|&k| k < order.len()).find_map(|k| {
                    let t = resolved[order[k]].0;
                    let n = (1u64 << t.canonical.z) as f64;
                    let (x, y) = ((m[0] * n - t.wrap as f64 * n - t.canonical.x as f64), (m[1] * n - t.canonical.y as f64));
                    ((0.0..1.0).contains(&x) && (0.0..1.0).contains(&y)).then_some((k, x, y))
                });
                if let Some((k, x, y)) = found {
                    last = k;
                    let dem = dems[order[k]].as_ref().unwrap();
                    let d = dem.dim() as f64;
                    elevation = dem.elevation_at(x * d, y * d);
                }
            }
            // Terrarium: (r * 256 + g + b / 256) - 32768.
            let v = (elevation + 32768.0).clamp(0.0, 65535.0 + 255.0 / 256.0);
            rgba.extend_from_slice(&[(v / 256.0).floor() as u8, (v % 256.0).floor() as u8, ((v.fract()) * 256.0).floor() as u8, 255]);
        }
        rgba
    }

    /// The vector tile of a vector or GeoJSON source `src` (see `export_tile`).
    fn export_vector(&self, id: &str, src: &Source, warp: &reproject::Warp) -> Vec<u8> {
        const EXTENT: f64 = 4096.0;
        const BUFFER: f64 = 128.0;
        let id = id.to_string();
        let mut layers: Vec<mvt::Layer> = Vec::new();
        // The pieces of polygons the source tiles cut, by layer and properties (ids need not be the
        // same in each tile), with the longest segment of their tiles and their id if they share it.
        let mut cut: hash::IndexMap<(String, String), (mvt::Feature, Vec<Vec<[f64; 2]>>, f64, Option<J>)> = Default::default();
        for (rt, tl, _) in self.tile_layers(src) {
            let n = (1u64 << rt.canonical.z) as f64;
            let (ox, oy) = (rt.wrap as f64 * n + rt.canonical.x as f64, rt.canonical.y as f64);
            for l in tl.iter().flat_map(|ls| ls.iter()) {
                let name = if src.ty == "geojson" { &id } else { &l.name };
                let e = l.extent as f64;
                let to_world = |g: &[Vec<[f64; 2]>]| -> Vec<Vec<[f64; 2]>> { g.iter().map(|r| r.iter().map(|p| [(ox + p[0] / e) / n, (oy + p[1] / e) / n]).collect()).collect() };
                let index = match layers.iter().position(|o| &o.name == name) {
                    Some(i) => i,
                    None => {
                        layers.push(mvt::Layer { name: name.clone(), extent: EXTENT as u32, features: vec![] });
                        layers.len() - 1
                    }
                };
                for f in &l.features {
                    let geom: Vec<Vec<[f64; 2]>> = f.geometry.iter().map(|r| r.iter().map(|p| [p[0] as f64, p[1] as f64]).collect()).collect();
                    if f.ty == mvt::GeomType::Polygon {
                        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
                        for p in geom.iter().flatten() {
                            (lo, hi) = ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]);
                        }
                        // In the buffer only: the neighbor the polygon is in draws it.
                        if hi[0] <= 0.0 || hi[1] <= 0.0 || lo[0] >= e || lo[1] >= e {
                            continue;
                        }
                        // Cut by the tile's edge: merged below with its other pieces, so that the
                        // cuts add no edges.
                        if lo[0] < 0.0 || lo[1] < 0.0 || hi[0] > e || hi[1] > e {
                            let props = f.properties.to_object().iter().map(|(k, v)| (k.to_string(), v.to_json())).collect::<serde_json::Map<_, _>>();
                            let id = f.id.as_ref().map(|v| v.to_json());
                            let entry = cut.entry((name.clone(), J::Object(props).to_string())).or_insert_with(|| (mvt::Feature { geometry: vec![], ..f.clone() }, vec![], 0.0, id.clone()));
                            entry.1.extend(to_world(&geom));
                            entry.2 = f64::max(entry.2, 1.0 / n / 32.0);
                            if entry.3 != id {
                                entry.3 = None;
                            }
                            continue;
                        }
                        // Whole, unclipped.
                        let geometry = warp.project(f.ty, to_world(&geom), 1.0 / n / 32.0, EXTENT, BUFFER);
                        if !geometry.is_empty() {
                            layers[index].features.push(mvt::Feature { geometry, ..f.clone() });
                        }
                        continue;
                    }
                    // Clipped to the tile, so that neighbors do not overlap.
                    let geom = mvt::clip_axis(&mvt::clip_axis(&geom, f.ty, 0.0, e, 0), f.ty, 0.0, e, 1);
                    let geometry = warp.project(f.ty, to_world(&geom), 1.0 / n / 32.0, EXTENT, BUFFER);
                    if !geometry.is_empty() {
                        layers[index].features.push(mvt::Feature { geometry, ..f.clone() });
                    }
                }
            }
        }
        for ((name, _), (feature, rings, segment, id)) in cut {
            let layer = layers.iter_mut().find(|l| l.name == name).unwrap();
            // A feature for each polygon of the union, so that separate features stay apart.
            for shape in union(rings) {
                let geometry = warp.project(mvt::GeomType::Polygon, shape, segment, EXTENT, BUFFER);
                if !geometry.is_empty() {
                    let id = id.as_ref().and_then(J::as_f64).map(expression::Value::Number);
                    layer.features.push(mvt::Feature { geometry, id, ..feature.clone() });
                }
            }
        }
        mvt::encode(&layers)
    }

    /// Drops the least recently needed tiles of each source beyond `MAX_CACHED_TILES`, with
    /// everything built from them. Tiles needed at `keep_since` or later stay.
    fn evict(&mut self, keep_since: u64) {
        for src in self.sources.values_mut() {
            let last_used = src.last_used.get_mut();
            let Some(excess) = last_used.len().checked_sub(MAX_CACHED_TILES).filter(|&n| n > 0) else { continue };
            let mut old: Vec<(u64, TileId)> = last_used.iter().filter(|&(_, &t)| t < keep_since).map(|(&id, &t)| (t, id)).collect();
            old.sort_unstable();
            let evicted: hash::HashSet<TileId> = old.into_iter().take(excess).map(|(_, id)| id).collect();
            last_used.retain(|id, _| !evicted.contains(id));
            src.tiles.retain(|id, _| !evicted.contains(id));
            src.vector_layers.get_mut().retain(|(data, _), _| !evicted.contains(data));
            src.buckets.get_mut().retain(|(data, _, _), _| !evicted.contains(data));
        }
    }

    /// MapLibre's `crossSourceCollisions` map option: when off, symbols only collide with
    /// symbols of the same source.
    pub fn set_cross_source_collisions(&mut self, enabled: bool) {
        self.cross_source_collisions = enabled;
    }

    /// Sources referenced by layers visible at the current zoom.
    fn used_sources(&self) -> BTreeSet<&str> {
        if let Some(id) = &self.export {
            return BTreeSet::from([id.as_str()]);
        }
        self.style
            .layers
            .iter()
            .filter(|l| !l.is_hidden(self.transform.zoom, &self.style.global_state))
            .filter_map(|l| l.source.as_deref())
            .collect()
    }

    /// Ideal tiles of a tiled source for the current view (MapLibre `coveringTiles`).
    fn covering(&self, src: &Source) -> Vec<RenderTile> {
        let (tile_size, minzoom, maxzoom, bounds) = match &src.data {
            Data::Tiled(tj) => (tj.tile_size, tj.minzoom, tj.maxzoom, tj.bounds),
            Data::GeoJson(g) => (512.0, 0, g.maxzoom(), None),
            _ => return vec![],
        };
        let raster = matches!(src.ty.as_str(), "raster" | "raster-dem");
        let z = self.transform.covering_zoom(tile_size, raster);
        if z < minzoom as i32 {
            return vec![];
        }
        // MapLibre's default `zoomLevelsToOverscale` (4) below the map maxZoom (22): vector
        // tiles up to that zoom are sliced out of the source's maxzoom tile.
        let maxzoom = if src.ty == "vector" { maxzoom.max((self.max_zoom as i32 - ZOOM_LEVELS_TO_OVERSCALE as i32).max(0) as u8) } else { maxzoom };
        let mut out = BTreeSet::new();
        for (x, y, wrap) in self.transform.covering_tiles(z) {
            let id = TileId { z: z as u8, x: x as u32, y: y as u32 };
            let canonical = if z > maxzoom as i32 { id.parent_at(maxzoom) } else { id };
            if let Some(b) = bounds {
                let n = (1u64 << canonical.z) as f64;
                let (x0, x1) = (canonical.x as f64 / n, (canonical.x + 1) as f64 / n);
                let (y0, y1) = (canonical.y as f64 / n, (canonical.y + 1) as f64 / n);
                let (bx0, bx1) = (tile::mercator_x(b[0]), tile::mercator_x(b[2]));
                let (by0, by1) = (tile::mercator_y(b[3].min(tile::MAX_LAT)), tile::mercator_y(b[1].max(-tile::MAX_LAT)));
                if x1 <= bx0 || x0 >= bx1 || y1 <= by0 || y0 >= by1 {
                    continue;
                }
            }
            let tile = RenderTile { overscaled_z: z as u8, wrap, canonical };
            // In a projected view, also the world copies that land on the map across the
            // projection's antimeridian (drawn geometry is unwrapped around its central meridian).
            // Not for rasters and DEMs, drawn in the Mercator picture.
            if let Some((_, warp)) = self.projected.as_ref().filter(|_| !raster) {
                let n = (1u64 << canonical.z) as f64;
                let x0 = (wrap as f64 * n + canonical.x as f64) / n;
                for k in warp.branch_shifts(x0, x0 + 1.0 / n) {
                    out.insert(RenderTile { wrap: wrap + k, ..tile });
                }
            }
            out.insert(tile);
        }
        out.into_iter().collect()
    }

    /// The tile actually fetched for `id`: its ancestor at the source maxzoom when sliced.
    fn data_tile(tj: &TileJson, id: TileId) -> TileId {
        if id.z > tj.maxzoom { id.parent_at(tj.maxzoom) } else { id }
    }

    /// Tiles to draw: each ideal tile, or for one that failed to load, its nearest loaded
    /// ancestor (MapLibre retains parents for missing tiles). Also returns tiles still to fetch.
    fn resolve(&self, src: &Source) -> (Vec<(RenderTile, TileId)>, BTreeSet<TileId>) {
        let mut draw = BTreeSet::new();
        let mut fetch = BTreeSet::new();
        let mut last_used = src.last_used.borrow_mut();
        let Data::Tiled(tj) = &src.data else {
            let covering = self.covering(src);
            if matches!(src.data, Data::GeoJson(_)) {
                for t in &covering {
                    stamp(&mut last_used, t.canonical, self.clock);
                }
            }
            return (covering.into_iter().map(|t| (t, t.canonical)).collect(), fetch);
        };
        // `TileManager.maxUnderzooming`: parents are only sought this far below the ideal zoom.
        let covering = self.covering(src);
        let min_z = covering.first().map_or(0, |t| (t.overscaled_z as i32 - 10).max(tj.minzoom as i32)) as u8;
        for t in covering {
            let mut id = Self::data_tile(tj, t.canonical);
            loop {
                match src.tiles.get(&id) {
                    None => {
                        fetch.insert(id);
                        break;
                    }
                    Some(Some(_)) => {
                        stamp(&mut last_used, id, self.clock);
                        let canonical = if id == Self::data_tile(tj, t.canonical) { t.canonical } else { id };
                        let overscaled_z = if canonical == t.canonical { t.overscaled_z } else { canonical.z };
                        draw.insert((RenderTile { overscaled_z, wrap: t.wrap, canonical }, id));
                        break;
                    }
                    Some(None) => {
                        stamp(&mut last_used, id, self.clock);
                        if id.z <= min_z {
                            break;
                        }
                        id = id.parent_at(id.z - 1);
                    }
                }
            }
        }
        (draw.into_iter().collect(), fetch)
    }

    /// Resources still required before `render` can run. May grow after `provide`
    /// (tiles → glyph ranges → ...), so call it again after every round of `provide`.
    pub fn needs(&self) -> Vec<Resource> {
        if self.projected.as_ref().is_some_and(|(_, w)| w.is_empty()) {
            return vec![];
        }
        let mut out: BTreeSet<String> = match self.export {
            Some(_) => BTreeSet::new(),
            None => self.sprites.needs().into_iter().collect(),
        };
        for id in self.used_sources() {
            let Some(src) = self.sources.get(id) else { continue };
            match &src.data {
                Data::Pending(url) => {
                    out.insert(url.clone());
                }
                Data::Tiled(tj) if !tj.tiles.is_empty() => {
                    for t in self.resolve(src).1 {
                        let url = tile::tile_url(&tj.tiles, t, tj.tms);
                        let mut requested = self.requested.borrow_mut();
                        let targets = requested.entry(url.clone()).or_default();
                        if !targets.iter().any(|(s, u)| s == id && *u == t) {
                            targets.push((id.to_string(), t));
                        }
                        out.insert(url);
                    }
                }
                Data::Image { url, texture: None, .. } => {
                    out.insert(url.clone());
                }
                _ => {}
            }
        }
        let glyphs = if self.export.is_some() { vec![] } else { self.glyph_needs() };
        out.extend(glyphs.iter().map(|(url, _)| url.clone()));
        let mut glyph_urls = self.glyph_urls.borrow_mut();
        for (url, (key, cps)) in glyphs {
            glyph_urls.entry(url).or_insert_with(|| (key, BTreeSet::new())).1.extend(cps);
        }
        out.into_iter().map(|url| Resource { url }).collect()
    }

    fn ctx(&self) -> render::Ctx<'_> {
        render::Ctx::new(&self.transform, &self.style.global_state, &self.sprites.images)
    }

    /// Glyph ranges still to fetch for the text of the loaded tiles. Text is only known once
    /// its tiles are decoded, so this grows as tiles arrive.
    fn glyph_needs(&self) -> Vec<(String, symbol::glyphs::RangeNeed)> {
        if self.glyphs.url.is_none() {
            return vec![];
        }
        if let Some(deps) = self.glyph_dependencies.borrow().as_ref() {
            return self.glyphs.needs(deps);
        }
        let ctx = self.ctx();
        let mut deps = symbol::glyphs::Dependencies::new();
        let no_mask = render::raster::Mask::empty();
        for layer in &self.style.layers {
            if layer.ty != "symbol" || layer.is_hidden(self.transform.zoom, &self.style.global_state) {
                continue;
            }
            let Some(src) = layer.source.as_deref().and_then(|s| self.sources.get(s)) else { continue };
            let raw = self.style.layer_json(&layer.id);
            for (rt, tl, _) in self.tile_layers(src) {
                let Some(l) = self.source_layer(src, layer, tl.as_deref()) else { continue };
                let tf = render::TileFeatures { tile: rt, mask: &no_mask, layer: l, buckets: None };
                symbol::layout::glyph_dependencies(layer, &ctx, &tf, raw, &mut deps);
            }
        }
        let needs = self.glyphs.needs(&deps);
        *self.glyph_dependencies.borrow_mut() = Some(deps);
        needs
    }

    /// Resolved tiles of a vector/GeoJSON source with their decoded layers (sliced when
    /// overzoomed).
    /// Each also gets its bucket cache.
    /// Calls `f` with the source's resolved tiles: their clip masks and `layer`'s features.
    fn with_tiles<R>(&self, ctx: &render::Ctx, src: &Source, layer: &style::Layer, w: usize, h: usize, f: impl FnOnce(&[render::TileFeatures]) -> R) -> R {
        let empty = mvt::Layer { name: String::new(), extent: 4096, features: vec![] };
        let layers_by_tile = self.tile_layers(src);
        let render_tiles: Vec<RenderTile> = layers_by_tile.iter().map(|r| r.0).collect();
        let masks = ctx.masks(&render_tiles, w, h);
        let tiles: Vec<render::TileFeatures> = layers_by_tile
            .iter()
            .zip(masks.iter())
            .map(|((rt, tl, buckets), mask)| render::TileFeatures { tile: *rt, mask, layer: self.source_layer(src, layer, tl.as_deref()).unwrap_or(&empty), buckets: Some(buckets) })
            .collect();
        f(&tiles)
    }

    /// `*-layer-opacity`, if the layer type has one.
    fn layer_opacity(&self, layer: &style::Layer) -> Option<f64> {
        let p = layer.paint.get(&format!("{}-layer-opacity", layer.ty))?;
        Some(render::num(&p.eval(self.transform.zoom, None, None, &self.style.global_state)))
    }

    fn tile_layers(&self, src: &Source) -> Vec<(RenderTile, Option<Rc<Vec<mvt::Layer>>>, Rc<render::TileBuckets>)> {
        self.resolve(src)
            .0
            .into_iter()
            .map(|(rt, data_id)| {
                let layers = match &src.data {
                    Data::GeoJson(g) => Some(Rc::clone(
                        src.vector_layers.borrow_mut().entry((rt.canonical, rt.canonical))
                            .or_insert_with(|| Rc::new(vec![g.tile(rt.canonical.z, rt.canonical.x, rt.canonical.y)])),
                    )),
                    Data::Tiled(_) => match src.tiles.get(&data_id).cloned().flatten() {
                        Some(TileData::Vector(ls)) if data_id == rt.canonical => Some(ls),
                        Some(TileData::Vector(ls)) => Some(Rc::clone(
                            src.vector_layers.borrow_mut().entry((data_id, rt.canonical))
                                .or_insert_with(|| Rc::new(ls.iter().map(|l| mvt::slice_layer(l, data_id, rt.canonical)).collect())),
                        )),
                        _ => None,
                    },
                    _ => None,
                };
                let buckets = Rc::clone(src.buckets.borrow_mut().entry((data_id, rt.canonical, rt.overscaled_z)).or_default());
                (rt, layers, buckets)
            })
            .collect()
    }

    /// The style layer's source layer within a tile.
    fn source_layer<'a>(&self, src: &Source, layer: &style::Layer, layers: Option<&'a Vec<mvt::Layer>>) -> Option<&'a mvt::Layer> {
        let ls = layers?;
        match &src.data {
            Data::GeoJson(_) => ls.first(),
            _ => ls.iter().find(|l| Some(l.name.as_str()) == layer.source_layer.as_deref()),
        }
    }

    /// Hand over fetched bytes for `url` (as returned by `needs()`). `None` means the host could
    /// not fetch it (e.g. 404): a vector tile is then empty, a raster tile is replaced by an
    /// ancestor, a glyph range leaves its glyphs missing.
    pub fn provide(&mut self, url: &str, bytes: Option<&[u8]>) {
        let image_count = self.sprites.images.len();
        self.sprites.provide(url, bytes);
        let mut changed = self.sprites.images.len() != image_count;
        // Glyph ranges are recognized from the last `needs()` (evaluating text is costly).
        let glyph_key = self.glyph_urls.get_mut().remove(url);
        if let Some((key, wanted)) = glyph_key {
            self.glyphs.provide(key, &wanted, bytes);
        }
        let requested = self.requested.get_mut().remove(url).unwrap_or_default();
        for (id, src) in self.sources.iter_mut() {
            match &mut src.data {
                Data::Pending(u) if u == url => {
                    changed = true;
                    let doc = bytes.and_then(|b| serde_json::from_slice::<J>(b).ok());
                    src.data = match (doc, src.ty.as_str()) {
                        (Some(doc), "geojson") => Data::GeoJson(geojson_index(&doc, &src.json)),
                        (Some(doc), _) => Data::Tiled(tilejson_from(&doc, &src.json)),
                        (None, _) => Data::Failed,
                    };
                }
                Data::Tiled(tj) => {
                    for &(_, t) in requested.iter().filter(|(s, _)| s == id) {
                        if src.tiles.contains_key(&t) {
                            continue;
                        }
                        changed = true;
                        // Like MapLibre: a missing vector tile is just empty; a missing raster
                        // tile lets a parent show through, while an empty raster response
                        // (HTTP 204) is a transparent tile.
                        // A DEM tile that fails to decode also lets a parent show through.
                        let raster = src.ty == "raster" || src.ty == "raster-dem";
                        let decoded = match (src.ty.as_str(), bytes) {
                            (_, Some([])) if raster => Some(TileData::Raster(Rc::new(image::Texture::new(&image::Image { w: 1, h: 1, data: vec![0; 4] }, true)))),
                            ("raster", Some(b)) => image::Image::decode(b).map(|i| TileData::Raster(Rc::new(image::Texture::new(&i, true)))),
                            ("raster-dem", Some(b)) => image::Image::decode(b).and_then(|i| dem_from(&i, &src.json, tj.encoding.as_deref())).map(|d| TileData::Dem(Rc::new(d))),
                            _ if raster => None,
                            _ => {
                                let all = src.all_layers;
                                let used = |name: &[u8]| {
                                    let name = String::from_utf8_lossy(name);
                                    all || self.style.layers.iter().any(|l| l.source.as_deref() == Some(id) && l.source_layer.as_deref() == Some(&*name))
                                };
                                Some(TileData::Vector(Rc::new(bytes.and_then(|b| mvt::decode(b, used)).unwrap_or_default())))
                            }
                        };
                        src.tiles.insert(t, decoded);
                        stamp(src.last_used.get_mut(), t, self.clock);
                    }
                }
                Data::Image { url: u, texture, .. } if u == url && texture.is_none() => {
                    changed = true;
                    // Image sources are uploaded without mipmaps.
                    *texture = Some(bytes.and_then(image::Image::decode).map(|i| Rc::new(image::Texture::new(&i, false))));
                }
                _ => {}
            }
        }
        if changed {
            *self.glyph_dependencies.get_mut() = None;
        }
    }

    /// Non-premultiplied RGBA8, row-major.
    pub fn render(&mut self) -> Result<Vec<u8>, Error> {
        self.render_retaining(self.clock)
    }

    /// `render`, keeping the cached tiles needed by tickets `oldest` and later (see `set_ticket`).
    pub fn render_retaining(&mut self, oldest: u64) -> Result<Vec<u8>, Error> {
        if !self.needs().is_empty() {
            return Err(Error::NotReady);
        }
        if let Some((_, w)) = self.projected.as_ref().filter(|(_, w)| w.is_empty()) {
            return Ok(vec![
                0;
                (w.size[0] - 2 * w.margin)
                    * (w.size[1] - 2 * w.margin)
                    * 4
            ]);
        }
        self.evict(oldest.min(self.clock));
        let t = &self.transform;
        let (w, h) = ((t.width * t.pixel_ratio).round() as usize, (t.height * t.pixel_ratio).round() as usize);
        let visible: Vec<bool> = self.style.layers.iter().map(|l| !l.is_hidden(t.zoom, &self.style.global_state)).collect();
        // In a projected view the Mercator picture (rasters and DEMs) is warped into `out` before
        // each layer drawn directly (and at the end); those layers are drawn with `out_ctx`.
        let warp = self.projected.as_ref().map(|(_, w)| Rc::clone(w));
        let direct = |l: &style::Layer| warp.is_some() && matches!(l.ty.as_str(), "symbol" | "fill" | "line" | "circle" | "heatmap" | "background");
        // The Mercator picture, unless a projected view has no layer drawn in it.
        let mercator_drawn = warp.is_none() || self.style.layers.iter().zip(&visible).any(|(l, v)| *v && !direct(l));
        let mut target = if mercator_drawn { render::raster::Target::new(w, h) } else { render::raster::Target::new(0, 0) };
        let ctx = self.ctx();
        // Triangles skip the margin, which is cropped. Not the Mercator picture of a projected
        // view: the warp reads it around each output pixel.
        let inner = |t: &mut render::raster::Target, m: usize| t.clip = [m, m, t.w.saturating_sub(m), t.h.saturating_sub(m)];
        if warp.is_none() {
            inner(&mut target, (self.view.margin as f64 * t.pixel_ratio).round() as usize);
        }
        let mut out = warp.as_ref().map(|w| {
            let mut o = render::raster::Target::new(w.size[0], w.size[1]);
            inner(&mut o, w.margin);
            o
        });
        let out_transform = warp.as_ref().map(|w| t.reprojected(Rc::clone(w)));
        let out_ctx = out_transform.as_ref().map(|ot| render::Ctx::new(ot, &self.style.global_state, &self.sprites.images));
        let out_ctx = out_ctx.as_ref().unwrap_or(&ctx);
        let (symbol_layers, leader_of, placements) = self.place_symbols(out_ctx);
        // Heatmaps drawn so far: MapLibre draws them without depth test, so they stay on top of
        // the opaque-pass pixels (opaque backgrounds and fill interiors) of layers above them.
        let mut heatmaps: Vec<Vec<render::raster::Rgba>> = Vec::new();
        // Opaque pass, top-down (see `render::Pass`). Not for a projected view: the warp blends
        // neighboring pixels, so occluded ones must still be drawn.
        let opaque_drawn = match warp {
            None => self.opaque_pass(&mut target, &ctx, &visible),
            Some(_) => vec![false; self.style.layers.len()],
        };
        // In a projected view, layers drawn from vertices go straight into `out` (through `out_ctx`).
        // Rasters and DEMs are drawn in Mercator into `target`, which is warped into
        // `out` before the next layer drawn directly, and at the end.
        let mut warped_pending = false;
        // Without `keep_buckets`: the last layer drawing each (source, layout group)'s buckets.
        let mut last_bucket_use: HashMap<(&str, &str), usize> = HashMap::default();
        if !self.keep_buckets {
            for (i, layer) in self.style.layers.iter().enumerate() {
                if visible[i] && matches!(layer.ty.as_str(), "fill" | "line") && let Some(src) = layer.source.as_deref() {
                    last_bucket_use.insert((src, layer.layout_group.as_str()), i);
                }
            }
        }
        for (i, layer) in self.style.layers.iter().enumerate() {
            if !visible[i] {
                continue;
            }
            let raw = self.style.layer_json(&layer.id);
            let direct = direct(layer);
            if let (Some(wp), Some(o), true) = (&warp, &mut out, direct && warped_pending) {
                wp.composite(&mut target, t, o);
                warped_pending = false;
            }
            warped_pending |= warp.is_some() && !direct;
            let (main, lctx) = match &mut out {
                Some(o) if direct => (o, out_ctx),
                _ => (&mut target, &ctx),
            };
            let (lw, lh) = (main.w, main.h);
            main.layer = i as u16 + 1;
            if !heatmaps.is_empty() {
                render::heatmap::reapply(main, &heatmaps);
                main.opaque = vec![false; lw * lh];
            }
            if layer.ty == "background" {
                if !opaque_drawn[i] {
                    render::draw_background(main, layer, lctx, raw, render::Pass::All);
                }
                continue;
            }
            if layer.ty == "symbol" {
                if let Some(&i) = leader_of.get(layer.id.as_str()) {
                    symbol::draw::draw_symbol_layer(main, layer, lctx, raw, &symbol_layers[i].2, &placements[i]);
                }
                continue;
            }
            if layer.ty == "hillshade" || layer.ty == "color-relief" {
                if let Some(src) = layer.source.as_deref().and_then(|s| self.sources.get(s)) {
                    self.draw_dem_layer(main, layer, lctx, src, lw, lh);
                }
                continue;
            }
            if !matches!(layer.ty.as_str(), "fill" | "line" | "circle" | "raster" | "heatmap") {
                continue;
            }
            let Some(src) = layer.source.as_deref().and_then(|s| self.sources.get(s)) else { continue };
            if layer.ty == "raster" {
                self.draw_raster(main, layer, lctx, src, lw, lh);
                continue;
            }
            // `*-layer-opacity`: the layer is drawn offscreen, then composited.
            let layer_opacity = self.layer_opacity(layer);
            if layer_opacity == Some(0.0) {
                continue;
            }
            let clip = main.clip;
            let mut offscreen = layer_opacity.filter(|o| *o < 1.0).map(|_| render::raster::Target { clip, ..render::raster::Target::new(lw, lh) });
            let dest = offscreen.as_mut().unwrap_or(&mut *main);
            self.with_tiles(lctx, src, layer, lw, lh, |tiles| match layer.ty.as_str() {
                "fill" => {
                    render::draw_fill(dest, layer, lctx, tiles, raw, if opaque_drawn[i] { render::Pass::Translucent } else { render::Pass::All });
                }
                "line" => {
                    let source_maxzoom = match &src.data {
                        Data::Tiled(tj) => tj.maxzoom,
                        Data::GeoJson(g) => g.maxzoom(),
                        _ => 22,
                    };
                    let has_gradient = raw["paint"].get("line-gradient").is_some();
                    let has_pattern = raw["paint"].get("line-pattern").is_some();
                    render::draw_line(dest, layer, lctx, tiles, source_maxzoom, self.max_zoom, has_gradient, has_pattern)
                }
                "circle" => render::draw_circle(dest, layer, lctx, tiles),
                "heatmap" => {
                    let colors = render::heatmap::draw_heatmap(dest, layer, lctx, tiles);
                    render::heatmap::blend(dest, &colors, None);
                    heatmaps.push(colors);
                }
                _ => {}
            });
            if let Some(&last) = last_bucket_use.get(&(layer.source.as_deref().unwrap_or(""), layer.layout_group.as_str()))
                && last == i
            {
                let projected = format!("{}\0projected", layer.layout_group);
                for buckets in src.buckets.borrow().values() {
                    let mut buckets = buckets.borrow_mut();
                    buckets.remove(&layer.layout_group);
                    buckets.remove(&projected);
                }
            }
            if let (Some(off), Some(o)) = (offscreen, layer_opacity) {
                for (i, p) in off.px.iter().enumerate() {
                    if (p[3] != 0 || p[0] != 0 || p[1] != 0 || p[2] != 0) && main.visible(i) {
                        main.blend(i, p.map(|c| c as f32 / 255.0 * o as f32));
                    }
                }
            }
        }
        match (&warp, &mut out) {
            (Some(wp), Some(o)) => {
                if warped_pending {
                    wp.composite(&mut target, t, o);
                }
                render::heatmap::reapply(o, &heatmaps);
            }
            _ => render::heatmap::reapply(&mut target, &heatmaps),
        }
        let (rgba, w, margin) = match (warp, out) {
            (Some(warp), Some(out)) => (out.into_unpremultiplied(), warp.size[0], warp.margin),
            _ => (
                target.into_unpremultiplied(),
                w,
                (self.view.margin as f64 * t.pixel_ratio).round() as usize,
            ),
        };
        Ok(crop(rgba, w, margin))
    }

    /// Lays out (or reuses) the symbol buckets of all symbol layers and places them, from the top
    /// layer down. Layers with identical layout share their leader's buckets.
    #[allow(clippy::type_complexity)]
    fn place_symbols<'a>(
        &'a self,
        ctx: &render::Ctx,
    ) -> (
        Vec<(&'a style::Layer, &'a serde_json::Value, Vec<Rc<symbol::layout::SymbolBucket>>)>,
        HashMap<&'a str, usize>,
        Vec<Vec<Vec<symbol::placement::InstancePlacement>>>,
    ) {
        let t = ctx.transform;
        let empty = mvt::Layer { name: String::new(), extent: 4096, features: vec![] };
        // Symbols are laid out and placed for all layers before drawing (placement runs from
        // the top layer down).
        let no_mask = render::raster::Mask::empty();
        // Layers with identical layout share one bucket (`groupByLayout`): it is laid out and
        // placed once, for the first layer, and drawn by each with its own paint.
        let mut symbol_layers: Vec<(&style::Layer, &serde_json::Value, Vec<Rc<symbol::layout::SymbolBucket>>)> = Vec::new();
        let mut leader_of: HashMap<&str, usize> = HashMap::default();
        for layer in &self.style.layers {
            if layer.ty != "symbol" || layer.is_hidden(t.zoom, &self.style.global_state) {
                continue;
            }
            let Some(src) = layer.source.as_deref().and_then(|s| self.sources.get(s)) else { continue };
            let raw = self.style.layer_json(&layer.id);
            const REF: [&str; 7] = ["type", "source", "source-layer", "minzoom", "maxzoom", "filter", "layout"];
            if let Some(i) = symbol_layers.iter().position(|(_, r, _)| REF.iter().all(|k| r.get(*k) == raw.get(*k))) {
                leader_of.insert(&layer.id, i);
                continue;
            }
            let builder = symbol::layout::Builder { layer, ctx, glyphs: &self.glyphs, raw };
            let mut buckets = Vec::new();
            for (rt, tl, tile_buckets) in self.tile_layers(src) {
                let l = self.source_layer(src, layer, tl.as_deref()).unwrap_or(&empty);
                let tf = render::TileFeatures { tile: rt, mask: &no_mask, layer: l, buckets: Some(&tile_buckets) };
                // The layout is camera-independent except for the world copy (in its tile) and the
                // pixel ratio (icon scaling and the sprite); placement is redone each render.
                let key = format!("{}\0{}\0{}", layer.layout_group, rt.wrap, t.pixel_ratio);
                buckets.push(tf.bucket_keyed(&key, || builder.build(&tf)));
            }
            leader_of.insert(&layer.id, symbol_layers.len());
            symbol_layers.push((layer, raw, buckets));
        }
        let mut groups: Vec<&str> = Vec::new();
        let placement_input: Vec<symbol::placement::LayerBuckets> = symbol_layers
            .iter()
            .map(|(layer, raw, buckets)| {
                let mut group = 0;
                if !self.cross_source_collisions {
                    let src = layer.source.as_deref().unwrap_or("");
                    group = groups.iter().position(|g| *g == src).unwrap_or_else(|| {
                        groups.push(src);
                        groups.len() - 1
                    }) as u32
                        + 1;
                }
                symbol::placement::LayerBuckets { layer, raw, buckets, group }
            })
            .collect();
        let placements = symbol::placement::place(ctx, &placement_input);
        (symbol_layers, leader_of, placements)
    }

    /// The opaque pass of a Mercator render, top-down (see `render::Pass`): records the pixels
    /// hidden under opaque backgrounds and fill interiors. Returns which layers drew in it.
    fn opaque_pass(&self, target: &mut render::raster::Target, ctx: &render::Ctx, visible: &[bool]) -> Vec<bool> {
        let mut opaque_drawn = vec![false; self.style.layers.len()];
        // Heatmaps stay on top of the opaque pixels of the layers above them, which the occlusion
        // buffer can't express, so they draw all layers in style order. Only a background or fill
        // above another visible layer can hide anything.
        let lowest = visible.iter().position(|v| *v);
        let occlusion = self.style.layers.len() < u16::MAX as usize
            && !self.style.layers.iter().zip(visible).any(|(l, v)| *v && l.ty == "heatmap")
            && self.style.layers.iter().zip(visible).enumerate().any(|(i, (l, v))| *v && Some(i) > lowest && matches!(l.ty.as_str(), "background" | "fill"));
        if occlusion {
            target.occ = vec![0; target.w * target.h];
            target.cull = true;
            for (i, layer) in self.style.layers.iter().enumerate().rev() {
                if !visible[i] {
                    continue;
                }
                target.layer = i as u16 + 1;
                let raw = self.style.layer_json(&layer.id);
                opaque_drawn[i] = match layer.ty.as_str() {
                    "background" => render::draw_background(target, layer, ctx, raw, render::Pass::Opaque),
                    "fill" if self.layer_opacity(layer).is_none_or(|o| o >= 1.0) => {
                        let Some(src) = layer.source.as_deref().and_then(|s| self.sources.get(s)) else { continue };
                        self.with_tiles(ctx, src, layer, target.w, target.h, |tiles| render::draw_fill(target, layer, ctx, tiles, raw, render::Pass::Opaque))
                    }
                    _ => false,
                };
            }
            target.cull = opaque_drawn.iter().rposition(|d| *d) > lowest;
        }
        opaque_drawn
    }

    fn draw_raster(&self, target: &mut render::raster::Target, layer: &style::Layer, ctx: &render::Ctx, src: &Source, w: usize, h: usize) {
        use render::raster_layer::{draw_quad, Quad, RasterParams};
        let params = RasterParams::new(layer, ctx);
        if let Data::Image { coordinates, texture: Some(Some(tex)), .. } = &src.data {
            let (id, corners) = image_tile(coordinates);
            let warp = image_warp(&corners);
            let parallelogram = warp == [0.0, 0.0, 1.0] && {
                let [tl, tr, br, bl] = corners;
                tl[0] + br[0] == tr[0] + bl[0] && tl[1] + br[1] == tr[1] + bl[1]
            };
            // World copies of the image tile that may be visible.
            let corners_world = [[0.0, 0.0], [w as f64, 0.0], [0.0, h as f64], [w as f64, h as f64]].map(|p| ctx.transform.screen_to_world(p));
            let ws = ctx.transform.world_size;
            let (x0, x1) = corners_world.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p[0]), b.max(p[0])));
            for wrap in (x0 / ws).floor() as i32 - 1..=(x1 / ws).floor() as i32 + 1 {
                let tile = RenderTile { overscaled_z: id.z, wrap, canonical: id };
                let q = Quad { tile, corners, warp, subdivided: warp[2] > 0.0 && !parallelogram };
                draw_quad(target, ctx, &q, tex, &params, None);
            }
            return;
        }
        let resolved = self.resolve(src).0;
        let tiles: Vec<RenderTile> = resolved.iter().map(|r| r.0).collect();
        // Same-zoom tiles abut and are drawn without stencil; mixed zooms (fallback parents) are
        // clipped so that the higher zoom wins.
        let mixed = tiles.iter().any(|t| t.canonical.z != tiles[0].canonical.z);
        let masks = if mixed { Some(ctx.masks(&tiles, w, h)) } else { None };
        for (i, (rt, data_id)) in resolved.iter().enumerate() {
            if let Some(Some(TileData::Raster(tex))) = src.tiles.get(data_id) {
                draw_quad(target, ctx, &Quad::tile(*rt), tex, &params, masks.as_ref().map(|m| &m[i]));
            }
        }
    }

    /// Hillshade / color-relief: each drawn DEM tile takes its 2px border from the drawn tiles
    /// around it (MapLibre's `backfillDEM` over the renderable tiles), then is drawn.
    /// The DEMs of the `resolved` tiles of `src`, with their borders filled from their neighbors
    /// (`backfillDEM`).
    fn backfilled_dems(&self, src: &Source, resolved: &[(RenderTile, TileId)]) -> Vec<Option<render::dem::Dem>> {
        let tiles: Vec<RenderTile> = resolved.iter().map(|r| r.0).collect();
        let raw: Vec<Option<&Rc<render::dem::Dem>>> = resolved
            .iter()
            .map(|(_, id)| match src.tiles.get(id) {
                Some(Some(TileData::Dem(d))) => Some(d),
                _ => None,
            })
            .collect();
        tiles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut dem = (**raw[i]?).clone();
                let n = 1i64 << t.canonical.z;
                let world_x = |t: &RenderTile| t.wrap as i64 * n + t.canonical.x as i64;
                for (j, u) in tiles.iter().enumerate() {
                    let (dx, dy) = (world_x(u) - world_x(t), u.canonical.y as i64 - t.canonical.y as i64);
                    let neighbor = u.overscaled_z == t.overscaled_z && u.canonical.z == t.canonical.z && dx.abs() <= 1 && dy.abs() <= 1 && (dx, dy) != (0, 0);
                    if let (true, Some(b)) = (neighbor, raw[j]) {
                        dem.backfill(b, dx as isize, dy as isize);
                    }
                }
                Some(dem)
            })
            .collect()
    }

    fn draw_dem_layer(&self, target: &mut render::raster::Target, layer: &style::Layer, ctx: &render::Ctx, src: &Source, w: usize, h: usize) {
        let hillshade = (layer.ty == "hillshade").then(|| render::dem::HillshadeParams::new(layer, ctx));
        let ramp = (layer.ty == "color-relief").then(|| render::dem::ColorRamp::new(layer, ctx));
        let resolved = self.resolve(src).0;
        let tiles: Vec<RenderTile> = resolved.iter().map(|r| r.0).collect();
        // The resolved tiles of a source are fixed for one render, so the source id keys them.
        let dems = ctx.dem_tiles(layer.source.as_deref().unwrap_or_default(), || {
            self.backfilled_dems(src, &resolved).into_iter().map(|d| d.map(render::dem::DemTile::new)).collect()
        });
        let mixed = tiles.iter().any(|t| t.canonical.z != tiles[0].canonical.z);
        let masks = if mixed { Some(ctx.masks(&tiles, w, h)) } else { None };
        for (i, t) in tiles.iter().enumerate() {
            let Some(dem) = &dems[i] else { continue };
            let mask = masks.as_ref().map(|m| &m[i]);
            if let Some(params) = &hillshade {
                render::dem::draw_hillshade_tile(target, ctx, t, dem, params, mask);
            }
            if let Some(ramp) = &ramp {
                render::dem::draw_color_relief_tile(target, ctx, t, &dem.dem, ramp, mask);
            }
        }
    }
}

/// `rgba` (`w` pixels wide) without `margin` pixels on each side.
fn crop(mut rgba: Vec<u8>, w: usize, margin: usize) -> Vec<u8> {
    if margin == 0 {
        return rgba;
    }
    let (width, height) = (w - 2 * margin, rgba.len() / 4 / w - 2 * margin);
    for (row, y) in (margin..margin + height).enumerate() {
        let start = (y * w + margin) * 4;
        rgba.copy_within(start..start + width * 4, row * width * 4);
    }
    rgba.truncate(width * height * 4);
    rgba
}

/// Decode a raster-dem tile with the source's `encoding` (and custom factors).
fn dem_from(img: &image::Image, src: &J, encoding: Option<&str>) -> Option<render::dem::Dem> {
    let f = |k: &str, d: f64| src[k].as_f64().unwrap_or(d);
    let factors = [f("redFactor", 1.0), f("greenFactor", 1.0), f("blueFactor", 1.0), f("baseShift", 0.0)];
    render::dem::Dem::new(img, encoding.unwrap_or("mapbox"), factors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::test_crs;

    #[test]
    fn glyph_requests_follow_staged_sources_and_failed_ranges() {
        let style = r#"{"version":8,"glyphs":"/glyphs/{fontstack}/{range}","sources":{"a":{"type":"geojson","data":"/a"},"b":{"type":"geojson","data":"/b"}},"layers":[{"id":"a","type":"symbol","source":"a","layout":{"text-field":["get","name"],"text-font":["Test Font"]}},{"id":"b","type":"symbol","source":"b","layout":{"text-field":["get","name"],"text-font":["Test Font"]}}]}"#;
        let mut renderer = Renderer::new(style, View::tile(0, 0, 0, 128)).unwrap();
        let urls = |r: &Renderer| r.needs().into_iter().map(|n| n.url).collect::<Vec<_>>();
        assert_eq!(urls(&renderer), ["/a", "/b"]);
        renderer.provide("/a", Some(br#"{"type":"Feature","properties":{"name":"A"},"geometry":{"type":"Point","coordinates":[0,0]}}"#));
        let expected = ["/b", "/glyphs/Test Font/0-255"];
        assert_eq!(urls(&renderer), expected);
        assert_eq!(urls(&renderer), expected);
        renderer.provide("/glyphs/Test Font/0-255", None);
        assert_eq!(urls(&renderer), ["/b"]);
        // Later source data can introduce a range after earlier glyphs have settled.
        renderer.provide("/b", Some(r#"{"type":"Feature","properties":{"name":"あ"},"geometry":{"type":"Point","coordinates":[0,0]}}"#.as_bytes()));
        assert_eq!(urls(&renderer), ["/glyphs/Test Font/12288-12543"]);
        renderer.provide("/glyphs/Test Font/12288-12543", None);
        assert!(renderer.needs().is_empty());
        assert!(renderer.render().is_ok());
    }

    #[test]
    fn glyph_ranges_are_requested_again_for_code_points_not_kept() {
        // A range file with glyphs "A" and "B" (1x1 px).
        let field = |n: u8, bytes: &[u8]| [&[n << 3 | 2, bytes.len() as u8][..], bytes].concat();
        let glyph = |c: u8| field(3, &[&[1 << 3, c][..], &field(2, &[0; 49]), &[3 << 3, 1, 4 << 3, 1, 7 << 3, 2]].concat());
        let range = field(1, &[glyph(b'A'), glyph(b'B')].concat());
        let style = r#"{"version":8,"glyphs":"/glyphs/{fontstack}/{range}","sources":{"a":{"type":"geojson","data":{"type":"Point","coordinates":[0,0]}}},"layers":[{"id":"a","type":"symbol","source":"a","layout":{"text-field":["step",["zoom"],"A",1,"B"],"text-font":["Test Font"]}}]}"#;
        let mut renderer = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
        let urls = |r: &Renderer| r.needs().into_iter().map(|n| n.url).collect::<Vec<_>>();
        assert_eq!(urls(&renderer), ["/glyphs/Test Font/0-255"]);
        renderer.provide("/glyphs/Test Font/0-255", Some(&range));
        assert!(renderer.needs().is_empty());
        assert!(renderer.glyphs.get("Test Font", 'A').is_some());
        assert!(renderer.glyphs.get("Test Font", 'B').is_none());
        renderer.set_view(View::tile(2, 1, 1, 512));
        assert_eq!(urls(&renderer), ["/glyphs/Test Font/0-255"]);
        renderer.provide("/glyphs/Test Font/0-255", Some(&range));
        assert!(renderer.needs().is_empty());
        assert!(renderer.glyphs.get("Test Font", 'B').is_some());
    }

    #[test]
    fn glyph_requests_follow_camera_clamping_after_discovery() {
        let style = r#"{"version":8,"glyphs":"/glyphs/{fontstack}/{range}","sources":{"a":{"type":"geojson","data":{"type":"Point","coordinates":[0,0]}}},"layers":[{"id":"a","type":"symbol","source":"a","layout":{"text-field":["step",["zoom"],"A",1,"あ"],"text-font":["Test Font"]}}]}"#;
        let view = View { center: [0.0, 0.0], zoom: 2.0, width: 128, height: 128, ..View::tile(0, 0, 0, 128) };
        let mut renderer = Renderer::new(style, view).unwrap();
        assert_eq!(renderer.needs()[0].url, "/glyphs/Test Font/12288-12543");
        renderer.provide("/glyphs/Test Font/12288-12543", None);
        assert!(renderer.needs().is_empty());
        renderer.set_max_zoom(0.0);
        assert_eq!(renderer.needs()[0].url, "/glyphs/Test Font/0-255");
    }

    #[test]
    fn overzoom_slices_are_shared_but_sources_and_children_are_distinct() {
        let style = r#"{"version":8,"sources":{"a":{"type":"vector","tiles":["/a/{z}/{x}/{y}"],"maxzoom":1},"b":{"type":"vector","tiles":["/b/{z}/{x}/{y}"],"maxzoom":1}},"layers":[]}"#;
        let view = View { width: 1024, ..View::tile(2, 1, 1, 512) };
        let mut renderer = Renderer::new(style, view).unwrap();
        let parent = TileId { z: 1, x: 0, y: 0 };
        let child = TileId { z: 2, x: 1, y: 1 };
        for (id, x) in [("a", 3072), ("b", 2560)] {
            let layer = mvt::Layer { name: "points".into(), extent: 4096, features: vec![mvt::Feature {
                id: None, ty: mvt::GeomType::Point, properties: Default::default(), geometry: vec![vec![[x, 3072]]],
            }] };
            renderer.sources.get_mut(id).unwrap().tiles.insert(parent, Some(TileData::Vector(Rc::new(vec![layer]))));
        }
        let layers = |id: &str, tile: TileId| {
            renderer.tile_layers(&renderer.sources[id]).into_iter().find(|(t, _, _)| t.canonical == tile).unwrap().1.unwrap()
        };
        let a = layers("a", child);
        assert!(Rc::ptr_eq(&a, &layers("a", child)));
        assert_eq!(a[0].features[0].geometry, vec![vec![[2048, 2048]]]);
        let b = layers("b", child);
        assert!(!Rc::ptr_eq(&a, &b));
        assert_eq!(b[0].features[0].geometry, vec![vec![[1024, 2048]]]);

        // A sibling uses the same parent but needs its own clipping and coordinates.
        let sibling = layers("a", TileId { z: 2, x: 0, y: 1 });
        assert!(!Rc::ptr_eq(&a, &sibling));
        assert!(sibling[0].features.is_empty());

        // Once zoom is clamped to the source zoom, use the original decoded tile.
        renderer.set_max_zoom(1.0);
        let unsliced = renderer.tile_layers(&renderer.sources["a"]).into_iter()
            .find(|(t, _, _)| t.canonical == parent).unwrap().1.unwrap();
        assert_eq!(unsliced[0].features[0].geometry, vec![vec![[3072, 3072]]]);
    }

    #[test]
    fn tile_view() {
        let v = View::tile(0, 0, 0, 512);
        assert_eq!((v.center, v.zoom), ([0.0, 0.0], 0.0));
        // 256px tiles sit one zoom level below 512px ones.
        let v = View::tile(1, 1, 0, 256);
        assert_eq!(v.zoom, 0.0);
        assert_eq!(v.center[0], 90.0);
        assert!((v.center[1] - 66.51326).abs() < 1e-5);
    }

    #[test]
    fn png_roundtrip_preserves_rgba_dimensions_and_margin_crop() {
        let style = r##"{"version":8,"sources":{"p":{"type":"geojson","data":{"type":"Point","coordinates":[0,0]}}},"layers":[{"id":"bg","type":"background","paint":{"background-color":"rgba(32,64,128,0.5)"}},{"id":"point","type":"circle","source":"p","paint":{"circle-radius":10,"circle-color":"rgba(255,0,0,0.5)"}}]}"##;
        for pixel_ratio in [1.0, 1.5, 2.0] {
            for margin in [0, 8] {
                let view = View { center: [0.0, 0.0], zoom: 1.0, bearing: 0.0, width: 63, height: 47, pixel_ratio, margin };
                let mut renderer = Renderer::new(style, view).unwrap();
                let rgba = renderer.render().unwrap();
                let (width, height) = ((view.width as f64 * pixel_ratio).round() as u32, (view.height as f64 * pixel_ratio).round() as u32);
                let png = encode_png(&rgba, width, height).unwrap();
                let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
                let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
                let info = reader.next_frame(&mut decoded).unwrap();
                assert_eq!((info.width, info.height), (width, height));
                assert_eq!(info.color_type, png::ColorType::Rgba);
                assert_eq!(&decoded[..info.buffer_size()], &rgba);
            }
        }
    }

    #[test]
    fn png_rejects_mismatched_dimensions() {
        assert!(matches!(encode_png(&[0; 16], 3, 1), Err(Error::ImageEncoding(_))));
    }

    #[test]
    fn webp_roundtrip_is_lossless() {
        let rgba: Vec<u8> = (0..5 * 3 * 4).map(|i| (i * 37 % 256) as u8).collect();
        let webp = encode_webp(&rgba, 5, 3).unwrap();
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(webp)).unwrap();
        assert_eq!(decoder.dimensions(), (5, 3));
        let mut decoded = vec![0; decoder.output_buffer_size().unwrap()];
        decoder.read_image(&mut decoded).unwrap();
        assert_eq!(decoded, rgba);
        assert!(matches!(encode_webp(&rgba, 4, 3), Err(Error::ImageEncoding(_))));
    }

    #[test]
    fn jpeg_flattens_over_black() {
        // Opaque white beside transparent white: the second decodes as black.
        let rgba = [[255u8, 255, 255, 255]; 8].into_iter().chain([[255, 255, 255, 0]; 8]).flatten().collect::<Vec<_>>();
        let jpeg = encode_jpeg(&rgba, 16, 1, 100).unwrap();
        let mut decoder = zune_jpeg::JpegDecoder::new(&jpeg);
        let rgb = decoder.decode().unwrap();
        assert_eq!(decoder.dimensions(), Some((16, 1)));
        assert!(rgb[..3].iter().all(|&v| v > 240) && rgb[rgb.len() - 3..].iter().all(|&v| v < 15));
        assert!(matches!(encode_jpeg(&rgba, 4, 3, 90), Err(Error::ImageEncoding(_))));
    }

    #[test]
    fn source_filter_precedes_clustering_and_property_reduction() {
        let data = serde_json::json!({
            "type": "FeatureCollection",
            "features": [
                {"type":"Feature","properties":{"keep":true,"amount":3},"geometry":{"type":"Point","coordinates":[0,0]}},
                {"type":"Feature","properties":{"keep":true,"amount":5},"geometry":{"type":"Point","coordinates":[0.1,0]}},
                {"type":"Feature","properties":{"keep":false,"amount":100},"geometry":{"type":"Point","coordinates":[0.2,0]}}
            ]
        });
        let mut source = serde_json::json!({
            "type": "geojson", "filter": ["==", ["get", "keep"], true],
            "cluster": true, "clusterProperties": {"total": ["+", ["get", "amount"]]}
        });
        let clustered = geojson_index(&data, &source).tile(0, 0, 0);
        assert_eq!(clustered.features.len(), 1);
        let props = &clustered.features[0].properties;
        assert_eq!(props.get("point_count").and_then(expression::Value::as_f64), Some(2.0));
        assert_eq!(props.get("total").and_then(expression::Value::as_f64), Some(8.0));
        // Source filtering also applies when clustering is disabled.
        source["cluster"] = J::Bool(false);
        let unclustered = geojson_index(&data, &source).tile(0, 0, 0);
        assert_eq!(unclustered.features.len(), 2);
    }

    #[test]
    fn tilejson_tile_size_and_encoding() {
        // The TileJSON may carry `tileSize`/`encoding` (e.g. terrarium DEMs); the style wins.
        let tj = serde_json::json!({"tiles": ["t/{z}/{x}/{y}.webp"], "tileSize": 512, "encoding": "terrarium"});
        let from_tilejson = tilejson_from(&tj, &serde_json::json!({"type": "raster-dem", "url": "x"}));
        assert_eq!((from_tilejson.tile_size, from_tilejson.encoding.as_deref()), (512.0, Some("terrarium")));
        let overridden = tilejson_from(&tj, &serde_json::json!({"tileSize": 256, "encoding": "mapbox"}));
        assert_eq!((overridden.tile_size, overridden.encoding.as_deref()), (256.0, Some("mapbox")));
    }

    #[test]
    fn hillshade_requests_dem_and_renders_with_margin() {
        let style = r##"{"version":8,"sources":{"dem":{"type":"raster-dem","tiles":["/dem/{z}/{x}/{y}.png"],"tileSize":64,"maxzoom":10}},"layers":[{"id":"bg","type":"background","paint":{"background-color":"white"}},{"id":"shade","type":"hillshade","source":"dem"}]}"##;
        // A curved elevation surface must produce visible, varying shading. This exercises
        // the entire source/provide/draw path, rather than only the DEM slope calculation.
        let data: Vec<u8> = (0..64).flat_map(|_| (0..64).flat_map(|x| {
            let elevation = (x as f64 / 63.0 * std::f64::consts::PI).sin() * 4000.0;
            let packed = ((elevation + 10000.0) * 10.0).round() as u32;
            [(packed >> 16) as u8, (packed >> 8) as u8, packed as u8, 255]
        })).collect();
        let png = encode_png(&data, 64, 64).unwrap();
        for margin in [0, 8] {
            let view = View { margin, ..View::tile(10, 512, 512, 64) };
            let mut renderer = Renderer::new(style, view).unwrap();
            assert!(!renderer.needs().is_empty(), "DEM tiles must be requested");
            for resource in renderer.needs() {
                renderer.provide(&resource.url, Some(&png));
            }
            assert!(renderer.needs().is_empty());
            let rgba = renderer.render().unwrap();
            assert_eq!(rgba.len(), 64 * 64 * 4);
            let center_row = &rgba[32 * 64 * 4..33 * 64 * 4];
            assert!(center_row.chunks_exact(4).any(|p| p != [255; 4]), "Hillshade must change the white background");
            assert!(center_row.chunks_exact(4).any(|p| p != &center_row[..4]), "A curved DEM must have varying shading");
        }
    }

    #[test]
    fn background_only() {
        let style = r##"{"version":8,"sources":{},"layers":[{"id":"bg","type":"background","paint":{"background-color":"#ff0000"}}]}"##;
        let mut r = Renderer::new(style, View { center: [0.0, 0.0], zoom: 0.0, bearing: 0.0, width: 4, height: 4, pixel_ratio: 1.0, margin: 0 }).unwrap();
        assert!(r.needs().is_empty());
        let px = r.render().unwrap();
        assert_eq!(&px[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn overlapping_renders_keep_each_others_tiles() {
        // Render 1 is given its tiles, then renders 2.. draw many other tiles meanwhile: with
        // ticket 1 still in progress, its tiles stay; without, they are evicted.
        let style = r#"{"version":8,"sources":{"v":{"type":"vector","tiles":["t/{z}/{x}/{y}"]}},"layers":[{"id":"f","type":"fill","source":"v","source-layer":"l"}]}"#;
        let provide_all = |r: &mut Renderer| {
            for need in r.needs() {
                r.provide(&need.url, None);
            }
        };
        for retain in [true, false] {
            let mut r = Renderer::new(style, View::tile(3, 0, 0, 512)).unwrap();
            r.set_ticket(1);
            provide_all(&mut r);
            for ticket in 2..20 {
                r.set_view(View::tile(3, ticket as u32 % 8, ticket as u32 / 8 + 2, 512));
                r.set_ticket(ticket);
                provide_all(&mut r);
                let oldest = if retain { 1 } else { ticket };
                r.render_retaining(oldest).unwrap();
            }
            r.set_view(View::tile(3, 0, 0, 512));
            r.set_ticket(1);
            assert_eq!(r.needs().is_empty(), retain, "retain {retain}");
        }
    }

    #[test]
    fn reused_renderer_matches_fresh_renders() {
        // Polygons and round-joined lines over the whole world, so that every tile has buckets.
        let mut features = Vec::new();
        for i in 0..64 {
            let (x, y) = (-178.0 + (i % 8) as f64 * 45.0, -82.0 + (i / 8) as f64 * 21.0);
            features.push(serde_json::json!({"type": "Feature", "properties": {"k": i % 3}, "geometry": {"type": "Polygon", "coordinates": [[[x, y], [x + 40.0, y + 3.0], [x + 15.0, y + 19.0], [x, y]]]}}));
            features.push(serde_json::json!({"type": "Feature", "properties": {"k": i % 3}, "geometry": {"type": "LineString", "coordinates": [[x, y], [x + 50.0, y + 15.0], [x + 10.0, y + 25.0]]}}));
        }
        let style = serde_json::json!({"version": 8, "sprite": "/sprite", "sources": {"g": {"type": "geojson", "maxzoom": 3, "data": {"type": "FeatureCollection", "features": features}}}, "layers": [
            {"id": "f", "type": "fill", "source": "g", "paint": {"fill-color": ["match", ["get", "k"], 0, "#c33", 1, "#3c3", "#33c"], "fill-opacity": 0.7}},
            {"id": "l", "type": "line", "source": "g", "layout": {"line-join": "round", "line-cap": "round"}, "paint": {"line-width": 3}},
            // Shares the fill buckets of "f" (same layout), drawn after another layer.
            {"id": "f2", "type": "fill", "source": "g", "paint": {"fill-color": "#000", "fill-opacity": 0.2}},
            // Line labels depend on the world copy; icon sizes on the sprite's pixel ratio.
            {"id": "s", "type": "symbol", "source": "g", "layout": {"icon-image": "marker", "symbol-placement": "line", "symbol-spacing": 40}},
        ]})
        .to_string();
        let png = encode_png(&[0, 0, 0, 255].repeat(64), 8, 8).unwrap();
        let render = |r: &mut Renderer| {
            for need in r.needs() {
                let ratio = if need.url.contains("@2x") { 2 } else { 1 };
                let json = format!(r#"{{"marker":{{"x":0,"y":0,"width":8,"height":8,"pixelRatio":{ratio}}}}}"#);
                r.provide(&need.url, Some(if need.url.ends_with(".json") { json.as_bytes() } else { &png }));
            }
            assert!(r.needs().is_empty());
            r.render().unwrap()
        };
        // The 64 z3 tiles exceed MAX_CACHED_TILES and are revisited after eviction. z4 views
        // overzoom the same z3 tiles, whose buckets differ by zoom.
        let tile = |z: u8, x: u32, y: u32| View { margin: 16, pixel_ratio: 0.25, ..View::tile(z, x, y, 512) };
        let mut views: Vec<View> = (0..64).map(|i| tile(3, i % 8, i / 8)).collect();
        views.extend((0..16).map(|i| tile(4, 4 + i % 4, 4 + i / 4)));
        views.extend((0..64).map(|i| tile(3, i % 8, i / 8)));
        views.push(View { pixel_ratio: 0.5, ..views[9] });
        views.push(View { pixel_ratio: 2.0, ..views[9] });
        // Another world copy of a cached tile.
        views.push(View { center: [views[9].center[0] + 360.0, views[9].center[1]], ..views[9] });
        let mut shared = Renderer::new(&style, views[0]).unwrap();
        let mut drawn = 0;
        for (i, &view) in views.iter().enumerate() {
            shared.set_view(view);
            let rgba = render(&mut shared);
            assert_eq!(rgba, render(&mut Renderer::new(&style, view).unwrap()), "view {i}");
            let mut once = Renderer::new(&style, view).unwrap();
            once.set_keep_buckets(false);
            assert_eq!(rgba, render(&mut once), "view {i} without keeping buckets");
            drawn += rgba.chunks_exact(4).any(|p| p[3] > 0) as usize;
        }
        assert!(drawn > views.len() / 2);
        let src = &shared.sources["g"];
        assert!(src.last_used.borrow().len() <= MAX_CACHED_TILES + 16);
        assert!(src.buckets.borrow().len() <= MAX_CACHED_TILES + 16);
    }

    #[test]
    fn provide_accepts_tiles_requested_before_a_view_change() {
        let style = r#"{"version":8,"sources":{"v":{"type":"vector","tiles":["/{z}/{x}/{y}"],"maxzoom":2}},"layers":[{"id":"l","type":"line","source":"v","source-layer":"lines"}]}"#;
        let (a, b) = (View::tile(2, 0, 0, 512), View::tile(2, 3, 3, 512));
        let mut renderer = Renderer::new(style, a).unwrap();
        let requested = renderer.needs();
        assert!(!requested.is_empty());
        renderer.set_view(b);
        for r in &requested {
            renderer.provide(&r.url, None);
        }
        renderer.set_view(a);
        assert!(renderer.needs().is_empty());
    }

    #[test]
    fn margin_fetches_neighbor_tiles_without_moving_camera() {
        let style = r#"{"version":8,"sources":{"v":{"type":"vector","tiles":["/{z}/{x}/{y}"],"maxzoom":2}},"layers":[{"id":"s","type":"symbol","source":"v","source-layer":"points","layout":{"text-field":"label"}}]}"#;
        let view = View::tile(2, 1, 0, 512);
        let plain = Renderer::new(style, view).unwrap();
        let mut padded = Renderer::new(style, View { margin: 32, ..view }).unwrap();
        assert_eq!(plain.transform.center, padded.transform.center);
        assert!(!plain.needs().iter().any(|r| r.url == "/2/2/0"));
        assert!(padded.needs().iter().any(|r| r.url == "/2/2/0"));
        padded.set_max_zoom(20.0);
        assert_eq!(plain.transform.center, padded.transform.center);
    }

    #[test]
    fn margin_renders_icon_anchored_in_neighbor_tile_and_crops_output() {
        // The anchor is just east of this view's right edge (longitude 0). With no
        // GeoJSON tile buffer, only loading the neighboring tile can recover the icon.
        let style = r#"{"version":8,"sprite":"/sprite","sources":{"p":{"type":"geojson","buffer":0,"maxzoom":2,"data":{"type":"Point","coordinates":[1,40]}}},"layers":[{"id":"s","type":"symbol","source":"p","layout":{"icon-image":"marker","icon-allow-overlap":true}}]}"#;
        let png = encode_png(&[255, 0, 0, 255].repeat(32 * 32), 32, 32).unwrap();
        for ratio in [1.0, 1.25, 1.5, 2.0] {
            for margin in [0, 31] {
                let view = View { margin, pixel_ratio: ratio, ..View::tile(2, 1, 1, 512) };
                let mut renderer = Renderer::new(style, view).unwrap();
                let suffix = if ratio > 1.0 { "@2x" } else { "" };
                renderer.provide(&format!("/sprite{suffix}.json"), Some(br#"{"marker":{"x":0,"y":0,"width":32,"height":32,"pixelRatio":1}}"#));
                renderer.provide(&format!("/sprite{suffix}.png"), Some(&png));
                assert!(renderer.needs().is_empty());
                let rgba = renderer.render().unwrap();
                let size = (512.0 * ratio).round() as usize;
                assert_eq!(rgba.len(), size * size * 4);
                let visible = rgba.chunks_exact(4).any(|p| p[3] > 0);
                assert_eq!(visible, margin > 0, "ratio={ratio}, margin={margin}");
                if margin > 0 {
                    assert!(rgba.chunks_exact(size * 4).any(|row| row[(size - 1) * 4 + 3] > 0));
                }
            }
        }
    }

    #[test]
    fn exports_sources_as_projected_vector_tiles() {
        use expression::Value;
        // Longlat 0°–10° on 4096 units: (x°, y°) is at (409.6 x, 409.6 (10 - y)).
        let view = reproject::ProjectedView { projjson: test_crs::geographic(), bbox: [0.0, 0.0, 10.0, 10.0], width: 512, height: 512, pixel_ratio: 1.0, margin: 0 };
        let near = |p: [i32; 2], lng: f64, lat: f64| (p[0] as f64 - 409.6 * lng).abs() < 2.0 && (p[1] as f64 - 409.6 * (10.0 - lat)).abs() < 2.0;
        let style = r#"{"version":8,"sources":{
            "g":{"type":"geojson","data":{"type":"FeatureCollection","features":[
                {"type":"Feature","properties":{"k":"square"},"geometry":{"type":"Polygon","coordinates":[[[2,2],[8,2],[8,8],[2,8],[2,2]]]}}]}},
            "v":{"type":"vector","tiles":["t/{z}/{x}/{y}"]}},
            "layers":[{"id":"l","type":"line","source":"v","source-layer":"used"}]}"#;
        let mut r = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
        r.set_projected_view(&view).unwrap();
        assert!(r.set_export_source(Some("nope")).is_err());

        r.set_export_source(Some("g")).unwrap();
        assert!(r.needs().is_empty());
        let layers = mvt::decode(&r.export_tile(0).unwrap(), |_| true).unwrap();
        assert_eq!(layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["g"]);
        let f = &layers[0].features[0];
        assert!(matches!(f.properties.get("k"), Some(Value::String(s)) if &**s == "square"));
        let ring = &f.geometry[0];
        for (lng, lat) in [(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)] {
            assert!(ring.iter().any(|&p| near(p, lng, lat)), "corner ({lng}, {lat}) in {ring:?}");
        }
        assert!(ring.iter().all(|&p| (800..=3280).contains(&p[0]) && (800..=3280).contains(&p[1])));

        // A vector source keeps the layers no style layer uses, and only its own tiles are needed.
        r.set_export_source(Some("v")).unwrap();
        let needs = r.needs();
        assert!(!needs.is_empty() && needs.iter().all(|n| n.url.starts_with("t/")));
        for n in needs {
            let zxy: Vec<u32> = n.url[2..].split('/').map(|v| v.parse().unwrap()).collect();
            let (z, x, y) = (2f64.powi(zxy[0] as i32), zxy[1] as f64, zxy[2] as f64);
            let at = |lng: f64, lat: f64| [((tile::mercator_x(lng) * z - x) * 4096.0).round() as i32, ((tile::mercator_y(lat) * z - y) * 4096.0).round() as i32];
            let feature = |ty, geometry| mvt::Feature { id: None, ty, properties: Default::default(), geometry };
            let layer = |name: &str, f| mvt::Layer { name: name.into(), extent: 4096, features: vec![f] };
            // A square across 0°, a tile boundary at every zoom: each tile has its part, with a buffer,
            // and an id of its own (like OpenMapTiles' oceans).
            let square = vec![vec![at(-3.0, 2.0), at(-3.0, 8.0), at(3.0, 8.0), at(3.0, 2.0), at(-3.0, 2.0)]];
            let square: Vec<Vec<[f64; 2]>> = square.iter().map(|r| r.iter().map(|p| [p[0] as f64, p[1] as f64]).collect()).collect();
            let clip = |axis| mvt::clip_axis(&square, mvt::GeomType::Polygon, -128.0, 4224.0, axis);
            let square: Vec<Vec<[i32; 2]>> = mvt::clip_axis(&clip(0), mvt::GeomType::Polygon, -128.0, 4224.0, 1)
                .iter().map(|r| r.iter().map(|p| [p[0].round() as i32, p[1].round() as i32]).collect()).collect();
            let tile = mvt::encode(&[
                layer("used", feature(mvt::GeomType::LineString, vec![vec![at(1.0, 5.0), at(9.0, 5.0)]])),
                layer("other", feature(mvt::GeomType::Point, vec![vec![at(5.0, 5.0)]])),
                layer("polygon", mvt::Feature { id: Some(expression::Value::Number(zxy[1] as f64)), ..feature(mvt::GeomType::Polygon, square) }),
            ]);
            r.provide(&n.url, Some(&tile));
        }
        assert!(r.needs().is_empty());
        let layers = mvt::decode(&r.export_tile(0).unwrap(), |_| true).unwrap();
        let get = |name: &str| layers.iter().find(|l| l.name == name).unwrap();
        assert!(get("other").features.iter().flat_map(|f| f.geometry.iter().flatten()).all(|&p| near(p, 5.0, 5.0)));
        // Cut at the source's tile seams; together the line still runs from 1° to 9°.
        let line: Vec<[i32; 2]> = get("used").features.iter().flat_map(|f| f.geometry.iter().flatten().copied()).collect();
        assert!(line.iter().all(|&p| (p[1] as f64 - 409.6 * 5.0).abs() < 3.0));
        let xs = line.iter().map(|p| p[0]);
        assert!(near([xs.clone().min().unwrap(), 2048], 1.0, 5.0) && near([xs.max().unwrap(), 2048], 9.0, 5.0));
        // The square's parts are merged back: one ring, no edge along the tile boundary, and the
        // orientation of an MVT outer ring (positive area in y-down coordinates).
        let polygon = &get("polygon").features;
        assert_eq!((polygon.len(), polygon[0].geometry.len()), (1, 1), "{polygon:?}");
        assert!(polygon[0].id.is_none(), "the pieces' ids differ");
        let ring = &polygon[0].geometry[0];
        assert!(!ring.windows(2).any(|w| w[0][0] == 0 && w[1][0] == 0), "{ring:?}");
        assert!(ring.iter().any(|&p| near(p, 3.0, 8.0)) && ring.iter().any(|&p| near(p, 3.0, 2.0)));
        let area: i64 = ring.windows(2).map(|w| w[0][0] as i64 * w[1][1] as i64 - w[1][0] as i64 * w[0][1] as i64).sum();
        assert!(area > 0, "{ring:?}");

        // Rendering again needs the style's resources, not the export's.
        r.set_export_source(None).unwrap();
        r.render().unwrap();
    }

    #[test]
    fn exports_rasters_images_and_dems_on_the_output_grid() {
        // Longlat 0°–10° on 100 px: pixel x is at longitude (x + 0.5) / 10.
        let view = reproject::ProjectedView { projjson: test_crs::geographic(), bbox: [0.0, 0.0, 10.0, 10.0], width: 100, height: 100, pixel_ratio: 1.0, margin: 0 };
        let style = r#"{"version":8,"sources":{
            "r":{"type":"raster","tiles":["r/{z}/{x}/{y}"],"tileSize":256},
            "i":{"type":"image","url":"image.png","coordinates":[[2,8],[8,8],[8,2],[2,2]]},
            "d":{"type":"raster-dem","tiles":["d/{z}/{x}/{y}"],"tileSize":256}},"layers":[]}"#;
        let mut r = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
        r.set_projected_view(&view).unwrap();
        let red = encode_png(&[255, 0, 0, 255].repeat(256 * 256), 256, 256).unwrap();
        let mut export = |source: &str| {
            r.set_export_source(Some(source)).unwrap();
            for n in r.needs() {
                let tile = if n.url.starts_with("d/") {
                    // Mapbox-encoded DEM, 100 m per degree of longitude (≥ 0 here).
                    let zxy: Vec<f64> = n.url[2..].split('/').map(|v| v.parse().unwrap()).collect();
                    let mut px = Vec::with_capacity(256 * 256 * 4);
                    for _ in 0..256 {
                        for x in 0..256 {
                            let lng = (zxy[1] + (x as f64 + 0.5) / 256.0) / 2f64.powf(zxy[0]) * 360.0 - 180.0;
                            let v = ((lng * 100.0 + 10000.0) * 10.0).round() as u32;
                            px.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]);
                        }
                    }
                    encode_png(&px, 256, 256).unwrap()
                } else {
                    red.clone()
                };
                r.provide(&n.url, Some(&tile));
            }
            assert!(r.needs().is_empty());
            let img = image::Image::decode(&r.export_tile(0).unwrap()).unwrap();
            assert_eq!((img.w, img.h), (100, 100));
            img.data
        };
        let at = |data: &[u8], x: usize, y: usize| -> [u8; 4] { data[(y * 100 + x) * 4..][..4].try_into().unwrap() };
        let raster = export("r");
        assert!(raster.chunks(4).all(|p| p == [255, 0, 0, 255]));
        // The image covers 2°–8°.
        let image = export("i");
        assert_eq!(at(&image, 50, 50), [255, 0, 0, 255]);
        assert_eq!(at(&image, 15, 50)[3], 0);
        assert_eq!(at(&image, 25, 50), [255, 0, 0, 255]);
        assert_eq!(at(&image, 50, 90)[3], 0);
        // Terrarium elevations of each pixel's longitude.
        let dem = export("d");
        for (x, y) in [(5, 5), (37, 60), (94, 98)] {
            let p = at(&dem, x, y);
            let elevation = p[0] as f64 * 256.0 + p[1] as f64 + p[2] as f64 / 256.0 - 32768.0;
            let lng = (x as f64 + 0.5) / 10.0;
            assert!((elevation - lng * 100.0).abs() < 1.0, "({x}, {y}): {elevation} vs {}", lng * 100.0);
        }
    }

    #[test]
    fn exports_follow_the_edge_of_a_geostationary_view() {
        let polygon = |w: f64, s: f64, e: f64, n: f64| format!(r#"{{"type":"geojson","data":{{"type":"Polygon","coordinates":[[[{w},{s}],[{e},{s}],[{e},{n}],[{w},{n}],[{w},{s}]]]}}}}"#);
        let style = format!(
            r#"{{"version":8,"sources":{{"band":{},"world":{},"line":{{"type":"geojson","data":{{"type":"LineString","coordinates":[[30,0],[100,0]]}}}}}},"layers":[]}}"#,
            polygon(30.0, -10.0, 100.0, 10.0),
            r#"{"type":"geojson","data":{"type":"Polygon","coordinates":[[[-180,-85],[180,-85],[180,85],[-180,85],[-180,-85]],[[130,-10],[130,10],[150,10],[150,-10],[130,-10]]]}}"#,
        );
        let r = 5_500_000.0;
        let view = reproject::ProjectedView { projjson: test_crs::himawari(), bbox: [-r, -r, r, r], width: 512, height: 512, pixel_ratio: 1.0, margin: 0 };
        let mut renderer = Renderer::new(&style, View::tile(0, 0, 0, 512)).unwrap();
        renderer.set_projected_view(&view).unwrap();
        let mut export = |source: &str| {
            renderer.set_export_source(Some(source)).unwrap();
            assert!(renderer.needs().is_empty());
            let layers = mvt::decode(&renderer.export_tile(0).unwrap(), |_| true).unwrap();
            layers.into_iter().flat_map(|l| l.features).flat_map(|f| f.geometry).collect::<Vec<_>>()
        };
        let dist = |p: [i32; 2]| ((p[0] as f64 - 2048.0).powi(2) + (p[1] as f64 - 2048.0).powi(2)).sqrt();
        let area = |ring: &[[i32; 2]]| ring.windows(2).map(|w| w[0][0] as f64 * w[1][1] as f64 - w[1][0] as f64 * w[0][1] as f64).sum::<f64>() / 2.0;
        // The world, all of it off the map but around it, is the disk the satellite sees, with its
        // hole after it (readers take the first ring's orientation for outer rings).
        let world = export("world");
        assert_eq!(world.len(), 2);
        assert!(area(&world[0]) > 0.0 && area(&world[1]) < 0.0, "{world:?}");
        let limb = world[0].iter().map(|&p| dist(p)).fold(0.0, f64::max);
        assert!((1900.0..2048.0).contains(&limb), "{limb}");
        assert!(world[0].iter().all(|&p| dist(p) > 0.99 * limb), "{:?}", world[0]);
        let disk = std::f64::consts::PI * limb * limb;
        assert!((area(&world[0]) / disk - 1.0).abs() < 0.01, "{} vs {disk}", area(&world[0]));
        // A band across the west limb follows it between its edges, inside the disk.
        let band = export("band");
        assert_eq!(band.len(), 1, "{band:?}");
        assert!(band[0].iter().all(|&p| dist(p) <= limb + 2.0));
        assert!(band[0].iter().filter(|&&p| dist(p) > limb - 2.0).count() >= 3, "{:?}", band[0]);
        // Around the equator on the west limb (vertices half a pixel off the edge apart).
        assert!(band[0].iter().any(|&p| dist(p) > limb - 5.0 && p[0] < 2048 && (p[1] - 2048).abs() < 150), "{:?}", band[0]);
        assert!(area(&band[0]) > 0.0);
        // A line ends at the limb (its parts in each source tile).
        let line = export("line");
        let west = line.iter().flatten().map(|p| p[0]).min().unwrap() as f64;
        assert!((west - (2048.0 - limb)).abs() < 3.0, "{west} vs {limb}");
    }

    #[test]
    fn projected_view_projects_fills_and_symbols() {
        // A 2°–8° square and an icon at (5°, 5°), both projected.
        let style = r##"{"version":8,"sprite":"/sprite","sources":{"g":{"type":"geojson","data":{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[2,2],[8,2],[8,8],[2,8],[2,2]]]}},
            {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[5,5]}}]}}},
            "layers":[{"id":"bg","type":"background","paint":{"background-color":"#fff"}},
            {"id":"f","type":"fill","source":"g","filter":["==","$type","Polygon"],"paint":{"fill-color":"#f00"}},
            {"id":"s","type":"symbol","source":"g","filter":["==","$type","Point"],"layout":{"icon-image":"marker"}}]}"##;
        let png = encode_png(&[0, 0, 255, 255].repeat(64), 8, 8).unwrap();
        let mut r = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
        let render = |r: &mut Renderer, view: Option<&reproject::ProjectedView>| {
            if let Some(view) = view {
                r.set_projected_view(view).unwrap();
            }
            for need in r.needs() {
                let json = br#"{"marker":{"x":0,"y":0,"width":8,"height":8,"pixelRatio":1}}"#;
                r.provide(
                    &need.url,
                    Some(if need.url.ends_with(".json") {
                        json
                    } else {
                        &png
                    }),
                );
            }
            r.render().unwrap()
        };
        let at = |rgba: &[u8], w: usize, x: f64, y: f64| -> [u8; 4] {
            let i = (y as usize * w + x as usize) * 4;
            rgba[i..i + 4].try_into().unwrap()
        };
        let (red, white, blue) = ([255, 0, 0, 255], [255, 255, 255, 255], [0, 0, 255, 255]);
        // Longlat 0°–10°, 10 px per degree: the square spans pixels 20–80 on both axes.
        // A margin renders more around the box and is cropped.
        for margin in [0, 16] {
            let ll = reproject::ProjectedView {
                projjson: test_crs::geographic(),
                bbox: [0.0, 0.0, 10.0, 10.0],
                width: 100,
                height: 100,
                pixel_ratio: 1.0,
                margin,
            };
            let rgba = render(&mut r, Some(&ll));
            assert_eq!(rgba.len(), 100 * 100 * 4);
            for (x, y, c) in [
                (18.5, 30.5, white),
                (21.5, 30.5, red),
                (78.5, 70.5, red),
                (81.5, 70.5, white),
                (30.5, 18.5, white),
                (30.5, 21.5, red),
                (70.5, 81.5, white),
            ] {
                assert_eq!(at(&rgba, 100, x, y), c, "margin {margin} ({x}, {y})");
            }
            // The 8 px icon is centered on (5°, 5°), axis-aligned and unscaled.
            for (x, y, c) in [
                (46.5, 46.5, blue),
                (53.5, 53.5, blue),
                (45.5, 50.5, red),
                (54.5, 50.5, red),
                (50.5, 45.5, red),
                (50.5, 54.5, red),
            ] {
                assert_eq!(at(&rgba, 100, x, y), c, "margin {margin} icon ({x}, {y})");
            }
        }
        // Wholly past the pole: transparent, without fetching. Straddling the Mercator limit:
        // drawn below it only.
        let pole = reproject::ProjectedView {
            projjson: test_crs::geographic(),
            bbox: [0.0, 95.0, 10.0, 105.0],
            width: 10,
            height: 10,
            pixel_ratio: 1.0,
            margin: 0,
        };
        assert_eq!(render(&mut r, Some(&pole)), vec![0; 10 * 10 * 4]);
        let edge = reproject::ProjectedView {
            projjson: test_crs::geographic(),
            bbox: [0.0, 80.0, 10.0, 90.0],
            width: 10,
            height: 10,
            pixel_ratio: 1.0,
            margin: 0,
        };
        let rgba = render(&mut r, Some(&edge));
        assert_eq!(
            (at(&rgba, 10, 5.5, 1.5)[3], at(&rgba, 10, 5.5, 8.5)),
            (0, white)
        );
        // UTM 31N (central meridian 3°), 2 km per CSS px at pixel ratio 2: the icon stays a
        // 16-device-pixel square at the projected point.
        let utm = test_crs::projected(
            "Transverse Mercator",
            test_crs::WGS84,
            &[("Longitude of natural origin", 3.0), ("Scale factor at natural origin", 0.9996), ("False easting", 500_000.0)],
        );
        let p = reproject::ProjectedView {
            projjson: utm.clone(),
            bbox: [300_000.0, 0.0, 1_100_000.0, 1_000_000.0],
            width: 400,
            height: 500,
            pixel_ratio: 2.0,
            margin: 0,
        };
        let rgba = render(&mut r, Some(&p));
        let (w, h) = (800, 1000);
        assert_eq!(rgba.len(), w * h * 4);
        let warp = &r.projected.as_ref().unwrap().1;
        let lat = 5f64;
        let [cx, cy] = warp.world_to_screen([tile::mercator_x(5.0), tile::mercator_y(lat)]);
        for (dx, dy, c) in [
            (-6.0, -6.0, blue),
            (6.0, 6.0, blue),
            (-6.0, 6.0, blue),
            (-10.0, 0.0, red),
            (10.0, 0.0, red),
            (0.0, -10.0, red),
            (0.0, 10.0, red),
        ] {
            assert_eq!(
                at(&rgba, w, cx + dx, cy + dy),
                c,
                "UTM icon ({dx}, {dy}) around ({cx}, {cy})"
            );
        }
        // Back to a Mercator camera.
        r.set_view(View::tile(0, 0, 0, 64));
        assert!(r.projected.is_none());
        assert_eq!(render(&mut r, None).len(), 64 * 64 * 4);
    }

    #[test]
    fn geostationary_full_disk_hides_the_far_side() {
        // Icons that always show, one under the satellite (140.7°E) and one on the far side.
        let style = r##"{"version":8,"sprite":"/sprite","sources":{"p":{"type":"geojson","data":{"type":"MultiPoint","coordinates":[[140.7,0],[-40,0]]}}},
            "layers":[{"id":"bg","type":"background","paint":{"background-color":"#fff"}},
            {"id":"s","type":"symbol","source":"p","layout":{"icon-image":"marker","icon-allow-overlap":true,"icon-ignore-placement":true}}]}"##;
        let png = encode_png(&[0, 0, 255, 255].repeat(64), 8, 8).unwrap();
        let r = 5_500_000.0;
        let view = reproject::ProjectedView {
            projjson: test_crs::himawari(),
            bbox: [-r, -r, r, r],
            width: 128,
            height: 128,
            pixel_ratio: 1.0,
            margin: 0,
        };
        let mut renderer = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
        renderer.set_projected_view(&view).unwrap();
        for need in renderer.needs() {
            let json = br#"{"marker":{"x":0,"y":0,"width":8,"height":8,"pixelRatio":1}}"#;
            renderer.provide(
                &need.url,
                Some(if need.url.ends_with(".json") {
                    json
                } else {
                    &png
                }),
            );
        }
        let rgba = renderer.render().unwrap();
        let at =
            |x: usize, y: usize| -> [u8; 4] { rgba[(y * 128 + x) * 4..][..4].try_into().unwrap() };
        // The disk (about 0.8 of the box wide) is drawn, space around it is empty, the icon under
        // the satellite is at the center and the one on the far side nowhere.
        assert_eq!(
            (at(64, 64), at(64, 20), at(2, 2)),
            ([0, 0, 255, 255], [255, 255, 255, 255], [0, 0, 0, 0])
        );
        assert_eq!(
            rgba.chunks_exact(4)
                .filter(|p| *p == [0, 0, 255, 255])
                .count(),
            64
        );
    }

    #[test]
    fn geometry_around_the_antipode_stays_off_the_map() {
        // LAEA Europe's antipode (52°S, 170°W) maps to the rim of its disk, far outside Europe:
        // a polygon around it and lines through it must not be drawn across the view as chords.
        let style = r##"{"version":8,"sources":{"g":{"type":"geojson","data":{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[-176,-58],[-164,-58],[-164,-46],[-176,-46],[-176,-58]]]}},
            {"type":"Feature","properties":{},"geometry":{"type":"MultiLineString","coordinates":[[[-170,-80],[-170,-20]],[[-180,-52],[180,-52]]]}}]}}},
            "layers":[{"id":"f","type":"fill","source":"g","paint":{"fill-color":"#f00"}},
            {"id":"l","type":"line","source":"g","paint":{"line-color":"#00f","line-width":3}}]}"##;
        let view = reproject::ProjectedView {
            projjson: test_crs::projected(
                "Lambert Azimuthal Equal Area",
                test_crs::GRS80,
                &[
                    ("Latitude of natural origin", 52.0),
                    ("Longitude of natural origin", 10.0),
                    ("False easting", 4_321_000.0),
                    ("False northing", 3_210_000.0),
                ],
            ),
            bbox: [1_000_000.0, 0.0, 8_000_000.0, 7_000_000.0],
            width: 64,
            height: 64,
            pixel_ratio: 1.0,
            margin: 0,
        };
        let mut renderer = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
        renderer.set_projected_view(&view).unwrap();
        assert!(renderer.needs().is_empty());
        assert!(renderer.render().unwrap().iter().all(|&v| v == 0));
    }

    #[test]
    fn projected_vectors_keep_screen_pixel_sizes_and_tile_seams() {
        let render = |style: &str, view: &reproject::ProjectedView| -> (Vec<u8>, Renderer) {
            let mut r = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
            r.set_projected_view(view).unwrap();
            assert!(r.needs().is_empty());
            (r.render().unwrap(), r)
        };
        // 50–60° from Himawari's nadir the disk is compressed about 2× radially; a 6 px meridian line
        // and a 6 px circle there keep their screen sizes.
        let style = r##"{"version":8,"sources":{"g":{"type":"geojson","data":{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{},"geometry":{"type":"LineString","coordinates":[[200.7,-30],[200.7,30]]}},
            {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[190,25]}}]}}},
            "layers":[{"id":"bg","type":"background","paint":{"background-color":"#fff"}},
            {"id":"l","type":"line","source":"g","filter":["==","$type","LineString"],"paint":{"line-color":"#00f","line-width":6}},
            {"id":"c","type":"circle","source":"g","filter":["==","$type","Point"],"paint":{"circle-color":"#f00","circle-radius":6}}]}"##;
        let r = 5_500_000.0;
        let geos = reproject::ProjectedView {
            projjson: test_crs::himawari(),
            bbox: [-r, -r, r, r],
            width: 256,
            height: 256,
            pixel_ratio: 1.0,
            margin: 0,
        };
        let (rgba, renderer) = render(style, &geos);
        // Buckets split for projected views are cached apart from Mercator ones.
        let mut shared = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
        for mercator in [false, true, false] {
            let rgba_shared = if mercator {
                shared.set_view(View::tile(1, 1, 0, 256));
                let fresh = Renderer::new(style, View::tile(1, 1, 0, 256))
                    .unwrap()
                    .render()
                    .unwrap();
                assert_eq!(shared.render().unwrap(), fresh);
                continue;
            } else {
                shared.set_projected_view(&geos).unwrap();
                shared.render().unwrap()
            };
            assert!(rgba_shared == rgba);
        }
        let warp = &renderer.projected.as_ref().unwrap().1;
        let at =
            |x: usize, y: usize| -> [u8; 4] { rgba[(y * 256 + x) * 4..][..4].try_into().unwrap() };
        let run = |x0: usize, y0: usize, dx: usize, dy: usize, hit: &dyn Fn([u8; 4]) -> bool| {
            (0..256)
                .filter(|&k| {
                    let (x, y) = (x0 + dx * k, y0 + dy * k);
                    x < 256 && y < 256 && hit(at(x, y))
                })
                .count()
        };
        let blue = |p: [u8; 4]| p[2] > 200 && p[0] < 128;
        let red = |p: [u8; 4]| p[0] > 200 && p[2] < 128;
        let [lx, ly] =
            warp.world_to_screen([tile::mercator_x(200.7 - 360.0), tile::mercator_y(0.0)]);
        assert!(
            lx > 200.0 && lx < 250.0,
            "line toward the eastern limb: {lx}"
        );
        let line = run(0, ly as usize, 1, 0, &blue);
        assert!(
            (5..=7).contains(&line),
            "line {line} px wide at ({lx}, {ly})"
        );
        let [cx, cy] =
            warp.world_to_screen([tile::mercator_x(190.0 - 360.0), tile::mercator_y(25.0)]);
        let (across, down) = (
            run(0, cy as usize, 1, 0, &red),
            run(cx as usize, 0, 0, 1, &red),
        );
        assert!(
            (11..=13).contains(&across) && (11..=13).contains(&down),
            "circle {across}×{down} px at ({cx}, {cy})"
        );
        // A polygon over many tiles of a Japan Plane Rectangular view: no seams between tiles.
        let style = r##"{"version":8,"sources":{"g":{"type":"geojson","data":{"type":"Polygon","coordinates":[[[130,30],[150,30],[150,42],[130,42],[130,30]]]}}},
            "layers":[{"id":"bg","type":"background","paint":{"background-color":"#fff"}},{"id":"f","type":"fill","source":"g","paint":{"fill-color":"#f00"}}]}"##;
        let jgd = reproject::ProjectedView {
            projjson: test_crs::projected(
                "Transverse Mercator",
                test_crs::GRS80,
                &[
                    ("Latitude of natural origin", 36.0),
                    ("Longitude of natural origin", 139.833333333333),
                    ("Scale factor at natural origin", 0.9999),
                ],
            ),
            bbox: [41_000.0, -30_000.0, 101_000.0, 30_000.0],
            width: 256,
            height: 256,
            pixel_ratio: 2.0,
            margin: 0,
        };
        let (rgba, renderer) = render(style, &jgd);
        let tiles = renderer.resolve(&renderer.sources["g"]).0.len();
        assert!(tiles > 1, "{tiles} tiles");
        assert!(rgba.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
    }

    #[test]
    fn projected_dashes_restart_at_each_line_of_a_feature() {
        // Two lines meeting end to start: dashed as one MultiLineString or as two features, each
        // line's dashes count from its own start.
        let render = |data: &str| {
            let style = format!(
                r##"{{"version":8,"sources":{{"g":{{"type":"geojson","data":{data}}}}},
                "layers":[{{"id":"l","type":"line","source":"g","paint":{{"line-color":"#00f","line-width":2,"line-dasharray":[3,2]}}}}]}}"##
            );
            let view = reproject::ProjectedView {
                projjson: test_crs::geographic(),
                bbox: [-1.0, -1.0, 11.0, 11.0],
                width: 120,
                height: 120,
                pixel_ratio: 1.0,
                margin: 0,
            };
            let mut r = Renderer::new(&style, View::tile(0, 0, 0, 512)).unwrap();
            r.set_projected_view(&view).unwrap();
            assert!(r.needs().is_empty());
            r.render().unwrap()
        };
        let (a, b) = ([[0.0, 0.0], [10.0, 0.0]], [[10.0, 0.0], [10.0, 10.0]]);
        let multi = render(&format!(r#"{{"type":"MultiLineString","coordinates":[{a:?},{b:?}]}}"#));
        let separate = render(&format!(
            r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","properties":{{}},"geometry":{{"type":"LineString","coordinates":{a:?}}}}},{{"type":"Feature","properties":{{}},"geometry":{{"type":"LineString","coordinates":{b:?}}}}}]}}"#
        ));
        assert!(multi.iter().any(|&v| v != 0));
        assert!(multi == separate);
    }

    #[test]
    fn small_projected_world_views_warp_the_whole_raster() {
        // A world image in a 64×32 Equal Earth world: its Mercator zoom is below the minimum
        // (-2), and the picture must still cover the whole view.
        let style = r##"{"version":8,"sources":{"i":{"type":"image","url":"/i.png","coordinates":[[-180,85],[180,85],[180,-85],[-180,-85]]}},
            "layers":[{"id":"r","type":"raster","source":"i","paint":{"raster-fade-duration":0}}]}"##;
        let png = encode_png(&[255, 0, 0, 255].repeat(16), 4, 4).unwrap();
        let e = 17_243_959.06;
        let view = reproject::ProjectedView {
            projjson: test_crs::projected("Equal Earth", test_crs::WGS84, &[]),
            bbox: [-e, -e / 2.0, e, e / 2.0],
            width: 64,
            height: 32,
            pixel_ratio: 1.0,
            margin: 0,
        };
        let mut r = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
        r.set_projected_view(&view).unwrap();
        for need in r.needs() {
            r.provide(&need.url, Some(&png));
        }
        let rgba = r.render().unwrap();
        // Along the equator, from 150°W to 150°E.
        for x in 6..58 {
            assert_eq!(rgba[(16 * 64 + x) * 4 + 3], 255, "x {x}");
        }
    }

    #[test]
    fn projected_lines_across_the_antimeridian_stay_at_the_edges() {
        // Equal Earth's antimeridian is at 180°: a line from 170°E to 170°W shows at both edges of
        // the world, never across it.
        let style = r##"{"version":8,"sources":{"g":{"type":"geojson","data":{"type":"LineString","coordinates":[[170,10],[190,10]]}}},
            "layers":[{"id":"bg","type":"background","paint":{"background-color":"#fff"}},
            {"id":"l","type":"line","source":"g","paint":{"line-color":"#00f","line-width":4}}]}"##;
        let (w, h) = (512usize, 256usize);
        let e = 17_243_959.06;
        let view = reproject::ProjectedView {
            projjson: test_crs::projected("Equal Earth", test_crs::WGS84, &[]),
            bbox: [-e, -e / 2.0, e, e / 2.0],
            width: w as u32,
            height: h as u32,
            pixel_ratio: 1.0,
            margin: 0,
        };
        let mut r = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
        r.set_projected_view(&view).unwrap();
        assert!(r.needs().is_empty());
        let rgba = r.render().unwrap();
        let [_, y] = r
            .projected
            .as_ref()
            .unwrap()
            .1
            .world_to_screen([tile::mercator_x(175.0), tile::mercator_y(10.0)]);
        let blue: Vec<usize> = (0..w)
            .filter(|&x| {
                let p = &rgba[(y as usize * w + x) * 4..][..4];
                p[2] > 200 && p[0] < 128
            })
            .collect();
        assert!(
            blue.iter().any(|&x| x < w / 10) && blue.iter().any(|&x| x > w * 9 / 10),
            "{blue:?}"
        );
        assert!(
            blue.iter().all(|&x| x < w / 10 || x > w * 9 / 10),
            "line across the map: {blue:?}"
        );
    }

    #[test]
    fn projected_views_west_of_an_eastern_central_meridian_draw() {
        // Equal Earth Asia-Pacific (central meridian 150°E): a view of the Americas (west of 0°,
        // more than 180° from 150°E in raw longitude) still draws.
        let style = r##"{"version":8,"sources":{"g":{"type":"geojson","data":{"type":"Polygon","coordinates":[[[-120,0],[-80,0],[-80,40],[-120,40],[-120,0]]]}}},
            "layers":[{"id":"f","type":"fill","source":"g","paint":{"fill-color":"#f00"}},
            {"id":"c","type":"circle","source":"g","paint":{"circle-color":"#00f","circle-radius":3}}]}"##;
        let crs = test_crs::projected("Equal Earth", test_crs::WGS84, &[("Longitude of natural origin", 150.0)]);
        let [x, y] = projection::Projection::parse(&crs)
            .unwrap()
            .forward(-100.0, 20.0);
        let d = 300_000.0;
        let view = reproject::ProjectedView {
            projjson: crs.clone(),
            bbox: [x - d, y - d, x + d, y + d],
            width: 64,
            height: 64,
            pixel_ratio: 1.0,
            margin: 0,
        };
        let mut r = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
        r.set_projected_view(&view).unwrap();
        assert!(r.needs().is_empty());
        let rgba = r.render().unwrap();
        assert!(rgba.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
    }

    #[test]
    fn projected_patterns_and_dashes_repeat_evenly_on_the_output() {
        // An 8 px pattern (red | blue halves) and 8 px dashes, across Himawari's disk: the period
        // on the output is the same near the limb (where the disk is compressed) as at the center.
        let row: Vec<u8> = (0..8).flat_map(|x| if x < 4 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        let png = encode_png(&row.repeat(8), 8, 8).unwrap();
        let r = 5_500_000.0;
        let view = reproject::ProjectedView {
            projjson: test_crs::himawari(),
            bbox: [-r, -r, r, r],
            width: 256,
            height: 256,
            pixel_ratio: 1.0,
            margin: 0,
        };
        let render_view = |style: &str, view: &reproject::ProjectedView| -> Vec<u8> {
            let mut renderer = Renderer::new(style, View::tile(0, 0, 0, 512)).unwrap();
            renderer.set_projected_view(view).unwrap();
            for need in renderer.needs() {
                let json = br#"{"stripe":{"x":0,"y":0,"width":8,"height":8,"pixelRatio":1}}"#;
                renderer.provide(
                    &need.url,
                    Some(if need.url.ends_with(".json") {
                        json
                    } else {
                        &png
                    }),
                );
            }
            renderer.render().unwrap()
        };
        let render = |style: &str| render_view(style, &view);
        // Color changes along the equator's row in a span of 48 px. Like on a Mercator map, the
        // pattern and the dashes are 8 px at the integer zoom, so larger at this one.
        let changes = |rgba: &[u8], x0: usize, is_a: &dyn Fn(&[u8]) -> bool| {
            let row = |x: usize| is_a(&rgba[(128 * 256 + x) * 4..][..4]);
            (x0..x0 + 48).filter(|&x| row(x) != row(x + 1)).count()
        };
        let red = |p: &[u8]| p[0] > p[2];
        let fill = render(
            r##"{"version":8,"sprite":"/sprite","sources":{"g":{"type":"geojson","data":{"type":"Polygon","coordinates":[[[60,-60],[220,-60],[220,60],[60,60],[60,-60]]]}}},
            "layers":[{"id":"f","type":"fill","source":"g","paint":{"fill-pattern":"stripe"}},{"id":"b","type":"background","paint":{"background-pattern":"stripe"}}]}"##,
        );
        let (limb, center) = (changes(&fill, 12, &red), changes(&fill, 104, &red));
        assert!(
            center >= 4 && limb.abs_diff(center) <= 1,
            "fill-pattern changes: limb {limb}, center {center}"
        );
        // Split into two tiles (each with its own Mercator zoom), the pattern runs on across the seam.
        let half = |x0: f64, x1: f64| reproject::ProjectedView {
            bbox: [x0, -r, x1, r],
            width: 128,
            ..view.clone()
        };
        let style = r##"{"version":8,"sprite":"/sprite","sources":{},"layers":[{"id":"b","type":"background","paint":{"background-pattern":"stripe"}}]}"##;
        let (whole, left, right) = (
            render(style),
            render_view(style, &half(-r, 0.0)),
            render_view(style, &half(0.0, r)),
        );
        let tiled = |x: usize| {
            if x < 128 {
                &left[(128 * 128 + x) * 4..][..4]
            } else {
                &right[(128 * 128 + x - 128) * 4..][..4]
            }
        };
        let differ = (0..256)
            .filter(|&x| red(&whole[(128 * 256 + x) * 4..][..4]) != red(tiled(x)))
            .count();
        assert!(
            differ <= 2,
            "{differ} pixels of the equator differ between tiled and whole renders"
        );
        let dashes = render(
            r##"{"version":8,"sources":{"g":{"type":"geojson","data":{"type":"LineString","coordinates":[[60,0],[220,0]]}}},
            "layers":[{"id":"bg","type":"background","paint":{"background-color":"#fff"}},{"id":"l","type":"line","source":"g","paint":{"line-color":"#f00","line-width":2,"line-dasharray":[2,2]}}]}"##,
        );
        let dash = |p: &[u8]| p[1] < 128;
        let (limb, center) = (changes(&dashes, 12, &dash), changes(&dashes, 104, &dash));
        assert!(
            center >= 4 && limb.abs_diff(center) <= 1,
            "dash changes: limb {limb}, center {center}"
        );
    }
}
