//! Symbol bucket construction (`SymbolBucket.populate` + `performSymbolLayout`): text and icon
//! evaluation, shaping, anchors, quads and collision boxes for one tile of one layer.

use super::anchors::{self, Anchor, P};
use super::glyphs::{Glyphs, BORDER};
use super::shaping::{self, PositionedIcon, Shaping, Tex, WritingMode, IMAGE_PADDING, ONE_EM, SHAPING_DEFAULT_OFFSET};
use super::unicode;
use crate::expression::value::{js_round, Formatted};
use crate::expression::{Kind, Value};
use crate::mvt::GeomType;
use crate::render::{num, num2, BucketFeature, Ctx, TileFeatures, EXTENT};
use crate::style::{Layer, PropValue};
use crate::tile::RenderTile;
use crate::hash::HashMap;

const SIZE_PACK_FACTOR: f64 = 128.0;
const MAX_PACKED_SIZE: f64 = 255.0 * SIZE_PACK_FACTOR;
const BASELINE_OFFSET: f64 = 7.0;

/// JS typed-array store into an Int16 slot.
pub fn int16(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (v.trunc() as i64 as i16) as f64
}

/// `SizeData` for `text-size` / `icon-size`.
#[derive(Debug, Clone)]
pub enum SizeData {
    Constant(f64),
    Source,
    Camera { stops: Vec<f64>, sizes: Vec<f64>, layout_size: f64, interp: Option<crate::expression::Interpolation> },
    Composite { min_zoom: f64, max_zoom: f64, interp: Option<crate::expression::Interpolation> },
}

impl SizeData {
    fn new(p: &PropValue, tile_zoom: f64, gs: &std::rc::Rc<crate::expression::Object>) -> SizeData {
        let (kind, stops, interp) = p.curve();
        let eval = |z: f64| num(&p.eval(z, None, None, gs));
        match kind {
            Kind::Constant => SizeData::Constant(eval(tile_zoom + 1.0)),
            Kind::Source => SizeData::Source,
            Kind::Composite => {
                let mut lower = 0;
                while lower < stops.len() && stops[lower] <= tile_zoom {
                    lower += 1;
                }
                let lower = lower.saturating_sub(1);
                let mut upper = lower;
                while upper < stops.len() && stops[upper] < tile_zoom + 1.0 {
                    upper += 1;
                }
                let upper = upper.min(stops.len().saturating_sub(1));
                SizeData::Composite { min_zoom: stops.get(lower).copied().unwrap_or(0.0), max_zoom: stops.get(upper).copied().unwrap_or(0.0), interp }
            }
            Kind::Camera => {
                let sizes = stops.iter().map(|&z| eval(if z == f64::NEG_INFINITY { stops[1] - 1.0 } else { z })).collect();
                SizeData::Camera { stops, sizes, layout_size: eval(tile_zoom + 1.0), interp }
            }
        }
    }

    /// `evaluateSizeForZoom`: (`u_size_t`, `u_size`).
    pub fn for_zoom(&self, zoom: f64) -> (f64, f64) {
        match self {
            SizeData::Constant(s) => (0.0, *s),
            SizeData::Source => (0.0, 0.0),
            SizeData::Camera { stops, sizes, layout_size, interp } => {
                let mut lower = stops.len() - 1;
                while lower > 0 && stops[lower] > zoom {
                    lower -= 1;
                }
                let upper = (lower + 1).min(stops.len() - 1);
                let t = interp.map_or(0.0, |i| i.factor(zoom, stops[lower], stops[upper]).clamp(0.0, 1.0));
                (0.0, (sizes[lower] + (sizes[upper] - sizes[lower]) * t).min(*layout_size))
            }
            SizeData::Composite { min_zoom, max_zoom, interp } => (interp.map_or(0.0, |i| i.factor(zoom, *min_zoom, *max_zoom).clamp(0.0, 1.0)), 0.0),
        }
    }

    /// The size a vertex gets in the shader (`a_size` / `u_size`).
    pub fn shader_size(&self, zoom: f64, packed: [f64; 2]) -> f64 {
        let (t, u) = self.for_zoom(zoom);
        match self {
            SizeData::Source => packed[0] / SIZE_PACK_FACTOR,
            SizeData::Composite { .. } => (packed[0] + (packed[1] - packed[0]) * t as f32 as f64) / SIZE_PACK_FACTOR,
            _ => u,
        }
    }

    /// `evaluateSizeForFeature` (CPU side, unpacked like the placed symbol arrays).
    pub fn for_feature(&self, zoom: f64, packed: [f64; 2]) -> f64 {
        let (t, u) = self.for_zoom(zoom);
        match self {
            SizeData::Source => packed[0] / SIZE_PACK_FACTOR,
            SizeData::Composite { .. } => packed[0] / SIZE_PACK_FACTOR + (packed[1] / SIZE_PACK_FACTOR - packed[0] / SIZE_PACK_FACTOR) * t,
            _ => u,
        }
    }
}

/// A glyph or icon quad (`SymbolQuad`), offsets in em/px space as packed into the vertex buffer.
#[derive(Clone)]
pub struct Quad {
    /// tl, tr, bl, br.
    pub corners: [P; 4],
    pub tex: Tex,
    /// Texture rect within the padded image: x, y, w, h.
    pub rect: [f64; 4],
    pub pixel_offset_tl: P,
    pub pixel_offset_br: P,
    pub min_font_scale: P,
    pub glyph_offset: P,
    pub section: usize,
    pub is_sdf: bool,
}

/// `PlacedSymbol`: one text justification or one icon of a symbol instance.
pub struct PlacedSymbol {
    pub anchor: P,
    pub quads: Vec<Quad>,
    /// Packed sizes (`SIZE_PACK_FACTOR`), for source/composite sizes.
    pub size: [f64; 2],
    pub line_offset: P,
    pub line_start: usize,
    pub line_length: usize,
    pub segment: Option<usize>,
    pub writing_mode: WritingMode,
    /// Index into `SymbolBucket::features`.
    pub feature: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct CollisionBox {
    pub anchor: P,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

pub struct SymbolInstance {
    /// Int16 anchor.
    pub anchor: P,
    /// right, center, left justified text symbols.
    pub text: [Option<usize>; 3],
    pub vertical_text: Option<usize>,
    pub icon: Option<usize>,
    pub vertical_icon: Option<usize>,
    pub text_box: Option<CollisionBox>,
    pub vertical_text_box: Option<CollisionBox>,
    pub icon_box: Option<CollisionBox>,
    pub vertical_icon_box: Option<CollisionBox>,
    pub feature_index: usize,
    pub num_glyph_vertices: usize,
    pub num_vertical_glyph_vertices: usize,
    pub num_icon_vertices: usize,
    pub collision_circle_diameter: Option<f64>,
    pub text_box_scale: f64,
    /// `text-variable-anchor(-offset)`: anchors with their offsets (em units).
    pub anchor_offsets: Vec<(String, P)>,
}

pub struct SymbolFeature {
    pub f: BucketFeature,
    pub text: Option<Formatted>,
    pub font_stack: String,
    pub icon: Option<String>,
    pub sort_key: Option<f64>,
}

pub struct SymbolBucket {
    pub tile: RenderTile,
    pub features: Vec<SymbolFeature>,
    pub instances: Vec<SymbolInstance>,
    pub text: Vec<PlacedSymbol>,
    pub icon: Vec<PlacedSymbol>,
    /// Line vertices (Int16 x, y, distance from anchor) for line labels.
    pub line_vertices: Vec<(P, f64)>,
    pub text_size: SizeData,
    pub icon_size: SizeData,
    pub sdf_icons: bool,
    pub icons_need_linear: bool,
    pub icons_in_text: bool,
    /// `(sort key, first instance, end instance)`.
    pub sort_key_ranges: Vec<(f64, usize, usize)>,
    pub tile_pixel_ratio: f64,
    pub can_overlap: bool,
    pub sort_features_by_y: bool,
    /// `text-writing-mode` (point placement) includes vertical.
    pub allow_vertical_placement: bool,
    /// `text-writing-mode` in order (point placement).
    pub writing_modes: Vec<WritingMode>,
}

/// Layout properties with `auto` resolved (`SymbolStyleLayer.recalculate`).
pub struct Resolved {
    pub placement: String,
    pub text_rotation_alignment: String,
    pub text_pitch_alignment: String,
    pub icon_rotation_alignment: String,
    pub icon_pitch_alignment: String,
    pub z_order: String,
    pub sort_by_key: bool,
}

impl Resolved {
    pub fn new(layer: &Layer, ctx: &Ctx, tile: &RenderTile) -> Resolved {
        let s = |n: &str| ctx.layout(layer, n, None, tile).as_str().unwrap_or("").to_string();
        let placement = s("symbol-placement");
        let auto = |v: String| if v == "auto" { if placement != "point" { "map".into() } else { "viewport".into() } } else { v };
        let text_rotation_alignment = auto(s("text-rotation-alignment"));
        let icon_rotation_alignment = if layer.layout("icon-rotation-alignment").is_constant() { auto(s("icon-rotation-alignment")) } else { auto("auto".into()) };
        let pitch = |v: String, rot: &str| if v == "auto" { if rot == "map" { "map".into() } else { "viewport".into() } } else { v };
        let text_pitch_alignment = pitch(s("text-pitch-alignment"), &text_rotation_alignment);
        let icon_pitch_alignment = pitch(s("icon-pitch-alignment"), &icon_rotation_alignment);
        let z_order = s("symbol-z-order");
        let sort_by_key = z_order != "viewport-y" && !layer.layout("symbol-sort-key").is_constant();
        Resolved { placement, text_rotation_alignment, text_pitch_alignment, icon_rotation_alignment, icon_pitch_alignment, z_order, sort_by_key }
    }

    pub fn text_along_line(&self) -> bool {
        self.text_rotation_alignment != "viewport" && self.placement != "point"
    }
}

/// `getOverlapMode`.
pub fn overlap_mode(layer: &Layer, ctx: &Ctx, tile: &RenderTile, raw: &serde_json::Value, kind: &str) -> &'static str {
    if raw["layout"].get(format!("{kind}-overlap")).is_some_and(|v| !v.is_null()) {
        match ctx.layout(layer, &format!("{kind}-overlap"), None, tile).as_str() {
            Some("always") => return "always",
            Some("cooperative") => return "cooperative",
            Some("never") => return "never",
            _ => {}
        }
    }
    if ctx.layout(layer, &format!("{kind}-allow-overlap"), None, tile).truthy() { "always" } else { "never" }
}

/// `resolveTokens`: `{name}` → the feature property (JS `String()`), or empty.
fn resolve_tokens(props: &crate::expression::Properties, s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find(['{', '}']) {
            Some(close) if after.as_bytes()[close] == b'}' && close > 0 => {
                out.push_str(&rest[..open]);
                if let Some(v) = props.get(&after[..close]) {
                    out.push_str(&v.to_js_string());
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push_str(&rest[..open + 1]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether a layout value is resolved for `{token}`s (a plain or camera-function string).
fn resolves_tokens(raw: &serde_json::Value, prop: &PropValue) -> bool {
    !crate::expression::is_expression(raw) && prop.is_feature_constant()
}

pub struct FeatureText {
    pub text: Option<Formatted>,
    pub font_stack: String,
}

/// The text of a feature (`text-field` with tokens and `text-transform`), if the layer has text.
pub fn feature_text(layer: &Layer, ctx: &Ctx, tile: &RenderTile, raw: &serde_json::Value, f: &BucketFeature) -> FeatureText {
    let font = ctx.layout(layer, "text-font", Some(f), tile);
    let font_stack = match &font {
        Value::Array(a) => a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join(","),
        _ => String::new(),
    };
    let field = layer.layout("text-field");
    let has_text = !(field.is_constant() && matches!(ctx.layout(layer, "text-field", None, tile), Value::Formatted(ref t) if t.is_empty()))
        && !(layer.layout("text-font").is_constant() && font_stack.is_empty() && matches!(&font, Value::Array(a) if a.is_empty()));
    if !has_text {
        return FeatureText { text: None, font_stack };
    }
    let mut text = match ctx.layout(layer, "text-field", Some(f), tile) {
        Value::Formatted(t) => (*t).clone(),
        Value::Null => Formatted { sections: vec![] },
        v => Formatted::from_string(v.to_js_string()),
    };
    if resolves_tokens(&raw["layout"]["text-field"], field) {
        for s in &mut text.sections {
            s.text = resolve_tokens(&f.feature.properties, &s.text);
        }
    }
    let transform = ctx.layout(layer, "text-transform", Some(f), tile);
    for s in &mut text.sections {
        match transform.as_str() {
            Some("uppercase") => s.text = s.text.to_uppercase(),
            Some("lowercase") => s.text = s.text.to_lowercase(),
            _ => {}
        }
    }
    FeatureText { text: Some(text), font_stack }
}

fn feature_icon(layer: &Layer, ctx: &Ctx, tile: &RenderTile, raw: &serde_json::Value, f: &BucketFeature) -> Option<String> {
    let prop = layer.layout("icon-image");
    let name = match ctx.layout(layer, "icon-image", Some(f), tile) {
        Value::ResolvedImage(i) => i.name.clone(),
        Value::String(s) if resolves_tokens(&raw["layout"]["icon-image"], prop) => resolve_tokens(&f.feature.properties, &s),
        Value::String(s) => s.to_string(),
        _ => return None,
    };
    (!name.is_empty()).then_some(name)
}

/// Glyph ranges a tile's features need: `(font stack, range)`.
pub fn glyph_dependencies(layer: &Layer, ctx: &Ctx, tf: &TileFeatures, raw: &serde_json::Value, out: &mut crate::symbol::glyphs::Dependencies) {
    let res = Resolved::new(layer, ctx, &tf.tile);
    let keep_upright = ctx.layout(layer, "text-keep-upright", None, &tf.tile).truthy();
    let allow_vertical = res.placement == "point"
        && matches!(ctx.layout(layer, "text-writing-mode", None, &tf.tile), Value::Array(a) if a.iter().any(|m| m.as_str() == Some("vertical")));
    for f in ctx.filtered_features(layer, tf, None, false) {
        let ft = feature_text(layer, ctx, &tf.tile, raw, &f);
        let Some(text) = ft.text else { continue };
        let unformatted: String = text.sections.iter().map(|s| s.text.as_str()).collect();
        // Vertical labels may swap punctuation for its vertical form.
        let vertical_forms = ((res.text_along_line() && keep_upright) || allow_vertical) && unicode::allows_vertical_writing_mode(&unformatted);
        for s in text.sections.iter().filter(|s| s.image.is_none()) {
            let stack = s.font_stack.as_ref().unwrap_or(&ft.font_stack);
            let cps = match out.get_mut(stack) {
                Some(cps) => cps,
                None => out.entry(stack.clone()).or_default(),
            };
            for c in s.text.chars() {
                cps.insert(c as u32);
                if let Some(v) = unicode::verticalized(c).filter(|_| vertical_forms) {
                    cps.insert(v as u32);
                }
            }
        }
    }
}

struct Sizes<'a> {
    text: &'a PropValue,
    icon: &'a PropValue,
}

pub struct Builder<'a> {
    pub layer: &'a Layer,
    pub ctx: &'a Ctx<'a>,
    pub glyphs: &'a Glyphs,
    pub raw: &'a serde_json::Value,
}

impl Builder<'_> {
    fn lay(&self, name: &str, f: Option<&BucketFeature>, tile: &RenderTile) -> Value {
        self.ctx.layout(self.layer, name, f, tile)
    }

    fn lay_at(&self, name: &str, zoom: f64, f: &BucketFeature, tile: &RenderTile) -> f64 {
        num(&self.layer.layout(name).eval(zoom, Some(&f.feature), Some(Ctx::canonical(tile)), self.ctx.global_state))
    }

    pub fn build(&self, tf: &TileFeatures) -> SymbolBucket {
        let layer = self.layer;
        let tile = tf.tile;
        let zoom = tile.overscaled_z as f64;
        let overscaling = 2f64.powi(tile.overscaled_z as i32 - tile.canonical.z as i32);
        let tile_pixel_ratio = EXTENT / (512.0 * overscaling);
        let res = Resolved::new(layer, self.ctx, &tile);
        let gs = self.ctx.global_state;
        let can_overlap = overlap_mode(layer, self.ctx, &tile, self.raw, "text") != "never"
            || overlap_mode(layer, self.ctx, &tile, self.raw, "icon") != "never"
            || self.lay("text-ignore-placement", None, &tile).truthy()
            || self.lay("icon-ignore-placement", None, &tile).truthy();
        let by_y = res.z_order == "viewport-y" || (res.z_order == "auto" && !res.sort_by_key);
        let mut bucket = SymbolBucket {
            tile,
            features: Vec::new(),
            instances: Vec::new(),
            text: Vec::new(),
            icon: Vec::new(),
            line_vertices: Vec::new(),
            text_size: SizeData::new(layer.layout("text-size"), zoom, gs),
            icon_size: SizeData::new(layer.layout("icon-size"), zoom, gs),
            sdf_icons: false,
            icons_need_linear: false,
            icons_in_text: false,
            sort_key_ranges: Vec::new(),
            tile_pixel_ratio,
            can_overlap,
            sort_features_by_y: by_y && can_overlap,
            allow_vertical_placement: false,
            writing_modes: Vec::new(),
        };
        if res.placement == "point" {
            let mut modes = Vec::new();
            if let Value::Array(a) = self.lay("text-writing-mode", None, &tile) {
                for m in a.iter().filter_map(|v| v.as_str()) {
                    let m = if m == "vertical" { WritingMode::Vertical } else { WritingMode::Horizontal };
                    if !modes.contains(&m) {
                        modes.push(m);
                    }
                }
            }
            if modes.is_empty() {
                modes.push(WritingMode::Horizontal);
            }
            bucket.allow_vertical_placement = modes.contains(&WritingMode::Vertical);
            bucket.writing_modes = modes;
        }
        self.populate(&mut bucket, tf, &res);
        let sizes = Sizes { text: layer.layout("text-size"), icon: layer.layout("icon-size") };
        let mut compare_text: HashMap<String, Vec<P>> = HashMap::default();
        let mut sdf_icons: Option<bool> = None;
        let features = std::mem::take(&mut bucket.features);
        for (fi, feature) in features.iter().enumerate() {
            self.layout_feature(&mut bucket, feature, fi, &res, &sizes, &mut compare_text, &mut sdf_icons, overscaling);
        }
        bucket.features = features;
        bucket.sdf_icons = sdf_icons.unwrap_or(false);
        bucket
    }

    fn populate(&self, bucket: &mut SymbolBucket, tf: &TileFeatures, res: &Resolved) {
        let tile = tf.tile;
        let has_icon = !self.layer.layout("icon-image").is_constant() || self.lay("icon-image", None, &tile).as_str().is_some_and(|s| !s.is_empty()) || matches!(self.lay("icon-image", None, &tile), Value::ResolvedImage(_));
        let point = res.placement == "point";
        let sort_prop = res.sort_by_key.then_some("symbol-sort-key");
        let mut features = Vec::new();
        for f in self.ctx.bucket_features(self.layer, tf, None) {
            if point && f.ty == GeomType::Point && f.geometry.iter().flatten().all(|p| is_outside(*p)) {
                continue;
            }
            let FeatureText { text, font_stack } = feature_text(self.layer, self.ctx, &tile, self.raw, &f);
            let icon = if has_icon { feature_icon(self.layer, self.ctx, &tile, self.raw, &f) } else { None };
            if text.is_none() && icon.is_none() {
                continue;
            }
            let sort_key = sort_prop.map(|p| num(&self.ctx.layout(self.layer, p, Some(&f), &tile)));
            features.push(SymbolFeature { f, text, font_stack, icon, sort_key });
        }
        if res.placement == "line" {
            let items: Vec<(Option<String>, Vec<P>)> = features
                .iter()
                .map(|s| (s.text.as_ref().map(|t| t.sections.iter().map(|s| s.text.as_str()).collect()), s.f.geometry.first().cloned().unwrap_or_default()))
                .collect();
            let merged = anchors::merge_lines(&items);
            features = features
                .into_iter()
                .zip(merged)
                .filter_map(|(mut s, m)| {
                    let line = m?;
                    if let Some(g) = s.f.geometry.first_mut() {
                        *g = line;
                    }
                    Some(s)
                })
                .collect();
        }
        if res.sort_by_key {
            features.sort_by(|a, b| a.sort_key.partial_cmp(&b.sort_key).unwrap_or(std::cmp::Ordering::Equal));
        }
        bucket.features = features;
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_feature(
        &self,
        bucket: &mut SymbolBucket,
        feature: &SymbolFeature,
        fi: usize,
        res: &Resolved,
        sizes: &Sizes,
        compare_text: &mut HashMap<String, Vec<P>>,
        sdf_icons: &mut Option<bool>,
        overscaling: f64,
    ) {
        let tile = bucket.tile;
        let zoom = tile.overscaled_z as f64;
        let f = &feature.f;
        let lay = |n: &str| self.lay(n, Some(f), &tile);
        let layout_text_size_this_zoom = num(&lay("text-size"));
        let layout_text_size = self.lay_at("text-size", zoom + 1.0, f, &tile);
        let layout_icon_size = self.lay_at("icon-size", zoom + 1.0, f, &tile);
        let line_height = num(&lay("text-line-height")) * ONE_EM;
        let along_line = res.text_along_line();
        let fontstack = &feature.font_stack;

        // (justification, shaping) in insertion order.
        let mut horizontal: Vec<(&'static str, Shaping)> = Vec::new();
        let mut vertical: Option<Shaping> = None;
        let keep_upright = self.lay("text-keep-upright", None, &tile).truthy();
        let mut text_offset = [0.0, 0.0];
        let variable = self.variable_anchor_offset(f, &tile);
        if let Some(text) = &feature.text {
            let unformatted: String = text.sections.iter().map(|s| s.text.as_str()).collect();
            let spacing = num(&lay("text-letter-spacing")) * ONE_EM;
            let spacing = if unicode::allows_letter_spacing(&unformatted) { spacing } else { 0.0 };
            let text_anchor = lay("text-anchor").as_str().unwrap_or("center").to_string();
            if variable.is_none() {
                let radial = num(&lay("text-radial-offset"));
                text_offset = if radial != 0.0 {
                    evaluate_variable_offset(&text_anchor, [radial * ONE_EM, f64::INFINITY])
                } else {
                    let o = num2(&lay("text-offset"));
                    [o[0] * ONE_EM, o[1] * ONE_EM]
                };
            }
            let mut justify = if along_line { "center".to_string() } else { lay("text-justify").as_str().unwrap_or("center").to_string() };
            let max_width = if res.placement == "point" { num(&lay("text-max-width")) * ONE_EM } else { f64::INFINITY };
            let shape_mode = |anchor: &str, justify: &str, mode: WritingMode, allow_vertical: bool| {
                shaping::shape_text(
                    text,
                    self.glyphs,
                    self.ctx.images,
                    fontstack,
                    max_width,
                    line_height,
                    anchor,
                    justify,
                    spacing,
                    text_offset,
                    mode,
                    allow_vertical,
                    layout_text_size,
                    layout_text_size_this_zoom,
                )
            };
            let shape = |anchor: &str, justify: &str| shape_mode(anchor, justify, WritingMode::Horizontal, false);
            let can_vertical = unicode::allows_vertical_writing_mode(&unformatted);
            // Vertical point labels (`text-writing-mode`) use left justification.
            let point_vertical = || (bucket.allow_vertical_placement && can_vertical).then(|| shape_mode(&text_anchor, "left", WritingMode::Vertical, true)).flatten();
            let static_j = |j: &str| -> &'static str {
                match j {
                    "left" => "left",
                    "right" => "right",
                    _ => "center",
                }
            };
            if let (false, Some(offsets)) = (along_line, &variable) {
                let mut justifications: Vec<&'static str> = Vec::new();
                if justify == "auto" {
                    for (a, _) in offsets {
                        let j = shaping::anchor_justification(a);
                        if !justifications.contains(&j) {
                            justifications.push(j);
                        }
                    }
                } else {
                    justifications.push(static_j(&justify));
                }
                let mut single_line = false;
                for j in justifications {
                    if single_line {
                        let first = horizontal[0].1.clone();
                        horizontal.push((j, first));
                    } else if let Some(s) = shape("center", j) {
                        single_line = s.lines.len() == 1;
                        horizontal.push((j, s));
                    }
                }
                vertical = point_vertical();
            } else {
                if justify == "auto" {
                    justify = shaping::anchor_justification(&text_anchor).to_string();
                }
                if let Some(s) = shape(&text_anchor, &justify) {
                    horizontal.push((static_j(&justify), s));
                }
                vertical = point_vertical();
                // Verticalized line label.
                if can_vertical && along_line && keep_upright {
                    vertical = shape_mode(&text_anchor, &justify, WritingMode::Vertical, false);
                }
            }
        }

        let mut shaped_icon = None;
        let mut is_sdf_icon = false;
        if let Some(img) = feature.icon.as_ref().and_then(|n| self.ctx.images.get(n)) {
            let offset = num2(&lay("icon-offset"));
            shaped_icon = Some(shaping::shape_icon(img.clone(), offset, lay("icon-anchor").as_str().unwrap_or("center")));
            is_sdf_icon = img.sdf;
            if sdf_icons.is_none() {
                *sdf_icons = Some(is_sdf_icon);
            }
            // `icon-rotate`.constantOr(1) at the tile zoom.
            let rotate = self.layer.layout("icon-rotate");
            let rotate = if rotate.is_feature_constant() { num(&self.lay("icon-rotate", None, &tile)) } else { 1.0 };
            if img.pixel_ratio != self.ctx.transform.pixel_ratio || rotate != 0.0 {
                bucket.icons_need_linear = true;
            }
        }
        if let Some(s) = horizontal.first().map(|h| &h.1).or(vertical.as_ref()) {
            bucket.icons_in_text |= s.icons_in_text;
        }
        if horizontal.is_empty() && vertical.is_none() && shaped_icon.is_none() {
            return;
        }

        // addFeature
        let text_max_size = self.lay_at("text-size", 18.0, f, &tile);
        let icon_offset = num2(&lay("icon-offset"));
        let font_scale = layout_text_size / 24.0;
        let tpr = bucket.tile_pixel_ratio;
        let text_box_scale = tpr * font_scale;
        let text_max_box_scale = tpr * text_max_size / 24.0;
        let icon_box_scale = tpr * layout_icon_size;
        let symbol_min_distance = tpr * num(&lay("symbol-spacing"));
        let text_padding = num(&lay("text-padding")) * tpr;
        let icon_padding = match lay("icon-padding") {
            Value::Padding(p) => p.map(|v| v * tpr),
            v => [num(&v) * tpr; 4],
        };
        let text_max_angle = num(&lay("text-max-angle")) / 180.0 * std::f64::consts::PI;
        let text_repeat_distance = symbol_min_distance / 2.0;
        let fit = lay("icon-text-fit").as_str().unwrap_or("none").to_string();
        let mut vertical_icon = None;
        if let (Some(icon), true) = (&shaped_icon, fit != "none") {
            let padding = match lay("icon-text-fit-padding") {
                Value::Array(a) if a.len() == 4 => std::array::from_fn(|i| num(&a[i])),
                _ => [0.0; 4],
            };
            if let (true, Some(v)) = (bucket.allow_vertical_placement, &vertical) {
                vertical_icon = Some(shaping::fit_icon_to_text(icon, v, &fit, padding, icon_offset, font_scale));
            }
            if let Some((_, text)) = horizontal.first() {
                shaped_icon = Some(shaping::fit_icon_to_text(icon, text, &fit, padding, icon_offset, font_scale));
            }
        }
        let default_shaping = horizontal.first().map(|h| h.1.clone());
        // Anchors along lines measure the vertical shaping when there is one.
        let anchor_shaping = vertical.as_ref().or(default_shaping.as_ref());
        let label_length = shaped_label_length(anchor_shaping, shaped_icon.as_ref());
        let window = if anchor_shaping.is_some() { 3.0 / 5.0 * 24.0 * text_max_box_scale } else { 0.0 };

        let ctx = AddSymbol {
            fi,
            horizontal: &horizontal,
            vertical: vertical.as_ref(),
            shaped_icon: shaped_icon.as_ref(),
            vertical_icon: vertical_icon.as_ref(),
            is_sdf_icon,
            text_box_scale,
            text_padding,
            text_along_line: along_line,
            text_offset,
            icon_box_scale,
            icon_padding,
            icon_offset,
            layout_text_size,
            sizes,
            has_icon_text_fit: fit != "none",
            variable: &variable,
        };
        let add = |bucket: &mut SymbolBucket, line: &[P], anchor: Anchor| {
            if is_outside([anchor.x, anchor.y]) {
                return;
            }
            self.add_symbol(bucket, feature, anchor, line, &ctx);
        };
        let geometry = &f.geometry;
        let ty = f.ty;
        match res.placement.as_str() {
            "line" => {
                for line in anchors::clip_line(geometry, 0.0, 0.0, EXTENT, EXTENT) {
                    let found = anchors::line_anchors(&line, symbol_min_distance, text_max_angle, window, label_length, text_max_box_scale, overscaling, EXTENT);
                    for anchor in found {
                        let too_close = match &default_shaping {
                            Some(s) => {
                                let others = compare_text.entry(s.text.clone()).or_default();
                                let close = others.iter().rev().any(|o| anchors::dist(*o, [anchor.x, anchor.y]) < text_repeat_distance);
                                if !close {
                                    others.push([anchor.x, anchor.y]);
                                }
                                close
                            }
                            None => false,
                        };
                        if !too_close {
                            add(bucket, &line, anchor);
                        }
                    }
                }
            }
            "line-center" => {
                for line in geometry.iter().filter(|l| l.len() > 1) {
                    if let Some(anchor) = anchors::center_anchor(line, text_max_angle, window, label_length * text_max_box_scale) {
                        add(bucket, line, anchor);
                    }
                }
            }
            _ if ty == GeomType::Polygon => {
                for polygon in crate::render::classify_rings(geometry, 0) {
                    let poi = anchors::pole_of_inaccessibility(&polygon, 16.0);
                    let mut ring = polygon[0].clone();
                    if ring.len() >= 2 && ring.first() != ring.last() {
                        ring.push(ring[0]);
                    }
                    add(bucket, &ring, Anchor { x: poi[0], y: poi[1], segment: None });
                }
            }
            _ if ty == GeomType::LineString => {
                for line in geometry.iter().filter(|l| l.len() >= 2) {
                    add(bucket, line, Anchor { x: line[0][0], y: line[0][1], segment: None });
                }
            }
            _ if ty == GeomType::Point => {
                for p in geometry.iter().flatten() {
                    add(bucket, std::slice::from_ref(p), Anchor { x: p[0], y: p[1], segment: None });
                }
            }
            _ => {}
        }
    }

    /// `getTextVariableAnchorOffset`: anchors with offsets in em*ONE_EM units.
    fn variable_anchor_offset(&self, f: &BucketFeature, tile: &RenderTile) -> Option<Vec<(String, P)>> {
        let raw = &self.raw["layout"];
        if raw.get("text-variable-anchor-offset").is_some_and(|v| !v.is_null())
            && let Value::VariableAnchorOffsetCollection(v) = self.lay("text-variable-anchor-offset", Some(f), tile) {
                let mut out = Vec::new();
                for pair in v.chunks(2) {
                    let anchor = pair[0].as_str().unwrap_or("center").to_string();
                    let o = num2(&pair[1]);
                    let mut offset = [o[0] * ONE_EM, o[1] * ONE_EM];
                    if anchor.starts_with("top") {
                        offset[1] -= BASELINE_OFFSET;
                    } else if anchor.starts_with("bottom") {
                        offset[1] += BASELINE_OFFSET;
                    }
                    out.push((anchor, offset));
                }
                return Some(out);
            }
        if !raw.get("text-variable-anchor").is_some_and(|v| !v.is_null()) {
            return None;
        }
        let anchors = match self.lay("text-variable-anchor", Some(f), tile) {
            Value::Array(a) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>(),
            _ => return None,
        };
        let offset = if raw.get("text-radial-offset").is_some_and(|v| !v.is_null()) {
            [num(&self.lay("text-radial-offset", Some(f), tile)) * ONE_EM, f64::INFINITY]
        } else {
            let o = num2(&self.lay("text-offset", Some(f), tile));
            [o[0] * ONE_EM, o[1] * ONE_EM]
        };
        Some(anchors.into_iter().map(|a| {
            let o = evaluate_variable_offset(&a, offset);
            (a, o)
        }).collect())
    }

    fn add_symbol(&self, bucket: &mut SymbolBucket, feature: &SymbolFeature, anchor: Anchor, line: &[P], c: &AddSymbol) {
        let tile = bucket.tile;
        let zoom = tile.overscaled_z as f64;
        let f = &feature.f;
        let text_rotate = num(&self.lay("text-rotate", Some(f), &tile));
        let icon_rotate = num(&self.lay("icon-rotate", Some(f), &tile));
        let text_size = self.packed_size(&bucket.text_size, c.sizes.text, f, &tile, zoom);
        let icon_size = self.packed_size(&bucket.icon_size, c.sizes.icon, f, &tile, zoom);
        let (line_start, line_length) = add_line_vertices(bucket, &anchor, line);
        let allow_vertical = bucket.allow_vertical_placement;
        let mut circle_diameter: Option<f64> = None;
        let mut vertical_text_box = None;
        let mut vertical_icon_box = None;
        if let (true, Some(v)) = (allow_vertical, c.vertical) {
            let rot = text_rotate + 90.0;
            vertical_text_box = collision_box(&anchor, CollisionShape::Text(v), c.text_box_scale, [c.text_padding; 4], c.text_along_line, rot, &mut circle_diameter);
            if let Some(vi) = c.vertical_icon {
                vertical_icon_box = collision_box(&anchor, CollisionShape::Icon(vi), c.icon_box_scale, c.icon_padding, c.text_along_line, rot, &mut circle_diameter);
            }
        }
        let symbol = |quads: Vec<Quad>, size: [f64; 2], line_offset: P, writing_mode: WritingMode| PlacedSymbol {
            anchor: [anchor.x, anchor.y],
            quads,
            size,
            line_offset,
            line_start,
            line_length,
            segment: anchor.segment,
            writing_mode,
            feature: c.fi,
        };
        let mut icon_index = None;
        let mut vertical_icon_index = None;
        let mut icon_box = None;
        let mut num_icon_vertices = 0;
        if let Some(icon) = c.shaped_icon {
            let quads = icon_quads(icon, icon_rotate, c.is_sdf_icon, c.has_icon_text_fit);
            icon_box = collision_box(&anchor, CollisionShape::Icon(icon), c.icon_box_scale, c.icon_padding, false, icon_rotate, &mut circle_diameter);
            num_icon_vertices = quads.len() * 4;
            bucket.icon.push(symbol(quads, icon_size, c.icon_offset, WritingMode::Horizontal));
            icon_index = Some(bucket.icon.len() - 1);
            if let Some(vi) = c.vertical_icon {
                let quads = icon_quads(vi, icon_rotate, c.is_sdf_icon, c.has_icon_text_fit);
                bucket.icon.push(symbol(quads, icon_size, c.icon_offset, WritingMode::Vertical));
                vertical_icon_index = Some(bucket.icon.len() - 1);
            }
        }
        let mut text_idx: [Option<usize>; 3] = [None; 3];
        let mut text_box = None;
        let (mut num_glyph_vertices, mut num_vertical_glyph_vertices) = (0, 0);
        let all: Vec<&str> = c.horizontal.iter().map(|h| h.0).collect();
        let horizontal_mode = if c.vertical.is_some() { WritingMode::Horizontal } else { WritingMode::HorizontalOnly };
        let mut text_circle = None;
        for (j, shaping) in c.horizontal {
            if text_box.is_none() && text_circle.is_none() {
                let mut d = None;
                text_box = collision_box(&anchor, CollisionShape::Text(shaping), c.text_box_scale, [c.text_padding; 4], c.text_along_line, text_rotate, &mut d);
                text_circle = Some(d);
            }
            let single = shaping.lines.len() == 1;
            let quads = glyph_quads(shaping, c.text_offset, c.text_along_line, text_rotate, allow_vertical);
            num_glyph_vertices += quads.len() * 4;
            bucket.text.push(symbol(quads, text_size, c.text_offset, horizontal_mode));
            let idx = bucket.text.len() - 1;
            let targets: &[&str] = if single { &all } else { std::slice::from_ref(j) };
            for t in targets {
                let slot = match *t {
                    "right" => 0,
                    "center" => 1,
                    _ => 2,
                };
                text_idx[slot] = Some(idx);
            }
            if single {
                break;
            }
        }
        let mut vertical_text = None;
        if let Some(v) = c.vertical {
            let quads = glyph_quads(v, c.text_offset, c.text_along_line, text_rotate, allow_vertical);
            num_vertical_glyph_vertices = quads.len() * 4;
            bucket.text.push(symbol(quads, text_size, c.text_offset, WritingMode::Vertical));
            vertical_text = Some(bucket.text.len() - 1);
        }
        // The tallest circle diameter of all collision features.
        if let Some(Some(d)) = text_circle {
            circle_diameter = Some(circle_diameter.map_or(d, |c| c.max(d)));
        }
        let circle_diameter = circle_diameter.map(|d| d * c.layout_text_size / ONE_EM);
        if let Some(key) = feature.sort_key {
            let i = bucket.instances.len();
            match bucket.sort_key_ranges.last_mut() {
                Some(r) if r.0 == key => r.2 = i + 1,
                _ => bucket.sort_key_ranges.push((key, i, i + 1)),
            }
        }
        bucket.instances.push(SymbolInstance {
            anchor: [int16(anchor.x), int16(anchor.y)],
            text: text_idx,
            vertical_text,
            icon: icon_index,
            vertical_icon: vertical_icon_index,
            text_box,
            vertical_text_box,
            icon_box,
            vertical_icon_box,
            feature_index: f.index,
            num_glyph_vertices,
            num_vertical_glyph_vertices,
            num_icon_vertices,
            collision_circle_diameter: circle_diameter,
            text_box_scale: c.text_box_scale,
            anchor_offsets: c.variable.clone().unwrap_or_default(),
        });
    }

    /// Packed `a_size` for source/composite sizes.
    fn packed_size(&self, data: &SizeData, prop: &PropValue, f: &BucketFeature, tile: &RenderTile, zoom: f64) -> [f64; 2] {
        let pack = |v: f64| js_round(v * SIZE_PACK_FACTOR).min(MAX_PACKED_SIZE);
        let at = |z: f64| num(&prop.eval(z, Some(&f.feature), Some(Ctx::canonical(tile)), self.ctx.global_state));
        match data {
            SizeData::Source => [pack(at(zoom)), 0.0],
            SizeData::Composite { min_zoom, max_zoom, .. } => [pack(at(*min_zoom)), pack(at(*max_zoom))],
            _ => [0.0, 0.0],
        }
    }
}

struct AddSymbol<'a> {
    fi: usize,
    horizontal: &'a [(&'static str, Shaping)],
    vertical: Option<&'a Shaping>,
    shaped_icon: Option<&'a PositionedIcon>,
    vertical_icon: Option<&'a PositionedIcon>,
    is_sdf_icon: bool,
    text_box_scale: f64,
    text_padding: f64,
    text_along_line: bool,
    text_offset: P,
    icon_box_scale: f64,
    icon_padding: [f64; 4],
    icon_offset: P,
    layout_text_size: f64,
    sizes: &'a Sizes<'a>,
    has_icon_text_fit: bool,
    variable: &'a Option<Vec<(String, P)>>,
}

fn is_outside(p: P) -> bool {
    p[0] < 0.0 || p[0] >= EXTENT || p[1] < 0.0 || p[1] >= EXTENT
}

fn shaped_label_length(text: Option<&Shaping>, icon: Option<&PositionedIcon>) -> f64 {
    text.map_or(0.0, |t| t.right - t.left).max(icon.map_or(0.0, |i| i.right - i.left))
}

/// `evaluateVariableOffset`: radial offsets come as `[r, INFINITY]`.
pub fn evaluate_variable_offset(anchor: &str, offset: P) -> P {
    let (mut x, mut y) = (0.0, 0.0);
    if offset[1] != f64::INFINITY {
        let (ox, oy) = (offset[0].abs(), offset[1].abs());
        match anchor {
            "top-right" | "top-left" | "top" => y = oy - BASELINE_OFFSET,
            "bottom-right" | "bottom-left" | "bottom" => y = -oy + BASELINE_OFFSET,
            _ => {}
        }
        match anchor {
            "top-right" | "bottom-right" | "right" => x = -ox,
            "top-left" | "bottom-left" | "left" => x = ox,
            _ => {}
        }
    } else {
        let r = offset[0].max(0.0);
        let hyp = r / std::f64::consts::SQRT_2;
        match anchor {
            "top-right" | "top-left" => y = hyp - BASELINE_OFFSET,
            "bottom-right" | "bottom-left" => y = -hyp + BASELINE_OFFSET,
            "bottom" => y = -r + BASELINE_OFFSET,
            "top" => y = r - BASELINE_OFFSET,
            _ => {}
        }
        match anchor {
            "top-right" | "bottom-right" => x = -hyp,
            "top-left" | "bottom-left" => x = hyp,
            "left" => x = r,
            "right" => x = -r,
            _ => {}
        }
    }
    [x, y]
}

/// `addToLineVertexArray`: the line with Int16 coordinates and distances from the anchor.
fn add_line_vertices(bucket: &mut SymbolBucket, anchor: &Anchor, line: &[P]) -> (usize, usize) {
    let start = bucket.line_vertices.len();
    if let Some(seg) = anchor.segment {
        let a = [anchor.x, anchor.y];
        let mut dists = vec![0.0; line.len()];
        let mut forward = anchors::dist(a, line[seg + 1]);
        for i in seg + 1..line.len() {
            dists[i] = forward;
            if i < line.len() - 1 {
                forward += anchors::dist(line[i + 1], line[i]);
            }
        }
        let mut backward = anchors::dist(a, line[seg]);
        for i in (0..=seg).rev() {
            dists[i] = backward;
            if i > 0 {
                backward += anchors::dist(line[i - 1], line[i]);
            }
        }
        for (p, d) in line.iter().zip(dists) {
            bucket.line_vertices.push(([int16(p[0]), int16(p[1])], int16(d)));
        }
    }
    (start, bucket.line_vertices.len() - start)
}

enum CollisionShape<'a> {
    Text(&'a Shaping),
    Icon(&'a PositionedIcon),
}

/// `CollisionFeature`: one Int16 box, or (along lines) just the circle diameter.
fn collision_box(anchor: &Anchor, shape: CollisionShape, scale: f64, padding: [f64; 4], align_line: bool, rotate: f64, circle: &mut Option<f64>) -> Option<CollisionBox> {
    let (mut b, collision_padding) = match shape {
        CollisionShape::Text(s) => ([s.left, s.top, s.right, s.bottom], None),
        CollisionShape::Icon(i) => {
            let b = if i.image.content.is_some() && (i.image.text_fit_width.is_some() || i.image.text_fit_height.is_some()) {
                shaping::apply_text_fit(i)
            } else {
                [i.left, i.top, i.right, i.bottom]
            };
            (b, i.collision_padding)
        }
    };
    if align_line {
        let (mut top, mut bottom) = (b[1], b[3]);
        if let Some(cp) = collision_padding {
            top -= cp[1];
            bottom += cp[3];
        }
        let height = bottom - top;
        if height > 0.0 {
            let d = height.max(10.0);
            *circle = Some(circle.map_or(d, |c| c.max(d)));
        }
        return None;
    }
    b = [b[0] * scale - padding[3], b[1] * scale - padding[0], b[2] * scale + padding[1], b[3] * scale + padding[2]];
    if let Some(cp) = collision_padding {
        b[0] -= cp[0] * scale;
        b[1] -= cp[1] * scale;
        b[2] += cp[2] * scale;
        b[3] += cp[3] * scale;
    }
    if rotate != 0.0 {
        let r = rotate.to_radians();
        let (s, c) = r.sin_cos();
        let pts = [[b[0], b[1]], [b[2], b[1]], [b[0], b[3]], [b[2], b[3]]].map(|p| [p[0] * c - p[1] * s, p[0] * s + p[1] * c]);
        b = [
            pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
            pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
            pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max),
            pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max),
        ];
    }
    Some(CollisionBox { anchor: [int16(anchor.x), int16(anchor.y)], x1: int16(b[0]), y1: int16(b[1]), x2: int16(b[2]), y2: int16(b[3]) })
}

fn rotate_quad(corners: &mut [P; 4], radians: f64) {
    if radians == 0.0 {
        return;
    }
    let (s, c) = radians.sin_cos();
    // `_matMult([cos, -sin, sin, cos])`
    for p in corners.iter_mut() {
        *p = [c * p[0] - s * p[1], s * p[0] + c * p[1]];
    }
}

/// `getGlyphQuads`: vertical glyphs are pre-rotated -90° (the label is turned at render).
fn glyph_quads(shaping: &Shaping, text_offset: P, along_line: bool, text_rotate_deg: f64, allow_vertical_placement: bool) -> Vec<Quad> {
    let rotate = text_rotate_deg * std::f64::consts::PI / 180.0;
    let mut quads = Vec::new();
    for line in &shaping.lines {
        for g in &line.glyphs {
            let (mut rect_buffer, mut is_sdf, mut pixel_ratio) = (BORDER as f64 + 1.0, true, 1.0);
            let is_image = matches!(g.tex, Tex::Image(_));
            if let Tex::Image(img) = &g.tex {
                is_sdf = img.sdf;
                pixel_ratio = img.pixel_ratio;
                rect_buffer = IMAGE_PADDING / pixel_ratio;
            }
            let rotate_vertical = (along_line || allow_vertical_placement) && g.vertical;
            let half_advance = g.metrics.advance * g.scale / 2.0;
            let mut line_offset = 0.0;
            if allow_vertical_placement && shaping.verticalizable {
                let scaled_glyph_offset = (g.scale - 1.0) * ONE_EM;
                let image_offset = (ONE_EM - g.metrics.width * g.scale) / 2.0;
                line_offset = line.line_offset / 2.0 - if is_image { -image_offset } else { scaled_glyph_offset };
            }
            let glyph_offset = if along_line { [g.x + half_advance, g.y] } else { [0.0, 0.0] };
            let mut built_in = if along_line { [0.0, 0.0] } else { [g.x + half_advance + text_offset[0], g.y + text_offset[1] - line_offset] };
            let mut verticalized_offset = [0.0, 0.0];
            if rotate_vertical {
                verticalized_offset = built_in;
                built_in = [0.0, 0.0];
            }
            let (rw, rh) = g.tex.rect_size();
            let x1 = (g.metrics.left - rect_buffer) * g.scale - half_advance + built_in[0];
            let y1 = (-g.metrics.top - rect_buffer) * g.scale + built_in[1];
            let x2 = x1 + rw * g.scale / pixel_ratio;
            let y2 = y1 + rh * g.scale / pixel_ratio;
            let mut corners = [[x1, y1], [x2, y1], [x1, y2], [x2, y2]];
            if rotate_vertical {
                let center = [-half_advance, half_advance - SHAPING_DEFAULT_OFFSET];
                let (s, c) = (-std::f64::consts::FRAC_PI_2).sin_cos();
                let x_half = ONE_EM / 2.0 - half_advance;
                let y_image = if is_image { x_half } else { 0.0 };
                let correction = [5.0 - SHAPING_DEFAULT_OFFSET - x_half, -y_image];
                for p in corners.iter_mut() {
                    // `_rotateAround(-PI/2, center)`, then the offsets.
                    let (dx, dy) = (p[0] - center[0], p[1] - center[1]);
                    *p = [
                        center[0] + c * dx - s * dy + correction[0] + verticalized_offset[0],
                        center[1] + s * dx + c * dy + correction[1] + verticalized_offset[1],
                    ];
                }
            }
            rotate_quad(&mut corners, rotate);
            quads.push(Quad {
                corners,
                tex: g.tex.clone(),
                rect: [0.0, 0.0, rw, rh],
                pixel_offset_tl: [0.0, 0.0],
                pixel_offset_br: [0.0, 0.0],
                min_font_scale: [0.0, 0.0],
                glyph_offset,
                section: g.section,
                is_sdf,
            });
        }
    }
    quads
}

/// `getIconQuads`, including nine-patch stretching for `icon-text-fit`.
fn icon_quads(icon: &PositionedIcon, rotate_deg: f64, is_sdf: bool, has_icon_text_fit: bool) -> Vec<Quad> {
    let image = &icon.image;
    let pixel_ratio = image.pixel_ratio;
    let (image_w, image_h) = (image.image.w as f64, image.image.h as f64);
    let mut b = [icon.left, icon.top, icon.right, icon.bottom];
    let full = |len: f64| vec![[0.0, len]];
    let stretch_x = image.stretch_x.clone().unwrap_or_else(|| full(image_w));
    let stretch_y = image.stretch_y.clone().unwrap_or_else(|| full(image_h));
    let sum = |r: &[[f64; 2]]| r.iter().map(|r| r[1] - r[0]).sum::<f64>();
    let (stretch_w, stretch_h) = (sum(&stretch_x), sum(&stretch_y));
    let (fixed_w, fixed_h) = (image_w - stretch_w, image_h - stretch_h);
    let (mut so_x, mut sc_w, mut so_y, mut sc_h) = (0.0, stretch_w, 0.0, stretch_h);
    let (mut fo_x, mut fc_w, mut fo_y, mut fc_h) = (0.0, fixed_w, 0.0, fixed_h);
    if let (Some(c), true) = (image.content, has_icon_text_fit) {
        let (cw, ch) = (c[2] - c[0], c[3] - c[1]);
        if image.text_fit_width.is_some() || image.text_fit_height.is_some() {
            b = shaping::apply_text_fit(icon);
        }
        let within = |r: &[[f64; 2]], min: f64, max: f64| r.iter().map(|r| min.max(max.min(r[1])) - min.max(max.min(r[0]))).sum::<f64>();
        so_x = within(&stretch_x, 0.0, c[0]);
        so_y = within(&stretch_y, 0.0, c[1]);
        sc_w = within(&stretch_x, c[0], c[2]);
        sc_h = within(&stretch_y, c[1], c[3]);
        fo_x = c[0] - so_x;
        fo_y = c[1] - so_y;
        fc_w = cw - sc_w;
        fc_h = ch - sc_h;
    }
    let (icon_left, icon_top, icon_w, icon_h) = (b[0], b[1], b[2] - b[0], b[3] - b[1]);
    #[derive(Clone, Copy)]
    struct Cut {
        fixed: f64,
        stretch: f64,
    }
    let em = |stretch_offset: f64, stretch_size: f64, size: f64, offset: f64| stretch_offset / stretch_size * size + offset;
    let px = |fixed_offset: f64, fixed_size: f64, stretch_offset: f64, stretch_size: f64| fixed_offset - fixed_size * stretch_offset / stretch_size;
    let make = |l: Cut, t: Cut, r: Cut, bo: Cut| {
        let left_em = em(l.stretch - so_x, sc_w, icon_w, icon_left);
        let left_px = px(l.fixed - fo_x, fc_w, l.stretch, stretch_w);
        let top_em = em(t.stretch - so_y, sc_h, icon_h, icon_top);
        let top_px = px(t.fixed - fo_y, fc_h, t.stretch, stretch_h);
        let right_em = em(r.stretch - so_x, sc_w, icon_w, icon_left);
        let right_px = px(r.fixed - fo_x, fc_w, r.stretch, stretch_w);
        let bottom_em = em(bo.stretch - so_y, sc_h, icon_h, icon_top);
        let bottom_px = px(bo.fixed - fo_y, fc_h, bo.stretch, stretch_h);
        let mut corners = [[left_em, top_em], [right_em, top_em], [left_em, bottom_em], [right_em, bottom_em]];
        rotate_quad(&mut corners, rotate_deg * std::f64::consts::PI / 180.0);
        let (x1, x2) = (l.stretch + l.fixed, r.stretch + r.fixed);
        let (y1, y2) = (t.stretch + t.fixed, bo.stretch + bo.fixed);
        Quad {
            corners,
            tex: Tex::Image(image.clone()),
            rect: [IMAGE_PADDING + x1, IMAGE_PADDING + y1, x2 - x1, y2 - y1],
            pixel_offset_tl: [left_px / pixel_ratio, top_px / pixel_ratio],
            pixel_offset_br: [right_px / pixel_ratio, bottom_px / pixel_ratio],
            min_font_scale: [fc_w / pixel_ratio / icon_w, fc_h / pixel_ratio / icon_h],
            glyph_offset: [0.0, 0.0],
            section: 0,
            is_sdf,
        }
    };
    if !has_icon_text_fit || (image.stretch_x.is_none() && image.stretch_y.is_none()) {
        let c = |stretch: f64| Cut { fixed: 0.0, stretch };
        return vec![make(c(-1.0), c(-1.0), c(image_w + 1.0), c(image_h + 1.0))];
    }
    let cuts = |zones: &[[f64; 2]], fixed: f64, stretch: f64| {
        let mut out = vec![Cut { fixed: -IMAGE_PADDING, stretch: 0.0 }];
        for z in zones {
            let last = *out.last().unwrap();
            out.push(Cut { fixed: z[0] - last.stretch, stretch: last.stretch });
            out.push(Cut { fixed: z[0] - last.stretch, stretch: last.stretch + (z[1] - z[0]) });
        }
        out.push(Cut { fixed: fixed + IMAGE_PADDING, stretch });
        out
    };
    let (xc, yc) = (cuts(&stretch_x, fixed_w, stretch_w), cuts(&stretch_y, fixed_h, stretch_h));
    let mut quads = Vec::new();
    for xi in 0..xc.len() - 1 {
        for yi in 0..yc.len() - 1 {
            quads.push(make(xc[xi], yc[yi], xc[xi + 1], yc[yi + 1]));
        }
    }
    quads
}
