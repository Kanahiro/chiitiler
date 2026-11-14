//! Text and icon shaping (`shaping.ts`, `tagged_string.ts`): line breaking, justification and
//! anchor alignment from PBF glyph metrics. Grapheme clusters are taken one code point at a time,
//! which is what MapLibre does for clusters no glyph file covers.

use super::glyphs::{Glyph, Glyphs, Metrics, BORDER};
use super::unicode;
use crate::expression::value::Formatted;
use crate::sprite::StyleImage;
use crate::hash::HashMap;
use std::rc::Rc;

pub const ONE_EM: f64 = 24.0;
pub const SHAPING_DEFAULT_OFFSET: f64 = -17.0;
/// `IMAGE_PADDING` of the image atlas.
pub const IMAGE_PADDING: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritingMode {
    Horizontal,
    Vertical,
    /// Horizontal text without a vertical alternative.
    HorizontalOnly,
}

#[derive(Debug, Clone)]
pub enum SectionKind {
    Text { font_stack: String },
    Image { name: String },
}

#[derive(Debug, Clone)]
pub struct Section {
    pub scale: f64,
    /// `verticalAlign` factor: top 0, center 0.5, bottom 1.
    pub valign: f64,
    pub kind: SectionKind,
}

#[derive(Clone)]
pub enum Tex {
    Glyph(Rc<Glyph>),
    Image(Rc<StyleImage>),
}

impl Tex {
    /// Atlas rect size (with the 1px atlas padding).
    pub fn rect_size(&self) -> (f64, f64) {
        match self {
            Tex::Glyph(g) => ((g.bw + 2) as f64, (g.bh + 2) as f64),
            Tex::Image(i) => ((i.image.w + 2) as f64, (i.image.h + 2) as f64),
        }
    }
}

#[derive(Clone)]
pub struct PositionedGlyph {
    pub x: f64,
    pub y: f64,
    pub vertical: bool,
    pub scale: f64,
    pub section: usize,
    pub metrics: Metrics,
    pub tex: Tex,
}

#[derive(Clone, Default)]
pub struct PositionedLine {
    pub glyphs: Vec<PositionedGlyph>,
    pub line_offset: f64,
}

#[derive(Clone)]
pub struct Shaping {
    pub lines: Vec<PositionedLine>,
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
    pub text: String,
    pub icons_in_text: bool,
    pub verticalizable: bool,
}

/// `getAnchorAlignment`: (horizontal, vertical) in 0..1.
pub fn anchor_alignment(anchor: &str) -> (f64, f64) {
    let h = match anchor {
        "right" | "top-right" | "bottom-right" => 1.0,
        "left" | "top-left" | "bottom-left" => 0.0,
        _ => 0.5,
    };
    let v = match anchor {
        "bottom" | "bottom-right" | "bottom-left" => 1.0,
        "top" | "top-right" | "top-left" => 0.0,
        _ => 0.5,
    };
    (h, v)
}

/// `getAnchorJustification`.
pub fn anchor_justification(anchor: &str) -> &'static str {
    match anchor {
        "right" | "top-right" | "bottom-right" => "right",
        "left" | "top-left" | "bottom-left" => "left",
        _ => "center",
    }
}

/// Display size of a style image in CSS px (`ImagePosition.displaySize`).
pub fn display_size(img: &StyleImage) -> [f64; 2] {
    [img.image.w as f64 / img.pixel_ratio, img.image.h as f64 / img.pixel_ratio]
}

struct Ctx<'a> {
    glyphs: &'a Glyphs,
    images: &'a HashMap<String, Rc<StyleImage>>,
    sections: Vec<Section>,
}

/// A line of text: characters and the section of each.
#[derive(Clone)]
struct Tagged {
    chars: Vec<char>,
    section: Vec<usize>,
}

impl Ctx<'_> {
    fn glyph(&self, s: &Section, c: char) -> Option<&Rc<Glyph>> {
        match &s.kind {
            SectionKind::Text { font_stack } => self.glyphs.get(font_stack, c),
            SectionKind::Image { .. } => None,
        }
    }

    fn image(&self, s: &Section) -> Option<&Rc<StyleImage>> {
        match &s.kind {
            SectionKind::Image { name } => self.images.get(name),
            SectionKind::Text { .. } => None,
        }
    }

    /// `getGlyphAdvance`.
    fn advance(&self, c: char, s: &Section, spacing: f64, layout_text_size: f64) -> f64 {
        match &s.kind {
            SectionKind::Text { .. } => self.glyph(s, c).map_or(0.0, |g| g.metrics.advance * s.scale + spacing),
            SectionKind::Image { .. } => {
                self.image(s).map_or(0.0, |img| display_size(img)[0] * s.scale * ONE_EM / layout_text_size + spacing)
            }
        }
    }

    fn total_width(&self, t: &Tagged, spacing: f64, layout_text_size: f64) -> f64 {
        t.chars.iter().zip(&t.section).map(|(&c, &s)| self.advance(c, &self.sections[s], spacing, layout_text_size)).sum()
    }

    /// `determineLineBreaks`: indices to break at, chosen by least badness.
    fn line_breaks(&self, t: &Tagged, spacing: f64, max_width: f64, layout_text_size: f64) -> Vec<usize> {
        let total = self.total_width(t, spacing, layout_text_size);
        if spacing >= 0.0 && total > 0.0 && total <= max_width && !t.chars.iter().any(|&c| c == '\n' || c == '\r') {
            return vec![t.chars.len()];
        }
        struct Break {
            index: usize,
            x: f64,
            prior: Option<usize>,
            badness: f64,
        }
        let target = total / (total / max_width).ceil().max(1.0);
        let has_zwsp = t.chars.contains(&'\u{200b}');
        let mut breaks: Vec<Break> = Vec::new();
        let evaluate = |breaks: &Vec<Break>, index: usize, x: f64, penalty: f64, last: bool| {
            let badness = |w: f64| {
                let raggedness = (w - target).powi(2);
                if last {
                    if w < target { raggedness / 2.0 } else { raggedness * 2.0 }
                } else {
                    raggedness + penalty.abs() * penalty
                }
            };
            let mut best = None;
            let mut best_badness = badness(x);
            for (i, b) in breaks.iter().enumerate() {
                let v = badness(x - b.x) + b.badness;
                if v <= best_badness {
                    best = Some(i);
                    best_badness = v;
                }
            }
            Break { index, x, prior: best, badness: best_badness }
        };
        let mut x = 0.0;
        for i in 0..t.chars.len() {
            let c = t.chars[i];
            if i > 0 {
                let prev = t.chars[i - 1];
                let ideographic = unicode::allows_ideographic_breaking(prev);
                let breakable = matches!(
                    prev as u32,
                    0x0a | 0x0d | 0x20 | 0x26 | 0x29 | 0x2b | 0x2d | 0x2f | 0xad | 0xb7 | 0x200b | 0x2010 | 0x2013 | 0x2027
                );
                let image_before = matches!(self.sections[t.section[i - 1]].kind, SectionKind::Image { .. });
                // ponytail: no word segmenter, so scripts written without spaces (Thai, ...) only
                // break at the characters above.
                if breakable || ideographic || image_before || (i + 1 < t.chars.len() && c == '(') {
                    let mut penalty = 0.0;
                    if prev == '\n' || prev == '\r' {
                        penalty -= 10000.0;
                    }
                    if ideographic && has_zwsp {
                        penalty += 150.0;
                    }
                    if prev == '(' || prev == '\u{ff08}' {
                        penalty += 50.0;
                    }
                    if c == ')' || c == '\u{ff09}' {
                        penalty += 50.0;
                    }
                    let b = evaluate(&breaks, i, x, penalty, false);
                    breaks.push(b);
                }
            }
            if !unicode::is_whitespace(c) {
                x += self.advance(c, &self.sections[t.section[i]], spacing, layout_text_size);
            }
        }
        let last = evaluate(&breaks, t.chars.len(), x, 0.0, true);
        let mut out = vec![last.index];
        let mut prior = last.prior;
        while let Some(p) = prior {
            out.push(breaks[p].index);
            prior = breaks[p].prior;
        }
        out.reverse();
        out
    }
}

/// `shapeText`. `None` when nothing could be placed (no glyphs available).
#[allow(clippy::too_many_arguments)]
pub fn shape_text(
    text: &Formatted,
    glyphs: &Glyphs,
    images: &HashMap<String, Rc<StyleImage>>,
    default_font_stack: &str,
    max_width: f64,
    line_height: f64,
    text_anchor: &str,
    text_justify: &str,
    spacing: f64,
    translate: [f64; 2],
    writing_mode: WritingMode,
    allow_vertical_placement: bool,
    layout_text_size: f64,
    layout_text_size_this_zoom: f64,
) -> Option<Shaping> {
    let mut ctx = Ctx { glyphs, images, sections: Vec::new() };
    let mut input = Tagged { chars: Vec::new(), section: Vec::new() };
    let mut image_cp = 0xE000u32;
    for s in &text.sections {
        let valign = match s.vertical_align.as_deref() {
            Some("top") => 0.0,
            Some("center") => 0.5,
            _ => 1.0,
        };
        let (kind, chars): (SectionKind, Vec<char>) = match &s.image {
            Some(img) if !img.name.is_empty() => {
                // Each image takes a private-use code point.
                let c = char::from_u32(image_cp).unwrap_or('\u{e000}');
                image_cp += 1;
                (SectionKind::Image { name: img.name.clone() }, vec![c])
            }
            Some(_) => continue,
            None => (SectionKind::Text { font_stack: s.font_stack.clone().unwrap_or_else(|| default_font_stack.to_string()) }, s.text.chars().collect()),
        };
        let scale = if matches!(kind, SectionKind::Image { .. }) { 1.0 } else { s.scale.filter(|&v| v != 0.0).unwrap_or(1.0) };
        ctx.sections.push(Section { scale, valign, kind });
        input.section.extend(std::iter::repeat_n(ctx.sections.len() - 1, chars.len()));
        input.chars.extend(chars);
    }
    if writing_mode == WritingMode::Vertical {
        // No font provides vertical glyph variants (PBF glyphs), so compatibility forms are used.
        input.chars = unicode::verticalize_punctuation(&input.chars);
    }
    // ponytail: no bidi reordering; right-to-left text is laid out in logical order.
    let breaks = ctx.line_breaks(&input, spacing, max_width, layout_text_size);
    let mut lines = Vec::new();
    let mut start = 0;
    for b in breaks {
        lines.push(Tagged { chars: input.chars[start..b].to_vec(), section: input.section[start..b].to_vec() });
        start = b;
    }
    if start < input.chars.len() {
        lines.push(Tagged { chars: input.chars[start..].to_vec(), section: input.section[start..].to_vec() });
    }
    let mut shaping = Shaping {
        lines: Vec::new(),
        top: translate[1],
        bottom: translate[1],
        left: translate[0],
        right: translate[0],
        text: input.chars.iter().collect(),
        icons_in_text: false,
        verticalizable: false,
    };
    shape_lines(&mut shaping, &mut ctx, lines, line_height, text_anchor, text_justify, writing_mode, allow_vertical_placement, spacing, layout_text_size_this_zoom);
    if shaping.lines.iter().all(|l| l.glyphs.is_empty()) {
        return None;
    }
    Some(shaping)
}

#[allow(clippy::too_many_arguments)]
fn shape_lines(
    shaping: &mut Shaping,
    ctx: &mut Ctx,
    lines: Vec<Tagged>,
    line_height: f64,
    text_anchor: &str,
    text_justify: &str,
    writing_mode: WritingMode,
    allow_vertical_placement: bool,
    spacing: f64,
    layout_text_size_this_zoom: f64,
) {
    let (mut x, mut y) = (0.0, 0.0);
    let (mut max_line_length, mut max_line_height): (f64, f64) = (0.0, 0.0);
    let justify = match text_justify {
        "right" => 1.0,
        "left" => 0.0,
        _ => 0.5,
    };
    let size_factor = ONE_EM / layout_text_size_this_zoom;
    let n_lines = lines.len();
    for mut line in lines {
        // trim()
        while line.chars.first().is_some_and(|&c| unicode::is_whitespace(c)) {
            line.chars.remove(0);
            line.section.remove(0);
        }
        while line.chars.last().is_some_and(|&c| unicode::is_whitespace(c)) {
            line.chars.pop();
            line.section.pop();
        }
        let line_max_scale = line.section.iter().map(|&s| ctx.sections[s].scale).fold(0.0, f64::max);
        let mut positioned = PositionedLine::default();
        let mut image_offset: f64 = 0.0;
        if line.chars.is_empty() {
            y += line_height;
            shaping.lines.push(positioned);
            continue;
        }
        // calculateLineContentSize
        let max_image = |k: usize| line.section.iter().filter_map(|&s| ctx.image(&ctx.sections[s]).map(|i| display_size(i)[k])).fold(0.0, f64::max);
        let content_height = (line_max_scale * ONE_EM).max(max_image(1) * size_factor);
        let content_width = (line_max_scale * ONE_EM).max(max_image(0) * size_factor);
        let line_verticals = (writing_mode == WritingMode::Vertical && !allow_vertical_placement).then(|| line_verticals(&mut line, &ctx.sections));
        for i in 0..line.chars.len() {
            let c = line.chars[i];
            let si = line.section[i];
            let vertical = match &line_verticals {
                Some(v) => v[i],
                None => is_line_vertical(writing_mode, allow_vertical_placement, c),
            };
            if matches!(ctx.sections[si].kind, SectionKind::Image { .. }) {
                shaping.icons_in_text = true;
                ctx.sections[si].scale *= size_factor;
            }
            let section = &ctx.sections[si];
            let (tex, metrics, baseline_offset) = match &section.kind {
                SectionKind::Text { .. } => {
                    let Some(g) = ctx.glyph(section, c) else { continue };
                    let offset = if vertical { content_width - section.scale * ONE_EM } else { (content_height - section.scale * ONE_EM) * section.valign };
                    (Tex::Glyph(g.clone()), g.metrics, offset)
                }
                SectionKind::Image { .. } => {
                    let Some(img) = ctx.image(section) else { continue };
                    let size = display_size(img);
                    let metrics = Metrics { width: size[0], height: size[1], left: IMAGE_PADDING, top: -BORDER as f64, advance: if vertical { size[1] } else { size[0] } };
                    let offset = if vertical { content_width - size[1] * section.scale } else { (content_height - size[1] * section.scale) * section.valign };
                    image_offset = image_offset.max((if vertical { size[0] } else { size[1] }) * section.scale - ONE_EM * line_max_scale);
                    (Tex::Image(img.clone()), metrics, offset)
                }
            };
            let is_image = matches!(tex, Tex::Image(_));
            positioned.glyphs.push(PositionedGlyph {
                x,
                y: y + SHAPING_DEFAULT_OFFSET + baseline_offset,
                vertical,
                scale: section.scale,
                section: si,
                metrics,
                tex,
            });
            if vertical {
                shaping.verticalizable = true;
                x += (if is_image { metrics.advance } else { ONE_EM }) * section.scale + spacing;
            } else {
                x += metrics.advance * section.scale + spacing;
            }
        }
        if !positioned.glyphs.is_empty() {
            max_line_length = max_line_length.max(x - spacing);
            // justifyLine
            if justify != 0.0 {
                let last = positioned.glyphs.last().unwrap();
                let indent = (last.x + last.metrics.advance * last.scale) * justify;
                for g in &mut positioned.glyphs {
                    g.x -= indent;
                }
            }
        }
        x = 0.0;
        positioned.line_offset = image_offset.max((line_max_scale - 1.0) * ONE_EM);
        let current_line_height = line_height * line_max_scale + image_offset;
        y += current_line_height;
        max_line_height = max_line_height.max(current_line_height);
        shaping.lines.push(positioned);
    }
    let (h_align, v_align) = anchor_alignment(text_anchor);
    // align()
    let shift_x = (justify - h_align) * max_line_length;
    let shift_y = if max_line_height != line_height {
        -y * v_align - SHAPING_DEFAULT_OFFSET
    } else {
        -v_align * n_lines as f64 * line_height + 0.5 * line_height
    };
    for g in shaping.lines.iter_mut().flat_map(|l| l.glyphs.iter_mut()) {
        g.x += shift_x;
        g.y += shift_y;
    }
    shaping.top += -v_align * y;
    shaping.bottom = shaping.top + y;
    shaping.left += -h_align * max_line_length;
    shaping.right = shaping.left + max_line_length;
}

/// `isLineVertical`.
fn is_line_vertical(mode: WritingMode, allow_vertical_placement: bool, c: char) -> bool {
    !(mode != WritingMode::Vertical
        || (!allow_vertical_placement && !unicode::has_upright_vertical_orientation(c))
        || (allow_vertical_placement && (unicode::is_whitespace(c) || unicode::is_arabic(c))))
}

/// `runIsUpright`: numbers (with symbols) and short uppercase codes stand upright.
fn run_is_upright(run: &[char]) -> bool {
    let number = run.iter().any(|&c| unicode::is_decimal_digit(c)) && run.iter().all(|&c| unicode::is_decimal_digit(c) || unicode::is_symbol_or_punctuation(c));
    let code = run.len() <= 3 && run.iter().all(|&c| unicode::is_uppercase_letter(c) || unicode::is_decimal_digit(c));
    number || code
}

fn upright_in_run(c: char) -> bool {
    (unicode::is_decimal_digit(c) || unicode::is_uppercase_letter(c)) && !unicode::is_arabic(c)
}

/// `determineLineVerticals` (line labels): which characters stand upright, replacing
/// punctuation surrounded by upright characters with its vertical form.
fn line_verticals(line: &mut Tagged, sections: &[Section]) -> Vec<bool> {
    let cps = line.chars.clone();
    let mut verticals: Vec<bool> = cps.iter().map(|&c| unicode::has_upright_vertical_orientation(c)).collect();
    let is_image = |i: usize| matches!(sections[line.section[i]].kind, SectionKind::Image { .. });
    let run_char = |v: &[bool], i: usize| !v[i] && !unicode::is_whitespace(cps[i]) && !is_image(i);
    let mut start = 0;
    while start < cps.len() {
        if !run_char(&verticals, start) {
            start += 1;
            continue;
        }
        let mut end = start;
        while end + 1 < cps.len() && run_char(&verticals, end + 1) {
            end += 1;
        }
        let upright = run_is_upright(&cps[start..=end]);
        for i in start..=end {
            verticals[i] = upright && upright_in_run(cps[i]);
        }
        start = end + 1;
    }
    // verticalizeSurroundedPunctuation
    for i in 0..cps.len() {
        if verticals[i] {
            continue;
        }
        let Some(v) = unicode::verticalized(line.chars[i]) else { continue };
        if (i == 0 || verticals[i - 1]) && (i == cps.len() - 1 || verticals[i + 1]) {
            verticals[i] = true;
            line.chars[i] = v;
        }
    }
    verticals
}

#[derive(Clone)]
pub struct PositionedIcon {
    pub image: Rc<StyleImage>,
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
    pub collision_padding: Option<[f64; 4]>,
}

/// `shapeIcon`.
pub fn shape_icon(image: Rc<StyleImage>, offset: [f64; 2], anchor: &str) -> PositionedIcon {
    let (h, v) = anchor_alignment(anchor);
    let size = display_size(&image);
    let x1 = offset[0] - size[0] * h;
    let y1 = offset[1] - size[1] * v;
    PositionedIcon { image, top: y1, bottom: y1 + size[1], left: x1, right: x1 + size[0], collision_padding: None }
}

/// `applyTextFit`: the icon box `[x1, y1, x2, y2]` honoring `textFitWidth/Height`.
pub fn apply_text_fit(icon: &PositionedIcon) -> [f64; 4] {
    let (mut left, mut top) = (icon.left, icon.top);
    let (mut width, mut height) = (icon.right - left, icon.bottom - top);
    let c = icon.image.content.unwrap_or([0.0; 4]);
    let (cw, ch) = (c[2] - c[0], c[3] - c[1]);
    let fit_w = icon.image.text_fit_width.as_deref().unwrap_or("stretchOrShrink");
    let fit_h = icon.image.text_fit_height.as_deref().unwrap_or("stretchOrShrink");
    let ratio = cw / ch;
    if fit_h == "proportional" {
        if (fit_w == "stretchOnly" && width / height < ratio) || fit_w == "proportional" {
            let w = (height * ratio).ceil();
            left *= w / width;
            width = w;
        }
    } else if fit_w == "proportional" && fit_h == "stretchOnly" && ratio != 0.0 && width / height > ratio {
        let h = (width / ratio).ceil();
        top *= h / height;
        height = h;
    }
    [left, top, left + width, top + height]
}

/// `fitIconToText`.
pub fn fit_icon_to_text(icon: &PositionedIcon, text: &Shaping, fit: &str, padding: [f64; 4], offset: [f64; 2], font_scale: f64) -> PositionedIcon {
    let image = icon.image.clone();
    let size = display_size(&image);
    let collision_padding = image.content.map(|c| {
        let r = image.pixel_ratio;
        [c[0] / r, c[1] / r, size[0] - c[2] / r, size[1] - c[3] / r]
    });
    let (tl, tr) = (text.left * font_scale, text.right * font_scale);
    let (left, right) = if fit == "width" || fit == "both" {
        (offset[0] + tl - padding[3], offset[0] + tr + padding[1])
    } else {
        let l = offset[0] + (tl + tr - size[0]) / 2.0;
        (l, l + size[0])
    };
    let (tt, tb) = (text.top * font_scale, text.bottom * font_scale);
    let (top, bottom) = if fit == "height" || fit == "both" {
        (offset[1] + tt - padding[0], offset[1] + tb + padding[2])
    } else {
        let t = offset[1] + (tt + tb - size[1]) / 2.0;
        (t, t + size[1])
    };
    PositionedIcon { image, top, right, bottom, left, collision_padding }
}
