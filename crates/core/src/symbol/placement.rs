//! Collision detection and placement (`grid_index.ts`, `collision_index.ts`, `placement.ts`) for
//! a static, unpitched camera: one placement pass, no fading (render tests use `fadeDuration: 0`,
//! where the settled state is "visible iff placed").

use super::anchors::{self, P};
use super::layout::{overlap_mode, CollisionBox, Resolved, SymbolBucket};
use super::shaping::{self, WritingMode};
use crate::render::{num, Ctx, EXTENT};
use crate::style::Layer;
use std::rc::Rc;

pub const VIEWPORT_PADDING: f64 = 100.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlap {
    Never,
    Always,
    Cooperative,
}

impl Overlap {
    pub fn from(s: &str) -> Overlap {
        match s {
            "always" => Overlap::Always,
            "cooperative" => Overlap::Cooperative,
            _ => Overlap::Never,
        }
    }
}

fn overlap_allowed(a: Overlap, b: Overlap) -> bool {
    a == Overlap::Always || (a != Overlap::Never && b != Overlap::Never)
}

/// `GridIndex` (boxes and circles in 25px cells).
pub struct GridIndex {
    box_cells: Vec<Vec<usize>>,
    circle_cells: Vec<Vec<usize>>,
    boxes: Vec<([f64; 4], Overlap, u32)>,
    circles: Vec<([f64; 3], Overlap, u32)>,
    x_cells: usize,
    y_cells: usize,
    width: f64,
    height: f64,
    x_scale: f64,
    y_scale: f64,
}

impl GridIndex {
    pub fn new(width: f64, height: f64, cell: f64) -> GridIndex {
        let (x_cells, y_cells) = ((width / cell).ceil() as usize, (height / cell).ceil() as usize);
        GridIndex {
            box_cells: vec![Vec::new(); x_cells * y_cells],
            circle_cells: vec![Vec::new(); x_cells * y_cells],
            boxes: Vec::new(),
            circles: Vec::new(),
            x_cells,
            y_cells,
            width,
            height,
            x_scale: x_cells as f64 / width,
            y_scale: y_cells as f64 / height,
        }
    }

    fn cells(&self, x1: f64, y1: f64, x2: f64, y2: f64) -> impl Iterator<Item = usize> + use<> {
        let cx = |x: f64, s: f64, n: usize| ((x * s).floor().max(0.0) as usize).min(n - 1);
        let (cx1, cy1) = (cx(x1, self.x_scale, self.x_cells), cx(y1, self.y_scale, self.y_cells));
        let (cx2, cy2) = (cx(x2, self.x_scale, self.x_cells), cx(y2, self.y_scale, self.y_cells));
        let n = self.x_cells;
        (cx1..=cx2).flat_map(move |x| (cy1..=cy2).map(move |y| n * y + x))
    }

    pub fn insert(&mut self, mode: Overlap, group: u32, b: [f64; 4]) {
        let uid = self.boxes.len();
        for c in self.cells(b[0], b[1], b[2], b[3]) {
            self.box_cells[c].push(uid);
        }
        self.boxes.push((b, mode, group));
    }

    pub fn insert_circle(&mut self, mode: Overlap, group: u32, x: f64, y: f64, r: f64) {
        let uid = self.circles.len();
        for c in self.cells(x - r, y - r, x + r, y + r) {
            self.circle_cells[c].push(uid);
        }
        self.circles.push(([x, y, r], mode, group));
    }

    /// `group` 0 collides with everything; otherwise only within the same group (collision
    /// groups per source when `crossSourceCollisions` is off).
    pub fn hit_test(&self, b: [f64; 4], mode: Overlap, group: u32) -> bool {
        let [x1, y1, x2, y2] = b;
        if x2 < 0.0 || x1 > self.width || y2 < 0.0 || y1 > self.height {
            return false;
        }
        if x1 <= 0.0 && y1 <= 0.0 && self.width <= x2 && self.height <= y2 {
            return true;
        }
        let eps = 1e-6;
        let mut seen_box = vec![false; self.boxes.len()];
        let mut seen_circle = vec![false; self.circles.len()];
        for cell in self.cells(x1, y1, x2, y2) {
            for &uid in &self.box_cells[cell] {
                if std::mem::replace(&mut seen_box[uid], true) {
                    continue;
                }
                let (o, key, g) = self.boxes[uid];
                if x1 <= o[2] + eps && y1 <= o[3] + eps && x2 >= o[0] - eps && y2 >= o[1] - eps && (group == 0 || g == group) && !overlap_allowed(mode, key) {
                    return true;
                }
            }
            for &uid in &self.circle_cells[cell] {
                if std::mem::replace(&mut seen_circle[uid], true) {
                    continue;
                }
                let (c, key, g) = self.circles[uid];
                if circle_and_rect(c[0], c[1], c[2], x1, y1, x2, y2) && (group == 0 || g == group) && !overlap_allowed(mode, key) {
                    return true;
                }
            }
        }
        false
    }

    pub fn hit_test_circle(&self, x: f64, y: f64, r: f64, mode: Overlap, group: u32) -> bool {
        let (x1, y1, x2, y2) = (x - r, y - r, x + r, y + r);
        if x2 < 0.0 || x1 > self.width || y2 < 0.0 || y1 > self.height {
            return false;
        }
        let mut seen_box = vec![false; self.boxes.len()];
        let mut seen_circle = vec![false; self.circles.len()];
        for cell in self.cells(x1, y1, x2, y2) {
            for &uid in &self.box_cells[cell] {
                if std::mem::replace(&mut seen_box[uid], true) {
                    continue;
                }
                let (o, key, g) = self.boxes[uid];
                if circle_and_rect(x, y, r, o[0], o[1], o[2], o[3]) && (group == 0 || g == group) && !overlap_allowed(mode, key) {
                    return true;
                }
            }
            for &uid in &self.circle_cells[cell] {
                if std::mem::replace(&mut seen_circle[uid], true) {
                    continue;
                }
                let (c, key, g) = self.circles[uid];
                let (dx, dy, rr) = (x - c[0], y - c[1], r + c[2]);
                if rr * rr > dx * dx + dy * dy && (group == 0 || g == group) && !overlap_allowed(mode, key) {
                    return true;
                }
            }
        }
        false
    }
}

fn circle_and_rect(cx: f64, cy: f64, r: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> bool {
    let hw = (x2 - x1) / 2.0;
    let dx = (cx - (x1 + hw)).abs();
    if dx > hw + r {
        return false;
    }
    let hh = (y2 - y1) / 2.0;
    let dy = (cy - (y1 + hh)).abs();
    if dy > hh + r {
        return false;
    }
    if dx <= hw || dy <= hh {
        return true;
    }
    let (ex, ey) = (dx - hw, dy - hh);
    ex * ex + ey * ey <= r * r
}

pub struct CollisionIndex {
    pub grid: GridIndex,
    pub ignored: GridIndex,
    screen_right: f64,
    screen_bottom: f64,
    grid_right: f64,
    grid_bottom: f64,
}

impl CollisionIndex {
    pub fn new(width: f64, height: f64) -> CollisionIndex {
        let (w, h) = (width + 2.0 * VIEWPORT_PADDING, height + 2.0 * VIEWPORT_PADDING);
        CollisionIndex {
            grid: GridIndex::new(w, h, 25.0),
            ignored: GridIndex::new(w, h, 25.0),
            screen_right: width + VIEWPORT_PADDING,
            screen_bottom: height + VIEWPORT_PADDING,
            grid_right: w,
            grid_bottom: h,
        }
    }

    fn inside_grid(&self, b: [f64; 4]) -> bool {
        b[2] >= 0.0 && b[0] < self.grid_right && b[3] >= 0.0 && b[1] < self.grid_bottom
    }

    #[allow(dead_code)]
    fn offscreen(&self, b: [f64; 4]) -> bool {
        b[2] < VIEWPORT_PADDING || b[0] >= self.screen_right || b[3] < VIEWPORT_PADDING || b[1] > self.screen_bottom
    }

    fn insert(&mut self, b: [f64; 4], mode: Overlap, group: u32, ignore: bool) {
        if ignore { &mut self.ignored } else { &mut self.grid }.insert(mode, group, b);
    }
}

/// Where a variable-anchor label was placed (`VariableOffset`).
#[derive(Debug, Clone)]
pub struct VariableOffset {
    pub text_offset: P,
    pub width: f64,
    pub height: f64,
    pub anchor: String,
    pub text_box_scale: f64,
}

/// Placement result of one symbol instance.
#[derive(Debug, Clone, Default)]
pub struct InstancePlacement {
    pub text: bool,
    pub icon: bool,
    pub variable: Option<VariableOffset>,
    /// Writing mode the text was placed in.
    pub orientation: Option<WritingMode>,
}

/// Per-tile projection for collision boxes (CSS px, shifted by the viewport padding).
pub struct TileProjection {
    origin: P,
    scale: f64,
    transform: crate::tile::Transform,
}

impl TileProjection {
    pub fn new(ctx: &Ctx, tile: &crate::tile::RenderTile) -> TileProjection {
        TileProjection { origin: ctx.tile_origin(tile), scale: ctx.tile_px(tile) / EXTENT, transform: ctx.transform.clone() }
    }

    /// Whether a tile point is on the map (see `Transform::in_domain`).
    pub fn in_domain(&self, p: P) -> bool {
        self.transform.in_domain([
            self.origin[0] + p[0] * self.scale,
            self.origin[1] + p[1] * self.scale,
        ])
    }

    /// Tile units → CSS px of the collision grid.
    pub fn project(&self, p: P) -> P {
        let s = self.transform.world_to_screen([self.origin[0] + p[0] * self.scale, self.origin[1] + p[1] * self.scale]);
        let r = self.transform.pixel_ratio;
        [s[0] / r + VIEWPORT_PADDING, s[1] / r + VIEWPORT_PADDING]
    }
}

/// `calculateVariableLayoutShift`.
pub fn variable_layout_shift(anchor: &str, width: f64, height: f64, text_offset: P, text_box_scale: f64) -> P {
    let (h, v) = shaping::anchor_alignment(anchor);
    [-(h - 0.5) * width + text_offset[0] * text_box_scale, -(v - 0.5) * height + text_offset[1] * text_box_scale]
}

struct BoxParams<'a> {
    proj: &'a TileProjection,
    text_pixel_ratio: f64,
    rotate_with_map: bool,
    translation: P,
    group: u32,
}

impl CollisionIndex {
    /// `placeCollisionBox` at pitch 0. Returns (box, placeable).
    fn place_box(&self, b: &CollisionBox, mode: Overlap, p: &BoxParams, shift: Option<P>) -> ([f64; 4], bool) {
        let x = b.anchor[0] + p.translation[0];
        let y = b.anchor[1] + p.translation[1];
        let pt = p.proj.project([x, y]);
        let m = p.text_pixel_ratio;
        let bx = if !p.rotate_with_map {
            let (sx, sy) = shift.map_or((0.0, 0.0), |s| (s[0] * m, s[1] * m));
            [pt[0] + sx + b.x1 * m, pt[1] + sy + b.y1 * m, pt[0] + sx + b.x2 * m, pt[1] + sy + b.y2 * m]
        } else {
            // `_projectCollisionBox` with the screen direction of the tile's x axis.
            let east = p.proj.project([x + 1.0, y]);
            let (tx, ty) = (east[0] - pt[0], east[1] - pt[1]);
            let angle = (ty / tx).atan() + if tx < 0.0 { std::f64::consts::PI } else { 0.0 };
            let (sin, cos) = angle.sin_cos();
            let (ex, ey, sx, sy) = (cos, sin, -sin, cos);
            let (mut bx, mut by) = (pt[0], pt[1]);
            if let Some(s) = shift {
                bx += ex * s[0] * m + sx * s[1] * m;
                by += ey * s[0] * m + sy * s[1] * m;
            }
            let (x1, x2, y1, y2) = (b.x1 * m, b.x2 * m, b.y1 * m, b.y2 * m);
            let (xh, yh) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
            let offsets = [[x1, y1], [xh, y1], [x2, y1], [x2, yh], [x2, y2], [xh, y2], [x1, y2], [x1, yh]];
            let pts = offsets.map(|o| [bx + ex * o[0] + sx * o[1], by + ey * o[0] + sy * o[1]]);
            [
                pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
                pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
                pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max),
                pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max),
            ]
        };
        if !self.inside_grid(bx) || (mode != Overlap::Always && self.grid.hit_test(bx, mode, p.group)) {
            return (bx, false);
        }
        (bx, true)
    }
}

/// One symbol layer's buckets, as handed to placement.
pub struct LayerBuckets<'a> {
    pub layer: &'a Layer,
    pub raw: &'a serde_json::Value,
    pub buckets: &'a [Rc<SymbolBucket>],
    /// Collision group (0 = collides across sources).
    pub group: u32,
}

/// Place all symbol layers (top-most first). Returns, per layer and bucket, the placement of
/// each instance.
pub fn place(ctx: &Ctx, layers: &[LayerBuckets]) -> Vec<Vec<Vec<InstancePlacement>>> {
    let t = ctx.transform;
    let mut index = CollisionIndex::new(t.width, t.height);
    let mut out: Vec<Vec<Vec<InstancePlacement>>> =
        layers.iter().map(|l| l.buckets.iter().map(|b| vec![InstancePlacement::default(); b.instances.len()]).collect()).collect();
    for li in (0..layers.len()).rev() {
        let lb = &layers[li];
        let Some(first) = lb.buckets.first() else { continue };
        let res = Resolved::new(lb.layer, ctx, &first.tile);
        // Tiles in placement order: higher zoom first, then wrap, z, x, y.
        let mut order: Vec<usize> = (0..lb.buckets.len()).collect();
        order.sort_by_key(|&i| {
            let tl = &lb.buckets[i].tile;
            (std::cmp::Reverse(tl.overscaled_z), tl.wrap, tl.overscaled_z, tl.canonical.x, tl.canonical.y)
        });
        // Bucket parts: whole buckets, or sort-key ranges sorted across tiles.
        let mut parts: Vec<(f64, usize, usize, usize)> = Vec::new();
        for &bi in &order {
            let b = &lb.buckets[bi];
            if res.sort_by_key {
                parts.extend(b.sort_key_ranges.iter().map(|r| (r.0, bi, r.1, r.2)));
            } else {
                parts.push((0.0, bi, 0, b.instances.len()));
            }
        }
        if res.sort_by_key {
            parts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        }
        for (_, bi, start, end) in parts {
            place_bucket_part(ctx, lb, &res, &lb.buckets[bi], start, end, &mut index, &mut out[li][bi]);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn place_bucket_part(
    ctx: &Ctx,
    lb: &LayerBuckets,
    res: &Resolved,
    bucket: &SymbolBucket,
    start: usize,
    end: usize,
    index: &mut CollisionIndex,
    out: &mut [InstancePlacement],
) {
    let layer = lb.layer;
    let tile = bucket.tile;
    let lay = |n: &str| ctx.layout(layer, n, None, &tile);
    let text_optional = lay("text-optional").truthy();
    let icon_optional = lay("icon-optional").truthy();
    let text_mode = Overlap::from(overlap_mode(layer, ctx, &tile, lb.raw, "text"));
    let icon_mode = Overlap::from(overlap_mode(layer, ctx, &tile, lb.raw, "icon"));
    let has_text_data = !bucket.text.is_empty();
    let has_icon_data = !bucket.icon.is_empty();
    let always_show_text = text_mode == Overlap::Always && (icon_mode == Overlap::Always || !has_icon_data || icon_optional);
    let always_show_icon = icon_mode == Overlap::Always && (text_mode == Overlap::Always || !has_text_data || text_optional);
    let rotate_with_map = res.text_rotation_alignment == "map";
    let has_icon_text_fit = lay("icon-text-fit").as_str().is_some_and(|s| s != "none");
    let text_ignore = lay("text-ignore-placement").truthy();
    let icon_ignore = lay("icon-ignore-placement").truthy();
    let overscale = 2f64.powi(tile.overscaled_z as i32 - tile.canonical.z as i32);
    let proj = TileProjection::new(ctx, &tile);
    let to_tile = EXTENT / ctx.tile_px(&tile);
    let tr_text = ctx.translate(layer, "text", &tile);
    let tr_icon = ctx.translate(layer, "icon", &tile);
    let group = lb.group;
    let params = |translation: P| BoxParams {
        proj: &proj,
        text_pixel_ratio: 512.0 * overscale / EXTENT,
        rotate_with_map,
        translation: [translation[0] * to_tile, translation[1] * to_tile],
        group,
    };
    let (text_params, icon_params) = (params(tr_text), params(tr_icon));
    let text_padding = num(&lay("text-padding"));

    let mut place_symbol = |i: usize, out: &mut InstancePlacement| {
        let inst = &bucket.instances[i];
        // A world copy past a projection's antimeridian (also drawn on the map's other side).
        if !proj.in_domain(inst.anchor) {
            return;
        }
        let mut placed_text: Option<([f64; 4], bool)> = None;
        let mut placed_vertical = false;
        let mut shift: Option<P> = None;
        if let Some(text_box) = &inst.text_box {
            // Variable anchors: try each anchor, first without overlap, then with the layer's
            // overlap mode.
            let mut place_variable = |text_box: &CollisionBox, icon_box: Option<&CollisionBox>, out: &mut InstancePlacement| {
                let (width, height) = (text_box.x2 - text_box.x1, text_box.y2 - text_box.y1);
                let icon_box = if has_icon_text_fit && icon_mode == Overlap::Never { icon_box } else { None };
                let passes = if text_mode == Overlap::Never { 1 } else { 2 };
                let mut mode = Overlap::Never;
                let mut last = None;
                for _ in 0..passes {
                    for (anchor, offset) in &inst.anchor_offsets {
                        let s = variable_layout_shift(anchor, width, height, *offset, inst.text_box_scale);
                        let r = index.place_box(text_box, mode, &text_params, Some(s));
                        if let Some(ib) = icon_box
                            && !index.place_box(ib, mode, &icon_params, Some(s)).1 {
                                continue;
                            }
                        if r.1 {
                            out.variable = Some(VariableOffset { text_offset: *offset, width, height, anchor: anchor.clone(), text_box_scale: inst.text_box_scale });
                            shift = Some(s);
                            return Some(r);
                        }
                        last = Some((r.0, false));
                    }
                    mode = text_mode;
                }
                last
            };
            let vertical_box = inst.vertical_text_box.filter(|_| bucket.allow_vertical_placement && inst.num_vertical_glyph_vertices > 0);
            let modes: &[WritingMode] = if vertical_box.is_some() { &bucket.writing_modes } else { &[WritingMode::Horizontal] };
            for &m in modes {
                let r = match (m, vertical_box) {
                    (WritingMode::Vertical, Some(vb)) => {
                        let r = if inst.anchor_offsets.is_empty() {
                            Some(index.place_box(&vb, text_mode, &text_params, None))
                        } else if !placed_text.is_some_and(|p| p.1) {
                            place_variable(&vb, inst.vertical_icon_box.as_ref(), out)
                        } else {
                            None
                        };
                        placed_vertical = r.is_some_and(|r| r.1);
                        r
                    }
                    _ if inst.anchor_offsets.is_empty() => Some(index.place_box(text_box, text_mode, &text_params, None)),
                    _ => place_variable(text_box, inst.icon_box.as_ref(), out),
                };
                placed_text = r;
                if r.is_some_and(|r| r.1) {
                    out.orientation = Some(m);
                    break;
                }
            }
        }
        let mut place_text = placed_text.is_some_and(|p| p.1);
        let mut circles: Option<(Vec<[f64; 3]>, bool)> = None;
        if let (Some(d), Some(sym)) = (inst.collision_circle_diameter, inst.text[1]) {
            let c = place_circles(ctx, bucket, &bucket.text[sym], index, text_mode, group, d, text_padding, text_params.translation);
            place_text = text_mode == Overlap::Always || (!c.0.is_empty() && !c.1);
            circles = Some(c);
        }
        let mut placed_icon: Option<([f64; 4], bool)> = None;
        if let Some(icon_box) = &inst.icon_box {
            let b = if placed_vertical { inst.vertical_icon_box.as_ref().unwrap_or(icon_box) } else { icon_box };
            placed_icon = Some(index.place_box(b, icon_mode, &icon_params, if has_icon_text_fit { shift } else { None }));
        }
        let mut place_icon = placed_icon.is_some_and(|p| p.1);
        let icon_without_text = text_optional || (inst.num_glyph_vertices == 0 && inst.num_vertical_glyph_vertices == 0);
        let text_without_icon = icon_optional || inst.num_icon_vertices == 0;
        if !icon_without_text && !text_without_icon {
            place_text = place_text && place_icon;
            place_icon = place_text;
        } else if !text_without_icon {
            place_text = place_icon && place_text;
        } else if !icon_without_text {
            place_icon = place_icon && place_text;
        }
        if place_text && placed_text.is_some_and(|p| p.1) {
            index.insert(placed_text.unwrap().0, text_mode, group, text_ignore);
        }
        if place_icon && placed_icon.is_some_and(|p| p.1) {
            index.insert(placed_icon.unwrap().0, icon_mode, group, icon_ignore);
        }
        if let (Some((cs, _)), true) = (&circles, place_text) {
            let grid = if text_ignore { &mut index.ignored } else { &mut index.grid };
            for c in cs {
                grid.insert_circle(text_mode, group, c[0], c[1], c[2]);
            }
        }
        out.text = place_text || always_show_text;
        out.icon = place_icon || always_show_icon;
    };

    if res.z_order == "viewport-y" {
        for &i in sorted_symbol_indexes(bucket, ctx.transform.bearing_rad).iter().rev() {
            place_symbol(i, &mut out[i]);
        }
    } else {
        for (i, o) in out.iter_mut().enumerate().take(end).skip(start) {
            place_symbol(i, o);
        }
    }
}

/// `getSortedSymbolIndexes`: by rotated anchor y, then feature index (descending).
pub fn sorted_symbol_indexes(bucket: &SymbolBucket, angle: f64) -> Vec<usize> {
    let (sin, cos) = angle.sin_cos();
    let ys: Vec<i64> = bucket.instances.iter().map(|s| crate::expression::value::js_round(sin * s.anchor[0] + cos * s.anchor[1]) as i64).collect();
    let mut idx: Vec<usize> = (0..bucket.instances.len()).collect();
    idx.sort_by(|&a, &b| ys[a].cmp(&ys[b]).then(bucket.instances[b].feature_index.cmp(&bucket.instances[a].feature_index)));
    idx
}

/// `placeCollisionCircles` at pitch 0: circles along the label's path on screen. Returns the
/// circles (x, y, r in grid px) and whether one collided.
#[allow(clippy::too_many_arguments)]
fn place_circles(
    ctx: &Ctx,
    bucket: &SymbolBucket,
    symbol: &super::layout::PlacedSymbol,
    index: &CollisionIndex,
    mode: Overlap,
    group: u32,
    diameter: f64,
    padding: f64,
    translation: P,
) -> (Vec<[f64; 3]>, bool) {
    let font_size = bucket.text_size.for_feature(ctx.transform.zoom, symbol.size);
    let Some(path) = super::projection::label_path(ctx, bucket, symbol, font_size, translation) else { return (vec![], false) };
    let radius = diameter * 0.5 + padding;
    let (min, max) = ([-VIEWPORT_PADDING; 2], [index.screen_right, index.screen_bottom]);
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in &path {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let segments = if lo[0] >= min[0] && hi[0] <= max[0] && lo[1] >= min[1] && hi[1] <= max[1] {
        vec![path]
    } else if hi[0] < min[0] || lo[0] > max[0] || hi[1] < min[1] || lo[1] > max[1] {
        vec![]
    } else {
        anchors::clip_line(&[path], min[0], min[1], max[0], max[1])
    };
    let mut circles = Vec::new();
    let mut in_grid = false;
    for seg in &segments {
        let interp = anchors::PathInterpolator::new(seg, radius * 0.25);
        let n = if interp.length <= 0.5 * radius { 1 } else { (interp.padded_length / (radius * 2.5)).ceil() as usize + 1 };
        for i in 0..n {
            let t = i as f64 / (n.max(2) - 1) as f64;
            let p = interp.lerp(t);
            let (cx, cy) = (p[0] + VIEWPORT_PADDING, p[1] + VIEWPORT_PADDING);
            circles.push([cx, cy, radius]);
            in_grid |= index.inside_grid([cx - radius, cy - radius, cx + radius, cy + radius]);
            if mode != Overlap::Always && index.grid.hit_test_circle(cx, cy, radius, mode, group) {
                return (vec![], true);
            }
        }
    }
    (if in_grid { circles } else { vec![] }, false)
}
