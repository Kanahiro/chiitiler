//! GeoJSON source tiling, ported from `@maplibre/geojson-vt` (convert, simplify, clip,
//! wrap, tile, transform) with MapLibre's options (extent 8192, buffer/tolerance scaled by 16).

use crate::expression::{Object, Properties, Value};
use crate::mvt::{Feature, GeomType, Layer};
use serde_json::Value as J;
use std::rc::Rc;

pub const EXTENT: f64 = 8192.0;
pub const CLIP_START: &str = "geojsonvt_clip_start";
pub const CLIP_END: &str = "geojsonvt_clip_end";

/// A line or ring: `[x, y, importance]` in unit mercator space, plus metrics.
#[derive(Debug, Clone)]
struct Slice {
    points: Vec<[f64; 3]>,
    /// Length (lines) or absolute area (rings) in unit space.
    size: f64,
    /// Distance along the original line where this slice starts/ends (line metrics).
    start: f64,
    end: f64,
}

#[derive(Debug, Clone)]
enum Geom {
    Points(Vec<[f64; 3]>),
    Lines(Vec<Slice>),
    /// Polygons, each a list of rings (outer first).
    Polygons(Vec<Vec<Slice>>),
}

#[derive(Debug, Clone)]
struct GFeature {
    id: Option<Value>,
    geom: Geom,
    props: Rc<Object>,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

pub struct Options {
    pub maxzoom: u8,
    /// In tile units (extent 8192).
    pub buffer: f64,
    pub tolerance: f64,
    pub generate_id: bool,
    pub promote_id: Option<String>,
    pub line_metrics: bool,
    /// `cluster: true`: point clustering instead of tiling.
    pub cluster: Option<crate::supercluster::Options>,
}

pub struct GeoJsonIndex {
    features: Vec<GFeature>,
    options: Options,
    cluster: Option<crate::supercluster::ClusterIndex>,
}

fn project_x(lng: f64) -> f64 {
    lng / 360.0 + 0.5
}

fn project_y(lat: f64) -> f64 {
    let sin = (lat * std::f64::consts::PI / 180.0).sin();
    let y = 0.5 - 0.25 * ((1.0 + sin) / (1.0 - sin)).ln() / std::f64::consts::PI;
    y.clamp(0.0, 1.0)
}

fn simplify(coords: &mut [[f64; 3]], first: usize, last: usize, sq_tolerance: f64) {
    let mut stack = vec![(first, last)];
    while let Some((first, last)) = stack.pop() {
        let mut max_sq = sq_tolerance;
        let mid = first + ((last - first) >> 1);
        let mut min_pos_to_mid = last - first;
        let mut index = None;
        let ([ax, ay, _], [bx, by, _]) = (coords[first], coords[last]);
        for i in first + 1..last {
            let d = sq_seg_dist(coords[i][0], coords[i][1], ax, ay, bx, by);
            if d > max_sq {
                index = Some(i);
                max_sq = d;
            } else if d == max_sq {
                let pos = i.abs_diff(mid);
                if pos < min_pos_to_mid {
                    index = Some(i);
                    min_pos_to_mid = pos;
                }
            }
        }
        if max_sq > sq_tolerance
            && let Some(index) = index
        {
            coords[index][2] = max_sq;
            if index - first > 1 {
                stack.push((first, index));
            }
            if last - index > 1 {
                stack.push((index, last));
            }
        }
    }
}

fn sq_seg_dist(px: f64, py: f64, mut x: f64, mut y: f64, bx: f64, by: f64) -> f64 {
    let (mut dx, mut dy) = (bx - x, by - y);
    if dx != 0.0 || dy != 0.0 {
        let t = ((px - x) * dx + (py - y) * dy) / (dx * dx + dy * dy);
        if t > 1.0 {
            x = bx;
            y = by;
        } else if t > 0.0 {
            x += dx * t;
            y += dy * t;
        }
    }
    dx = px - x;
    dy = py - y;
    dx * dx + dy * dy
}

fn position(v: &J) -> Option<[f64; 2]> {
    Some([v.get(0)?.as_f64()?, v.get(1)?.as_f64()?])
}

fn convert_line(ring: &J, sq_tolerance: f64, is_polygon: bool) -> Option<Slice> {
    let ring = ring.as_array()?;
    let mut points = Vec::with_capacity(ring.len());
    let mut size = 0.0;
    let (mut x0, mut y0) = (0.0, 0.0);
    for (j, p) in ring.iter().enumerate() {
        let [lng, lat] = position(p)?;
        let (x, y) = (project_x(lng), project_y(lat));
        points.push([x, y, 0.0]);
        if j > 0 {
            size += if is_polygon { (x0 * y - x * y0) / 2.0 } else { ((x - x0).powi(2) + (y - y0).powi(2)).sqrt() };
        }
        x0 = x;
        y0 = y;
    }
    if points.is_empty() {
        return None;
    }
    let last = points.len() - 1;
    points[0][2] = 1.0;
    if sq_tolerance > 0.0 && last > 0 {
        simplify(&mut points, 0, last, sq_tolerance);
    }
    points[last][2] = 1.0;
    Some(Slice { points, size: size.abs(), start: 0.0, end: size.abs() })
}

impl GeoJsonIndex {
    pub fn new(data: &J, mut options: Options) -> GeoJsonIndex {
        let mut features = Vec::new();
        let sq_tolerance = (options.tolerance / ((1u64 << options.maxzoom) as f64 * EXTENT)).powi(2);
        let mut add = |f: &J, index: usize| convert_feature(&mut features, f, index, sq_tolerance, &options, 0);
        match data["type"].as_str() {
            Some("FeatureCollection") => {
                for (i, f) in data["features"].as_array().into_iter().flatten().enumerate() {
                    add(f, i);
                }
            }
            Some("Feature") => add(data, 0),
            Some(_) => add(&serde_json::json!({"type": "Feature", "geometry": data}), 0),
            None => {}
        }
        // World copies for features crossing the antimeridian (geojson-vt `wrap`).
        let b = options.buffer / EXTENT;
        let left = clip(&features, -1.0 - b, b, Axis::X);
        let right = clip(&features, 1.0 - b, 2.0 + b, Axis::X);
        if !left.is_empty() || !right.is_empty() {
            let mut merged: Vec<GFeature> = left.into_iter().map(|f| shift(f, 1.0)).collect();
            merged.extend(clip(&features, -b, 1.0 + b, Axis::X));
            merged.extend(right.into_iter().map(|f| shift(f, -1.0)));
            features = merged;
        }
        // Clustering only takes the (wrapped) points; other geometries are dropped.
        let cluster = options.cluster.take().map(|o| {
            let points = features
                .iter()
                .flat_map(|f| match &f.geom {
                    Geom::Points(ps) => ps.iter().map(|p| crate::supercluster::Point { id: f.id.clone(), xy: [p[0], p[1]], props: f.props.clone() }).collect(),
                    _ => vec![],
                })
                .collect();
            crate::supercluster::ClusterIndex::new(points, o)
        });
        GeoJsonIndex { features, options, cluster }
    }

    pub fn maxzoom(&self) -> u8 {
        self.options.maxzoom
    }

    /// The tile `z/x/y` as a single vector tile layer with extent 8192.
    pub fn tile(&self, z: u8, x: u32, y: u32) -> Layer {
        if let Some(c) = &self.cluster {
            return c.tile(z, x, y);
        }
        let z2 = (1u64 << z) as f64;
        let b = self.options.buffer / EXTENT;
        let cx = clip(&self.features, (x as f64 - b) / z2, (x as f64 + 1.0 + b) / z2, Axis::X);
        let clipped = clip(&cx, (y as f64 - b) / z2, (y as f64 + 1.0 + b) / z2, Axis::Y);
        let tolerance = if z == self.options.maxzoom { 0.0 } else { self.options.tolerance / (z2 * EXTENT) };
        let sq = tolerance * tolerance;
        let tp = |p: &[f64; 3]| -> [i32; 2] {
            [(EXTENT * (p[0] * z2 - x as f64)).round() as i32, (EXTENT * (p[1] * z2 - y as f64)).round() as i32]
        };
        let add_line = |out: &mut Vec<Vec<[i32; 2]>>, s: &Slice, is_polygon: bool, is_outer: bool| {
            if tolerance > 0.0 && s.size < if is_polygon { sq } else { tolerance } {
                return;
            }
            let mut ring: Vec<[f64; 3]> = s.points.iter().filter(|p| tolerance == 0.0 || p[2] > sq).copied().collect();
            if is_polygon {
                rewind(&mut ring, is_outer);
            }
            out.push(ring.iter().map(tp).collect());
        };
        let mut features = Vec::new();
        for f in &clipped {
            let (ty, geometry) = match &f.geom {
                Geom::Points(p) => (GeomType::Point, p.iter().map(|p| vec![tp(p)]).collect::<Vec<_>>()),
                Geom::Lines(lines) if self.options.line_metrics => {
                    for l in lines {
                        let mut g = Vec::new();
                        add_line(&mut g, l, false, false);
                        if g.is_empty() {
                            continue;
                        }
                        let mut props = (*f.props).clone();
                        props.insert(CLIP_START.into(), Value::Number(l.start / l.size));
                        props.insert(CLIP_END.into(), Value::Number(l.end / l.size));
                        features.push(Feature { id: f.id.clone(), ty: GeomType::LineString, properties: Properties::Object(Rc::new(props)), geometry: g });
                    }
                    continue;
                }
                Geom::Lines(lines) => {
                    let mut g = Vec::new();
                    for l in lines {
                        add_line(&mut g, l, false, false);
                    }
                    (GeomType::LineString, g)
                }
                Geom::Polygons(polys) => {
                    let mut g = Vec::new();
                    for poly in polys {
                        for (i, r) in poly.iter().enumerate() {
                            add_line(&mut g, r, true, i == 0);
                        }
                    }
                    (GeomType::Polygon, g)
                }
            };
            if geometry.is_empty() {
                continue;
            }
            features.push(Feature { id: f.id.clone(), ty, properties: Properties::Object(f.props.clone()), geometry });
        }
        Layer { name: String::new(), extent: EXTENT as u32, features }
    }
}

fn rewind(ring: &mut [[f64; 3]], clockwise: bool) {
    let len = ring.len();
    let mut area = 0.0;
    let mut j = len.wrapping_sub(1);
    for i in 0..len {
        area += (ring[i][0] - ring[j][0]) * (ring[i][1] + ring[j][1]);
        j = i;
    }
    if (area > 0.0) == clockwise {
        ring.reverse();
    }
}

fn convert_feature(out: &mut Vec<GFeature>, f: &J, index: usize, sq_tolerance: f64, o: &Options, depth: usize) {
    let geometry = &f["geometry"];
    if geometry.is_null() || depth > 1024 {
        return;
    }
    if geometry["type"] == "GeometryCollection" {
        for g in geometry["geometries"].as_array().into_iter().flatten() {
            let mut sub = f.clone();
            sub["geometry"] = g.clone();
            convert_feature(out, &sub, index, sq_tolerance, o, depth + 1);
        }
        return;
    }
    let coords = &geometry["coordinates"];
    if coords.as_array().is_none_or(|c| c.is_empty()) {
        return;
    }
    let props = match Value::from_json(&f["properties"]) {
        Value::Object(o) => o,
        _ => Default::default(),
    };
    let id = if let Some(p) = &o.promote_id {
        props.get(p.as_str()).cloned()
    } else if o.generate_id {
        Some(Value::Number(index as f64))
    } else {
        f.get("id").filter(|v| !v.is_null()).map(Value::from_json)
    };
    let point = |p: &J| position(p).map(|[x, y]| [project_x(x), project_y(y), 0.0]);
    let lines = |ls: &J, poly: bool| -> Vec<Slice> { ls.as_array().into_iter().flatten().filter_map(|l| convert_line(l, sq_tolerance, poly)).collect() };
    let geom = match geometry["type"].as_str() {
        Some("Point") => Geom::Points(point(coords).into_iter().collect()),
        Some("MultiPoint") => Geom::Points(coords.as_array().into_iter().flatten().filter_map(point).collect()),
        Some("LineString") => Geom::Lines(convert_line(coords, sq_tolerance, false).into_iter().collect()),
        Some("MultiLineString") if o.line_metrics => {
            for l in lines(coords, false) {
                out.push(with_bbox(id.clone(), Geom::Lines(vec![l]), props.clone()));
            }
            return;
        }
        Some("MultiLineString") => Geom::Lines(lines(coords, false)),
        Some("Polygon") => Geom::Polygons(vec![lines(coords, true)]),
        Some("MultiPolygon") => Geom::Polygons(coords.as_array().into_iter().flatten().map(|p| lines(p, true)).filter(|p| !p.is_empty()).collect()),
        _ => return,
    };
    out.push(with_bbox(id, geom, props));
}

fn with_bbox(id: Option<Value>, geom: Geom, props: Rc<Object>) -> GFeature {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    let mut upd = |p: &[f64; 3]| {
        min_x = min_x.min(p[0]);
        max_x = max_x.max(p[0]);
        min_y = min_y.min(p[1]);
        max_y = max_y.max(p[1]);
    };
    match &geom {
        Geom::Points(ps) => ps.iter().for_each(&mut upd),
        Geom::Lines(ls) => ls.iter().flat_map(|l| &l.points).for_each(&mut upd),
        Geom::Polygons(ps) => ps.iter().flatten().flat_map(|l| &l.points).for_each(&mut upd),
    }
    GFeature { id, geom, props, min_x, max_x, min_y, max_y }
}

fn shift(mut f: GFeature, dx: f64) -> GFeature {
    let mv = |p: &mut [f64; 3]| p[0] += dx;
    match &mut f.geom {
        Geom::Points(ps) => ps.iter_mut().for_each(mv),
        Geom::Lines(ls) => ls.iter_mut().flat_map(|l| &mut l.points).for_each(mv),
        Geom::Polygons(ps) => ps.iter_mut().flatten().flat_map(|l| &mut l.points).for_each(mv),
    }
    f.min_x += dx;
    f.max_x += dx;
    f
}

#[derive(Clone, Copy, PartialEq)]
enum Axis {
    X,
    Y,
}

fn clip(features: &[GFeature], start: f64, end: f64, axis: Axis) -> Vec<GFeature> {
    let mut out = Vec::new();
    for f in features {
        let (min, max) = if axis == Axis::X { (f.min_x, f.max_x) } else { (f.min_y, f.max_y) };
        if min >= start && max < end {
            out.push(f.clone());
            continue;
        }
        if max < start || min >= end {
            continue;
        }
        let a = |p: &[f64; 3]| if axis == Axis::X { p[0] } else { p[1] };
        let geom = match &f.geom {
            Geom::Points(ps) => Geom::Points(ps.iter().filter(|p| a(p) >= start && a(p) <= end).copied().collect()),
            Geom::Lines(ls) => {
                let mut nl = Vec::new();
                for l in ls {
                    clip_line(l, &mut nl, start, end, axis, false);
                }
                Geom::Lines(nl)
            }
            Geom::Polygons(ps) => Geom::Polygons(
                ps.iter()
                    .map(|poly| {
                        let mut np = Vec::new();
                        for r in poly {
                            clip_line(r, &mut np, start, end, axis, true);
                        }
                        np
                    })
                    .filter(|p| !p.is_empty())
                    .collect(),
            ),
        };
        let empty = match &geom {
            Geom::Points(p) => p.is_empty(),
            Geom::Lines(l) => l.is_empty(),
            Geom::Polygons(p) => p.is_empty(),
        };
        if !empty {
            out.push(with_bbox(f.id.clone(), geom, f.props.clone()));
        }
    }
    out
}

fn clip_line(g: &Slice, out: &mut Vec<Slice>, start: f64, end: f64, axis: Axis, is_polygon: bool) {
    let new_slice = || Slice { points: vec![], size: g.size, start: g.start, end: g.end };
    let mut slice = new_slice();
    let pts = &g.points;
    let ax_of = |p: &[f64; 3]| if axis == Axis::X { p[0] } else { p[1] };
    let intersect = |s: &mut Slice, p: &[f64; 3], q: &[f64; 3], v: f64| -> f64 {
        if axis == Axis::X {
            let t = (v - p[0]) / (q[0] - p[0]);
            s.points.push([v, p[1] + (q[1] - p[1]) * t, 1.0]);
            t
        } else {
            let t = (v - p[1]) / (q[1] - p[1]);
            s.points.push([p[0] + (q[0] - p[0]) * t, v, 1.0]);
            t
        }
    };
    let mut len = g.start;
    let mut t = 0.0;
    for i in 0..pts.len().saturating_sub(1) {
        let (p, q) = (&pts[i], &pts[i + 1]);
        let (a, b) = (ax_of(p), ax_of(q));
        let seg_len = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
        let mut exited = false;
        if a < start {
            if b >= start {
                t = intersect(&mut slice, p, q, start);
                slice.start = len + seg_len * t;
            }
        } else if a > end {
            if b <= end {
                t = intersect(&mut slice, p, q, end);
                slice.start = len + seg_len * t;
            }
        } else {
            slice.points.push(*p);
        }
        if b < start && a >= start {
            t = intersect(&mut slice, p, q, start);
            exited = true;
        }
        if b > end && a <= end {
            t = intersect(&mut slice, p, q, end);
            exited = true;
        }
        if !is_polygon && exited {
            slice.end = len + seg_len * t;
            out.push(std::mem::replace(&mut slice, new_slice()));
        }
        len += seg_len;
    }
    if let Some(last) = pts.last() {
        let a = ax_of(last);
        if a >= start && a <= end {
            slice.points.push(*last);
        }
    }
    if is_polygon && slice.points.len() >= 2 {
        let (f, l) = (slice.points[0], slice.points[slice.points.len() - 1]);
        if f[0] != l[0] || f[1] != l[1] {
            slice.points.push(f);
        }
    }
    if !slice.points.is_empty() {
        out.push(slice);
    }
}
