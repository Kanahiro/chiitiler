//! raster-dem layers: `DEMData` (2px padded, backfilled from neighbors); hillshade (the
//! `hillshadePrepare` pass into an 8-bit slope texture, in f32 like the GPU, and the `hillshade`
//! shader with its five methods); color-relief (elevation stops and color ramp textures).

use super::raster::{self, Mask, Rgba, Target};
use super::{color, num, Ctx, EXTENT};
use crate::expression::Value;
use crate::image::{Filter, Image, Texture};
use crate::style::Layer;
use crate::tile::RenderTile;
use std::cell::OnceCell;
use std::f32::consts::PI;

/// Decoded DEM tile: RGB texels with a 2px border (edge-replicated until backfilled).
#[derive(Clone)]
pub struct Dem {
    dim: usize,
    stride: usize,
    data: Vec<[u8; 4]>,
    /// red, green, blue factors and base shift.
    unpack: [f32; 4],
}

impl Dem {
    /// `encoding`: "mapbox" (default), "terrarium" or "custom" with `factors`
    /// (red, green, blue, baseShift).
    pub fn new(img: &Image, encoding: &str, factors: [f64; 4]) -> Option<Dem> {
        if img.w != img.h || img.w == 0 {
            return None;
        }
        let unpack = match encoding {
            "terrarium" => [256.0, 1.0, 1.0 / 256.0, 32768.0],
            "custom" => factors.map(|f| f as f32),
            _ => [6553.6, 25.6, 0.1, 10000.0],
        };
        let dim = img.w;
        let stride = dim + 4;
        let mut data = Vec::with_capacity(stride * stride);
        for y in 0..stride {
            for x in 0..stride {
                let (sx, sy) = ((x as isize - 2).clamp(0, dim as isize - 1) as usize, (y as isize - 2).clamp(0, dim as isize - 1) as usize);
                let i = (sy * dim + sx) * 4;
                data.push([img.data[i], img.data[i + 1], img.data[i + 2], img.data[i + 3]]);
            }
        }
        Some(Dem { dim, stride, data, unpack })
    }

    fn idx(&self, x: isize, y: isize) -> usize {
        ((y + 2) as usize) * self.stride + (x + 2) as usize
    }

    /// `backfillBorder`: copy the neighbor's edge into this tile's border (`dx`, `dy` in -1..=1).
    pub fn backfill(&mut self, border: &Dem, dx: isize, dy: isize) {
        if self.dim != border.dim {
            return;
        }
        let d = self.dim as isize;
        let (mut x0, mut x1, mut y0, mut y1) = (dx * d, dx * d + d, dy * d, dy * d + d);
        match dx {
            -1 => x0 = x1 - 2,
            1 => x1 = x0 + 2,
            _ => {}
        }
        match dy {
            -1 => y0 = y1 - 2,
            1 => y1 = y0 + 2,
            _ => {}
        }
        let (ox, oy) = (-dx * d, -dy * d);
        for y in y0..y1 {
            for x in x0..x1 {
                let v = border.data[border.idx(x + ox, y + oy)];
                let i = self.idx(x, y);
                self.data[i] = v;
            }
        }
    }

    /// Side in texels, without the border.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Elevation (meters) at a position in texels from the tile's top-left corner, interpolated
    /// bilinearly between texel centers, across the border near the edges.
    pub fn elevation_at(&self, x: f64, y: f64) -> f64 {
        let u = self.unpack;
        let at = |x: isize, y: isize| {
            let p = self.data[self.idx(x, y)];
            p[0] as f64 * u[0] as f64 + p[1] as f64 * u[1] as f64 + p[2] as f64 * u[2] as f64 - u[3] as f64
        };
        let max = self.dim as f64 + 1.0 - 1e-9;
        let (tx, ty) = ((x - 0.5).clamp(-2.0, max - 1.0), (y - 0.5).clamp(-2.0, max - 1.0));
        let (x0, y0) = (tx.floor(), ty.floor());
        let (fx, fy) = (tx - x0, ty - y0);
        let (x0, y0) = (x0 as isize, y0 as isize);
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
        let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    /// The `hillshadePrepare` pass: slopes as `(dx, dy)` unorm8, `(dim+2)²` texels.
    pub fn prepare(&self, zoom: f64) -> Texture {
        let size = self.dim + 2;
        let u = self.unpack;
        let elevations: Vec<f32> = self.data.iter().map(|p| p[0] as f32 * u[0] + p[1] as f32 * u[1] + p[2] as f32 * u[2] - u[3]).collect();
        let elevation = |x: usize, y: usize| elevations[y * self.stride + x];
        let zoom = zoom as f32;
        let factor = if zoom < 2.0 { 0.4 } else if zoom < 4.5 { 0.35 } else { 0.3 };
        let exaggeration = if zoom < 15.0 { (zoom - 15.0) * factor } else { 0.0 };
        // log2 of the mercator world width in meters.
        let scale = self.dim as f32 / (exaggeration + (3.0 + 25.2562 - zoom)).exp2();
        let unorm = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let mut data = Vec::with_capacity(size * size * 4);
        for py in 0..size {
            for px in 0..size {
                // Texel (px+1, py+1) of the padded DEM and its eight neighbors.
                let e = |dx: usize, dy: usize| elevation(px + dx, py + dy);
                let (a, b, c) = (e(0, 0), e(1, 0), e(2, 0));
                let (d, f) = (e(0, 1), e(2, 1));
                let (g, h, i) = (e(0, 2), e(1, 2), e(2, 2));
                let dx = ((c + f + f + i) - (a + d + d + g)) * scale;
                let dy = ((g + h + h + i) - (a + b + b + c)) * scale;
                data.extend_from_slice(&[unorm(dx / 8.0 + 128.0 / 255.0), unorm(dy / 8.0 + 128.0 / 255.0), 255, 255]);
            }
        }
        Texture::new(&Image { w: size, h: size, data }, false)
    }
}

/// A drawn tile's DEM after `backfillDEM`, shared by its source's layers in one render; the
/// hillshade slopes are prepared on first use (`needsHillshadePrepare`).
pub struct DemTile {
    pub dem: Dem,
    slopes: OnceCell<Texture>,
}

impl DemTile {
    pub fn new(dem: Dem) -> DemTile {
        DemTile { dem, slopes: OnceCell::new() }
    }

    pub fn slopes(&self, zoom: f64) -> &Texture {
        self.slopes.get_or_init(|| self.dem.prepare(zoom))
    }
}

/// `hillshadeUniformValues`.
pub struct HillshadeParams {
    method: u8,
    exaggeration: f32,
    accent: Rgba,
    /// The first light's azimuth + π (standard, igor).
    azimuth: f32,
    /// Per light: sin(altitude), cos(altitude), and the azimuth's cos, sin as used by `cang`.
    lights: Vec<[f32; 4]>,
    /// Standard: the exaggeration's power base and `base^(π/2) - 1`.
    base: f32,
    base_max: f32,
    shadows: Vec<Rgba>,
    highlights: Vec<Rgba>,
    /// `NUM_ILLUMINATION_SOURCES`: the number of highlight colors.
    sources: usize,
    filter: Filter,
}

impl HillshadeParams {
    pub fn new(layer: &Layer, ctx: &Ctx) -> HillshadeParams {
        let t = super::dummy_tile();
        let p = |n: &str| ctx.paint(layer, n, None, &t);
        let numbers = |v: Value| match v {
            Value::NumberArray(a) => a.to_vec(),
            Value::Number(n) => vec![n],
            _ => vec![],
        };
        let colors = |v: Value| match v {
            Value::ColorArray(a) => a.iter().map(|c| color(&Value::Color(*c), 1.0)).collect(),
            Value::Color(_) => vec![color(&v, 1.0)],
            _ => vec![],
        };
        let mut direction = numbers(p("hillshade-illumination-direction"));
        let mut altitude = numbers(p("hillshade-illumination-altitude"));
        let mut highlights: Vec<Rgba> = colors(p("hillshade-highlight-color"));
        let mut shadows: Vec<Rgba> = colors(p("hillshade-shadow-color"));
        let sources = highlights.len().max(1);
        let n = direction.len().max(altitude.len()).max(highlights.len()).max(shadows.len()).max(1);
        fn pad<T: Copy>(v: &mut Vec<T>, n: usize, fallback: T) {
            let last = v.last().copied().unwrap_or(fallback);
            v.resize(n, last);
        }
        pad(&mut direction, n, 335.0);
        pad(&mut altitude, n, 45.0);
        pad(&mut highlights, n, [1.0; 4]);
        pad(&mut shadows, n, [0.0, 0.0, 0.0, 1.0]);
        let viewport = p("hillshade-illumination-anchor").as_str() != Some("map");
        let bearing = -ctx.transform.bearing_rad;
        let azimuths: Vec<f32> = direction.iter().map(|d| (d.to_radians() + if viewport { bearing } else { 0.0 }) as f32).collect();
        let altitudes: Vec<f32> = altitude.iter().map(|a| a.to_radians() as f32).collect();
        let method = match p("hillshade-method").as_str() {
            Some("basic") => 4,
            Some("combined") => 1,
            Some("igor") => 2,
            Some("multidirectional") => 3,
            _ => 0,
        };
        let azimuth = azimuths[0] + PI;
        let lights = (0..if method == 3 { sources } else { 1 })
            .map(|i| {
                let (cos_az, sin_az) = if method == 3 { (-azimuths[i].cos(), -azimuths[i].sin()) } else { (azimuth.cos(), azimuth.sin()) };
                [altitudes[i].sin(), altitudes[i].cos(), cos_az, sin_az]
            })
            .collect();
        let exaggeration = num(&p("hillshade-exaggeration")) as f32;
        let base = 1.875 - exaggeration * 1.75;
        HillshadeParams {
            method,
            exaggeration,
            accent: color(&p("hillshade-accent-color"), 1.0),
            azimuth,
            lights,
            base,
            base_max: base.powf(0.5 * PI) - 1.0,
            shadows,
            highlights,
            sources,
            filter: if p("resampling").as_str() == Some("nearest") { Filter::Nearest } else { Filter::Linear },
        }
    }

    /// The fragment shader for slopes `deriv`.
    fn shade(&self, deriv: [f32; 2]) -> Rgba {
        let mix = |a: Rgba, b: Rgba, t: f32| -> Rgba { std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t) };
        let scale = |c: Rgba, s: f32| -> Rgba { c.map(|v| v * s) };
        let add = |a: Rgba, b: Rgba| -> Rgba { std::array::from_fn(|k| a[k] + b[k]) };
        let len = |d: [f32; 2]| (d[0] * d[0] + d[1] * d[1]).sqrt();
        let aspect = |d: [f32; 2]| if d[0] != 0.0 { d[1].atan2(-d[0]) } else { PI / 2.0 * if d[1] > 0.0 { 1.0 } else { -1.0 } };
        // GLSL `mod`.
        let gmod = |x: f32, y: f32| x - y * (x / y).floor();
        let boosted = [deriv[0] * self.exaggeration * 2.0, deriv[1] * self.exaggeration * 2.0];
        let cang = |d: [f32; 2], [sin_alt, cos_alt, cos_az, sin_az]: [f32; 4]| {
            (sin_alt - (d[1] * cos_az * cos_alt - d[0] * sin_az * cos_alt)) / (1.0 + d[0] * d[0] + d[1] * d[1]).sqrt()
        };
        let lit = |shade: f32, hi: Rgba, sh: Rgba| if shade > 0.5 { scale(hi, 2.0 * shade - 1.0) } else { scale(sh, 1.0 - 2.0 * shade) };
        match self.method {
            // igor
            2 => {
                let d = boosted;
                let slope = len(d).atan() * 2.0 / PI;
                let aspect_strength = 1.0 - (gmod((aspect(d) + self.azimuth) / PI + 0.5, 2.0) - 1.0).abs();
                add(scale(self.shadows[0], slope * aspect_strength), scale(self.highlights[0], slope * (1.0 - aspect_strength)))
            }
            // basic
            4 => {
                let shade = cang(boosted, self.lights[0]).clamp(0.0, 1.0);
                lit(shade, self.highlights[0], self.shadows[0])
            }
            // multidirectional
            3 => {
                let n = self.sources as f32;
                let mut out = [0.0; 4];
                for i in 0..self.sources {
                    let shade = cang(boosted, self.lights[i]).clamp(0.0, 1.0);
                    out = add(out, scale(lit(shade, self.highlights[i], self.shadows[i]), 1.0 / n));
                }
                out
            }
            // combined
            1 => {
                let c = cang(boosted, self.lights[0]).acos().clamp(0.0, PI / 2.0);
                let s = len(boosted).atan() * 4.0 / PI / PI;
                add(scale(self.shadows[0], c * s), scale(self.highlights[0], (PI / 2.0 - c) * s))
            }
            // standard
            _ => {
                let slope = (0.625 * len(deriv)).atan();
                let intensity = self.exaggeration;
                let max = 0.5 * PI;
                let scaled = if intensity != 0.5 { ((self.base.powf(slope) - 1.0) / self.base_max) * max } else { slope };
                let k = (intensity * 2.0).clamp(0.0, 1.0);
                let accent = scale(self.accent, (1.0 - scaled.cos()) * k);
                let shade = (gmod((aspect(deriv) + self.azimuth) / PI + 0.5, 2.0) - 1.0).abs();
                let shade_color = scale(mix(self.shadows[0], self.highlights[0], shade), scaled.sin() * k);
                add(scale(accent, 1.0 - shade_color[3]), shade_color)
            }
        }
    }
}

/// Draw a tile quad, pixel-aligned like rasters; `shade` gets the tile fraction (`a_pos / 8192`).
fn draw_quad(target: &mut Target, ctx: &Ctx, tile: &RenderTile, mask: Option<&Mask>, shade: &mut impl FnMut(&[f32; 2]) -> Option<Rgba>) {
    let to_world = ctx.tile_to_world(tile);
    let off = ctx.transform.aligned_offset();
    let screen = |p: [f64; 2]| {
        let w = to_world(p);
        ctx.transform.world_to_screen([w[0] + off[0], w[1] + off[1]])
    };
    let corners = [[0.0, 0.0], [EXTENT, 0.0], [0.0, EXTENT], [EXTENT, EXTENT]];
    let pts = corners.map(screen);
    let uv = corners.map(|c| [(c[0] / EXTENT) as f32, (c[1] / EXTENT) as f32]);
    for tri in [[1, 0, 2], [1, 2, 3]] {
        raster::triangle(target, mask, tri.map(|i| pts[i]), tri.map(|i| uv[i]), shade);
    }
}

/// Draw one tile's hillshade, preparing its slopes only if a pixel is drawn (a tile in the
/// cropped margin only lends its border to its neighbors).
pub fn draw_hillshade_tile(target: &mut Target, ctx: &Ctx, tile: &RenderTile, dem: &DemTile, params: &HillshadeParams, mask: Option<&Mask>) {
    // `u_latrange`: latitudes of the tile's top and bottom edges.
    let n = (1u64 << tile.canonical.z) as f64;
    let lat = |y: f64| crate::tile::mercator_lat(y / n);
    let (lat0, lat1) = (lat(tile.canonical.y as f64) as f32, lat(tile.canonical.y as f64 + 1.0) as f32);
    // The slopes texture is `dim + 2` texels square.
    let (sw, sh) = ((dem.dem.dim + 2) as f32, (dem.dem.dim + 2) as f32);
    // Raster sources are not reparsed when overzoomed: their tiles keep `overscaledZ` = the
    // canonical zoom.
    let zoom = tile.canonical.z as f64;
    draw_quad(target, ctx, tile, mask, &mut |v: &[f32; 2]| {
        let tp = [(v[0] * (sw - 2.0) + 1.0) / sw, (v[1] * (sh - 2.0) + 1.0) / sh];
        let px = dem.slopes(zoom).sample(tp[0], tp[1], params.filter, params.filter, 0.0);
        let scale_factor = ((lat0 - lat1) * (1.0 - v[1]) + lat1).to_radians().cos();
        let deriv = [(px[0] - 128.0 / 255.0) * 8.0 / scale_factor, (px[1] - 128.0 / 255.0) * 8.0 / scale_factor];
        Some(params.shade(deriv))
    });
}

/// `color-relief-color` as a ramp: elevation stops and their colors (`_createColorRamp`), only
/// from a top-level `interpolate` (anything else draws transparent).
pub struct ColorRamp {
    elevations: Vec<f64>,
    /// Premultiplied on upload, from unpremultiplied bytes (`RGBAImage.setPixel`).
    colors: Vec<[u8; 4]>,
    opacity: f32,
    filter: Filter,
}

/// `RGBAImage.setPixel` (unpremultiplied bytes) followed by the premultiplying upload.
pub fn ramp_texel(c: &crate::expression::color::Color) -> [u8; 4] {
    let a = (c.a * 255.0).round();
    let un = |v: f64| if c.a == 0.0 { 0.0 } else { (v * 255.0 / c.a).round() };
    let pm = |v: f64| (un(v) * a / 255.0).round().clamp(0.0, 255.0) as u8;
    [pm(c.r), pm(c.g), pm(c.b), a as u8]
}

impl ColorRamp {
    pub fn new(layer: &Layer, ctx: &Ctx) -> ColorRamp {
        let t = super::dummy_tile();
        let prop = layer.paint("color-relief-color");
        let mut elevations = Vec::new();
        let mut colors = Vec::new();
        if let crate::style::PropValue::Expr(e) = prop
            && let (true, Some(labels)) = (e.is_zoom_constant(), e.interpolate_labels()) {
                for &l in labels {
                    let g = crate::expression::Globals { zoom: ctx.transform.zoom, elevation: Some(l), global_state: Some(ctx.global_state.clone()), ..Default::default() };
                    let c = match prop.eval_globals(g) {
                        Value::Color(c) => c,
                        _ => crate::expression::color::Color::TRANSPARENT,
                    };
                    elevations.push(l);
                    colors.push(ramp_texel(&c));
                }
            }
        if elevations.is_empty() {
            elevations.push(0.0);
            colors.push([0; 4]);
        }
        if elevations.len() < 2 {
            elevations.push(elevations[0] + 1.0);
            colors.push(colors[0]);
        }
        ColorRamp {
            elevations,
            colors,
            opacity: num(&ctx.paint(layer, "color-relief-opacity", None, &t)) as f32,
            filter: if ctx.paint(layer, "resampling", None, &t).as_str() == Some("nearest") { Filter::Nearest } else { Filter::Linear },
        }
    }
}

/// `packDEMData`: an elevation in the DEM's 8-bit encoding.
fn pack(v: f64, u: [f32; 4]) -> [u8; 3] {
    let f = u.map(|x| x as f64);
    let min = [f[0], f[1], f[2]].into_iter().filter(|&x| x != 0.0).fold(f64::INFINITY, f64::min);
    if min == f64::INFINITY {
        return [0; 3];
    }
    let scaled = ((v + f[3]) / min).round();
    let ch = |factor: f64| if factor == 0.0 { 0 } else { ((scaled * min / factor).floor() % 256.0) as i64 as u8 };
    [ch(f[0]), ch(f[1]), ch(f[2])]
}

/// Draw one tile of a color-relief layer.
pub fn draw_color_relief_tile(target: &mut Target, ctx: &Ctx, tile: &RenderTile, dem: &Dem, ramp: &ColorRamp, mask: Option<&Mask>) {
    let u = dem.unpack;
    let unpack = |p: [f32; 3]| p[0] * 255.0 * u[0] + p[1] * 255.0 * u[1] + p[2] * 255.0 * u[2] - u[3];
    // Elevation stops round-trip through the 8-bit encoding.
    let stops: Vec<f32> = ramp.elevations.iter().map(|&e| pack(e, u).map(|b| b as f32 / 255.0)).map(unpack).collect();
    let n = stops.len();
    let stride = dem.stride as f32;
    // The DEM texture (`premultiply: false`), filtered on the encoded bytes.
    let texel = |x: i64, y: i64| {
        let (x, y) = (x.clamp(0, dem.stride as i64 - 1) as usize, y.clamp(0, dem.stride as i64 - 1) as usize);
        let p = dem.data[y * dem.stride + x];
        [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]
    };
    let sample = |s: f32, t: f32| -> [f32; 3] {
        let (x, y) = (s * stride, t * stride);
        if ramp.filter == Filter::Nearest {
            return texel(x.floor() as i64, y.floor() as i64);
        }
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (a, b, c, d) = (texel(x0 as i64, y0 as i64), texel(x0 as i64 + 1, y0 as i64), texel(x0 as i64, y0 as i64 + 1), texel(x0 as i64 + 1, y0 as i64 + 1));
        std::array::from_fn(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
    };
    let color = |x: f32| -> Rgba {
        // GPUs sample a NaN coordinate (equal stops: 0 / 0) as 0.
        let x = if x.is_nan() { 0.0 } else { x };
        let t = x * n as f32 - 0.5;
        let i0 = t.floor();
        let f = t - i0;
        let c = |i: i64| ramp.colors[i.clamp(0, n as i64 - 1) as usize].map(|v| v as f32 / 255.0);
        let (a, b) = (c(i0 as i64), c(i0 as i64 + 1));
        std::array::from_fn(|k| a[k] * (1.0 - f) + b[k] * f)
    };
    let scale = (stride - 4.0) / stride;
    let eps = 2.0 / stride;
    draw_quad(target, ctx, tile, mask, &mut |v: &[f32; 2]| {
        let el = unpack(sample(v[0] * scale + eps, v[1] * scale + eps));
        // Binary search for the stops around the elevation.
        let (mut l, mut r) = (0usize, n - 1);
        let (mut el_l, mut el_r) = (stops[l], stops[r]);
        while r - l > 1 {
            let m = (l + r) / 2;
            if el < stops[m] {
                r = m;
                el_r = stops[m];
            } else {
                l = m;
                el_l = stops[m];
            }
        }
        let x = (l as f32 + (el - el_l) / (el_r - el_l) + 0.5) / n as f32;
        Some(color(x).map(|c| c * ramp.opacity))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_dem_has_no_slope() {
        // mapbox encoding of 0 m: 10000 / 0.1 = 100000 = (1, 134, 160).
        let img = Image { w: 4, h: 4, data: [1u8, 134, 160, 255].repeat(16) };
        let dem = Dem::new(&img, "mapbox", [0.0; 4]).unwrap();
        let tex = dem.prepare(10.0);
        let p = tex.sample(0.5, 0.5, Filter::Nearest, Filter::Nearest, 0.0);
        assert_eq!((p[0] * 255.0).round(), 128.0);
        assert_eq!((p[1] * 255.0).round(), 128.0);
    }
}
