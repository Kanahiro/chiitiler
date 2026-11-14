//! Drawing symbol layers (`draw_symbol.ts` + the `symbol_sdf` / `symbol_icon` /
//! `symbol_text_and_icon` shaders): glyph and icon quads rasterized with the GL rules.

use super::layout::{int16, PlacedSymbol, Resolved, SymbolBucket};
use super::placement::InstancePlacement;
use super::projection::LineProjector;
use super::shaping::{self, Tex, WritingMode, ONE_EM};
use crate::expression::Value;
use crate::image::Filter;
use crate::render::raster::{self, Rgba, Target};
use crate::render::{color, num, Ctx, EXTENT};
use crate::style::Layer;
use std::rc::Rc;

type P = [f64; 2];

/// One placed symbol to draw: bucket, symbol and where its quads go.
struct Item {
    bucket: usize,
    symbol: usize,
    sort_key: f64,
    /// Per-quad label-plane point and angle (line labels); otherwise the anchor for all quads.
    positions: Vec<(P, f64)>,
}

/// Fragment paint of a symbol (per feature/section).
struct Paint {
    fill: Rgba,
    halo: Rgba,
    opacity: f32,
    halo_width: f32,
    halo_blur: f32,
}

pub fn draw_symbol_layer(target: &mut Target, layer: &Layer, ctx: &Ctx, raw: &serde_json::Value, buckets: &[Rc<SymbolBucket>], placements: &[Vec<InstancePlacement>]) {
    let Some(first) = buckets.first() else { return };
    let res = Resolved::new(layer, ctx, &first.tile);
    let constant_or = |name: &str, default: f64| {
        let p = layer.paint(name);
        if p.is_feature_constant() { num(&ctx.paint(layer, name, None, &first.tile)) } else { default }
    };
    // Draw order: tiles by zoom (high first), then by y and x rotated with the map.
    let angle = -ctx.transform.bearing_rad;
    let (sin, cos) = angle.sin_cos();
    let rotated = |i: usize| {
        let c = &buckets[i].tile.canonical;
        let (x, y) = (c.x as f64, c.y as f64);
        (cos * x - sin * y, sin * x + cos * y)
    };
    let mut order: Vec<usize> = (0..buckets.len()).collect();
    order.sort_by(|&a, &b| {
        let (ra, rb) = (rotated(a), rotated(b));
        buckets[b].tile.overscaled_z.cmp(&buckets[a].tile.overscaled_z).then(ra.1.total_cmp(&rb.1)).then(ra.0.total_cmp(&rb.0))
    });
    if constant_or("icon-opacity", 1.0) != 0.0 {
        draw_pass(target, layer, ctx, raw, &res, buckets, placements, &order, false);
    }
    if constant_or("text-opacity", 1.0) != 0.0 {
        draw_pass(target, layer, ctx, raw, &res, buckets, placements, &order, true);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_pass(
    target: &mut Target,
    layer: &Layer,
    ctx: &Ctx,
    raw: &serde_json::Value,
    res: &Resolved,
    buckets: &[Rc<SymbolBucket>],
    placements: &[Vec<InstancePlacement>],
    order: &[usize],
    is_text: bool,
) {
    let t = ctx.transform;
    let prefix = if is_text { "text" } else { "icon" };
    let rotation_alignment = if is_text { &res.text_rotation_alignment } else { &res.icon_rotation_alignment };
    let rotate_with_map = rotation_alignment == "map";
    let pitch_with_map = if is_text { res.text_pitch_alignment == "map" } else { res.icon_pitch_alignment == "map" };
    let along_line = rotation_alignment != "viewport" && res.placement != "point";
    let has_variable = ["text-variable-anchor", "text-variable-anchor-offset"].iter().any(|k| raw["layout"].get(*k).is_some_and(|v| !v.is_null()));
    // Layer-level layout values are evaluated at the map zoom (bucket values at the tile zoom).
    let lay_now = |n: &str| layer.layout(n).eval(t.zoom, None, None, ctx.global_state);
    let keep_upright = lay_now(&format!("{prefix}-keep-upright")).truthy();
    let has_sort_key = !layer.layout("symbol-sort-key").is_constant();
    let mut sort_by_key = false;
    // Screen angle of the tile x axis (`u_rotate_symbol` / rotate-with-map label planes).
    let map_angle = t.bearing_rad;
    let mut items: Vec<Item> = Vec::new();
    for &bi in order {
        let bucket = &buckets[bi];
        let symbols = if is_text { &bucket.text } else { &bucket.icon };
        if symbols.is_empty() {
            continue;
        }
        let tile = bucket.tile;
        let to_tile = EXTENT / ctx.tile_px(&tile);
        let tr = ctx.translate(layer, prefix, &tile);
        let translation = [tr[0] * to_tile, tr[1] * to_tile];
        let projector = LineProjector::new(ctx, bucket, translation);
        let size_data = if is_text { &bucket.text_size } else { &bucket.icon_size };
        let update_text_fit_icon = !is_text && has_variable && lay_now("icon-text-fit").as_str().is_some_and(|s| s != "none") && !bucket.text.is_empty();
        let text_size = &bucket.text_size;
        let has_icon_text_fit = lay_now("icon-text-fit").as_str().is_some_and(|s| s != "none");
        // Symbol index → (instance, whether this symbol is the one drawn for it).
        let mut owner: Vec<Option<(usize, bool)>> = vec![None; symbols.len()];
        for (ii, inst) in bucket.instances.iter().enumerate() {
            let p = &placements[bi][ii];
            let horizontal_hidden = p.orientation == Some(WritingMode::Vertical);
            let vertical_hidden = matches!(p.orientation, Some(WritingMode::Horizontal | WritingMode::HorizontalOnly));
            if is_text {
                // `markUsedJustification`: the shaping of the placed anchor's justification (or
                // the vertical one); without such a shaping every shaping stays in use.
                let used = p.variable.as_ref().map(|v| match (p.orientation, shaping::anchor_justification(&v.anchor)) {
                    (Some(WritingMode::Vertical), _) => inst.vertical_text,
                    (_, "right") => inst.text[0],
                    (_, "center") => inst.text[1],
                    _ => inst.text[2],
                });
                let in_use = |s: usize| !has_variable || matches!(used, Some(None)) || used == Some(Some(s));
                for s in inst.text.iter().flatten() {
                    let visible = p.text && !horizontal_hidden && in_use(*s);
                    owner[*s] = Some((ii, visible || owner[*s].is_some_and(|o| o.1)));
                }
                if let Some(s) = inst.vertical_text {
                    owner[s] = Some((ii, p.text && !vertical_hidden && in_use(s)));
                }
            } else {
                let use_horizontal = !(has_icon_text_fit && inst.vertical_icon.is_some() && horizontal_hidden);
                if let Some(s) = inst.icon {
                    owner[s] = Some((ii, p.icon && use_horizontal));
                }
                if let Some(s) = inst.vertical_icon {
                    owner[s] = Some((ii, p.icon && !use_horizontal));
                }
            }
        }
        // `updateLineLabels`: in symbol order, a horizontal label too steep for its line hands
        // over to the vertical one that follows it.
        let mut line_positions: Vec<Option<Vec<(P, f64)>>> = vec![None; if along_line { symbols.len() } else { 0 }];
        if along_line {
            let mut use_vertical = false;
            let rotate_to_line = res.text_rotation_alignment == "map";
            let buf = 256.0 / t.pixel_ratio;
            for (si, sym) in symbols.iter().enumerate() {
                let visible = owner[si].is_some_and(|o| o.1);
                if !visible || (sym.writing_mode == WritingMode::Vertical && !use_vertical) {
                    continue;
                }
                use_vertical = false;
                let a = projector.project([int16(sym.anchor[0]), int16(sym.anchor[1])]);
                if a[0] < -buf || a[0] > t.width + buf || a[1] < -buf || a[1] > t.height + buf {
                    continue;
                }
                let size = size_data.for_feature(t.zoom, sym.size);
                match projector.place_symbol(sym, size, keep_upright, rotate_to_line) {
                    super::projection::LinePlacement::Placed(p) => line_positions[si] = Some(p),
                    super::projection::LinePlacement::UseVertical => use_vertical = true,
                    super::projection::LinePlacement::Hidden => {}
                }
            }
        }
        let symbol_order: Vec<usize> = if bucket.sort_features_by_y {
            let mut v = Vec::new();
            for ii in super::placement::sorted_symbol_indexes(bucket, map_angle) {
                let inst = &bucket.instances[ii];
                if is_text {
                    for (k, s) in inst.text.iter().enumerate() {
                        if let Some(s) = s
                            && !inst.text[..k].contains(&Some(*s)) {
                                v.push(*s);
                            }
                    }
                    v.extend(inst.vertical_text);
                } else {
                    v.extend(inst.icon);
                    v.extend(inst.vertical_icon);
                }
            }
            v
        } else {
            (0..symbols.len()).collect()
        };
        for si in symbol_order {
            let Some((ii, true)) = owner[si] else { continue };
            let sym = &symbols[si];
            let inst = &bucket.instances[ii];
            let placement = &placements[bi][ii];
            let positions = if along_line {
                match line_positions[si].take() {
                    Some(p) => p,
                    None => continue,
                }
            } else {
                let base = projector.project(sym.anchor);
                let angle = if rotate_with_map { map_angle } else { 0.0 };
                let shift_source = if is_text {
                    has_variable.then_some(placement.variable.as_ref())
                } else if update_text_fit_icon {
                    Some(placement.variable.as_ref())
                } else {
                    None
                };
                // Vertical point labels are turned a quarter (`addDynamicAttributes`).
                let vertical_angle = |vertical: bool| if bucket.allow_vertical_placement && vertical { std::f64::consts::FRAC_PI_2 } else { 0.0 };
                let (point, glyph_angle) = match shift_source {
                    Some(None) => continue,
                    Some(Some(v)) => {
                        let render_size = text_size.for_feature(t.zoom, inst.text.iter().flatten().next().or(inst.vertical_text.as_ref()).map_or(sym.size, |&s| bucket.text[s].size)) / ONE_EM;
                        let (h, vv) = shaping::anchor_alignment(&v.anchor);
                        let shift = [
                            (-(h - 0.5) * v.width / v.text_box_scale + v.text_offset[0]) * render_size,
                            (-(vv - 0.5) * v.height / v.text_box_scale + v.text_offset[1]) * render_size,
                        ];
                        let shift = if rotate_with_map { rotate(shift, angle) } else { shift };
                        let base = projector.project([int16(sym.anchor[0]), int16(sym.anchor[1])]);
                        ([base[0] + shift[0], base[1] + shift[1]], vertical_angle(placement.orientation == Some(WritingMode::Vertical)))
                    }
                    None => (base, vertical_angle(sym.writing_mode == WritingMode::Vertical)),
                };
                vec![(point, glyph_angle); sym.quads.len()]
            };
            let sort_key = bucket.features[sym.feature].sort_key.unwrap_or(0.0);
            if has_sort_key && bucket.can_overlap {
                sort_by_key = true;
            }
            items.push(Item { bucket: bi, symbol: si, sort_key, positions });
        }
    }
    if sort_by_key {
        items.sort_by(|a, b| a.sort_key.partial_cmp(&b.sort_key).unwrap_or(std::cmp::Ordering::Equal));
    }

    let halo_width_prop = layer.paint(&format!("{prefix}-halo-width"));
    let halo_const = halo_width_prop.is_feature_constant().then(|| num(&ctx.paint(layer, &format!("{prefix}-halo-width"), None, &buckets[0].tile)));
    let letter_spacing = if layer.layout("text-letter-spacing").is_feature_constant() { num(&lay_now("text-letter-spacing")) } else { 0.0 };
    let glyph_overlap = letter_spacing * ONE_EM < 0.0 || halo_const.unwrap_or(f64::INFINITY) > 1.0;
    let icon_size_const = if layer.layout("icon-size").is_feature_constant() { num(&lay_now("icon-size")) } else { 0.0 };

    // Each tile is drawn (halo pass, then fill pass) before the next; with sort keys the
    // segments of all tiles interleave.
    let mut i = 0;
    while i < items.len() {
        let mut j = i + 1;
        while j < items.len() && items[j].bucket == items[i].bucket && !sort_by_key {
            j += 1;
        }
        if sort_by_key {
            while j < items.len() && items[j].bucket == items[i].bucket && items[j].sort_key == items[i].sort_key {
                j += 1;
            }
        }
        let bucket = &buckets[items[i].bucket];
        let is_sdf = is_text || bucket.sdf_icons;
        let has_halo = is_sdf && halo_const.unwrap_or(1.0) != 0.0;
        let icon_linear = is_sdf || icon_size_const != 1.0 || bucket.icons_need_linear || pitch_with_map;
        let size_data = if is_text { &bucket.text_size } else { &bucket.icon_size };
        // Glyphs are always LINEAR; images inside text only when transformed or sized by zoom.
        let zoom_sized = matches!(size_data, super::layout::SizeData::Camera { .. } | super::layout::SizeData::Composite { .. });
        let filter = if is_text {
            if pitch_with_map || zoom_sized { Filter::Linear } else { Filter::Nearest }
        } else if icon_linear {
            Filter::Linear
        } else {
            Filter::Nearest
        };
        let mode = |halo: bool, plain: bool| Mode { halo, plain, is_text, filter };
        let passes: Vec<Mode> = if has_halo && glyph_overlap {
            vec![mode(true, false), mode(false, true)]
        } else {
            vec![mode(has_halo, true)]
        };
        for m in passes {
            for item in &items[i..j] {
                let symbols = if is_text { &bucket.text } else { &bucket.icon };
                let sym = &symbols[item.symbol];
                draw_symbol(target, layer, ctx, bucket, sym, &item.positions, size_data, rotate_with_map && !along_line, map_angle, &m);
            }
        }
        i = j;
    }
}

struct Mode {
    halo: bool,
    plain: bool,
    is_text: bool,
    filter: Filter,
}

fn rotate(v: P, a: f64) -> P {
    let (s, c) = a.sin_cos();
    [v[0] * c - v[1] * s, v[0] * s + v[1] * c]
}

fn paint_for(layer: &Layer, ctx: &Ctx, bucket: &SymbolBucket, sym: &PlacedSymbol, section: usize, is_text: bool) -> Paint {
    let prefix = if is_text { "text" } else { "icon" };
    let feature = &bucket.features[sym.feature];
    let p = |n: &str| ctx.paint(layer, &format!("{prefix}-{n}"), Some(&feature.f), &bucket.tile);
    let mut fill = p("color");
    if is_text {
        let override_color = feature.text.as_ref().and_then(|t| t.sections.get(section)).and_then(|s| s.text_color);
        if let Some(c) = override_color {
            fill = Value::Color(c);
        }
    }
    Paint {
        fill: color(&fill, 1.0),
        halo: color(&p("halo-color"), 1.0),
        opacity: num(&p("opacity")) as f32,
        halo_width: num(&p("halo-width")) as f32,
        halo_blur: num(&p("halo-blur")) as f32,
    }
}

/// Bilinear/nearest sample of a glyph SDF (alpha) at texel coordinates of its padded rect.
fn sample_glyph(g: &super::glyphs::Glyph, tx: f32, ty: f32) -> f32 {
    let texel = |x: i64, y: i64| -> f32 {
        // The padded rect has a 1px empty border around the bitmap.
        let (bx, by) = (x - 1, y - 1);
        if bx < 0 || by < 0 || bx >= g.bw as i64 || by >= g.bh as i64 {
            0.0
        } else {
            g.bitmap[by as usize * g.bw + bx as usize] as f32 / 255.0
        }
    };
    let (x, y) = (tx - 0.5, ty - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let (a, b, c, d) = (texel(x0, y0), texel(x0 + 1, y0), texel(x0, y0 + 1), texel(x0 + 1, y0 + 1));
    (a * (1.0 - fx) + b * fx) * (1.0 - fy) + (c * (1.0 - fx) + d * fx) * fy
}

/// Sample a quad's texture at texel coordinates of the padded rect: (sdf alpha, rgba).
fn sample(tex: &Tex, tx: f32, ty: f32, filter: Filter) -> Rgba {
    match tex {
        Tex::Glyph(g) => [0.0, 0.0, 0.0, sample_glyph(g, tx, ty)],
        Tex::Image(img) if img.sdf => {
            let (w, h) = img.icon.size();
            img.icon.sample(tx / w as f32, ty / h as f32, Filter::Linear, Filter::Linear, 0.0)
        }
        Tex::Image(img) => {
            let (w, h) = img.icon.size();
            img.icon.sample(tx / w as f32, ty / h as f32, filter, filter, 0.0)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_symbol(
    target: &mut Target,
    layer: &Layer,
    ctx: &Ctx,
    bucket: &SymbolBucket,
    sym: &PlacedSymbol,
    positions: &[(P, f64)],
    size_data: &super::layout::SizeData,
    rotate_symbol: bool,
    map_angle: f64,
    m: &Mode,
) {
    let t = ctx.transform;
    let dpr = t.pixel_ratio;
    let size = size_data.shader_size(t.zoom, sym.size) as f32 as f64;
    let font_scale = if m.is_text { size / 24.0 } else { size };
    let mut paint_cache: Option<(usize, Paint)> = None;
    for (qi, q) in sym.quads.iter().enumerate() {
        let Some(&(pos, glyph_angle)) = positions.get(qi) else { continue };
        if paint_cache.as_ref().is_none_or(|c| c.0 != q.section) {
            paint_cache = Some((q.section, paint_for(layer, ctx, bucket, sym, q.section, m.is_text)));
        }
        let paint = &paint_cache.as_ref().unwrap().1;
        let total_opacity = paint.opacity;
        if total_opacity < 0.1 {
            continue;
        }
        let angle = glyph_angle + if rotate_symbol { map_angle } else { 0.0 };
        let (sin, cos) = angle.sin_cos();
        let pxo = |v: f64| int16(v * 16.0) / 16.0;
        let mfs = [int16(q.min_font_scale[0] * 256.0) / 256.0, int16(q.min_font_scale[1] * 256.0) / 256.0];
        let scale = [mfs[0].max(font_scale), mfs[1].max(font_scale)];
        let y = q.glyph_offset[1];
        let corner = |i: usize, px: P| {
            let o = q.corners[i];
            let off = [int16(js_round32(o[0] * 32.0)) / 32.0, int16(js_round32((y + o[1]) * 32.0)) / 32.0];
            let v = [off[0] * scale[0] + pxo(px[0]), off[1] * scale[1] + pxo(px[1])];
            let r = [v[0] * cos - v[1] * sin, v[0] * sin + v[1] * cos];
            [(pos[0] + r[0]) * dpr, (pos[1] + r[1]) * dpr]
        };
        let (tl, br) = (q.pixel_offset_tl, q.pixel_offset_br);
        let pts = [corner(0, tl), corner(1, [br[0], tl[1]]), corner(2, [tl[0], br[1]]), corner(3, br)];
        let [rx, ry, rw, rh] = q.rect.map(|v| v as f32);
        let uv = [[rx, ry], [rx + rw, ry], [rx, ry + rh], [rx + rw, ry + rh]];
        let gamma_base = 0.105 / dpr as f32;
        let fs = font_scale as f32;
        let sdf = q.is_sdf;
        let mut shade = |v: &[f32; 2]| -> Option<Rgba> {
            let s = sample(&q.tex, v[0], v[1], m.filter);
            if !sdf {
                return Some(s.map(|c| c * total_opacity));
            }
            let dist = s[3];
            let gamma = gamma_base / fs;
            let mut text = [0.0f32; 4];
            let mut halo = [0.0f32; 4];
            if m.plain {
                let alpha = raster::smoothstep(0.75 - gamma, 0.75 + gamma, dist);
                text = paint.fill.map(|c| c * total_opacity * alpha);
            }
            if m.halo {
                let gamma_halo = (paint.halo_blur * 1.19 / 8.0 + gamma_base) / fs;
                let inner = 0.75 + gamma_halo;
                let alpha_halo = raster::smoothstep(inner - gamma_halo, inner + gamma_halo, dist);
                let edge = (6.0 - paint.halo_width / fs) / 8.0;
                let a = raster::smoothstep(edge - gamma_halo, edge + gamma_halo, dist).min(1.0 - alpha_halo);
                halo = paint.halo.map(|c| c * total_opacity * a);
            }
            let out: Rgba = if m.plain && m.halo {
                std::array::from_fn(|k| text[k] + (1.0 - text[3]) * halo[k])
            } else if m.halo {
                halo
            } else {
                text
            };
            Some(out)
        };
        // `shade` with the texture sampled per pixel and the rest four pixels at a time.
        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        {
            use crate::render::x4;
            use core::arch::wasm32::*;
            let f = |x: f32| f32x4_splat(std::hint::black_box(x));
            let to = total_opacity;
            let gamma = gamma_base / fs;
            let text_ss = m.plain.then(|| x4::Smoothstep::new(0.75 - gamma, 0.75 + gamma));
            let halo_ss = m.halo.then(|| {
                let gamma_halo = (paint.halo_blur * 1.19 / 8.0 + gamma_base) / fs;
                let inner = 0.75 + gamma_halo;
                let edge = (6.0 - paint.halo_width / fs) / 8.0;
                (x4::Smoothstep::new(inner - gamma_halo, inner + gamma_halo), x4::Smoothstep::new(edge - gamma_halo, edge + gamma_halo))
            });
            let (fill, halo, one, tov) = (paint.fill.map(|c| f(c * to)), paint.halo.map(|c| f(c * to)), f(1.0), f(to));
            let mut shade4 = |v: &[v128; 2], k: &x4::K| {
                let (u, w) = (x4::lanes(v[0]), x4::lanes(v[1]));
                let s: [Rgba; 4] = std::array::from_fn(|j| sample(&q.tex, u[j], w[j], m.filter));
                let channel = |c: usize| f32x4(s[0][c], s[1][c], s[2][c], s[3][c]);
                let draw = u32x4_splat(u32::MAX);
                if !sdf {
                    return x4::Shaded { draw, c: std::array::from_fn(|c| f32x4_mul(channel(c), tov)), uniform: None };
                }
                let dist = channel(3);
                let text = match &text_ss {
                    Some(ss) => {
                        let alpha = ss.eval(dist, k);
                        fill.map(|c| f32x4_mul(c, alpha))
                    }
                    None => [f32x4_splat(0.0); 4],
                };
                let halo = match &halo_ss {
                    Some((sh, se)) => {
                        let a = x4::min(se.eval(dist, k), f32x4_sub(one, sh.eval(dist, k)));
                        halo.map(|c| f32x4_mul(c, a))
                    }
                    None => [f32x4_splat(0.0); 4],
                };
                let c = if m.plain && m.halo {
                    std::array::from_fn(|c| f32x4_add(text[c], f32x4_mul(f32x4_sub(one, text[3]), halo[c])))
                } else if m.halo {
                    halo
                } else {
                    text
                };
                x4::Shaded { draw, c, uniform: None }
            };
            x4::triangle(target, None, [pts[0], pts[2], pts[1]], [uv[0], uv[2], uv[1]], &mut shade, &mut shade4);
            x4::triangle(target, None, [pts[1], pts[2], pts[3]], [uv[1], uv[2], uv[3]], &mut shade, &mut shade4);
        }
        // Index order (0, 2, 1), (1, 2, 3).
        #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
        {
            raster::triangle(target, None, [pts[0], pts[2], pts[1]], [uv[0], uv[2], uv[1]], &mut shade);
            raster::triangle(target, None, [pts[1], pts[2], pts[3]], [uv[1], uv[2], uv[3]], &mut shade);
        }
    }
}

fn js_round32(v: f64) -> f64 {
    crate::expression::value::js_round(v)
}
