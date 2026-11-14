//! Placing glyphs along lines (`projection.ts`) for an unpitched camera: the label plane is the
//! screen (CSS px), which at pitch 0 gives the same result for both pitch alignments.

use super::anchors::P;
use super::layout::{PlacedSymbol, SymbolBucket};
use super::shaping::WritingMode;
use crate::render::{Ctx, EXTENT};
use crate::hash::HashMap;

/// A placed glyph: label-plane point, rotation and the path walked to reach it.
pub struct PlacedGlyph {
    pub point: P,
    pub angle: f64,
    pub path: Vec<P>,
}

pub struct LineProjector<'a> {
    bucket: &'a SymbolBucket,
    origin: P,
    scale: f64,
    transform: crate::tile::Transform,
    translation: P,
}

/// `projectionCache`, per symbol.
#[derive(Default)]
pub struct Cache {
    projections: HashMap<usize, P>,
    offsets: HashMap<usize, P>,
    anchor: Option<P>,
}

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

fn mag(a: P) -> f64 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

/// `transformToOffsetNormal`: `segment._unit()._perp()._mult(offset * direction)`.
fn offset_normal(v: P, offset: f64, direction: f64) -> P {
    let m = mag(v);
    let u = [v[0] / m, v[1] / m];
    [-u[1] * offset * direction, u[0] * offset * direction]
}

/// `findLineIntersection`.
fn line_intersection(a1: P, a2: P, b1: P, b2: P) -> Option<P> {
    let a_dx = a2[0] - a1[0];
    let a_dy = a2[1] - a1[1];
    let b_dx = b2[0] - b1[0];
    let b_dy = b2[1] - b1[1];
    let denom = b_dy * a_dx - b_dx * a_dy;
    if denom == 0.0 {
        return None;
    }
    let ua = (b_dx * (a1[1] - b1[1]) - b_dy * (a1[0] - b1[0])) / denom;
    Some([a1[0] + ua * a_dx, a1[1] + ua * a_dy])
}

impl<'a> LineProjector<'a> {
    /// `translation` in tile units.
    pub fn new(ctx: &Ctx, bucket: &'a SymbolBucket, translation: P) -> Self {
        LineProjector { bucket, origin: ctx.tile_origin(&bucket.tile), scale: ctx.tile_px(&bucket.tile) / EXTENT, transform: ctx.transform.clone(), translation }
    }

    /// Tile units → label plane (CSS px).
    pub fn project(&self, p: P) -> P {
        let (x, y) = (p[0] + self.translation[0], p[1] + self.translation[1]);
        let s = self.transform.world_to_screen([self.origin[0] + x * self.scale, self.origin[1] + y * self.scale]);
        [s[0] / self.transform.pixel_ratio, s[1] / self.transform.pixel_ratio]
    }

    fn vertex(&self, i: usize, cache: &mut Cache) -> P {
        if let Some(p) = cache.projections.get(&i) {
            return *p;
        }
        let p = self.project(self.bucket.line_vertices[i].0);
        cache.projections.insert(i, p);
        p
    }

    #[allow(clippy::too_many_arguments)]
    fn offset_intersection(&self, index: usize, normal: P, current: P, start: usize, end: usize, offset_prev: P, line_offset_y: f64, direction: isize, cache: &mut Cache) -> P {
        if let Some(p) = cache.offsets.get(&index) {
            return *p;
        }
        let offset_current = [current[0] + normal[0], current[1] + normal[1]];
        let next_index = index as isize + direction;
        if next_index < start as isize || next_index >= end as isize {
            cache.offsets.insert(index, offset_current);
            return offset_current;
        }
        let next = self.vertex(next_index as usize, cache);
        let n = offset_normal(sub(next, current), line_offset_y, direction as f64);
        let r = line_intersection(offset_prev, offset_current, [current[0] + n[0], current[1] + n[1]], [next[0] + n[0], next[1] + n[1]]).unwrap_or(offset_current);
        cache.offsets.insert(index, r);
        r
    }

    /// `placeGlyphAlongLine`.
    #[allow(clippy::too_many_arguments)]
    pub fn place_glyph(&self, sym: &PlacedSymbol, offset_x: f64, line_offset_x: f64, line_offset_y: f64, flip: bool, rotate_to_line: bool, cache: &mut Cache) -> Option<PlacedGlyph> {
        let segment = sym.segment? as isize;
        let (start, end) = (sym.line_start as isize, (sym.line_start + sym.line_length) as isize);
        let combined = if flip { offset_x - line_offset_x } else { offset_x + line_offset_x };
        let mut direction: isize = if combined > 0.0 { 1 } else { -1 };
        let mut angle = 0.0;
        if flip {
            direction *= -1;
            angle = std::f64::consts::PI;
        }
        if direction < 0 {
            angle += std::f64::consts::PI;
        }
        let mut index = if direction > 0 { start + segment } else { start + segment + 1 };
        let anchor = *cache.anchor.get_or_insert_with(|| self.project(sym_anchor(sym)));
        let (mut current, mut previous) = (anchor, anchor);
        let mut offset_intersection: Option<P> = None;
        let mut offset_previous: Option<P> = None;
        let (mut distance, mut segment_distance) = (0.0, 0.0);
        let abs_offset = combined.abs();
        let mut path = Vec::new();
        let mut segment_vec = [0.0, 0.0];
        while distance + segment_distance <= abs_offset {
            index += direction;
            if index < start || index >= end {
                return None;
            }
            distance += segment_distance;
            previous = current;
            offset_previous = offset_intersection;
            current = self.vertex(index as usize, cache);
            if line_offset_y == 0.0 {
                path.push(previous);
                segment_vec = sub(current, previous);
            } else {
                let prev_to_current = sub(current, previous);
                let normal = if mag(prev_to_current) == 0.0 {
                    let next = self.vertex((index + direction) as usize, cache);
                    offset_normal(sub(next, current), line_offset_y, direction as f64)
                } else {
                    offset_normal(prev_to_current, line_offset_y, direction as f64)
                };
                let op = *offset_previous.get_or_insert([previous[0] + normal[0], previous[1] + normal[1]]);
                let oi = self.offset_intersection(index as usize, normal, current, start as usize, end as usize, op, line_offset_y, direction, cache);
                offset_intersection = Some(oi);
                path.push(op);
                segment_vec = sub(oi, op);
            }
            segment_distance = mag(segment_vec);
        }
        let t = (abs_offset - distance) / segment_distance;
        let base = offset_previous.unwrap_or(previous);
        let p = [segment_vec[0] * t + base[0], segment_vec[1] * t + base[1]];
        let segment_angle = angle + (current[1] - previous[1]).atan2(current[0] - previous[0]);
        path.push(p);
        Some(PlacedGlyph { point: p, angle: if rotate_to_line { segment_angle } else { 0.0 }, path })
    }

    /// `placeGlyphsAlongLine` + the keep-upright retry of `updateLineLabels`: per-quad
    /// (point, angle).
    pub fn place_symbol(&self, sym: &PlacedSymbol, font_size: f64, keep_upright: bool, rotate_to_line: bool) -> LinePlacement {
        let mut cache = Cache::default();
        match self.place_glyphs(sym, font_size, false, keep_upright, rotate_to_line, &mut cache) {
            Ok(g) => LinePlacement::Placed(g),
            Err(Change::Flip) => match self.place_glyphs(sym, font_size, true, keep_upright, rotate_to_line, &mut cache) {
                Ok(g) => LinePlacement::Placed(g),
                Err(_) => LinePlacement::Hidden,
            },
            Err(Change::Vertical) => LinePlacement::UseVertical,
            Err(Change::NoRoom) => LinePlacement::Hidden,
        }
    }

    fn place_glyphs(&self, sym: &PlacedSymbol, font_size: f64, flip: bool, keep_upright: bool, rotate_to_line: bool, cache: &mut Cache) -> Result<Vec<(P, f64)>, Change> {
        let font_scale = font_size / 24.0;
        let (lox, loy) = (sym.line_offset[0] * font_scale, sym.line_offset[1] * font_scale);
        let offsets: Vec<f64> = sym.quads.iter().map(|q| q.glyph_offset[0] as f32 as f64).collect();
        let n = offsets.len();
        // `requiresOrientationChange` (screen y grows downwards).
        let orientation_change = |a: P, b: P| -> Option<Change> {
            if sym.writing_mode == WritingMode::Horizontal && (b[1] - a[1]).abs() > (b[0] - a[0]).abs() {
                return Some(Change::Vertical);
            }
            let flip = if sym.writing_mode == WritingMode::Vertical { a[1] > b[1] } else { a[0] > b[0] };
            flip.then_some(Change::Flip)
        };
        let no_room = Change::NoRoom;
        if n > 1 {
            let first = self.place_glyph(sym, font_scale * offsets[0], lox, loy, flip, rotate_to_line, cache).ok_or(no_room)?;
            let last = self.place_glyph(sym, font_scale * offsets[n - 1], lox, loy, flip, rotate_to_line, cache).ok_or(Change::NoRoom)?;
            if keep_upright && !flip
                && let Some(c) = orientation_change(first.point, last.point) {
                    return Err(c);
                }
            let mut out = vec![(first.point, first.angle)];
            for &o in &offsets[1..n - 1] {
                let g = self.place_glyph(sym, font_scale * o, lox, loy, flip, rotate_to_line, cache).ok_or(Change::NoRoom)?;
                out.push((g.point, g.angle));
            }
            out.push((last.point, last.angle));
            Ok(out)
        } else {
            if keep_upright && !flip {
                let a = self.project(sym_anchor(sym));
                let i = sym.line_start + sym.segment.unwrap_or(0) + 1;
                let b = self.project(self.bucket.line_vertices[i].0);
                if let Some(c) = orientation_change(a, b) {
                    return Err(c);
                }
            }
            let g = self.place_glyph(sym, font_scale * offsets.first().copied().unwrap_or(0.0), lox, loy, flip, rotate_to_line, cache).ok_or(Change::NoRoom)?;
            Ok(vec![(g.point, g.angle); n.max(1)])
        }
    }
}

pub enum LinePlacement {
    Placed(Vec<(P, f64)>),
    /// Too steep for horizontal text: the vertical shaping is drawn instead.
    UseVertical,
    Hidden,
}

enum Change {
    Flip,
    Vertical,
    NoRoom,
}

/// Int16 anchor of a placed symbol.
fn sym_anchor(sym: &PlacedSymbol) -> P {
    [super::layout::int16(sym.anchor[0]), super::layout::int16(sym.anchor[1])]
}

/// `placeFirstAndLastGlyph` path for collision circles (label plane, unflipped).
pub fn label_path(ctx: &Ctx, bucket: &SymbolBucket, sym: &PlacedSymbol, font_size: f64, translation: P) -> Option<Vec<P>> {
    let proj = LineProjector::new(ctx, bucket, translation);
    let font_scale = font_size / 24.0;
    let (lox, loy) = (sym.line_offset[0] * font_scale, sym.line_offset[1] * font_scale);
    let first_off = sym.quads.first()?.glyph_offset[0] as f32 as f64;
    let last_off = sym.quads.last()?.glyph_offset[0] as f32 as f64;
    let mut cache = Cache::default();
    let first = proj.place_glyph(sym, font_scale * first_off, lox, loy, false, false, &mut cache)?;
    let last = proj.place_glyph(sym, font_scale * last_off, lox, loy, false, false, &mut cache)?;
    let mut path: Vec<P> = first.path[1..].iter().rev().copied().collect();
    path.extend_from_slice(&last.path[1..]);
    Some(path)
}
