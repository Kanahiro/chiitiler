//! Color (style-spec `types/color.ts`, `parse_css_color.ts`, `color_spaces.ts`).

use super::value::{js_number_to_string, js_round};

/// sRGB, premultiplied by alpha, components 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
    /// Original non-premultiplied components when constructed with alpha 0
    /// (otherwise they would be lost; needed for interpolation).
    orig: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    Rgb,
    Hcl,
    Lab,
}

impl Color {
    pub const BLACK: Color = Color::premultiplied(0.0, 0.0, 0.0, 1.0);
    pub const TRANSPARENT: Color = Color::premultiplied(0.0, 0.0, 0.0, 0.0);

    pub const fn premultiplied(r: f64, g: f64, b: f64, a: f64) -> Color {
        Color { r, g, b, a, orig: None }
    }

    pub fn new(r: f64, g: f64, b: f64, a: f64) -> Color {
        Color { r: r * a, g: g * a, b: b * a, a, orig: if a == 0.0 { Some([r, g, b, a]) } else { None } }
    }

    pub fn parse(input: &str) -> Option<Color> {
        parse_css_color(input).map(|[r, g, b, a]| Color::new(r, g, b, a))
    }

    /// Non-premultiplied `[r, g, b, a]`.
    pub fn rgb(&self) -> [f64; 4] {
        if let Some(o) = self.orig {
            return o;
        }
        let f = if self.a == 0.0 { f64::INFINITY } else { self.a };
        [self.r / f, self.g / f, self.b / f, self.a]
    }

    pub fn to_css_string(&self) -> String {
        let [r, g, b, a] = self.rgb();
        format!(
            "rgba({},{},{},{})",
            js_number_to_string(js_round(r * 255.0)),
            js_number_to_string(js_round(g * 255.0)),
            js_number_to_string(js_round(b * 255.0)),
            js_number_to_string(a)
        )
    }

    pub fn interpolate(from: &Color, to: &Color, t: f64, space: ColorSpace) -> Color {
        let lerp = |a: f64, b: f64| a + t * (b - a);
        match space {
            ColorSpace::Rgb => {
                let (f, o) = (from.rgb(), to.rgb());
                Color::new(lerp(f[0], o[0]), lerp(f[1], o[1]), lerp(f[2], o[2]), lerp(f[3], o[3]))
            }
            ColorSpace::Hcl => {
                let [h0, c0, l0, a0] = rgb_to_hcl(from.rgb());
                let [h1, c1, l1, a1] = rgb_to_hcl(to.rgb());
                let mut chroma = None;
                let hue = if !h0.is_nan() && !h1.is_nan() {
                    let mut dh = h1 - h0;
                    if h1 > h0 && dh > 180.0 {
                        dh -= 360.0;
                    } else if h1 < h0 && h0 - h1 > 180.0 {
                        dh += 360.0;
                    }
                    h0 + t * dh
                } else if !h0.is_nan() {
                    if l1 == 1.0 || l1 == 0.0 {
                        chroma = Some(c0);
                    }
                    h0
                } else if !h1.is_nan() {
                    if l0 == 1.0 || l0 == 0.0 {
                        chroma = Some(c1);
                    }
                    h1
                } else {
                    f64::NAN
                };
                let [r, g, b, a] = hcl_to_rgb([hue, chroma.unwrap_or(lerp(c0, c1)), lerp(l0, l1), lerp(a0, a1)]);
                Color::new(r, g, b, a)
            }
            ColorSpace::Lab => {
                let (f, o) = (rgb_to_lab(from.rgb()), rgb_to_lab(to.rgb()));
                let [r, g, b, a] = lab_to_rgb([lerp(f[0], o[0]), lerp(f[1], o[1]), lerp(f[2], o[2]), lerp(f[3], o[3])]);
                Color::new(r, g, b, a)
            }
        }
    }
}

const XN: f64 = 0.96422;
const YN: f64 = 1.0;
const ZN: f64 = 0.82521;
const T0: f64 = 4.0 / 29.0;
const T1: f64 = 6.0 / 29.0;
const T2: f64 = 3.0 * T1 * T1;
const T3: f64 = T1 * T1 * T1;

fn constrain_angle(a: f64) -> f64 {
    let a = a % 360.0;
    if a < 0.0 { a + 360.0 } else { a }
}

fn rgb_to_lab([r, g, b, alpha]: [f64; 4]) -> [f64; 4] {
    let rgb2xyz = |x: f64| if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) };
    let xyz2lab = |t: f64| if t > T3 { t.powf(1.0 / 3.0) } else { t / T2 + T0 };
    let (r, g, b) = (rgb2xyz(r), rgb2xyz(g), rgb2xyz(b));
    let y = xyz2lab((0.2225045 * r + 0.7168786 * g + 0.0606169 * b) / YN);
    let (x, z) = if r == g && g == b {
        (y, y)
    } else {
        (
            xyz2lab((0.4360747 * r + 0.3850649 * g + 0.1430804 * b) / XN),
            xyz2lab((0.0139322 * r + 0.0971045 * g + 0.7141733 * b) / ZN),
        )
    };
    let l = 116.0 * y - 16.0;
    [if l < 0.0 { 0.0 } else { l }, 500.0 * (x - y), 200.0 * (y - z), alpha]
}

fn lab_to_rgb([l, a, b, alpha]: [f64; 4]) -> [f64; 4] {
    let lab2xyz = |t: f64| if t > T1 { t * t * t } else { T2 * (t - T0) };
    let xyz2rgb = |x: f64| {
        let x = if x <= 0.00304 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        x.clamp(0.0, 1.0)
    };
    let y = (l + 16.0) / 116.0;
    let x = if a.is_nan() { y } else { y + a / 500.0 };
    let z = if b.is_nan() { y } else { y - b / 200.0 };
    let (y, x, z) = (YN * lab2xyz(y), XN * lab2xyz(x), ZN * lab2xyz(z));
    [
        xyz2rgb(3.1338561 * x - 1.6168667 * y - 0.4906146 * z),
        xyz2rgb(-0.9787684 * x + 1.9161415 * y + 0.033454 * z),
        xyz2rgb(0.0719453 * x - 0.2289914 * y + 1.4052427 * z),
        alpha,
    ]
}

fn rgb_to_hcl(rgb: [f64; 4]) -> [f64; 4] {
    let [l, a, b, alpha] = rgb_to_lab(rgb);
    let c = (a * a + b * b).sqrt();
    let h = if js_round(c * 10000.0) != 0.0 { constrain_angle(b.atan2(a).to_degrees()) } else { f64::NAN };
    [h, c, l, alpha]
}

fn hcl_to_rgb([h, c, l, alpha]: [f64; 4]) -> [f64; 4] {
    let h = if h.is_nan() { 0.0 } else { h.to_radians() };
    lab_to_rgb([l, h.cos() * c, h.sin() * c, alpha])
}

fn hsl_to_rgb([h, s, l, alpha]: [f64; 4]) -> [f64; 4] {
    let h = constrain_angle(h);
    let (s, l) = (s / 100.0, l / 100.0);
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (-1f64).max((k - 3.0).min(9.0 - k).min(1.0))
    };
    [f(0.0), f(8.0), f(4.0), alpha]
}

/// Non-premultiplied `[r, g, b, a]` in 0..1.
pub fn parse_css_color(input: &str) -> Option<[f64; 4]> {
    let input = input.to_lowercase();
    let input = input.trim();
    if input == "transparent" {
        return Some([0.0, 0.0, 0.0, 0.0]);
    }
    if let Some([r, g, b]) = named_color(input) {
        return Some([r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, 1.0]);
    }
    if let Some(hex) = input.strip_prefix('#')
        && matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            let step = if hex.len() < 6 { 1 } else { 2 };
            let ch = |i: usize| -> f64 {
                let s = hex.get(i * step..i * step + step).unwrap_or("ff");
                let s = if s.len() == 1 { s.repeat(2) } else { s.to_string() };
                u8::from_str_radix(&s, 16).unwrap() as f64 / 255.0
            };
            return Some([ch(0), ch(1), ch(2), ch(3)]);
        }
    let func = |name: &str| -> Option<Vec<(String, bool)>> {
        // Returns args with separators: ("value", is_percent) and ("," | "/" | " ") markers.
        let rest = input.strip_prefix(name)?;
        let rest = rest.strip_prefix('a').unwrap_or(rest);
        let body = rest.strip_prefix('(')?.strip_suffix(')')?;
        tokenize(body)
    };
    if input.starts_with("rgb") {
        let toks = func("rgb")?;
        let (vals, seps) = split_tokens(&toks)?;
        if vals.len() != 3 && vals.len() != 4 {
            return None;
        }
        check_separators(&seps, vals.len())?;
        let pcts: String = vals[..3].iter().map(|(_, p)| if *p { '%' } else { ' ' }).collect();
        let max = match pcts.as_str() {
            "%%%" => 100.0,
            "   " => 255.0,
            _ => return None,
        };
        let n = |i: usize| js_parse_float(&vals[i].0);
        let rgba = [
            (n(0)? / max).clamp(0.0, 1.0),
            (n(1)? / max).clamp(0.0, 1.0),
            (n(2)? / max).clamp(0.0, 1.0),
            if vals.len() == 4 { parse_alpha(n(3)?, vals[3].1) } else { 1.0 },
        ];
        return if rgba.iter().any(|v| v.is_nan()) { None } else { Some(rgba) };
    }
    if input.starts_with("hsl") {
        let mut toks = func("hsl")?;
        // Hue may carry a `deg` suffix.
        if let Some((v, false)) = toks.first_mut()
            && let Some(stripped) = v.strip_suffix("deg")
        {
            *v = stripped.to_string();
        }
        let (vals, seps) = split_tokens(&toks)?;
        if vals.len() != 3 && vals.len() != 4 {
            return None;
        }
        check_separators(&seps, vals.len())?;
        if vals[0].1 || !vals[1].1 || !vals[2].1 {
            return None;
        }
        let n = |i: usize| js_parse_float(&vals[i].0);
        let hsla = [
            n(0)?,
            n(1)?.clamp(0.0, 100.0),
            n(2)?.clamp(0.0, 100.0),
            if vals.len() == 4 { parse_alpha(n(3)?, vals[3].1) } else { 1.0 },
        ];
        return if hsla.iter().any(|v| v.is_nan()) { None } else { Some(hsl_to_rgb(hsla)) };
    }
    None
}

fn parse_alpha(a: f64, pct: bool) -> f64 {
    (if pct { a / 100.0 } else { a }).clamp(0.0, 1.0)
}

/// Upstream accepts `[\de.+-]+` then converts with unary `+`.
fn js_parse_float(s: &str) -> Option<f64> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'e' | b'.' | b'+' | b'-')) {
        return None;
    }
    Some(super::value::js_string_to_number(s))
}

/// Tokens: numbers (with `%` flag) and separators `,` / `/` (whitespace collapsed to " ").
fn tokenize(body: &str) -> Option<Vec<(String, bool)>> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut cur = String::new();
    let mut pending_space = false;
    let flush = |cur: &mut String, out: &mut Vec<(String, bool)>| {
        if !cur.is_empty() {
            let pct = cur.ends_with('%');
            let v = cur.trim_end_matches('%').to_string();
            if v.contains('%') {
                return false;
            }
            out.push((v, pct));
            cur.clear();
        }
        true
    };
    for c in body.chars() {
        match c {
            ' ' | '\t' | '\n' | '\r' | '\x0c' => {
                if !flush(&mut cur, &mut out) {
                    return None;
                }
                pending_space = true;
            }
            ',' | '/' => {
                if !flush(&mut cur, &mut out) {
                    return None;
                }
                out.push((c.to_string(), false));
                pending_space = false;
            }
            _ => {
                if pending_space && !cur.is_empty() {
                    return None;
                }
                if pending_space && out.last().is_some_and(|(v, _)| v != "," && v != "/") {
                    out.push((" ".into(), false));
                }
                pending_space = false;
                cur.push(c);
            }
        }
    }
    flush(&mut cur, &mut out).then_some(out)
}

#[allow(clippy::type_complexity)]
fn split_tokens(toks: &[(String, bool)]) -> Option<(Vec<(String, bool)>, Vec<String>)> {
    let mut vals = Vec::new();
    let mut seps = Vec::new();
    let mut expect_val = true;
    for (t, p) in toks {
        let is_sep = t == "," || t == "/" || t == " ";
        if is_sep == expect_val {
            return None;
        }
        if is_sep {
            seps.push(t.clone());
        } else {
            vals.push((t.clone(), *p));
        }
        expect_val = !expect_val;
    }
    if expect_val {
        return None;
    }
    Some((vals, seps))
}

fn check_separators(seps: &[String], n: usize) -> Option<()> {
    let s: Vec<&str> = seps.iter().map(String::as_str).collect();
    let ok = match n {
        3 => s == [" ", " "] || s == [",", ","],
        _ => s == [" ", " ", "/"] || s == [",", ",", ","],
    };
    ok.then_some(())
}

fn named_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "aliceblue" => [240, 248, 255],
        "antiquewhite" => [250, 235, 215],
        "aqua" => [0, 255, 255],
        "aquamarine" => [127, 255, 212],
        "azure" => [240, 255, 255],
        "beige" => [245, 245, 220],
        "bisque" => [255, 228, 196],
        "black" => [0, 0, 0],
        "blanchedalmond" => [255, 235, 205],
        "blue" => [0, 0, 255],
        "blueviolet" => [138, 43, 226],
        "brown" => [165, 42, 42],
        "burlywood" => [222, 184, 135],
        "cadetblue" => [95, 158, 160],
        "chartreuse" => [127, 255, 0],
        "chocolate" => [210, 105, 30],
        "coral" => [255, 127, 80],
        "cornflowerblue" => [100, 149, 237],
        "cornsilk" => [255, 248, 220],
        "crimson" => [220, 20, 60],
        "cyan" => [0, 255, 255],
        "darkblue" => [0, 0, 139],
        "darkcyan" => [0, 139, 139],
        "darkgoldenrod" => [184, 134, 11],
        "darkgray" => [169, 169, 169],
        "darkgreen" => [0, 100, 0],
        "darkgrey" => [169, 169, 169],
        "darkkhaki" => [189, 183, 107],
        "darkmagenta" => [139, 0, 139],
        "darkolivegreen" => [85, 107, 47],
        "darkorange" => [255, 140, 0],
        "darkorchid" => [153, 50, 204],
        "darkred" => [139, 0, 0],
        "darksalmon" => [233, 150, 122],
        "darkseagreen" => [143, 188, 143],
        "darkslateblue" => [72, 61, 139],
        "darkslategray" => [47, 79, 79],
        "darkslategrey" => [47, 79, 79],
        "darkturquoise" => [0, 206, 209],
        "darkviolet" => [148, 0, 211],
        "deeppink" => [255, 20, 147],
        "deepskyblue" => [0, 191, 255],
        "dimgray" => [105, 105, 105],
        "dimgrey" => [105, 105, 105],
        "dodgerblue" => [30, 144, 255],
        "firebrick" => [178, 34, 34],
        "floralwhite" => [255, 250, 240],
        "forestgreen" => [34, 139, 34],
        "fuchsia" => [255, 0, 255],
        "gainsboro" => [220, 220, 220],
        "ghostwhite" => [248, 248, 255],
        "gold" => [255, 215, 0],
        "goldenrod" => [218, 165, 32],
        "gray" => [128, 128, 128],
        "green" => [0, 128, 0],
        "greenyellow" => [173, 255, 47],
        "grey" => [128, 128, 128],
        "honeydew" => [240, 255, 240],
        "hotpink" => [255, 105, 180],
        "indianred" => [205, 92, 92],
        "indigo" => [75, 0, 130],
        "ivory" => [255, 255, 240],
        "khaki" => [240, 230, 140],
        "lavender" => [230, 230, 250],
        "lavenderblush" => [255, 240, 245],
        "lawngreen" => [124, 252, 0],
        "lemonchiffon" => [255, 250, 205],
        "lightblue" => [173, 216, 230],
        "lightcoral" => [240, 128, 128],
        "lightcyan" => [224, 255, 255],
        "lightgoldenrodyellow" => [250, 250, 210],
        "lightgray" => [211, 211, 211],
        "lightgreen" => [144, 238, 144],
        "lightgrey" => [211, 211, 211],
        "lightpink" => [255, 182, 193],
        "lightsalmon" => [255, 160, 122],
        "lightseagreen" => [32, 178, 170],
        "lightskyblue" => [135, 206, 250],
        "lightslategray" => [119, 136, 153],
        "lightslategrey" => [119, 136, 153],
        "lightsteelblue" => [176, 196, 222],
        "lightyellow" => [255, 255, 224],
        "lime" => [0, 255, 0],
        "limegreen" => [50, 205, 50],
        "linen" => [250, 240, 230],
        "magenta" => [255, 0, 255],
        "maroon" => [128, 0, 0],
        "mediumaquamarine" => [102, 205, 170],
        "mediumblue" => [0, 0, 205],
        "mediumorchid" => [186, 85, 211],
        "mediumpurple" => [147, 112, 219],
        "mediumseagreen" => [60, 179, 113],
        "mediumslateblue" => [123, 104, 238],
        "mediumspringgreen" => [0, 250, 154],
        "mediumturquoise" => [72, 209, 204],
        "mediumvioletred" => [199, 21, 133],
        "midnightblue" => [25, 25, 112],
        "mintcream" => [245, 255, 250],
        "mistyrose" => [255, 228, 225],
        "moccasin" => [255, 228, 181],
        "navajowhite" => [255, 222, 173],
        "navy" => [0, 0, 128],
        "oldlace" => [253, 245, 230],
        "olive" => [128, 128, 0],
        "olivedrab" => [107, 142, 35],
        "orange" => [255, 165, 0],
        "orangered" => [255, 69, 0],
        "orchid" => [218, 112, 214],
        "palegoldenrod" => [238, 232, 170],
        "palegreen" => [152, 251, 152],
        "paleturquoise" => [175, 238, 238],
        "palevioletred" => [219, 112, 147],
        "papayawhip" => [255, 239, 213],
        "peachpuff" => [255, 218, 185],
        "peru" => [205, 133, 63],
        "pink" => [255, 192, 203],
        "plum" => [221, 160, 221],
        "powderblue" => [176, 224, 230],
        "purple" => [128, 0, 128],
        "rebeccapurple" => [102, 51, 153],
        "red" => [255, 0, 0],
        "rosybrown" => [188, 143, 143],
        "royalblue" => [65, 105, 225],
        "saddlebrown" => [139, 69, 19],
        "salmon" => [250, 128, 114],
        "sandybrown" => [244, 164, 96],
        "seagreen" => [46, 139, 87],
        "seashell" => [255, 245, 238],
        "sienna" => [160, 82, 45],
        "silver" => [192, 192, 192],
        "skyblue" => [135, 206, 235],
        "slateblue" => [106, 90, 205],
        "slategray" => [112, 128, 144],
        "slategrey" => [112, 128, 144],
        "snow" => [255, 250, 250],
        "springgreen" => [0, 255, 127],
        "steelblue" => [70, 130, 180],
        "tan" => [210, 180, 140],
        "teal" => [0, 128, 128],
        "thistle" => [216, 191, 216],
        "tomato" => [255, 99, 71],
        "turquoise" => [64, 224, 208],
        "violet" => [238, 130, 238],
        "wheat" => [245, 222, 179],
        "white" => [255, 255, 255],
        "whitesmoke" => [245, 245, 245],
        "yellow" => [255, 255, 0],
        "yellowgreen" => [154, 205, 50],
        _ => return None,
    })
}
