//! Point clustering for GeoJSON sources with `cluster: true`, ported from `@maplibre/geojson-vt`'s
//! `ClusterTileIndex` (supercluster) and `kdbush`, keeping their traversal orders, which decide
//! the order of features in a tile and the order `clusterProperties` are reduced in.

use crate::expression::value::{js_number_to_string, js_round};
use crate::expression::{EvalCtx, Expression, Feature as EvalFeature, Globals, Object, Value};
use crate::mvt::{Feature, GeomType, Layer};
use std::rc::Rc;

const SCALE: f64 = 1073741824.0; // 0x40000000
const MAX_INTEGER: f64 = 2147483647.0;

fn encode(c: f64) -> f64 {
    (c - 0.5) * SCALE
}

fn decode(v: f64) -> f64 {
    v / SCALE + 0.5
}

/// `KDBush` over Int32 coordinates.
struct KdBush {
    ids: Vec<u32>,
    coords: Vec<i32>,
    node_size: usize,
}

impl KdBush {
    fn new(coords: &[i32], node_size: usize) -> KdBush {
        let n = coords.len() / 2;
        let mut t = KdBush { ids: (0..n as u32).collect(), coords: coords.to_vec(), node_size };
        if n > 0 {
            t.sort(0, n - 1, 0);
        }
        t
    }

    fn sort(&mut self, left: usize, right: usize, axis: usize) {
        if right - left <= self.node_size {
            return;
        }
        let m = (left + right) >> 1;
        self.select(m, left, right, axis);
        if m > 0 {
            self.sort(left, m - 1, 1 - axis);
        }
        self.sort(m + 1, right, 1 - axis);
    }

    fn select(&mut self, k: usize, mut left: usize, mut right: usize, axis: usize) {
        let c = |t: &KdBush, i: usize| t.coords[2 * i + axis];
        while right > left {
            if right - left > 600 {
                let n = (right - left + 1) as f64;
                let m = (k - left + 1) as f64;
                let z = n.ln();
                let s = 0.5 * (2.0 * z / 3.0).exp();
                let sd = 0.5 * (z * s * (n - s) / n).sqrt() * if m - n / 2.0 < 0.0 { -1.0 } else { 1.0 };
                let new_left = (left as f64).max((k as f64 - m * s / n + sd).floor()) as usize;
                let new_right = (right as f64).min((k as f64 + (n - m) * s / n + sd).floor()) as usize;
                self.select(k, new_left, new_right, axis);
            }
            let t = c(self, k);
            let (mut i, mut j) = (left as isize, right as isize);
            self.swap(left, k);
            if c(self, right) > t {
                self.swap(left, right);
            }
            while i < j {
                self.swap(i as usize, j as usize);
                i += 1;
                j -= 1;
                while c(self, i as usize) < t {
                    i += 1;
                }
                while c(self, j as usize) > t {
                    j -= 1;
                }
            }
            if c(self, left) == t {
                self.swap(left, j as usize);
            } else {
                j += 1;
                self.swap(j as usize, right);
            }
            if j as usize <= k {
                left = j as usize + 1;
            }
            if k <= j as usize {
                if j == 0 {
                    break;
                }
                right = j as usize - 1;
            }
        }
    }

    fn swap(&mut self, i: usize, j: usize) {
        self.ids.swap(i, j);
        self.coords.swap(2 * i, 2 * j);
        self.coords.swap(2 * i + 1, 2 * j + 1);
    }

    /// Depth-first traversal of the kd nodes in kdbush's stack order.
    fn visit(&self, inside_node: impl Fn(f64, f64) -> bool, go_left: impl Fn(usize, f64, f64) -> bool, go_right: impl Fn(usize, f64, f64) -> bool) -> Vec<usize> {
        let mut out = Vec::new();
        if self.ids.is_empty() {
            return out;
        }
        let mut stack: Vec<(usize, usize, usize)> = vec![(0, self.ids.len() - 1, 0)];
        let at = |i: usize| (self.coords[2 * i] as f64, self.coords[2 * i + 1] as f64);
        while let Some((left, right, axis)) = stack.pop() {
            if right - left <= self.node_size {
                for i in left..=right {
                    let (x, y) = at(i);
                    if inside_node(x, y) {
                        out.push(self.ids[i] as usize);
                    }
                }
                continue;
            }
            let m = (left + right) >> 1;
            let (x, y) = at(m);
            if inside_node(x, y) {
                out.push(self.ids[m] as usize);
            }
            if go_left(axis, x, y) && m > left {
                stack.push((left, m - 1, 1 - axis));
            }
            if go_right(axis, x, y) {
                stack.push((m + 1, right, 1 - axis));
            }
        }
        out
    }

    fn range(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<usize> {
        self.visit(
            |x, y| x >= min_x && x <= max_x && y >= min_y && y <= max_y,
            |axis, x, y| if axis == 0 { min_x <= x } else { min_y <= y },
            |axis, x, y| if axis == 0 { max_x >= x } else { max_y >= y },
        )
    }

    fn within(&self, qx: f64, qy: f64, r: f64) -> Vec<usize> {
        let r2 = r * r;
        self.visit(
            |x, y| (x - qx) * (x - qx) + (y - qy) * (y - qy) <= r2,
            |axis, x, y| if axis == 0 { qx - r <= x } else { qy - r <= y },
            |axis, x, y| if axis == 0 { qx + r >= x } else { qy + r >= y },
        )
    }
}

/// An input point: unit-square position, id and properties.
pub struct Point {
    pub id: Option<Value>,
    pub xy: [f64; 2],
    pub props: Rc<Object>,
}

/// `clusterProperties`: per property, the map and reduce expressions.
pub type ClusterProperties = Vec<(Rc<str>, Expression, Expression)>;

pub struct Options {
    pub max_zoom: u8,
    pub min_points: u32,
    /// In tile units of `extent`.
    pub radius: f64,
    pub extent: f64,
    pub generate_id: bool,
    pub properties: ClusterProperties,
}

/// Per-item data: zoom it was processed at, id, point count, props index.
#[derive(Clone, Copy)]
struct Item {
    zoom: i64,
    id: i64,
    num: i64,
    prop: i64,
}

struct Tree {
    kd: KdBush,
    coords: Vec<i32>,
    data: Vec<Item>,
}

pub struct ClusterIndex {
    options: Options,
    points: Vec<Point>,
    trees: Vec<Tree>,
    /// Tree index per zoom (0..=maxZoom+1).
    zoom_tree: Vec<usize>,
    cluster_props: Vec<Object>,
}

fn eval(e: &Expression, props: &Rc<Object>, accumulated: Option<Value>) -> Value {
    let f = EvalFeature { properties: crate::expression::Properties::Object(props.clone()), ..Default::default() };
    let mut ctx = EvalCtx { globals: Globals { zoom: 0.0, accumulated, ..Default::default() }, feature: Some(&f), ..Default::default() };
    e.evaluate(&mut ctx)
}

impl ClusterIndex {
    pub fn new(points: Vec<Point>, options: Options) -> ClusterIndex {
        let max_zoom = options.max_zoom as usize;
        let mut idx = ClusterIndex { options, points, trees: Vec::new(), zoom_tree: vec![0; max_zoom + 2], cluster_props: Vec::new() };
        let fits = |c: f64| encode(c).abs() < MAX_INTEGER;
        let (mut coords, mut data) = (Vec::new(), Vec::new());
        for (i, p) in idx.points.iter().enumerate() {
            if !fits(p.xy[0]) || !fits(p.xy[1]) {
                continue;
            }
            coords.push(js_round(encode(p.xy[0])) as i32);
            coords.push(js_round(encode(p.xy[1])) as i32);
            data.push(Item { zoom: max_zoom as i64 + 1, id: i as i64, num: 1, prop: -1 });
        }
        // Zoom levels share a tree when clustering changed nothing (`isSameAsTree`).
        let mut trees = vec![Tree { kd: KdBush::new(&coords, 64), coords, data }];
        let mut cur = 0;
        idx.zoom_tree[max_zoom + 1] = 0;
        for z in (0..=max_zoom).rev() {
            let (next_coords, next_data) = idx.cluster(&mut trees[cur], z as i64);
            let same = next_data.len() == trees[cur].data.len() && next_data.iter().zip(&trees[cur].data).all(|(a, b)| a.id == b.id);
            if !same {
                trees.push(Tree { kd: KdBush::new(&next_coords, 64), coords: next_coords, data: next_data });
                cur = trees.len() - 1;
            }
            idx.zoom_tree[z] = cur;
        }
        idx.trees = trees;
        idx
    }

    /// The mapped properties of a point (`options.map`), or a cluster's accumulated ones.
    fn map(&self, item: &Item) -> Object {
        if item.num > 1 {
            return self.cluster_props[item.prop as usize].clone();
        }
        let props = &self.points[item.id as usize].props;
        self.options.properties.iter().map(|(k, m, _)| (k.clone(), eval(m, props, None))).collect()
    }

    fn cluster(&mut self, tree: &mut Tree, zoom: i64) -> (Vec<i32>, Vec<Item>) {
        let o = &self.options;
        let r = o.radius / (o.extent * 2f64.powi(zoom as i32)) * SCALE;
        let reduce = !o.properties.is_empty();
        let n_points = self.points.len() as i64;
        let (mut next_coords, mut next_data) = (Vec::new(), Vec::new());
        for index in 0..tree.data.len() {
            if tree.data[index].zoom <= zoom {
                continue;
            }
            tree.data[index].zoom = zoom;
            let (x, y) = (tree.coords[index * 2] as f64, tree.coords[index * 2 + 1] as f64);
            let neighbors = tree.kd.within(x, y, r);
            let origin = tree.data[index].num;
            let mut num = origin;
            for &n in &neighbors {
                if tree.data[n].zoom > zoom {
                    num += tree.data[n].num;
                }
            }
            if num > origin && num >= self.options.min_points as i64 {
                let (mut wx, mut wy) = (x * origin as f64, y * origin as f64);
                let mut prop_index = -1;
                let id = ((index as i64) << 5) + (zoom + 1) + n_points;
                for &n in &neighbors {
                    if tree.data[n].zoom <= zoom {
                        continue;
                    }
                    tree.data[n].zoom = zoom;
                    let n2 = tree.data[n].num as f64;
                    wx += tree.coords[n * 2] as f64 * n2;
                    wy += tree.coords[n * 2 + 1] as f64 * n2;
                    if reduce {
                        if prop_index < 0 {
                            let p = self.map(&tree.data[index]);
                            prop_index = self.cluster_props.len() as i64;
                            self.cluster_props.push(p);
                        }
                        let other = Rc::new(self.map(&tree.data[n]));
                        let acc = &mut self.cluster_props[prop_index as usize];
                        for (k, _, red) in &self.options.properties {
                            let v = eval(red, &other, acc.get(k).cloned());
                            acc.insert(k.clone(), v);
                        }
                    }
                }
                next_coords.push(js_round(wx / num as f64) as i32);
                next_coords.push(js_round(wy / num as f64) as i32);
                next_data.push(Item { zoom: self.options.max_zoom as i64 + 1, id, num, prop: prop_index });
            } else {
                next_coords.extend_from_slice(&tree.coords[index * 2..index * 2 + 2]);
                next_data.push(tree.data[index]);
                if num > 1 {
                    for &n in &neighbors {
                        if tree.data[n].zoom <= zoom {
                            continue;
                        }
                        tree.data[n].zoom = zoom;
                        next_coords.extend_from_slice(&tree.coords[n * 2..n * 2 + 2]);
                        next_data.push(tree.data[n]);
                    }
                }
            }
        }
        (next_coords, next_data)
    }

    /// `getTile`: clusters and points of tile `z/x/y` (world-wrapped at the edges).
    pub fn tile(&self, z: u8, x: u32, y: u32) -> Layer {
        let mut layer = Layer { name: String::new(), extent: self.options.extent as u32, features: vec![] };
        let tree = &self.trees[self.zoom_tree[(z as usize).min(self.options.max_zoom as usize + 1)]];
        let z2 = 2f64.powi(z as i32);
        let p = self.options.radius / self.options.extent;
        let (x, y) = (x as f64, y as f64);
        let (top, bottom) = (encode((y - p) / z2), encode((y + 1.0 + p) / z2));
        self.add(&mut layer, tree, tree.kd.range(encode((x - p) / z2), top, encode((x + 1.0 + p) / z2), bottom), x, y, z2);
        if x == 0.0 {
            self.add(&mut layer, tree, tree.kd.range(encode(1.0 - p / z2), top, encode(1.0), bottom), z2, y, z2);
        }
        if x == z2 - 1.0 {
            self.add(&mut layer, tree, tree.kd.range(encode(0.0), top, encode(p / z2), bottom), -1.0, y, z2);
        }
        layer
    }

    fn add(&self, layer: &mut Layer, tree: &Tree, ids: Vec<usize>, x: f64, y: f64, z2: f64) {
        let extent = self.options.extent;
        for i in ids {
            let d = &tree.data[i];
            let cluster = d.num > 1;
            let (props, px, py, id) = if cluster {
                let mut props = if d.prop < 0 { Object::default() } else { self.cluster_props[d.prop as usize].clone() };
                let count = d.num as f64;
                let abbrev = if count >= 10000.0 {
                    Value::str(&format!("{}k", js_number_to_string(js_round(count / 1000.0))))
                } else if count >= 1000.0 {
                    Value::str(&format!("{}k", js_number_to_string(js_round(count / 100.0) / 10.0)))
                } else {
                    Value::Number(count)
                };
                props.insert("cluster".into(), Value::Bool(true));
                props.insert("cluster_id".into(), Value::Number(d.id as f64));
                props.insert("point_count".into(), Value::Number(count));
                props.insert("point_count_abbreviated".into(), abbrev);
                (Rc::new(props), decode(tree.coords[i * 2] as f64), decode(tree.coords[i * 2 + 1] as f64), Some(Value::Number(d.id as f64)))
            } else {
                let p = &self.points[d.id as usize];
                let id = if self.options.generate_id { Some(Value::Number(d.id as f64)) } else { p.id.clone() };
                (p.props.clone(), p.xy[0], p.xy[1], id)
            };
            let g = [js_round(extent * (px * z2 - x)) as i32, js_round(extent * (py * z2 - y)) as i32];
            layer.features.push(Feature { id, ty: GeomType::Point, properties: crate::expression::Properties::Object(props), geometry: vec![vec![g]] });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_nearby_points() {
        let pt = |x: f64, y: f64| Point { id: None, xy: [x, y], props: Rc::new(Object::default()) };
        let opts = Options { max_zoom: 16, min_points: 2, radius: 800.0, extent: 8192.0, generate_id: false, properties: vec![] };
        let idx = ClusterIndex::new(vec![pt(0.5, 0.5), pt(0.500001, 0.5), pt(0.9, 0.9)], opts);
        let z0 = idx.tile(0, 0, 0);
        assert_eq!(z0.features.len(), 2);
        let c = z0.features.iter().find(|f| f.properties.contains_key("cluster")).unwrap();
        assert_eq!(c.properties.get("point_count").and_then(|v| v.as_f64()), Some(2.0));
        // Unclustered past maxZoom.
        assert_eq!(idx.tile(17, 65536, 65536).features.len(), 2);
    }
}
