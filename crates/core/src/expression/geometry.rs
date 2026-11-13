//! `within` and `distance` (style-spec `definitions/within.ts`, `distance.ts`, `util/geometry_util.ts`).

use super::expr::CanonicalTileId;
use serde_json::Value as J;

pub const EXTENT: f64 = 8192.0;

type P = [f64; 2];
type BBox = [f64; 4];

#[derive(Debug, Clone)]
pub enum SimpleGeometry {
    Point(P),
    LineString(Vec<P>),
    Polygon(Vec<Vec<P>>),
}

fn coords(v: &J) -> Option<P> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
}

fn line(v: &J) -> Option<Vec<P>> {
    v.as_array()?.iter().map(coords).collect()
}

fn polygon(v: &J) -> Option<Vec<Vec<P>>> {
    v.as_array()?.iter().map(line).collect()
}

/// Polygons of a `within` argument, or `None` if it has none.
pub fn within_polygons(g: &J) -> Option<Vec<Vec<Vec<P>>>> {
    let from_geometry = |geom: &J| -> Option<Vec<Vec<Vec<P>>>> {
        match geom["type"].as_str()? {
            "Polygon" => Some(vec![polygon(&geom["coordinates"])?]),
            "MultiPolygon" => geom["coordinates"].as_array()?.iter().map(polygon).collect(),
            _ => None,
        }
    };
    match g["type"].as_str()? {
        "FeatureCollection" => {
            let mut out = Vec::new();
            for f in g["features"].as_array()? {
                if let Some(p) = from_geometry(&f["geometry"]) {
                    out.extend(p);
                }
            }
            (!out.is_empty()).then_some(out)
        }
        "Feature" => from_geometry(&g["geometry"]),
        _ => from_geometry(g),
    }
}

pub fn distance_geometries(g: &J) -> Option<Vec<SimpleGeometry>> {
    fn simple(geom: &J) -> Vec<SimpleGeometry> {
        let c = &geom["coordinates"];
        let each = |f: &dyn Fn(&J) -> Option<SimpleGeometry>| c.as_array().map(|a| a.iter().filter_map(f).collect()).unwrap_or_default();
        match geom["type"].as_str() {
            Some("Point") => coords(c).map(SimpleGeometry::Point).into_iter().collect(),
            Some("LineString") => line(c).map(SimpleGeometry::LineString).into_iter().collect(),
            Some("Polygon") => polygon(c).map(SimpleGeometry::Polygon).into_iter().collect(),
            Some("MultiPoint") => each(&|p| coords(p).map(SimpleGeometry::Point)),
            Some("MultiLineString") => each(&|l| line(l).map(SimpleGeometry::LineString)),
            Some("MultiPolygon") => each(&|p| polygon(p).map(SimpleGeometry::Polygon)),
            _ => vec![],
        }
    }
    let o = g.as_object()?;
    match o.get("type")?.as_str() {
        Some("FeatureCollection") => Some(g["features"].as_array()?.iter().flat_map(|f| simple(&f["geometry"])).collect()),
        Some("Feature") => Some(simple(&g["geometry"])),
        _ if o.contains_key("coordinates") => Some(simple(g)),
        _ => None,
    }
}

fn mercator_x(lng: f64) -> f64 {
    (180.0 + lng) / 360.0
}

fn mercator_y(lat: f64) -> f64 {
    (180.0 - (180.0 / std::f64::consts::PI) * (std::f64::consts::PI / 4.0 + lat * std::f64::consts::PI / 360.0).tan().ln()) / 360.0
}

pub fn tile_coordinates(p: P, c: CanonicalTileId) -> P {
    let tiles = 2f64.powi(c.z as i32);
    let round = super::value::js_round;
    [round(mercator_x(p[0]) * tiles * EXTENT), round(mercator_y(p[1]) * tiles * EXTENT)]
}

fn lnglat_from_tile(p: P, c: CanonicalTileId) -> P {
    let tiles = 2f64.powi(c.z as i32);
    let x = (p[0] / EXTENT + c.x as f64) / tiles;
    let y = (p[1] / EXTENT + c.y as f64) / tiles;
    [x * 360.0 - 180.0, (360.0 / std::f64::consts::PI) * ((180.0 - y * 360.0) * std::f64::consts::PI / 180.0).exp().atan() - 90.0]
}

fn update_bbox(b: &mut BBox, p: P) {
    b[0] = b[0].min(p[0]);
    b[1] = b[1].min(p[1]);
    b[2] = b[2].max(p[0]);
    b[3] = b[3].max(p[1]);
}

const EMPTY_BBOX: BBox = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];

fn box_within_box(a: &BBox, b: &BBox) -> bool {
    !(a[0] <= b[0] || a[2] >= b[2] || a[1] <= b[1] || a[3] >= b[3])
}

fn ray_intersect(p: P, p1: P, p2: P) -> bool {
    (p1[1] > p[1]) != (p2[1] > p[1]) && p[0] < (p2[0] - p1[0]) * (p[1] - p1[1]) / (p2[1] - p1[1]) + p1[0]
}

fn point_on_boundary(p: P, p1: P, p2: P) -> bool {
    let (x1, y1, x2, y2) = (p[0] - p1[0], p[1] - p1[1], p[0] - p2[0], p[1] - p2[1]);
    x1 * y2 - x2 * y1 == 0.0 && x1 * x2 <= 0.0 && y1 * y2 <= 0.0
}

fn perp(a: P, b: P) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn two_sided(p1: P, p2: P, q1: P, q2: P) -> bool {
    let (x1, y1) = (p1[0] - q1[0], p1[1] - q1[1]);
    let (x2, y2) = (p2[0] - q1[0], p2[1] - q1[1]);
    let (x3, y3) = (q2[0] - q1[0], q2[1] - q1[1]);
    let d1 = x1 * y3 - x3 * y1;
    let d2 = x2 * y3 - x3 * y2;
    (d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)
}

fn segment_intersect_segment(a: P, b: P, c: P, d: P) -> bool {
    let vp = [b[0] - a[0], b[1] - a[1]];
    let vq = [d[0] - c[0], d[1] - c[1]];
    if perp(vq, vp) == 0.0 {
        return false;
    }
    two_sided(a, b, c, d) && two_sided(c, d, a, b)
}

fn point_within_polygon(p: P, rings: &[Vec<P>], true_on_boundary: bool) -> bool {
    let mut inside = false;
    for ring in rings {
        for j in 0..ring.len().saturating_sub(1) {
            if point_on_boundary(p, ring[j], ring[j + 1]) {
                return true_on_boundary;
            }
            if ray_intersect(p, ring[j], ring[j + 1]) {
                inside = !inside;
            }
        }
    }
    inside
}

fn line_intersect_polygon(p1: P, p2: P, polygon: &[Vec<P>]) -> bool {
    polygon.iter().any(|ring| ring.windows(2).any(|w| segment_intersect_segment(p1, p2, w[0], w[1])))
}

fn line_within_polygon(line: &[P], polygon: &[Vec<P>]) -> bool {
    line.iter().all(|&p| point_within_polygon(p, polygon, false)) && !line.windows(2).any(|w| line_intersect_polygon(w[0], w[1], polygon))
}

/// Shift `p` by a world width if that brings it next to `poly`.
fn update_point(p: &mut P, bbox: &mut BBox, poly: &BBox, world: f64) {
    if p[0] < poly[0] || p[0] > poly[2] {
        let half = world * 0.5;
        let mut shift = if p[0] - poly[0] > half {
            -world
        } else if poly[0] - p[0] > half {
            world
        } else {
            0.0
        };
        if shift == 0.0 {
            shift = if p[0] - poly[2] > half {
                -world
            } else if poly[2] - p[0] > half {
                world
            } else {
                0.0
            };
        }
        p[0] += shift;
    }
    update_bbox(bbox, *p);
}

pub fn within(geom_type: Option<&str>, geometry: &[Vec<P>], c: CanonicalTileId, polygons: &[Vec<Vec<P>>]) -> bool {
    let mut poly_bbox = EMPTY_BBOX;
    let tile_polys: Vec<Vec<Vec<P>>> = polygons
        .iter()
        .map(|poly| {
            poly.iter()
                .map(|ring| {
                    ring.iter()
                        .map(|&p| {
                            let t = tile_coordinates(p, c);
                            update_bbox(&mut poly_bbox, t);
                            t
                        })
                        .collect()
                })
                .collect()
        })
        .collect();
    let world = 2f64.powi(c.z as i32) * EXTENT;
    let shift = [c.x as f64 * EXTENT, c.y as f64 * EXTENT];
    match geom_type {
        Some("Point") => {
            let mut bbox = EMPTY_BBOX;
            let mut points = Vec::new();
            for p in geometry.iter().flatten() {
                let mut q = [p[0] + shift[0], p[1] + shift[1]];
                update_point(&mut q, &mut bbox, &poly_bbox, world);
                points.push(q);
            }
            if !box_within_box(&bbox, &poly_bbox) {
                return false;
            }
            points.iter().all(|&p| tile_polys.iter().any(|poly| point_within_polygon(p, poly, false)))
        }
        Some("LineString") => {
            let mut bbox = EMPTY_BBOX;
            let mut lines: Vec<Vec<P>> = geometry
                .iter()
                .map(|l| {
                    l.iter()
                        .map(|p| {
                            let q = [p[0] + shift[0], p[1] + shift[1]];
                            update_bbox(&mut bbox, q);
                            q
                        })
                        .collect()
                })
                .collect();
            if bbox[2] - bbox[0] <= world / 2.0 {
                bbox = EMPTY_BBOX;
                for l in &mut lines {
                    for p in l {
                        update_point(p, &mut bbox, &poly_bbox, world);
                    }
                }
            }
            if !box_within_box(&bbox, &poly_bbox) {
                return false;
            }
            lines.iter().all(|l| tile_polys.iter().any(|poly| line_within_polygon(l, poly)))
        }
        _ => false,
    }
}

struct CheapRuler {
    kx: f64,
    ky: f64,
}

impl CheapRuler {
    fn new(lat: f64) -> Self {
        const RE: f64 = 6378.137;
        const FE: f64 = 1.0 / 298.257223563;
        const E2: f64 = FE * (2.0 - FE);
        let m = std::f64::consts::PI / 180.0 * RE * 1000.0;
        let coslat = lat.to_radians().cos();
        let w2 = 1.0 / (1.0 - E2 * (1.0 - coslat * coslat));
        let w = w2.sqrt();
        CheapRuler { kx: m * w * coslat, ky: m * w * w2 * (1.0 - E2) }
    }

    fn wrap(mut d: f64) -> f64 {
        while d < -180.0 {
            d += 360.0;
        }
        while d > 180.0 {
            d -= 360.0;
        }
        d
    }

    fn distance(&self, a: P, b: P) -> f64 {
        let dx = Self::wrap(a[0] - b[0]) * self.kx;
        let dy = (a[1] - b[1]) * self.ky;
        (dx * dx + dy * dy).sqrt()
    }

    fn point_on_line(&self, line: &[P], p: P) -> P {
        let mut min = f64::INFINITY;
        let mut best = [f64::NAN; 2];
        for i in 0..line.len().saturating_sub(1) {
            let (mut x, mut y) = (line[i][0], line[i][1]);
            let mut dx = Self::wrap(line[i + 1][0] - x) * self.kx;
            let mut dy = (line[i + 1][1] - y) * self.ky;
            if dx != 0.0 || dy != 0.0 {
                let t = (Self::wrap(p[0] - x) * self.kx * dx + (p[1] - y) * self.ky * dy) / (dx * dx + dy * dy);
                if t > 1.0 {
                    x = line[i + 1][0];
                    y = line[i + 1][1];
                } else if t > 0.0 {
                    x += dx / self.kx * t;
                    y += dy / self.ky * t;
                }
            }
            dx = Self::wrap(p[0] - x) * self.kx;
            dy = (p[1] - y) * self.ky;
            let sq = dx * dx + dy * dy;
            if sq < min {
                min = sq;
                best = [x, y];
            }
        }
        best
    }
}

type Range = [usize; 2];
const MIN_POINTS: usize = 100;
const MIN_LINE_POINTS: usize = 50;

fn range_size(r: Range) -> usize {
    r[1] - r[0] + 1
}

fn range_safe(r: Range, len: usize) -> bool {
    r[1] >= r[0] && r[1] < len
}

fn split_range(r: Range, is_line: bool) -> [Option<Range>; 2] {
    if r[0] > r[1] {
        return [None, None];
    }
    let size = range_size(r);
    if is_line {
        if size == 2 {
            return [Some(r), None];
        }
        let s1 = size / 2;
        return [Some([r[0], r[0] + s1]), Some([r[0] + s1, r[1]])];
    }
    if size == 1 {
        return [Some(r), None];
    }
    let s1 = size / 2 - 1;
    [Some([r[0], r[0] + s1]), Some([r[0] + s1 + 1, r[1]])]
}

fn bbox_of(coords: &[P], r: Range) -> BBox {
    let mut b = EMPTY_BBOX;
    if range_safe(r, coords.len()) {
        for &p in &coords[r[0]..=r[1]] {
            update_bbox(&mut b, p);
        }
    }
    b
}

fn polygon_bbox(poly: &[Vec<P>]) -> BBox {
    let mut b = EMPTY_BBOX;
    for &p in poly.iter().flatten() {
        update_bbox(&mut b, p);
    }
    b
}

fn bbox_distance(a: &BBox, b: &BBox, ruler: &CheapRuler) -> f64 {
    let valid = |b: &BBox| b[0] != f64::NEG_INFINITY && b[1] != f64::NEG_INFINITY && b[2] != f64::INFINITY && b[3] != f64::INFINITY;
    if !valid(a) || !valid(b) {
        return f64::NAN;
    }
    let mut dx = 0.0;
    let mut dy = 0.0;
    if a[2] < b[0] {
        dx = b[0] - a[2];
    }
    if a[0] > b[2] {
        dx = a[0] - b[2];
    }
    if a[1] > b[3] {
        dy = a[1] - b[3];
    }
    if a[3] < b[1] {
        dy = b[1] - a[3];
    }
    ruler.distance([0.0, 0.0], [dx, dy])
}

fn point_to_line(p: P, line: &[P], ruler: &CheapRuler) -> f64 {
    ruler.distance(p, ruler.point_on_line(line, p))
}

fn segment_to_segment(p1: P, p2: P, q1: P, q2: P, ruler: &CheapRuler) -> f64 {
    let d1 = point_to_line(p1, &[q1, q2], ruler).min(point_to_line(p2, &[q1, q2], ruler));
    let d2 = point_to_line(q1, &[p1, p2], ruler).min(point_to_line(q2, &[p1, p2], ruler));
    d1.min(d2)
}

fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

fn line_to_line(l1: &[P], r1: Range, l2: &[P], r2: Range, ruler: &CheapRuler) -> f64 {
    if !(range_safe(r1, l1.len()) && range_safe(r2, l2.len())) {
        return f64::INFINITY;
    }
    let mut d = f64::INFINITY;
    for i in r1[0]..r1[1] {
        for j in r2[0]..r2[1] {
            if segment_intersect_segment(l1[i], l1[i + 1], l2[j], l2[j + 1]) {
                return 0.0;
            }
            d = js_min(d, segment_to_segment(l1[i], l1[i + 1], l2[j], l2[j + 1], ruler));
        }
    }
    d
}

fn points_to_points(p1: &[P], r1: Range, p2: &[P], r2: Range, ruler: &CheapRuler) -> f64 {
    if !(range_safe(r1, p1.len()) && range_safe(r2, p2.len())) {
        return f64::NAN;
    }
    let mut d = f64::INFINITY;
    for a in &p1[r1[0]..=r1[1]] {
        for b in &p2[r2[0]..=r2[1]] {
            d = js_min(d, ruler.distance(*a, *b));
            if d == 0.0 {
                return d;
            }
        }
    }
    d
}

fn point_to_polygon(p: P, poly: &[Vec<P>], ruler: &CheapRuler) -> f64 {
    if point_within_polygon(p, poly, true) {
        return 0.0;
    }
    let mut d = f64::INFINITY;
    for ring in poly {
        let (front, back) = (ring[0], ring[ring.len() - 1]);
        if front != back {
            d = js_min(d, point_to_line(p, &[back, front], ruler));
            if d == 0.0 {
                return d;
            }
        }
        d = js_min(d, ruler.distance(p, ruler.point_on_line(ring, p)));
        if d == 0.0 {
            return d;
        }
    }
    d
}

fn line_to_polygon(line: &[P], r: Range, poly: &[Vec<P>], ruler: &CheapRuler) -> f64 {
    if !range_safe(r, line.len()) {
        return f64::NAN;
    }
    if line[r[0]..=r[1]].iter().any(|&p| point_within_polygon(p, poly, true)) {
        return 0.0;
    }
    let mut d = f64::INFINITY;
    for i in r[0]..r[1] {
        let (p1, p2) = (line[i], line[i + 1]);
        for ring in poly {
            let len = ring.len();
            for j in 0..len {
                let k = if j == 0 { len - 1 } else { j - 1 };
                if segment_intersect_segment(p1, p2, ring[k], ring[j]) {
                    return 0.0;
                }
                d = js_min(d, segment_to_segment(p1, p2, ring[k], ring[j], ruler));
            }
        }
    }
    d
}

fn polygon_intersect(a: &[Vec<P>], b: &[Vec<P>]) -> bool {
    a.iter().flatten().any(|&p| point_within_polygon(p, b, true))
}

fn polygon_to_polygon(a: &[Vec<P>], b: &[Vec<P>], ruler: &CheapRuler, current: f64) -> f64 {
    let (b1, b2) = (polygon_bbox(a), polygon_bbox(b));
    if current != f64::INFINITY && bbox_distance(&b1, &b2, ruler) >= current {
        return current;
    }
    if box_within_box(&b1, &b2) {
        if polygon_intersect(a, b) {
            return 0.0;
        }
    } else if polygon_intersect(b, a) {
        return 0.0;
    }
    let mut d = f64::INFINITY;
    for r1 in a {
        let n1 = r1.len();
        for i in 0..n1 {
            let l = if i == 0 { n1 - 1 } else { i - 1 };
            for r2 in b {
                let n2 = r2.len();
                for j in 0..n2 {
                    let k = if j == 0 { n2 - 1 } else { j - 1 };
                    if segment_intersect_segment(r1[l], r1[i], r2[k], r2[j]) {
                        return 0.0;
                    }
                    d = js_min(d, segment_to_segment(r1[l], r1[i], r2[k], r2[j], ruler));
                }
            }
        }
    }
    d
}

/// Pops the entry with the largest distance first, like upstream's TinyQueue comparator.
fn pop_max(q: &mut Vec<(f64, Range, Range)>) -> Option<(f64, Range, Range)> {
    let i = (0..q.len()).max_by(|&a, &b| q[a].0.partial_cmp(&q[b].0).unwrap_or(std::cmp::Ordering::Equal))?;
    Some(q.swap_remove(i))
}

fn points_to_polygon(points: &[P], is_line: bool, poly: &[Vec<P>], ruler: &CheapRuler, current: f64) -> f64 {
    let mut mini = js_min(ruler.distance(points[0], poly[0][0]), current);
    if mini == 0.0 {
        return mini;
    }
    let mut q = vec![(0.0, [0, points.len() - 1], [0, 0])];
    let pb = polygon_bbox(poly);
    while let Some((d, range, _)) = pop_max(&mut q) {
        if d >= mini {
            continue;
        }
        let threshold = if is_line { MIN_LINE_POINTS } else { MIN_POINTS };
        if range_size(range) <= threshold {
            if !range_safe(range, points.len()) {
                return f64::NAN;
            }
            if is_line {
                let t = line_to_polygon(points, range, poly, ruler);
                if t.is_nan() || t == 0.0 {
                    return t;
                }
                mini = js_min(mini, t);
            } else {
                for &p in &points[range[0]..=range[1]] {
                    mini = js_min(mini, point_to_polygon(p, poly, ruler));
                    if mini == 0.0 {
                        return 0.0;
                    }
                }
            }
        } else {
            for r in split_range(range, is_line).into_iter().flatten() {
                let t = bbox_distance(&bbox_of(points, r), &pb, ruler);
                if t < mini {
                    q.push((t, r, [0, 0]));
                }
            }
        }
    }
    mini
}

fn set_to_set(s1: &[P], l1: bool, s2: &[P], l2: bool, ruler: &CheapRuler, current: f64) -> f64 {
    let mut mini = js_min(current, ruler.distance(s1[0], s2[0]));
    if mini == 0.0 {
        return mini;
    }
    let mut q = vec![(0.0, [0, s1.len() - 1], [0, s2.len() - 1])];
    let t1 = if l1 { MIN_LINE_POINTS } else { MIN_POINTS };
    let t2 = if l2 { MIN_LINE_POINTS } else { MIN_POINTS };
    while let Some((d, ra, rb)) = pop_max(&mut q) {
        if d >= mini {
            continue;
        }
        if range_size(ra) <= t1 && range_size(rb) <= t2 {
            if !range_safe(ra, s1.len()) && range_safe(rb, s2.len()) {
                return f64::NAN;
            }
            if l1 && l2 {
                mini = js_min(mini, line_to_line(s1, ra, s2, rb, ruler));
            } else if l1 {
                let sub = &s1[ra[0]..=ra[1]];
                for &p in &s2[rb[0]..=rb[1]] {
                    mini = js_min(mini, point_to_line(p, sub, ruler));
                    if mini == 0.0 {
                        return mini;
                    }
                }
            } else if l2 {
                let sub = &s2[rb[0]..=rb[1]];
                for &p in &s1[ra[0]..=ra[1]] {
                    mini = js_min(mini, point_to_line(p, sub, ruler));
                    if mini == 0.0 {
                        return mini;
                    }
                }
            } else {
                mini = js_min(mini, points_to_points(s1, ra, s2, rb, ruler));
            }
        } else {
            let a = split_range(ra, l1);
            let b = split_range(rb, l2);
            for x in [a[0], a[1]] {
                for y in [b[0], b[1]] {
                    if let (Some(x), Some(y)) = (x, y) {
                        let t = bbox_distance(&bbox_of(s1, x), &bbox_of(s2, y), ruler);
                        if t < mini {
                            q.push((t, x, y));
                        }
                    }
                }
            }
        }
    }
    mini
}

fn signed_area(ring: &[P]) -> f64 {
    let mut sum = 0.0;
    let len = ring.len();
    for i in 0..len {
        let j = if i == 0 { len - 1 } else { i - 1 };
        sum += (ring[j][0] - ring[i][0]) * (ring[i][1] + ring[j][1]);
    }
    sum
}

/// Group rings into polygons (outer ring followed by its holes) by winding order.
pub fn classify_rings(rings: &[Vec<P>]) -> Vec<Vec<Vec<P>>> {
    if rings.len() <= 1 {
        return vec![rings.to_vec()];
    }
    let mut polygons = Vec::new();
    let mut polygon: Option<Vec<Vec<P>>> = None;
    let mut ccw: Option<bool> = None;
    for ring in rings {
        let area = signed_area(ring);
        if area == 0.0 {
            continue;
        }
        let ccw = *ccw.get_or_insert(area < 0.0);
        if ccw == (area < 0.0) {
            if let Some(p) = polygon.take() {
                polygons.push(p);
            }
            polygon = Some(vec![ring.clone()]);
        } else if let Some(p) = polygon.as_mut() {
            p.push(ring.clone());
        }
    }
    polygons.extend(polygon);
    polygons
}

pub fn distance(geom_type: Option<&str>, geometry: &[Vec<P>], c: CanonicalTileId, geoms: &[SimpleGeometry]) -> f64 {
    let to_ll = |p: &P| lnglat_from_tile(*p, c);
    match geom_type {
        Some(t @ ("Point" | "LineString")) => {
            if geometry.is_empty() {
                return f64::NAN;
            }
            let is_line = t == "LineString";
            let pts: Vec<P> = geometry.iter().flatten().map(to_ll).collect();
            if pts.is_empty() {
                return f64::NAN;
            }
            let ruler = CheapRuler::new(pts[0][1]);
            let mut d = f64::INFINITY;
            for g in geoms {
                d = js_min(
                    d,
                    match g {
                        SimpleGeometry::Point(p) => set_to_set(&pts, is_line, &[*p], false, &ruler, d),
                        SimpleGeometry::LineString(l) => set_to_set(&pts, is_line, l, true, &ruler, d),
                        SimpleGeometry::Polygon(poly) => points_to_polygon(&pts, is_line, poly, &ruler, d),
                    },
                );
                if d == 0.0 {
                    return d;
                }
            }
            d
        }
        Some("Polygon") => {
            if geometry.is_empty() || geometry[0].is_empty() {
                return f64::NAN;
            }
            let polygons: Vec<Vec<Vec<P>>> =
                classify_rings(geometry).into_iter().map(|poly| poly.iter().map(|r| r.iter().map(to_ll).collect()).collect()).collect();
            let ruler = CheapRuler::new(polygons[0][0][0][1]);
            let mut d = f64::INFINITY;
            for g in geoms {
                for poly in &polygons {
                    d = js_min(
                        d,
                        match g {
                            SimpleGeometry::Point(p) => points_to_polygon(&[*p], false, poly, &ruler, d),
                            SimpleGeometry::LineString(l) => points_to_polygon(l, true, poly, &ruler, d),
                            SimpleGeometry::Polygon(p2) => polygon_to_polygon(poly, p2, &ruler, d),
                        },
                    );
                    if d == 0.0 {
                        return d;
                    }
                }
            }
            d
        }
        _ => f64::NAN,
    }
}
