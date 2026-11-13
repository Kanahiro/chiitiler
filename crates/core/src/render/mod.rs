//! Layer drawing: background, fill, line, circle, emulating MapLibre's buckets and shaders.

pub mod dem;
pub mod heatmap;
mod line;
mod pattern;
pub mod raster;
pub mod raster_layer;

use crate::expression::{self as ex, CanonicalTileId, Object, Value};
use crate::mvt::{self, GeomType};
use crate::style::{Layer, PropValue};
use crate::tile::{RenderTile, Transform};
use raster::{Mask, Rgba, Target};
use std::cell::RefCell;
use crate::hash::HashMap;
use std::rc::Rc;

pub const EXTENT: f64 = 8192.0;

/// Longest segment (tile units) of lines and polygon rings drawn into a projected output, where
/// projected segments curve.
const PROJECTED_SEGMENT: f64 = EXTENT / 32.0;

/// `line` with segments longer than `max` split evenly.
pub fn densify(line: &[[f64; 2]], max: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::with_capacity(line.len());
    for (i, &p) in line.iter().enumerate() {
        if let Some(&a) = i.checked_sub(1).map(|j| &line[j]) {
            let n = (((p[0] - a[0]).powi(2) + (p[1] - a[1]).powi(2)).sqrt() / max).ceil() as usize;
            out.extend((1..n).map(|k| {
                let t = k as f64 / n as f64;
                [a[0] + (p[0] - a[0]) * t, a[1] + (p[1] - a[1]) * t]
            }));
        }
        out.push(p);
    }
    out
}

/// Splits every triangle edge longer than `max` at its midpoint (appended to `points`) until none
/// is, for projections that curve them. An edge is split the same way in both of its triangles, so
/// the mesh stays free of T-junctions (and of cracks between triangles).
fn subdivide(points: &mut Vec<[f64; 2]>, triangles: &[usize], max: f64) -> Vec<usize> {
    let mut mids: HashMap<(usize, usize), usize> = HashMap::default();
    let mut out = Vec::with_capacity(triangles.len());
    let mut todo: Vec<[usize; 3]> = triangles
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    while let Some(t) = todo.pop() {
        let long = |a: usize, b: usize, points: &[[f64; 2]]| {
            let (p, q) = (points[a], points[b]);
            (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) > max * max
        };
        // Edge i runs from t[i] to t[(i + 1) % 3].
        let (mut split, mut n) = ([0; 3], 0);
        for i in 0..3 {
            if long(t[i], t[(i + 1) % 3], points) {
                split[n] = i;
                n += 1;
            }
        }
        let split = &split[..n];
        if split.is_empty() {
            out.extend(t);
            continue;
        }
        let mut mid = |a: usize, b: usize, points: &mut Vec<[f64; 2]>| {
            *mids.entry((a.min(b), a.max(b))).or_insert_with(|| {
                let (p, q) = (points[a], points[b]);
                points.push([(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0]);
                points.len() - 1
            })
        };
        // Rotate so that the split edges come first: (a, b) and, then, (b, c).
        let r = match split {
            [i] | [i, _, _] => *i,
            [0, 2] => 2,
            [i, _] => *i,
            _ => unreachable!(),
        };
        let [a, b, c] = [t[r], t[(r + 1) % 3], t[(r + 2) % 3]];
        let mab = mid(a, b, points);
        match split.len() {
            1 => todo.extend([[a, mab, c], [mab, b, c]]),
            2 => {
                let mbc = mid(b, c, points);
                todo.extend([[mab, b, mbc], [a, mab, mbc], [a, mbc, c]]);
            }
            _ => {
                let (mbc, mca) = (mid(b, c, points), mid(c, a, points));
                todo.extend([[a, mab, mca], [mab, b, mbc], [mca, mbc, c], [mab, mbc, mca]]);
            }
        }
    }
    out
}

/// A feature's lines or rings as drawn with `ctx`: split for a projected output.
pub fn drawn_geometry<'a>(
    ctx: &Ctx,
    geometry: &'a [Vec<[f64; 2]>],
) -> std::borrow::Cow<'a, [Vec<[f64; 2]>]> {
    if ctx.transform.is_projected() {
        geometry
            .iter()
            .map(|l| densify(l, PROJECTED_SEGMENT))
            .collect::<Vec<_>>()
            .into()
    } else {
        geometry.into()
    }
}

pub struct Ctx<'a> {
    pub transform: &'a Transform,
    pub global_state: &'a Rc<Object>,
    pub images: &'a HashMap<String, Rc<crate::sprite::StyleImage>>,
    // A context borrows one immutable transform and lives for one render. Keeping the
    // cache here avoids stale masks after a camera clamp or a change in resolved tiles.
    masks: RefCell<HashMap<(usize, usize, Vec<RenderTile>), Rc<Vec<Mask>>>>,
    /// Backfilled DEM tiles by source id, aligned with the source's resolved tiles.
    dems: RefCell<HashMap<String, Rc<Vec<Option<dem::DemTile>>>>>,
    /// `rotations` by tile.
    rotations: RefCell<HashMap<RenderTile, Rc<RotationGrid>>>,
}

/// Camera-independent bucket data of one tile, by layout group. It lives as long as the tile's
/// data stays cached, so later renders of other views reuse it.
pub type TileBuckets = RefCell<HashMap<String, Rc<dyn std::any::Any>>>;

/// A tile's features for one style layer, with its clip mask.
pub struct TileFeatures<'a> {
    pub tile: RenderTile,
    pub mask: &'a Mask,
    pub layer: &'a mvt::Layer,
    /// Where `bucket` keeps its results; `None` builds them afresh.
    pub buckets: Option<&'a TileBuckets>,
}

impl TileFeatures<'_> {
    /// `build`'s result for `layer` in this tile, shared by its layout group. It must depend
    /// only on the tile, the layer's source data and layout, and whether `ctx` is projected
    /// (geometry is split then), not on paint or the camera: it is reused across layers and renders.
    pub fn bucket<T: 'static>(&self, layer: &Layer, ctx: &Ctx, build: impl FnOnce() -> T) -> Rc<T> {
        if ctx.transform.is_projected() {
            return self.bucket_keyed(&format!("{}\0projected", layer.layout_group), build);
        }
        self.bucket_keyed(&layer.layout_group, build)
    }

    /// `bucket` under an explicit key, for buckets that also depend on more than the tile's data
    /// region and layout (e.g. the world copy or the pixel ratio).
    pub fn bucket_keyed<T: 'static>(&self, key: &str, build: impl FnOnce() -> T) -> Rc<T> {
        let Some(cache) = self.buckets else { return Rc::new(build()) };
        if let Some(Ok(b)) = cache.borrow().get(key).map(|b| Rc::clone(b).downcast::<T>()) {
            return b;
        }
        let b = Rc::new(build());
        cache.borrow_mut().insert(key.to_string(), b.clone());
        b
    }
}

/// The rotations (sin, cos) of `Transform::local_frame` on a grid over a tile, interpolated in
/// between: they vary slowly, and each costs two extra projections.
pub struct RotationGrid(Vec<(f64, f64)>);

impl RotationGrid {
    const N: usize = 8;

    /// At tile point `p` (tile units).
    pub fn at(&self, p: [f64; 2]) -> (f64, f64) {
        let n = Self::N;
        let g = |v: f64| (v / EXTENT * n as f64).clamp(0.0, n as f64 - 1e-9);
        let (gx, gy) = (g(p[0]), g(p[1]));
        let (i, j, fx, fy) = (gx as usize, gy as usize, gx.fract(), gy.fract());
        let at = |i: usize, j: usize| self.0[j * (n + 1) + i];
        let lerp = |k: fn((f64, f64)) -> f64| {
            (k(at(i, j)) * (1.0 - fx) + k(at(i + 1, j)) * fx) * (1.0 - fy)
                + (k(at(i, j + 1)) * (1.0 - fx) + k(at(i + 1, j + 1)) * fx) * fy
        };
        let (s, c) = (lerp(|r| r.0), lerp(|r| r.1));
        let len = (s * s + c * c).sqrt();
        if len > 0.0 {
            (s / len, c / len)
        } else {
            (0.0, 1.0)
        }
    }
}

/// A fill layer's polygons in one tile, triangulated.
struct FillBucket {
    features: Vec<BucketFeature>,
    /// Per polygon feature (by index into `features`): its polygons.
    polygons: Vec<(usize, Vec<FillPolygon>)>,
}

struct FillPolygon {
    /// Ring vertices, concatenated (exterior first), in tile units.
    points: Vec<[f64; 2]>,
    ring_lens: Vec<usize>,
    /// `earcut` indices into `points`.
    triangles: Vec<usize>,
}

impl FillBucket {
    fn new(layer: &Layer, ctx: &Ctx, tf: &TileFeatures) -> Self {
        let features = ctx.bucket_features(layer, tf, Some("fill-sort-key"));
        let mut earcut = earcut::Earcut::new();
        let polygons = features
            .iter()
            .enumerate()
            .filter(|(_, f)| f.ty == GeomType::Polygon)
            .map(|(i, f)| {
                let rings = drawn_geometry(ctx, &f.geometry);
                let polygons = classify_ring_indices(&rings, 500)
                    .into_iter()
                    .map(|polygon| {
                        let polygon: Vec<&Vec<[f64; 2]>> = polygon.into_iter().map(|i| &rings[i]).collect();
                        let len = polygon.iter().map(|r| r.len()).sum();
                        let mut points: Vec<[f64; 2]> = Vec::with_capacity(len);
                        let mut holes = Vec::with_capacity(polygon.len().saturating_sub(1));
                        for (i, ring) in polygon.iter().enumerate() {
                            if i > 0 {
                                holes.push(points.len());
                            }
                            points.extend_from_slice(ring);
                        }
                        let mut triangles = Vec::new();
                        earcut.earcut(points.iter().copied(), &holes, &mut triangles);
                        triangles.shrink_to_fit();
                        // Long interior edges curve in a projected output too (after the rings' points).
                        if ctx.transform.is_projected() {
                            triangles = subdivide(&mut points, &triangles, PROJECTED_SEGMENT);
                        }
                        FillPolygon { points, ring_lens: polygon.iter().map(|r| r.len()).collect(), triangles }
                    })
                    .collect();
                (i, polygons)
            })
            .collect();
        // Only the triangulated polygons are drawn.
        let mut features = features;
        for f in &mut features {
            f.geometry = Vec::new();
        }
        Self { features, polygons }
    }
}

/// A feature ready for bucket building: geometry in EXTENT (8192) units.
pub struct BucketFeature {
    pub feature: ex::Feature,
    pub ty: GeomType,
    pub geometry: Vec<Vec<[f64; 2]>>,
    pub sort_key: f64,
    /// Index of the feature in its source layer.
    pub index: usize,
}

impl<'a> Ctx<'a> {
    pub fn new(transform: &'a Transform, global_state: &'a Rc<Object>, images: &'a HashMap<String, Rc<crate::sprite::StyleImage>>) -> Self {
        Self { transform, global_state, images, masks: RefCell::new(HashMap::default()), dems: RefCell::new(HashMap::default()), rotations: RefCell::default() }
    }

    /// `Transform::local_frame`'s rotations over a tile (see `RotationGrid`), built once per render.
    pub fn rotations(&self, tile: &RenderTile) -> Rc<RotationGrid> {
        Rc::clone(self.rotations.borrow_mut().entry(*tile).or_insert_with(|| {
            let to_world = self.tile_to_world(tile);
            let step = EXTENT / RotationGrid::N as f64;
            let n = RotationGrid::N;
            Rc::new(RotationGrid(
                (0..=n)
                    .flat_map(|j| (0..=n).map(move |i| [i as f64 * step, j as f64 * step]))
                    .map(|q| self.transform.local_frame(to_world(q)).1)
                    .collect(),
            ))
        }))
    }

    /// The DEM tiles of `source`, built once per render and shared by its hillshade and
    /// color-relief layers.
    pub fn dem_tiles(&self, source: &str, build: impl FnOnce() -> Vec<Option<dem::DemTile>>) -> Rc<Vec<Option<dem::DemTile>>> {
        if let Some(cached) = self.dems.borrow().get(source) {
            return Rc::clone(cached);
        }
        let tiles = Rc::new(build());
        self.dems.borrow_mut().insert(source.to_string(), Rc::clone(&tiles));
        tiles
    }

    pub fn tile_px(&self, tile: &RenderTile) -> f64 {
        self.transform.world_size / (1u64 << tile.canonical.z) as f64
    }

    /// World CSS-pixel origin of a tile.
    pub fn tile_origin(&self, tile: &RenderTile) -> [f64; 2] {
        let s = self.tile_px(tile);
        [tile.canonical.x as f64 * s + tile.wrap as f64 * self.transform.world_size, tile.canonical.y as f64 * s]
    }

    /// Tile units (EXTENT) → world CSS pixels.
    pub fn tile_to_world(&self, tile: &RenderTile) -> impl Fn([f64; 2]) -> [f64; 2] + use<> {
        let o = self.tile_origin(tile);
        let s = self.tile_px(tile) / EXTENT;
        move |p| [o[0] + p[0] * s, o[1] + p[1] * s]
    }

    pub fn tile_mask(&self, tile: &RenderTile, w: usize, h: usize) -> Mask {
        let f = self.tile_to_world(tile);
        let q = [[0.0, 0.0], [EXTENT, 0.0], [EXTENT, EXTENT], [0.0, EXTENT]].map(|p| self.transform.world_to_screen(f(p)));
        Mask::from_quad(w, h, q)
    }

    /// A tile owns its quad except where a higher-zoom child covers it. The whole
    /// ordered tile set matters: layers can share masks only for the same coverage.
    pub fn masks(&self, tiles: &[RenderTile], w: usize, h: usize) -> Rc<Vec<Mask>> {
        let key = (w, h, tiles.to_vec());
        if let Some(cached) = self.masks.borrow().get(&key) {
            return Rc::clone(cached);
        }
        if let Some(world) = self.transform.pixel_world().filter(|p| p.len() == w * h) {
            let masks = Rc::new(projected_masks(
                tiles,
                w,
                h,
                world,
                self.transform.central_x(),
            ));
            self.masks.borrow_mut().insert(key, Rc::clone(&masks));
            return masks;
        }
        let quads: Vec<Mask> = tiles.iter().map(|t| self.tile_mask(t, w, h)).collect();
        let masks = Rc::new(tiles.iter().enumerate().map(|(i, t)| {
            let mut m = quads[i].clone();
            for (j, u) in tiles.iter().enumerate() {
                if u.canonical.z > t.canonical.z && u.wrap == t.wrap && u.canonical.parent_at(t.canonical.z) == t.canonical {
                    m.subtract(&quads[j]);
                }
            }
            m
        }).collect());
        self.masks.borrow_mut().insert(key, Rc::clone(&masks));
        masks
    }

    pub fn canonical(tile: &RenderTile) -> CanonicalTileId {
        CanonicalTileId { z: tile.canonical.z, x: tile.canonical.x, y: tile.canonical.y }
    }

    /// Filter and prepare a tile layer's features (MapLibre `populate`): geometry scaled to
    /// EXTENT, filter evaluated at the tile's zoom, sorted by `sort_key_prop` if set.
    pub fn bucket_features(&self, layer: &Layer, tf: &TileFeatures, sort_key_prop: Option<&str>) -> Vec<BucketFeature> {
        self.filtered_features(layer, tf, sort_key_prop, true)
    }

    /// `bucket_features`, with empty `BucketFeature::geometry` unless `with_geometry`.
    pub fn filtered_features(&self, layer: &Layer, tf: &TileFeatures, sort_key_prop: Option<&str>, with_geometry: bool) -> Vec<BucketFeature> {
        let zoom = tf.tile.overscaled_z as f64;
        let scale = EXTENT / tf.layer.extent as f64;
        let canonical = Some(Self::canonical(&tf.tile));
        let mut out = Vec::new();
        for (index, f) in tf.layer.features.iter().enumerate() {
            let scale_geometry = || -> Vec<Vec<[f64; 2]>> {
                f.geometry.iter().map(|l| l.iter().map(|p| [(p[0] as f64 * scale).round(), (p[1] as f64 * scale).round()]).collect()).collect()
            };
            // Most filters only inspect properties. Avoid allocating and transforming
            // geometry for rejected features; geometry expressions still see EXTENT units.
            let geometry = layer.needs_geometry.then(scale_geometry);
            let feature = ex::Feature {
                id: f.id.clone(),
                geom_type: Some(f.ty.name()),
                properties: f.properties.clone(),
                geometry: geometry.clone(),
            };
            if let Some(filter) = &layer.filter {
                let mut ctx = ex::EvalCtx {
                    globals: ex::Globals { zoom, global_state: Some(self.global_state.clone()), ..Default::default() },
                    feature: Some(&feature),
                    canonical,
                    ..Default::default()
                };
                if !filter.evaluate(&mut ctx).truthy() {
                    continue;
                }
            }
            let geometry = if with_geometry { geometry.unwrap_or_else(scale_geometry) } else { Vec::new() };
            let sort_key = match sort_key_prop.map(|p| layer.layout(p)) {
                Some(p) if !p.is_constant() => num(&p.eval(zoom, Some(&feature), canonical, self.global_state)),
                _ => 0.0,
            };
            out.push(BucketFeature { feature, ty: f.ty, geometry, sort_key, index });
        }
        if sort_key_prop.is_some_and(|p| !layer.layout(p).is_constant()) {
            out.sort_by(|a, b| a.sort_key.partial_cmp(&b.sort_key).unwrap_or(std::cmp::Ordering::Equal));
        }
        out
    }

    /// Evaluate a paint property for a feature at the current zoom.
    pub fn paint(&self, layer: &Layer, name: &str, f: Option<&BucketFeature>, tile: &RenderTile) -> Value {
        self.eval(layer.paint(name), f, tile, self.transform.zoom)
    }

    pub fn layout(&self, layer: &Layer, name: &str, f: Option<&BucketFeature>, tile: &RenderTile) -> Value {
        self.eval(layer.layout(name), f, tile, tile.overscaled_z as f64)
    }

    fn eval(&self, p: &PropValue, f: Option<&BucketFeature>, tile: &RenderTile, zoom: f64) -> Value {
        p.eval(zoom, f.map(|f| &f.feature), Some(Self::canonical(tile)), self.global_state)
    }

    /// `translatePosition` as a world-space (CSS px) offset.
    pub fn translate(&self, layer: &Layer, prefix: &str, tile: &RenderTile) -> [f64; 2] {
        let t = num2(&self.paint(layer, &format!("{prefix}-translate"), None, tile));
        let anchor = self.paint(layer, &format!("{prefix}-translate-anchor"), None, tile);
        if anchor.as_str() == Some("viewport") {
            // Screen-aligned: undo the map rotation.
            let a = -self.transform.bearing_rad;
            let (s, c) = a.sin_cos();
            [t[0] * c - t[1] * s, t[0] * s + t[1] * c]
        } else {
            t
        }
    }
}

pub fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

pub fn num2(v: &Value) -> [f64; 2] {
    match v {
        Value::Array(a) if a.len() >= 2 => [num(&a[0]), num(&a[1])],
        _ => [0.0, 0.0],
    }
}

/// Premultiplied color times opacity.
pub fn color(v: &Value, opacity: f64) -> Rgba {
    match v {
        Value::Color(c) => [(c.r * opacity) as f32, (c.g * opacity) as f32, (c.b * opacity) as f32, (c.a * opacity) as f32],
        _ => [0.0; 4],
    }
}

/// Which part of a layer to draw. MapLibre draws opaque backgrounds and fill interiors in a
/// top-down opaque pass, then everything else bottom-up (`Target::occ` stands in for the depth
/// buffer that lets the opaque pass skip what it hides).
#[derive(Clone, Copy, PartialEq)]
pub enum Pass {
    /// Everything, in style order (no occlusion buffer).
    All,
    /// Only the opaque-pass part; the draw function returns whether there was one.
    Opaque,
    /// Everything but the opaque-pass part, which was drawn already.
    Translucent,
}

/// Returns whether anything was drawn in `Pass::Opaque`.
pub fn draw_background(target: &mut Target, layer: &Layer, ctx: &Ctx, raw: &serde_json::Value, pass: Pass) -> bool {
    let tile = dummy_tile();
    let opacity = num(&ctx.paint(layer, "background-opacity", None, &tile));
    if opacity == 0.0 {
        return false;
    }
    let t = ctx.transform;
    // A set pattern is cross-faded (evaluated at the integer zoom); with no resolvable image
    // MapLibre skips the layer.
    let image = if raw["paint"].get("background-pattern").is_some() {
        let v = layer.paint("background-pattern").eval(t.zoom.floor(), None, None, ctx.global_state);
        match pattern::image_name(&v).and_then(|n| ctx.images.get(&n)) {
            Some(img) => Some(img),
            None => return false,
        }
    } else {
        None
    };
    let Some(img) = image else {
        let c = color(&ctx.paint(layer, "background-color", None, &tile), opacity);
        if pass == Pass::Opaque && c[3] != 1.0 {
            return false;
        }
        // In a projected output: wherever the projection has Mercator coordinates.
        if let Some(world) = t.pixel_world().filter(|p| p.len() == target.w * target.h) {
            for (i, m) in world.iter().enumerate() {
                if m[0].is_finite() {
                    if c[3] == 1.0 {
                        target.cover(i, c);
                    } else if target.visible(i) {
                        target.blend(i, c);
                    }
                }
            }
            return true;
        }
        // Drawn over the covering tiles, i.e. only within the world's vertical extent.
        for y in 0..target.h {
            for x in 0..target.w {
                let wy = t.screen_to_world([x as f64 + 0.5, y as f64 + 0.5])[1];
                if (0.0..t.world_size).contains(&wy) {
                    if c[3] == 1.0 {
                        target.cover(y * target.w + x, c);
                    } else if target.visible(y * target.w + x) {
                        target.blend(y * target.w + x, c);
                    }
                }
            }
        }
        return true;
    };
    if pass == Pass::Opaque {
        return false;
    }
    let size = pattern::display_size(img);
    // In a projected output: the pattern on the output's pixels, wherever there is map.
    if let Some(world) = t.pixel_world().filter(|p| p.len() == target.w * target.h) {
        let tp = pattern::TilePattern::new(ctx, &tile);
        for (i, m) in world.iter().enumerate() {
            if m[0].is_finite() && target.visible(i) {
                let screen = [(i % target.w) as f64 + 0.5, (i / target.w) as f64 + 0.5];
                let v = tp.pos(size, [0.0; 2], screen);
                target.blend(i, pattern::sample_fill(img, v).map(|c| c * opacity as f32));
            }
        }
        return true;
    }
    // Pattern: one quad per covering tile at the integer zoom.
    let z = t.zoom.floor().max(0.0) as i32;
    for (x, y, wrap) in t.covering_tiles(z) {
        let rt = RenderTile { overscaled_z: z as u8, wrap, canonical: crate::tile::TileId { z: z as u8, x: x as u32, y: y as u32 } };
        let tp = pattern::TilePattern::new(ctx, &rt);
        let to_world = ctx.tile_to_world(&rt);
        let corners = [[0.0, 0.0], [EXTENT, 0.0], [0.0, EXTENT], [EXTENT, EXTENT]];
        let screen = corners.map(|p| t.world_to_screen(to_world(p)));
        let vary = corners.map(|p| tp.pattern_pos(size, p));
        for tri in [[1, 0, 2], [1, 2, 3]] {
            raster::triangle(target, None, tri.map(|i| screen[i]), tri.map(|i| vary[i]), &mut |v: &[f32; 2]| {
                Some(pattern::sample_fill(img, *v).map(|c| c * opacity as f32))
            });
        }
    }
    true
}

/// How a fill feature is painted: plain color, or an image pattern (optionally SDF, tinted
/// with the fill color).
enum FillPaint<'a> {
    Color(Rgba),
    Pattern { img: &'a crate::sprite::StyleImage, tint: Rgba, opacity: f32 },
}

impl FillPaint<'_> {
    /// Fragment color for pattern position `v` with screen gradients `g` (for `fwidth`).
    fn shade(&self, v: [f32; 2], g: &[[f32; 2]; 2]) -> Rgba {
        match self {
            FillPaint::Color(c) => *c,
            FillPaint::Pattern { img, tint, opacity } => {
                let c = pattern::sample_fill(img, v);
                if img.sdf {
                    let a = |d: [f32; 2]| pattern::sample_fill(img, [v[0] + d[0], v[1] + d[1]])[3];
                    let fwidth = (a(g[0]) - c[3]).abs() + (a(g[1]) - c[3]).abs();
                    let edge = (256.0 - 64.0) / 256.0;
                    let gamma = (fwidth * 0.5).max(1.0 / 255.0 / 16.0);
                    let alpha = raster::smoothstep(edge - gamma, edge + gamma, c[3]);
                    tint.map(|k| k * alpha * opacity)
                } else {
                    c.map(|k| k * opacity)
                }
            }
        }
    }
}

/// Returns whether anything was drawn in `Pass::Opaque`.
pub fn draw_fill(target: &mut Target, layer: &Layer, ctx: &Ctx, tiles: &[TileFeatures], raw: &serde_json::Value, pass: Pass) -> bool {
    let has_outline_color = raw["paint"].get("fill-outline-color").is_some();
    let has_pattern = raw["paint"].get("fill-pattern").is_some();
    let constant_opaque = !has_pattern
        && layer.paint("fill-color").is_constant()
        && layer.paint("fill-opacity").is_constant()
        && matches!(ctx.paint(layer, "fill-color", None, &tiles.first().map_or(dummy_tile(), |t| t.tile)), Value::Color(c) if c.a == 1.0)
        && tiles.first().is_some_and(|t| num(&ctx.paint(layer, "fill-opacity", None, &t.tile)) == 1.0);
    if pass == Pass::Opaque && !constant_opaque {
        return false;
    }
    let antialias = pass != Pass::Opaque && tiles.first().is_some_and(|t| ctx.paint(layer, "fill-antialias", None, &t.tile).truthy());
    // The layer's opaque interior hides its own outline (the depth test). In `Pass::All`,
    // `covered` records the interior (tile masks are disjoint, so one buffer serves all tiles);
    // in `Pass::Translucent`, the opaque pass recorded it in `Target::occ` as this layer.
    let outline_hidden = constant_opaque && antialias && !has_outline_color;
    let mut covered = vec![false; if outline_hidden && pass == Pass::All { target.w * target.h } else { 0 }];
    let pattern_prop = layer.paint("fill-pattern");
    // Per tile: its bucket and the features to outline (index into `polygons`, outline paint,
    // and the fill pattern's size if any). The edges are computed again in the outline pass:
    // holding them meanwhile took more memory than the buckets.
    let mut prepared = Vec::new();
    let screen_of = |tf: &TileFeatures| {
        let to_world = ctx.tile_to_world(&tf.tile);
        let tr = ctx.translate(layer, "fill", &tf.tile);
        move |p: [f64; 2]| {
            let w = to_world(p);
            ctx.transform.world_to_screen([w[0] + tr[0], w[1] + tr[1]])
        }
    };
    let vary_of = |tp: &pattern::TilePattern, size: Option<[f32; 2]>, points: &[[f64; 2]], screen: &[[f64; 2]]| -> Vec<[f32; 2]> {
        match size {
            Some(size) => points.iter().zip(screen).map(|(p, s)| tp.pos(size, *p, *s)).collect(),
            None => vec![[0.0; 2]; screen.len()],
        }
    };
    for tf in tiles {
        let bucket = tf.bucket(layer, ctx, || FillBucket::new(layer, ctx, tf));
        let to_screen = screen_of(tf);
        let tp = pattern::TilePattern::new(ctx, &tf.tile);
        let mut outlines = Vec::new();
        for (j, (i, polygons)) in bucket.polygons.iter().enumerate() {
            let f = &bucket.features[*i];
            let opacity = num(&ctx.paint(layer, "fill-opacity", Some(f), &tf.tile));
            let fill_color = color(&ctx.paint(layer, "fill-color", Some(f), &tf.tile), opacity);
            let paint = if has_pattern {
                // Cross-faded "to" image: the integer zoom, or the tile zoom when data-driven.
                let v = if pattern_prop.is_feature_constant() {
                    pattern_prop.eval(ctx.transform.zoom.floor(), None, Some(Ctx::canonical(&tf.tile)), ctx.global_state)
                } else {
                    pattern_prop.eval(tf.tile.overscaled_z as f64, Some(&f.feature), Some(Ctx::canonical(&tf.tile)), ctx.global_state)
                };
                let Some(img) = pattern::image_name(&v).and_then(|n| ctx.images.get(&n)) else { continue };
                FillPaint::Pattern { img, tint: color(&ctx.paint(layer, "fill-color", Some(f), &tf.tile), 1.0), opacity: opacity as f32 }
            } else {
                FillPaint::Color(fill_color)
            };
            let size = match &paint {
                FillPaint::Pattern { img, .. } => pattern::display_size(img),
                FillPaint::Color(_) => [1.0, 1.0],
            };
            let outline_paint = if has_outline_color {
                FillPaint::Color(color(&ctx.paint(layer, "fill-outline-color", Some(f), &tf.tile), opacity))
            } else {
                match &paint {
                    FillPaint::Color(c) => FillPaint::Color(*c),
                    FillPaint::Pattern { img, tint, opacity } => FillPaint::Pattern { img, tint: *tint, opacity: *opacity },
                }
            };
            // Only patterns read the varying (outlines too: theirs is a color or this pattern).
            let pattern_size = matches!(paint, FillPaint::Pattern { .. }).then_some(size);
            if antialias && opacity >= 0.01 {
                outlines.push((j, outline_paint, pattern_size));
            }
            if opacity < 0.01 || pass == Pass::Translucent {
                continue;
            }
            for polygon in polygons {
                let screen: Vec<[f64; 2]> = polygon.points.iter().map(|p| to_screen(*p)).collect();
                let vary = vary_of(&tp, pattern_size, &polygon.points, &screen);
                for t in polygon.triangles.chunks_exact(3) {
                    let p = [screen[t[0]], screen[t[1]], screen[t[2]]];
                    // Projections keep orientation and bend edges gently, except across a
                    // singularity (e.g. near the antipode of an azimuthal projection, which spreads
                    // a triangle's corners around the rim): its chord triangle would cover the map.
                    let tri = [polygon.points[t[0]], polygon.points[t[1]], polygon.points[t[2]]];
                    if ctx.transform.is_projected()
                        && (0..3).any(|i| bent(&to_screen, tri[i], tri[(i + 1) % 3], p[i], p[(i + 1) % 3]))
                    {
                        continue;
                    }
                    if constant_opaque {
                        let w = target.w;
                        raster::for_each_pixel(target.w, target.h, p, |x, y, _| {
                            if tf.mask.get(x, y) {
                                if let Some(c) = covered.get_mut(y * w + x) {
                                    *c = true;
                                }
                                target.cover(y * w + x, fill_color);
                            }
                        });
                        continue;
                    }
                    let v = [vary[t[0]], vary[t[1]], vary[t[2]]];
                    let g = raster::varying_gradients(p, v);
                    raster::triangle(target, Some(tf.mask), p, v, &mut |v: &[f32; 2]| Some(paint.shade(*v, &g)));
                }
            }
        }
        prepared.push((tf, bucket, outlines));
    }
    // Outlines are drawn after all fills (MapLibre's separate outline pass). Reading `occ`
    // while drawing needs it out of the target, so its culling is applied here instead.
    let (w, layer_index) = (target.w, target.layer);
    let occ = if outline_hidden && pass == Pass::Translucent { std::mem::take(&mut target.occ) } else { Vec::new() };
    let cull = target.cull;
    target.cull = cull && occ.is_empty();
    let visible = |i: usize| match (outline_hidden, pass) {
        (true, Pass::All) => !covered[i],
        (true, _) => occ[i] < layer_index || !cull && occ[i] != layer_index,
        _ => true,
    };
    for (tf, bucket, outlines) in prepared {
        let to_screen = screen_of(tf);
        let tp = pattern::TilePattern::new(ctx, &tf.tile);
        for (j, paint, pattern_size) in outlines {
            for polygon in &bucket.polygons[j].1 {
                let screen: Vec<[f64; 2]> = polygon.points.iter().map(|p| to_screen(*p)).collect();
                let vary = vary_of(&tp, pattern_size, &polygon.points, &screen);
                let mut start = 0;
                for &len in &polygon.ring_lens {
                    for i in 1..len {
                        let (a, b) = (start + i - 1, start + i);
                        if ctx.transform.is_projected() && bent(&to_screen, polygon.points[a], polygon.points[b], screen[a], screen[b]) {
                            continue;
                        }
                        let (va, vb) = (vary[a], vary[b]);
                        let none = [[0.0; 2]; 2];
                        raster::line(target, &|x, y| tf.mask.get(x, y) && visible(y * w + x), screen[a], screen[b], &mut |dist, t| {
                            let alpha = 1.0 - raster::smoothstep(0.0, 1.0, dist as f32);
                            let t = t as f32;
                            let v = [va[0] + (vb[0] - va[0]) * t, va[1] + (vb[1] - va[1]) * t];
                            Some(paint.shade(v, &none).map(|c| c * alpha))
                        });
                    }
                    start += len;
                }
            }
        }
    }
    if !occ.is_empty() {
        target.occ = occ;
    }
    target.cull = cull;
    pass == Pass::Opaque
}

/// Tile masks of a projected output: each pixel belongs to the highest-zoom tile containing its
/// Mercator position (tile boundaries curve there, so they are not drawn as quads), unwrapped
/// to within 0.5 of `central_x` like the drawn geometry.
fn projected_masks(
    tiles: &[RenderTile],
    w: usize,
    h: usize,
    world: &[[f64; 2]],
    central_x: Option<f64>,
) -> Vec<Mask> {
    let mut zooms: Vec<u8> = tiles.iter().map(|t| t.canonical.z).collect();
    zooms.sort_unstable_by(|a, b| b.cmp(a));
    zooms.dedup();
    // (z, x across world copies, y) → tile index.
    let index: HashMap<(u8, i64, i64), usize> = tiles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            (
                (
                    t.canonical.z,
                    t.wrap as i64 * (1i64 << t.canonical.z) + t.canonical.x as i64,
                    t.canonical.y as i64,
                ),
                i,
            )
        })
        .collect();
    let mut owners = vec![u32::MAX; w * h];
    // Neighboring pixels mostly fall in the same tiles: the last lookup per zoom.
    type Lookup = ((i64, i64), Option<usize>);
    let mut last: Vec<Option<Lookup>> = vec![None; zooms.len()];
    for (p, m) in world.iter().enumerate() {
        if m[0].is_nan() {
            continue;
        }
        let x = central_x.map_or(m[0], |c| m[0] - (m[0] - c).round());
        let found = zooms.iter().zip(&mut last).find_map(|(&z, last)| {
            let n = (1u64 << z) as f64;
            let key = ((x * n).floor() as i64, (m[1] * n).floor() as i64);
            match *last {
                Some((k, i)) if k == key => i,
                _ => {
                    let i = index.get(&(z, key.0, key.1)).copied();
                    *last = Some((key, i));
                    i
                }
            }
        });
        if let Some(i) = found {
            owners[p] = i as u32;
        }
    }
    Mask::from_owners(w, h, owners, tiles.len())
}

pub(crate) fn dummy_tile() -> RenderTile {
    RenderTile { overscaled_z: 0, wrap: 0, canonical: crate::tile::TileId { z: 0, x: 0, y: 0 } }
}

pub use line::draw_line;

pub fn draw_circle(target: &mut Target, layer: &Layer, ctx: &Ctx, tiles: &[TileFeatures]) {
    let dpr = ctx.transform.pixel_ratio;
    for tf in tiles {
        let features = ctx.bucket_features(layer, tf, Some("circle-sort-key"));
        let to_world = ctx.tile_to_world(&tf.tile);
        let tr = ctx.translate(layer, "circle", &tf.tile);
        for f in &features {
            let p = |name: &str| ctx.paint(layer, name, Some(f), &tf.tile);
            let radius = num(&p("circle-radius")) as f32;
            let blur = num(&p("circle-blur")) as f32;
            let opacity = num(&p("circle-opacity"));
            let stroke_width = num(&p("circle-stroke-width")) as f32;
            let fill = color(&p("circle-color"), opacity);
            let stroke = color(&p("circle-stroke-color"), num(&p("circle-stroke-opacity")));
            let size = (radius + stroke_width) as f64 * dpr;
            let aa_blur = -(1.0 / dpr as f32 / (radius + stroke_width)).max(blur);
            for pt in f.geometry.iter().flatten() {
                if pt[0] < 0.0 || pt[0] >= EXTENT || pt[1] < 0.0 || pt[1] >= EXTENT {
                    continue;
                }
                let w = to_world(*pt);
                let Some(c) = ctx.transform.on_map(w) else { continue };
                let c = if tr == [0.0; 2] { c } else { ctx.transform.world_to_screen([w[0] + tr[0], w[1] + tr[1]]) };
                let q = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
                let pos = q.map(|e| [c[0] + e[0] * size, c[1] + e[1] * size]);
                let v = q.map(|e| [e[0] as f32, e[1] as f32]);
                let mut shade = |e: &[f32; 2]| {
                    let len = (e[0] * e[0] + e[1] * e[1]).sqrt();
                    let opacity_t = raster::smoothstep(0.0, aa_blur, len - 1.0);
                    let color_t = if stroke_width < 0.01 { 0.0 } else { raster::smoothstep(aa_blur, 0.0, len - radius / (radius + stroke_width)) };
                    let c: Rgba = std::array::from_fn(|k| opacity_t * (fill[k] + (stroke[k] - fill[k]) * color_t));
                    (c.iter().any(|&x| x >= 0.5 / 255.0)).then_some(c)
                };
                raster::triangle(target, None, [pos[0], pos[1], pos[2]], [v[0], v[1], v[2]], &mut shade);
                raster::triangle(target, None, [pos[0], pos[2], pos[3]], [v[0], v[2], v[3]], &mut shade);
            }
        }
    }
}

/// Whether the projected segment `a`–`b` (tile units, drawn from `sa` to `sb`) strays from its chord by
/// over a quarter of the chord's length at its midpoint: it passes a singularity of the projection,
/// and its chord cuts across the map. Chords under `BENT_MIN` device pixels are not checked: they can
/// only misdraw that much, and they are most edges.
fn bent(to_screen: impl Fn([f64; 2]) -> [f64; 2], a: [f64; 2], b: [f64; 2], sa: [f64; 2], sb: [f64; 2]) -> bool {
    const BENT_MIN: f64 = 32.0;
    let len2 = (sb[0] - sa[0]).powi(2) + (sb[1] - sa[1]).powi(2);
    if len2 < BENT_MIN * BENT_MIN {
        return false;
    }
    let m = to_screen([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]);
    let d = [(sa[0] + sb[0]) / 2.0 - m[0], (sa[1] + sb[1]) / 2.0 - m[1]];
    16.0 * (d[0] * d[0] + d[1] * d[1]) > len2
}

fn signed_area(ring: &[[f64; 2]]) -> f64 {
    let mut sum = 0.0;
    let len = ring.len();
    for i in 0..len {
        let j = if i == 0 { len - 1 } else { i - 1 };
        sum += (ring[j][0] - ring[i][0]) * (ring[i][1] + ring[j][1]);
    }
    sum
}

/// Group rings into polygons by winding (first ring's winding = outer), keeping at most
/// `max_rings` rings per polygon (largest holes first), like MapLibre's `classifyRings`.
pub fn classify_rings(rings: &[Vec<[f64; 2]>], max_rings: usize) -> Vec<Vec<Vec<[f64; 2]>>> {
    classify_ring_indices(rings, max_rings).into_iter().map(|p| p.into_iter().map(|i| rings[i].clone()).collect()).collect()
}

/// `classify_rings` as indices into `rings`.
pub fn classify_ring_indices(rings: &[Vec<[f64; 2]>], max_rings: usize) -> Vec<Vec<usize>> {
    if rings.len() <= 1 {
        return vec![(0..rings.len()).collect()];
    }
    let mut polygons: Vec<Vec<(f64, usize)>> = Vec::new();
    let mut ccw: Option<bool> = None;
    for (i, ring) in rings.iter().enumerate() {
        let area = signed_area(ring);
        if area == 0.0 {
            continue;
        }
        let ccw = *ccw.get_or_insert(area < 0.0);
        if ccw == (area < 0.0) || polygons.is_empty() {
            polygons.push(vec![(area.abs(), i)]);
        } else {
            polygons.last_mut().unwrap().push((area.abs(), i));
        }
    }
    polygons
        .into_iter()
        .map(|mut p| {
            if max_rings > 1 && p.len() > max_rings {
                p[1..].sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                p.truncate(max_rings);
            }
            p.into_iter().map(|(_, r)| r).collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::TileId;

    #[test]
    fn masks_share_coverage_without_reusing_parent_only_or_wrapped_masks() {
        let transform = Transform::new([0.0, 0.0], 0.0, 0.0, 32.0, 32.0, 1.0);
        let state = Rc::new(Object::default());
        let images = HashMap::default();
        let ctx = Ctx::new(&transform, &state, &images);
        let parent = RenderTile { overscaled_z: 0, wrap: 0, canonical: TileId { z: 0, x: 0, y: 0 } };
        let child = RenderTile { overscaled_z: 1, wrap: 0, canonical: TileId { z: 1, x: 1, y: 1 } };
        let parent_only = ctx.masks(&[parent], 32, 32);
        let family = ctx.masks(&[parent, child], 32, 32);
        assert!(Rc::ptr_eq(&family, &ctx.masks(&[parent, child], 32, 32)));
        assert!(parent_only[0].get(24, 24));
        assert!(!family[0].get(24, 24));
        assert!(family[1].get(24, 24));
        assert!(family[0].get(8, 8));
        assert!(!family[1].get(8, 8));

        let reversed = ctx.masks(&[child, parent], 32, 32);
        for y in 0..32 {
            for x in 0..32 {
                assert_eq!(reversed[0].get(x, y), family[1].get(x, y));
                assert_eq!(reversed[1].get(x, y), family[0].get(x, y));
            }
        }
        let wrapped = ctx.masks(&[parent, RenderTile { wrap: 1, ..child }], 32, 32);
        assert!(wrapped[0].get(24, 24));
        assert!(!wrapped[1].get(24, 24));
        let larger = ctx.masks(&[parent, child], 64, 32);
        assert!(!larger[0].get(63, 24));
        assert!(larger[1].get(63, 24));
        assert!(!Rc::ptr_eq(&family, &larger));
    }

    #[test]
    fn new_render_context_reprojects_the_same_tile_set() {
        let state = Rc::new(Object::default());
        let images = HashMap::default();
        let tile = RenderTile { overscaled_z: 1, wrap: 0, canonical: TileId { z: 1, x: 1, y: 1 } };
        let first = Transform::new([0.0, 0.0], 0.0, 0.0, 32.0, 32.0, 1.0);
        let second = Transform::new([30.0, 0.0], 0.0, 0.0, 32.0, 32.0, 1.0);
        let a = Ctx::new(&first, &state, &images).masks(&[tile], 32, 32);
        let b = Ctx::new(&second, &state, &images).masks(&[tile], 32, 32);
        assert!(!a[0].get(8, 24));
        assert!(b[0].get(8, 24));
    }
}
