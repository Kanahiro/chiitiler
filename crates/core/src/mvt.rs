//! Mapbox Vector Tile (protobuf) decoding and encoding.

use crate::expression::{Properties, TagTable, Value};
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeomType {
    Unknown = 0,
    Point = 1,
    LineString = 2,
    Polygon = 3,
}

impl GeomType {
    pub fn name(self) -> &'static str {
        match self {
            GeomType::Unknown => "Unknown",
            GeomType::Point => "Point",
            GeomType::LineString => "LineString",
            GeomType::Polygon => "Polygon",
        }
    }
}

/// A decoded feature in tile coordinates of its layer's extent. Rings/lines are explicit
/// (polygon rings closed by repeating the first point, like vector-tile-js).
#[derive(Debug, Clone)]
pub struct Feature {
    pub id: Option<Value>,
    pub ty: GeomType,
    pub properties: Properties,
    pub geometry: Vec<Vec<[i32; 2]>>,
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub name: String,
    pub extent: u32,
    pub features: Vec<Feature>,
}

pub(crate) struct Reader<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = *self.buf.get(self.pos)?;
            self.pos += 1;
            v |= ((b & 0x7f) as u64) << shift;
            if b < 0x80 {
                return Some(v);
            }
        }
        None
    }

    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let len = self.varint()? as usize;
        let s = self.buf.get(self.pos..self.pos + len)?;
        self.pos += len;
        Some(s)
    }

    /// Next (field, wire type); `None` at end.
    pub fn field(&mut self) -> Option<(u64, u8)> {
        if self.pos >= self.buf.len() {
            return None;
        }
        let k = self.varint()?;
        Some((k >> 3, (k & 7) as u8))
    }

    pub fn skip(&mut self, wire: u8) -> Option<()> {
        match wire {
            0 => {
                self.varint()?;
            }
            1 => self.pos += 8,
            2 => {
                self.bytes()?;
            }
            5 => self.pos += 4,
            _ => return None,
        }
        (self.pos <= self.buf.len()).then_some(())
    }

    fn fixed<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.buf.get(self.pos..self.pos + N)?;
        self.pos += N;
        s.try_into().ok()
    }
}

/// Packed varints, into a reused buffer.
fn packed(data: &[u8], out: &mut Vec<u32>) -> Option<()> {
    let mut r = Reader { buf: data, pos: 0 };
    out.clear();
    while r.pos < data.len() {
        out.push(r.varint()? as u32);
    }
    Some(())
}

fn decode_value(data: &[u8]) -> Option<Value> {
    let mut r = Reader { buf: data, pos: 0 };
    let mut v = Value::Null;
    while let Some((f, w)) = r.field() {
        v = match (f, w) {
            (1, 2) => Value::str(std::str::from_utf8(r.bytes()?).ok()?),
            (2, 5) => Value::Number(f32::from_le_bytes(r.fixed()?) as f64),
            (3, 1) => Value::Number(f64::from_le_bytes(r.fixed()?)),
            (4, 0) => Value::Number(r.varint()? as i64 as f64),
            (5, 0) => Value::Number(r.varint()? as f64),
            (6, 0) => {
                let n = r.varint()?;
                Value::Number(((n >> 1) as i64 ^ -((n & 1) as i64)) as f64)
            }
            (7, 0) => Value::Bool(r.varint()? != 0),
            _ => {
                r.skip(w)?;
                continue;
            }
        };
    }
    Some(v)
}

fn decode_geometry(cmds: &[u32]) -> Vec<Vec<[i32; 2]>> {
    let mut lines: Vec<Vec<[i32; 2]>> = Vec::new();
    let mut line: Option<Vec<[i32; 2]>> = None;
    let (mut x, mut y) = (0i32, 0i32);
    let mut i = 0;
    let zz = |v: u32| ((v >> 1) as i32) ^ -((v & 1) as i32);
    while i < cmds.len() {
        let cmd = cmds[i] & 7;
        let count = cmds[i] >> 3;
        i += 1;
        match cmd {
            1 | 2 => {
                for _ in 0..count {
                    let (Some(&dx), Some(&dy)) = (cmds.get(i), cmds.get(i + 1)) else { return lines };
                    i += 2;
                    x += zz(dx);
                    y += zz(dy);
                    if cmd == 1 {
                        lines.extend(line.take());
                        // Sized for the LineTo (and ClosePath) that usually follow: growing by
                        // doubling would leave up to half of each ring unused.
                        let mut n = 1;
                        if let Some(&next) = cmds.get(i) && next & 7 == 2 {
                            n += (next >> 3) as usize;
                            let close = i + 1 + 2 * (next >> 3) as usize;
                            n += cmds.get(close).is_some_and(|&c| c & 7 == 7) as usize;
                        }
                        line = Some(Vec::with_capacity(n));
                    }
                    line.get_or_insert_with(Vec::new).push([x, y]);
                }
            }
            7 => {
                if let Some(l) = line.as_mut()
                    && let Some(&first) = l.first()
                {
                    l.push(first);
                }
            }
            _ => return lines,
        }
    }
    lines.extend(line);
    lines
}

fn layer_name(data: &[u8]) -> Option<&[u8]> {
    let mut r = Reader { buf: data, pos: 0 };
    while let Some((f, w)) = r.field() {
        if (f, w) == (1, 2) {
            return r.bytes();
        }
        r.skip(w)?;
    }
    Some(&[])
}

fn decode_layer(data: &[u8]) -> Option<Layer> {
    let mut r = Reader { buf: data, pos: 0 };
    let mut name = String::new();
    let mut extent = 4096;
    let mut table = TagTable::default();
    let mut raw_features: Vec<&[u8]> = Vec::new();
    while let Some((f, w)) = r.field() {
        match (f, w) {
            (1, 2) => name = String::from_utf8_lossy(r.bytes()?).into_owned(),
            (2, 2) => raw_features.push(r.bytes()?),
            (3, 2) => table.keys.push(String::from_utf8_lossy(r.bytes()?).into()),
            (4, 2) => table.values.push(decode_value(r.bytes()?)?),
            (5, 0) => extent = r.varint()? as u32,
            _ => r.skip(w)?,
        }
    }
    let mut decoded = Vec::with_capacity(raw_features.len());
    let (mut tags, mut geom) = (Vec::new(), Vec::new());
    for fd in raw_features {
        let mut r = Reader { buf: fd, pos: 0 };
        let (mut id, mut ty) = (None, GeomType::Unknown);
        tags.clear();
        geom.clear();
        while let Some((f, w)) = r.field() {
            match (f, w) {
                (1, 0) => id = Some(Value::Number(r.varint()? as f64)),
                (2, 2) => packed(r.bytes()?, &mut tags)?,
                (3, 0) => {
                    ty = match r.varint()? {
                        1 => GeomType::Point,
                        2 => GeomType::LineString,
                        3 => GeomType::Polygon,
                        _ => GeomType::Unknown,
                    }
                }
                (4, 2) => packed(r.bytes()?, &mut geom)?,
                _ => r.skip(w)?,
            }
        }
        let start = table.tags.len() as u32;
        table.tags.extend_from_slice(&tags[..tags.len() & !1]);
        decoded.push((id, ty, start, table.tags.len() as u32, decode_geometry(&geom)));
    }
    table.tags.shrink_to_fit();
    let table = Rc::new(table);
    let features = decoded
        .into_iter()
        .map(|(id, ty, start, end, geometry)| Feature { id, ty, properties: Properties::Tags(table.clone(), start, end), geometry })
        .collect();
    Some(Layer { name, extent, features })
}

/// Decode a (possibly gzip/zlib-compressed) vector tile, keeping only the layers whose name
/// passes `keep` (the others are skipped undecoded).
pub fn decode(data: &[u8], keep: impl Fn(&[u8]) -> bool) -> Option<Vec<Layer>> {
    let inflated;
    let data = if data.starts_with(&[0x1f, 0x8b]) {
        inflated = inflate_gzip(data)?;
        &inflated[..]
    } else {
        data
    };
    let mut r = Reader { buf: data, pos: 0 };
    let mut layers = Vec::new();
    while let Some((f, w)) = r.field() {
        if (f, w) == (3, 2) {
            let bytes = r.bytes()?;
            if keep(layer_name(bytes)?) {
                layers.push(decode_layer(bytes)?);
            }
        } else {
            r.skip(w)?;
        }
    }
    Some(layers)
}

fn inflate_gzip(data: &[u8]) -> Option<Vec<u8>> {
    // Header: magic(2) method(1) flags(1) mtime(4) xfl(1) os(1), then optional fields.
    let flags = *data.get(3)?;
    let mut pos = 10;
    if flags & 4 != 0 {
        let len = u16::from_le_bytes([*data.get(pos)?, *data.get(pos + 1)?]) as usize;
        pos += 2 + len;
    }
    for bit in [8, 16] {
        if flags & bit != 0 {
            pos += data.get(pos..)?.iter().position(|&b| b == 0)? + 1;
        }
    }
    if flags & 2 != 0 {
        pos += 2;
    }
    miniz_oxide::inflate::decompress_to_vec(data.get(pos..)?).ok()
}

/// MapLibre's overzoom slicing (`sliceVectorTileLayer`): a child tile below the source's
/// maxzoom is cut out of the parent tile, rescaled to the same extent, clipped with a buffer
/// and round-tripped through MVT encoding (whose deltas are truncated to integers).
pub fn slice_layer(layer: &Layer, parent: crate::tile::TileId, target: crate::tile::TileId) -> Layer {
    let extent = layer.extent as f64;
    let scale = (1u64 << (target.z - parent.z)) as f64;
    let ox = (target.x as f64 - parent.x as f64 * scale) * extent;
    let oy = (target.y as f64 - parent.y as f64 * scale) * extent;
    let buffer = 128f64.max((128.0 * extent / 4096.0).round());
    let (lo, hi) = (-buffer, extent + buffer);
    let mut features = Vec::new();
    for f in &layer.features {
        let geom: Vec<Vec<[f64; 2]>> =
            f.geometry.iter().map(|r| r.iter().map(|p| [p[0] as f64 * scale - ox, p[1] as f64 * scale - oy]).collect()).collect();
        let clipped = clip_axis(&clip_axis(&geom, f.ty, lo, hi, 0), f.ty, lo, hi, 1);
        if clipped.is_empty() {
            continue;
        }
        features.push(Feature { id: f.id.clone(), ty: f.ty, properties: f.properties.clone(), geometry: reencode(&clipped, f.ty) });
    }
    Layer { name: layer.name.clone(), extent: layer.extent, features }
}

/// vt-pbf `writeGeometry` + decode: each delta is truncated (`zigzag` uses int32 ops)
/// while the encoder's cursor tracks the exact float position.
fn reencode(geom: &[Vec<[f64; 2]>], ty: GeomType) -> Vec<Vec<[i32; 2]>> {
    let (mut fx, mut fy) = (0.0f64, 0.0f64);
    let (mut ix, mut iy) = (0i64, 0i64);
    let mut out = Vec::new();
    for ring in geom {
        let closed = ty == GeomType::Polygon && ring.len() > 1 && ring[0] == ring[ring.len() - 1];
        let n = if closed { ring.len() - 1 } else { ring.len() };
        let mut r = Vec::with_capacity(ring.len());
        for p in &ring[..n] {
            let (dx, dy) = (p[0] - fx, p[1] - fy);
            ix += dx as i32 as i64;
            iy += dy as i32 as i64;
            fx += dx;
            fy += dy;
            r.push([ix as i32, iy as i32]);
        }
        if ty == GeomType::Polygon
            && let Some(&first) = r.first()
        {
            r.push(first);
        }
        if ty == GeomType::Point {
            out.extend(r.into_iter().map(|p| vec![p]));
        } else {
            out.push(r);
        }
    }
    out
}

/// `clipGeometryOnAxis` (symbol/clip_line.ts).
pub(crate) fn clip_axis(geom: &[Vec<[f64; 2]>], ty: GeomType, start: f64, end: f64, axis: usize) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    match ty {
        GeomType::Point => {
            for p in geom.iter().flatten() {
                if p[axis] >= start && p[axis] <= end {
                    out.push(vec![*p]);
                }
            }
        }
        GeomType::LineString | GeomType::Polygon => {
            let is_polygon = ty == GeomType::Polygon;
            let at = |p: [f64; 2], q: [f64; 2], v: f64| -> [f64; 2] {
                let t = (v - p[axis]) / (q[axis] - p[axis]);
                if axis == 0 { [v, p[1] + (q[1] - p[1]) * t] } else { [p[0] + (q[0] - p[0]) * t, v] }
            };
            for line in geom {
                let mut slice: Vec<[f64; 2]> = Vec::new();
                for w in line.windows(2) {
                    let (p1, p2) = (w[0], w[1]);
                    let (a, b) = (p1[axis], p2[axis]);
                    let mut exited = false;
                    if a < start {
                        if b >= start {
                            slice.push(at(p1, p2, start));
                        }
                    } else if a > end {
                        if b <= end {
                            slice.push(at(p1, p2, end));
                        }
                    } else {
                        slice.push(p1);
                    }
                    if b < start && a >= start {
                        slice.push(at(p1, p2, start));
                        exited = true;
                    }
                    if b > end && a <= end {
                        slice.push(at(p1, p2, end));
                        exited = true;
                    }
                    if !is_polygon && exited {
                        out.push(std::mem::take(&mut slice));
                    }
                }
                if let Some(&last) = line.last()
                    && last[axis] >= start
                    && last[axis] <= end
                {
                    slice.push(last);
                }
                if is_polygon && !slice.is_empty() && slice[0] != slice[slice.len() - 1] {
                    slice.push(slice[0]);
                }
                if !slice.is_empty() {
                    out.push(slice);
                }
            }
        }
        GeomType::Unknown => {}
    }
    out
}

/// Encodes `layers` as a vector tile (MVT 2), geometry as `decode` returns it. Like vt-pbf,
/// null properties are left out and arrays/objects written as JSON strings. Repeated points are
/// dropped, then lines under 2 points, rings under 3 and layers without features.
pub fn encode(layers: &[Layer]) -> Vec<u8> {
    let mut tile = Vec::new();
    for l in layers {
        let mut out = Vec::new();
        let (mut keys, mut key_ids) = (Vec::new(), crate::hash::HashMap::<Rc<str>, u32>::default());
        let (mut values, mut value_ids) = (Vec::new(), crate::hash::HashMap::<Vec<u8>, u32>::default());
        for f in &l.features {
            let geom = encode_geometry(&f.geometry, f.ty);
            if geom.is_empty() {
                continue;
            }
            let mut tags = Vec::new();
            for (k, v) in f.properties.to_object().iter() {
                let Some(v) = encode_value(v) else { continue };
                tags.push(*key_ids.entry(k.clone()).or_insert_with(|| {
                    keys.push(k.clone());
                    keys.len() as u32 - 1
                }));
                tags.push(*value_ids.entry(v).or_insert_with_key(|v| {
                    values.push(v.clone());
                    values.len() as u32 - 1
                }));
            }
            let mut feature = Vec::new();
            if let Some(Value::Number(id)) = f.id
                && id >= 0.0
                && id.fract() == 0.0
                && id < 1.8e19
            {
                put_varint(&mut feature, 1 << 3, id as u64);
            }
            put_packed(&mut feature, 2, &tags);
            put_varint(&mut feature, 3 << 3, f.ty as u64);
            put_packed(&mut feature, 4, &geom);
            put_bytes(&mut out, 2, &feature);
        }
        if out.is_empty() {
            continue;
        }
        put_varint(&mut out, 15 << 3, 2);
        put_bytes(&mut out, 1, l.name.as_bytes());
        for k in &keys {
            put_bytes(&mut out, 3, k.as_bytes());
        }
        for v in &values {
            put_bytes(&mut out, 4, v);
        }
        put_varint(&mut out, 5 << 3, l.extent as u64);
        put_bytes(&mut tile, 3, &out);
    }
    tile
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// A varint field; `key` is `field << 3` (wire type 0).
fn put_varint(out: &mut Vec<u8>, key: u64, v: u64) {
    varint(out, key);
    varint(out, v);
}

fn put_bytes(out: &mut Vec<u8>, field: u64, bytes: &[u8]) {
    varint(out, field << 3 | 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn put_packed(out: &mut Vec<u8>, field: u64, vs: &[u32]) {
    let mut b = Vec::with_capacity(vs.len());
    for &v in vs {
        varint(&mut b, v as u64);
    }
    put_bytes(out, field, &b);
}

fn encode_value(v: &Value) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    match v {
        Value::Null => return None,
        Value::Bool(b) => put_varint(&mut out, 7 << 3, *b as u64),
        Value::String(s) => put_bytes(&mut out, 1, s.as_bytes()),
        &Value::Number(n) if n.fract() == 0.0 && n.abs() < 9.2e18 => match n >= 0.0 {
            true => put_varint(&mut out, 5 << 3, n as u64),
            false => put_varint(&mut out, 6 << 3, ((n as i64) << 1 ^ (n as i64) >> 63) as u64),
        },
        Value::Number(n) => {
            out.push(3 << 3 | 1);
            out.extend_from_slice(&n.to_le_bytes());
        }
        v => put_bytes(&mut out, 1, v.to_json().to_string().as_bytes()),
    }
    Some(out)
}

fn encode_geometry(geom: &[Vec<[i32; 2]>], ty: GeomType) -> Vec<u32> {
    let mut out = Vec::new();
    let mut cursor = [0i32; 2];
    let mut to = |out: &mut Vec<u32>, p: [i32; 2]| {
        for k in 0..2 {
            let d = p[k].wrapping_sub(cursor[k]);
            out.push(((d << 1) ^ (d >> 31)) as u32);
        }
        cursor = p;
    };
    if ty == GeomType::Point {
        let points: Vec<[i32; 2]> = geom.iter().flatten().copied().collect();
        if !points.is_empty() {
            out.push(1 | (points.len() as u32) << 3);
            points.into_iter().for_each(|p| to(&mut out, p));
        }
        return out;
    }
    let polygon = ty == GeomType::Polygon;
    for ring in geom {
        let mut pts = ring.clone();
        pts.dedup();
        if polygon && pts.len() > 1 && pts[0] == pts[pts.len() - 1] {
            pts.pop();
        }
        if pts.len() < if polygon { 3 } else { 2 } {
            continue;
        }
        out.push(1 | 1 << 3);
        to(&mut out, pts[0]);
        out.push(2 | ((pts.len() - 1) as u32) << 3);
        pts[1..].iter().for_each(|&p| to(&mut out, p));
        if polygon {
            out.push(7 | 1 << 3);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expression::Object;

    #[test]
    fn encode_roundtrips_through_decode() {
        let mut props = Object::default();
        props.insert("name".into(), Value::str("a"));
        props.insert("n".into(), Value::Number(-3.0));
        props.insert("f".into(), Value::Number(1.5));
        props.insert("skip".into(), Value::Null);
        let feature = |ty, geometry| Feature { id: Some(Value::Number(7.0)), ty, properties: Properties::Object(Rc::new(props.clone())), geometry };
        let layer = Layer {
            name: "l".into(),
            extent: 4096,
            features: vec![
                feature(GeomType::Point, vec![vec![[1, 2]], vec![[-5, 6]]]),
                feature(GeomType::LineString, vec![vec![[0, 0], [0, 0], [10, 10]], vec![[3, 3]]]),
                feature(GeomType::Polygon, vec![vec![[0, 0], [10, 0], [10, 10], [0, 0]]]),
            ],
        };
        let decoded = decode(&encode(&[layer]), |_| true).unwrap();
        let l = &decoded[0];
        assert_eq!((l.name.as_str(), l.extent, l.features.len()), ("l", 4096, 3));
        assert_eq!(l.features[0].geometry, vec![vec![[1, 2]], vec![[-5, 6]]]);
        assert_eq!(l.features[1].geometry, vec![vec![[0, 0], [10, 10]]]);
        assert_eq!(l.features[2].geometry, vec![vec![[0, 0], [10, 0], [10, 10], [0, 0]]]);
        let p = &l.features[2].properties;
        assert!(matches!(p.get("name"), Some(Value::String(s)) if &**s == "a"));
        assert!(matches!(p.get("n"), Some(Value::Number(-3.0))));
        assert!(matches!(p.get("f"), Some(Value::Number(1.5))));
        assert!(p.get("skip").is_none());
        assert!(matches!(l.features[0].id, Some(Value::Number(7.0))));
    }
}
