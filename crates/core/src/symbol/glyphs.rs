//! SDF glyph ranges (`glyphs` URL, PBF): parsing and the per-fontstack store.
//!
//! Like MapLibre's `GlyphManager` with a glyphs URL and no `font-faces`/local fonts: each
//! codepoint comes from the 256-codepoint range file of its font stack. A range that fails to
//! load leaves its glyphs missing (MapLibre would draw them with the browser's fonts instead).
//!
//! Only the glyphs of the code points the text needs are kept from a range file (CJK text uses
//! a few glyphs from each of many ranges); a range is needed again when text needs one of its
//! code points that was not looked up yet.

use crate::mvt::Reader;
use crate::hash::HashMap;
use std::collections::BTreeSet;
use std::rc::Rc;

/// SDF border around each glyph bitmap (`GLYPH_PBF_BORDER`).
pub const BORDER: i32 = 3;

#[derive(Debug, Clone, Copy, Default)]
pub struct Metrics {
    pub width: f64,
    pub height: f64,
    pub left: f64,
    pub top: f64,
    pub advance: f64,
}

#[derive(Debug)]
pub struct Glyph {
    /// Alpha SDF, `(width + 2*BORDER) x (height + 2*BORDER)`.
    pub bitmap: Vec<u8>,
    pub bw: usize,
    pub bh: usize,
    pub metrics: Metrics,
}

fn zigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

fn parse_glyph(data: &[u8]) -> Option<(u32, Glyph)> {
    let mut r = Reader { buf: data, pos: 0 };
    let (mut id, mut bitmap, mut m) = (0u32, Vec::new(), Metrics::default());
    while let Some((field, wire)) = r.field() {
        match (field, wire) {
            (1, 0) => id = r.varint()? as u32,
            (2, 2) => bitmap = r.bytes()?.to_vec(),
            (3, 0) => m.width = r.varint()? as f64,
            (4, 0) => m.height = r.varint()? as f64,
            (5, 0) => m.left = zigzag(r.varint()?) as f64,
            (6, 0) => m.top = zigzag(r.varint()?) as f64,
            (7, 0) => m.advance = r.varint()? as f64,
            (_, w) => r.skip(w)?,
        }
    }
    let (bw, bh) = (m.width as usize + 2 * BORDER as usize, m.height as usize + 2 * BORDER as usize);
    // Even an empty glyph (a space) gets a zero-filled bitmap and so an atlas rect and a quad.
    bitmap.resize(bw * bh, 0);
    Some((id, Glyph { bitmap, bw, bh, metrics: m }))
}

/// The code point (`id`) of a glyph message.
fn glyph_id(data: &[u8]) -> Option<u32> {
    let mut r = Reader { buf: data, pos: 0 };
    let mut id = 0;
    while let Some((field, wire)) = r.field() {
        if (field, wire) == (1, 0) {
            id = r.varint()? as u32;
        } else {
            r.skip(wire)?;
        }
    }
    Some(id)
}

/// `parseGlyphPbf`: glyphs → stacks (field 1) → glyph (field 3), for the code points in `wanted`.
pub fn parse(data: &[u8], wanted: &BTreeSet<u32>) -> HashMap<u32, Rc<Glyph>> {
    let mut out = HashMap::default();
    let mut r = Reader { buf: data, pos: 0 };
    while let Some((field, wire)) = r.field() {
        let Some(stack) = (if (field, wire) == (1, 2) { r.bytes() } else { r.skip(wire).map(|_| &[][..]) }) else { break };
        let mut s = Reader { buf: stack, pos: 0 };
        while let Some((field, wire)) = s.field() {
            if (field, wire) == (3, 2) {
                let Some(bytes) = s.bytes() else { break };
                if glyph_id(bytes).is_some_and(|id| wanted.contains(&id))
                    && let Some((id, g)) = parse_glyph(bytes)
                {
                    out.insert(id, Rc::new(g));
                }
            } else if s.skip(wire).is_none() {
                break;
            }
        }
    }
    out
}

/// A provided range file.
#[derive(Default)]
struct Range {
    /// The kept glyphs by code point.
    glyphs: HashMap<u32, Rc<Glyph>>,
    /// The code points looked up in it, found or not.
    looked_up: BTreeSet<u32>,
    /// The file failed to load: its glyphs are missing.
    failed: bool,
}

/// Provided glyph ranges by font stack (`"Font A,Font B"`) and range index (`codepoint / 256`).
#[derive(Default)]
pub struct Glyphs {
    pub url: Option<String>,
    ranges: HashMap<(String, u32), Range>,
}

/// The code points of text by font stack.
pub type Dependencies = std::collections::BTreeMap<String, BTreeSet<u32>>;

/// A range file to fetch: its key and the code points to keep from it.
pub type RangeNeed = ((String, u32), BTreeSet<u32>);

impl Glyphs {
    pub fn new(url: Option<&str>) -> Glyphs {
        Glyphs { url: url.map(String::from), ranges: HashMap::default() }
    }

    pub fn range_url(&self, stack: &str, range: u32) -> Option<String> {
        let begin = range * 256;
        Some(self.url.as_ref()?.replace("{fontstack}", stack).replace("{range}", &format!("{begin}-{}", begin + 255)))
    }

    /// URLs of the ranges with code points in `deps` not looked up yet.
    pub fn needs(&self, deps: &Dependencies) -> Vec<(String, RangeNeed)> {
        let mut out: Vec<(String, RangeNeed)> = Vec::new();
        for (stack, cp) in deps.iter().flat_map(|(stack, cps)| cps.iter().map(move |cp| (stack, cp))) {
            let key = (stack.clone(), cp / 256);
            if self.ranges.get(&key).is_some_and(|r| r.failed || r.looked_up.contains(cp)) {
                continue;
            }
            // `deps` is sorted, so a range's code points are adjacent.
            match out.last_mut() {
                Some((_, (k, cps))) if *k == key => {
                    cps.insert(*cp);
                }
                _ => {
                    let Some(url) = self.range_url(stack, key.1) else { continue };
                    out.push((url, (key, BTreeSet::from([*cp]))));
                }
            }
        }
        out
    }

    /// The range file `key` (`None`: failed to load), keeping the glyphs of `wanted`.
    pub fn provide(&mut self, key: (String, u32), wanted: &BTreeSet<u32>, bytes: Option<&[u8]>) {
        let range = self.ranges.entry(key).or_default();
        let wanted: BTreeSet<u32> = wanted.difference(&range.looked_up).copied().collect();
        if range.failed || wanted.is_empty() {
            return;
        }
        match bytes {
            Some(bytes) => range.glyphs.extend(parse(bytes, &wanted)),
            None if range.looked_up.is_empty() => range.failed = true,
            None => {}
        }
        range.looked_up.extend(wanted);
    }

    pub fn get(&self, stack: &str, c: char) -> Option<&Rc<Glyph>> {
        let cp = c as u32;
        self.ranges.get(&(stack.to_string(), cp / 256))?.glyphs.get(&cp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fixture_range() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/render/assets/glyphs/Open Sans Semibold,Arial Unicode MS Bold/0-255.pbf");
        let Ok(bytes) = std::fs::read(path) else { return };
        let glyphs = parse(&bytes, &(0..256).collect());
        let a = &glyphs[&('A' as u32)];
        assert!(a.metrics.advance > 0.0 && a.bitmap.iter().any(|&v| v > 192));
        assert_eq!(a.bitmap.len(), a.bw * a.bh);
        assert_eq!(glyphs[&(' ' as u32)].bw, 6);
    }
}
