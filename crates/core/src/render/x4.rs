//! Four pixels of a triangle row at once (WASM SIMD): the varyings, the shader and the blend of
//! `raster::triangle`, lane by lane with the same operations in the same order, so the image is
//! bit for bit the one of the scalar path (which native builds, e.g. the render-test runner, use).

use super::raster::{self, Mask, Rgba, Target};
use core::arch::wasm32::*;

/// A shader's result for four pixels: which lanes draw (all bits set), and their premultiplied
/// colors, one vector per channel. `uniform`: every lane's color is the opaque color of this
/// pixel (`raster::opaque_pixel`), which blending stores as is.
pub struct Shaded {
    pub draw: v128,
    pub c: [v128; 4],
    pub uniform: Option<[u8; 4]>,
}

/// Vector constants, built once per triangle: V8 rematerializes `v128.const` at every use in a
/// loop, which costs more than the arithmetic here (`black_box` keeps LLVM from folding them back).
pub struct K {
    zero: v128,
    one: v128,
    f255: v128,
    half: v128,
    sign: v128,
    lanes: v128,
    channel: [v128; 4],
}

impl K {
    pub fn new() -> K {
        let f = |x: f32| f32x4_splat(std::hint::black_box(x));
        let channel = std::array::from_fn(|k| {
            let k = std::hint::black_box(k as u8);
            u8x16(k, 255, 255, 255, k + 4, 255, 255, 255, k + 8, 255, 255, 255, k + 12, 255, 255, 255)
        });
        let lanes = u32x4(0, 1, 2, std::hint::black_box(3));
        K { zero: f(0.0), one: f(1.0), f255: f(255.0), half: f(0.5), sign: f(-0.0), lanes, channel }
    }
}

/// `raster::triangle`, shading four pixels at a time with `shade4` (lanes = consecutive pixels of a
/// row), which must agree with `shade` lane by lane. Leftover pixels of a row use `shade`.
pub fn triangle<const N: usize>(
    t: &mut Target,
    mask: Option<&Mask>,
    p: [[f64; 2]; 3],
    v: [[f32; N]; 3],
    shade: &mut impl FnMut(&[f32; N]) -> Option<Rgba>,
    shade4: &mut impl FnMut(&[v128; N], &K) -> Shaded,
) {
    let w = t.w;
    let Some((setup, gx, gy)) = raster::triangle_setup(t.clip, p, v) else { return };
    let (p0x, p0y) = (p[0][0], p[0][1]);
    let k = K::new();
    let (half64, p0x64) = (f64x2_splat(std::hint::black_box(0.5)), f64x2_splat(p0x));
    setup.runs(|y, x0, x1| {
        let dy = y as f64 + 0.5 - p0y;
        let row: [f64; N] = std::array::from_fn(|k| v[0][k] as f64 + gy[k] * dy);
        let mut x = x0;
        // Groups of four from x0; the last one's lanes past x1 are masked (but read and written
        // back as they were), unless they would run past the image.
        while x < x1 && y * w + x + 4 <= t.px.len() {
            let i = y * w + x;
            let n = (x1 - x).min(4) as u32;
            let mut keep = u32x4_lt(k.lanes, u32x4_splat(n));
            if t.cull {
                let occ = &t.occ[i..i + 4];
                // SAFETY: four u16 are 8 bytes; WASM loads need no alignment.
                let occ = u32x4_extend_low_u16x8(unsafe { v128_load64_zero(occ.as_ptr() as *const u64) });
                keep = v128_and(keep, u32x4_le(occ, u32x4_splat(t.layer as u32)));
            }
            if let Some(m) = mask {
                keep = v128_and(
                    keep,
                    match m.rect_run(y, x, x + 4) {
                        Some(r) => v128_and(u32x4_ge(k.lanes, u32x4_splat((r.start - x) as u32)), u32x4_lt(k.lanes, u32x4_splat((r.end - x) as u32))),
                        None => {
                            let lane = |j: usize| if j < n as usize && m.get(x + j, y) { u32::MAX } else { 0 };
                            u32x4(lane(0), lane(1), lane(2), lane(3))
                        }
                    },
                );
            }
            if v128_any_true(keep) {
                // dx = x + 0.5 - p0x for each pixel, then v = (row + gx dx) as f32.
                let dx_lo = f64x2_sub(f64x2_add(f64x2(x as f64, (x + 1) as f64), half64), p0x64);
                let dx_hi = f64x2_sub(f64x2_add(f64x2((x + 2) as f64, (x + 3) as f64), half64), p0x64);
                let vary: [v128; N] = std::array::from_fn(|k| {
                    let (r, g) = (f64x2_splat(row[k]), f64x2_splat(gx[k]));
                    let lo = f32x4_demote_f64x2_zero(f64x2_add(r, f64x2_mul(g, dx_lo)));
                    let hi = f32x4_demote_f64x2_zero(f64x2_add(r, f64x2_mul(g, dx_hi)));
                    u32x4_shuffle::<0, 1, 4, 5>(lo, hi)
                });
                let s = shade4(&vary, &k);
                let draw = v128_and(s.draw, keep);
                match s.uniform {
                    Some(px) if u32x4_all_true(draw) => t.px[i..i + 4].fill(px),
                    _ if v128_any_true(draw) => blend(t, i, &s.c, draw, &k),
                    _ => {}
                }
            }
            x += 4;
        }
        for x in x..x1 {
            if mask.is_some_and(|m| !m.get(x, y)) || !t.visible(y * w + x) {
                continue;
            }
            let dx = x as f64 + 0.5 - p0x;
            let vary: [f32; N] = std::array::from_fn(|k| (row[k] + gx[k] * dx) as f32);
            if let Some(c) = shade(&vary) {
                t.blend(y * w + x, c);
            }
        }
    });
}

/// `Target::blend` of the four pixels from `i` where `draw` is set (`raster::blend_pixel`'s
/// operations, with the lanes holding pixels instead of channels).
fn blend(t: &mut Target, i: usize, c: &[v128; 4], draw: v128, k: &K) {
    let px = &mut t.px[i..i + 4];
    // SAFETY: four RGBA8 pixels are 16 bytes; WASM loads and stores need no alignment.
    let d = unsafe { v128_load(px.as_ptr() as *const v128) };
    let (zero, one, f255) = (k.zero, k.one, k.f255);
    // `1 - c[3].clamp(0, 1)` and the alpha-1 shortcut, per pixel.
    let inv = f32x4_sub(one, clamp01(c[3], k));
    let opaque = f32x4_ge(c[3], one);
    // Drawn lanes all of alpha 1: the destination term is 0 in all of them.
    let all_opaque = u32x4_all_true(v128_or(opaque, v128_not(draw)));
    let mut out = zero;
    for (j, ck) in c.iter().enumerate() {
        let src = f32x4_min(f32x4_max(*ck, zero), one);
        let x = if all_opaque {
            f32x4_mul(src, f255)
        } else {
            let channel = u8x16_swizzle(d, k.channel[j]);
            let dst = f32x4_convert_u32x4(channel);
            v128_bitselect(f32x4_mul(src, f255), f32x4_mul(f32x4_add(src, f32x4_mul(f32x4_div(dst, f255), inv)), f255), opaque)
        };
        let tr = f32x4_trunc(x);
        let up = f32x4_ge(f32x4_abs(f32x4_sub(x, tr)), k.half);
        let r = f32x4_add(tr, v128_and(up, v128_bitselect(x, one, k.sign)));
        let v = i32x4_trunc_sat_f32x4(f32x4_min(f32x4_max(r, zero), f255));
        out = v128_or(out, i32x4_shl(v, 8 * j as u32));
    }
    let out = v128_bitselect(out, d, draw);
    // SAFETY: as above.
    unsafe { v128_store(px.as_mut_ptr() as *mut v128, out) };
}

/// `f32::clamp(x, 0, 1)` per lane (NaN stays NaN, -0 stays -0).
#[inline]
pub fn clamp01(x: v128, k: &K) -> v128 {
    let x = v128_bitselect(k.zero, x, f32x4_lt(x, k.zero));
    v128_bitselect(k.one, x, f32x4_gt(x, k.one))
}

/// `raster::smoothstep(e0, e1, x)` per lane, with `e0` and `e1 - e0` splatted.
pub struct Smoothstep {
    e0: v128,
    d: v128,
    two: v128,
    three: v128,
}

impl Smoothstep {
    pub fn new(e0: f32, e1: f32) -> Smoothstep {
        let f = |x: f32| f32x4_splat(std::hint::black_box(x));
        Smoothstep { e0: f(e0), d: f(e1 - e0), two: f(2.0), three: f(3.0) }
    }

    #[inline]
    pub fn eval(&self, x: v128, k: &K) -> v128 {
        let t = clamp01(f32x4_div(f32x4_sub(x, self.e0), self.d), k);
        f32x4_mul(f32x4_mul(t, t), f32x4_sub(self.three, f32x4_mul(self.two, t)))
    }
}

/// `draw_circle`'s shader, per lane.
pub struct Circle {
    one: v128,
    aa_blur: v128,
    edge: v128,
    min: v128,
    fill: [v128; 4],
    diff: [v128; 4],
    opacity: Smoothstep,
    color: Option<Smoothstep>,
    /// `interior` where `opacity_t` is exactly 1 and `color_t` 0: (color, drawn, `opaque_pixel`).
    interior: Option<([v128; 4], v128, Option<[u8; 4]>)>,
}

impl Circle {
    pub fn new(fill: Rgba, stroke: Rgba, radius: f32, stroke_width: f32, aa_blur: f32, interior: Option<Rgba>) -> Circle {
        let f = |x: f32| f32x4_splat(std::hint::black_box(x));
        let stroke_t = !(stroke_width < 0.01);
        Circle {
            one: f(1.0),
            aa_blur: f(aa_blur),
            edge: f(radius / (radius + stroke_width)),
            min: f(0.5 / 255.0),
            fill: fill.map(f),
            diff: std::array::from_fn(|k| f(stroke[k] - fill[k])),
            opacity: Smoothstep::new(0.0, aa_blur),
            color: stroke_t.then(|| Smoothstep::new(aa_blur, 0.0)),
            interior: (!stroke_t && aa_blur.is_finite()).then(|| match interior {
                Some(c) => (c.map(f), u32x4_splat(u32::MAX), raster::opaque_pixel(c)),
                None => ([f(0.0); 4], u32x4_splat(0), None),
            }),
        }
    }

    #[inline]
    pub fn shade(&self, e: &[v128; 2], k: &K) -> Shaded {
        let len2 = f32x4_add(f32x4_mul(e[0], e[0]), f32x4_mul(e[1], e[1]));
        let inside = v128_not(f32x4_ge(len2, self.one));
        let len = f32x4_sqrt(len2);
        let x = f32x4_sub(len, self.one);
        let lanes = match &self.interior {
            Some(_) => v128_and(inside, f32x4_le(x, self.aa_blur)),
            None => k.zero,
        };
        if let Some((c, draw, uniform)) = &self.interior
            && u32x4_all_true(lanes)
        {
            return Shaded { draw: *draw, c: *c, uniform: *uniform };
        }
        let opacity_t = self.opacity.eval(x, k);
        let color_t = match &self.color {
            Some(s) => s.eval(f32x4_sub(len, self.edge), k),
            None => k.zero,
        };
        let c: [v128; 4] = std::array::from_fn(|j| f32x4_mul(opacity_t, f32x4_add(self.fill[j], f32x4_mul(self.diff[j], color_t))));
        let visible = c.iter().fold(k.zero, |a, cj| v128_or(a, f32x4_ge(*cj, self.min)));
        let mut s = Shaded { draw: v128_and(inside, visible), c, uniform: None };
        if let Some((c, draw, _)) = &self.interior {
            s.draw = v128_bitselect(*draw, s.draw, lanes);
            s.c = std::array::from_fn(|j| v128_bitselect(c[j], s.c[j], lanes));
        }
        s
    }
}

/// `f32::min` per lane: a NaN argument yields the other.
#[inline]
pub fn min(a: v128, b: v128) -> v128 {
    let r = v128_bitselect(b, f32x4_min(a, b), f32x4_ne(a, a));
    v128_bitselect(a, r, f32x4_ne(b, b))
}

/// `draw_line`'s shader without dashes, pattern or gradient, per lane: `v` is (round, up).
pub struct Line {
    outset: v128,
    inset: v128,
    blur2: v128,
    one: v128,
    c: [v128; 4],
    uniform: Option<[u8; 4]>,
}

impl Line {
    pub fn new(c: Rgba, outset: f32, inset: f32, blur2: f32) -> Line {
        let f = |x: f32| f32x4_splat(std::hint::black_box(x));
        Line { outset: f(outset), inset: f(inset - blur2), blur2: f(blur2), one: f(1.0), c: c.map(f), uniform: raster::opaque_pixel(c) }
    }

    /// The scalar shader `shade` stands for.
    #[inline]
    pub fn shade_scalar(c: Rgba, outset: f32, inset: f32, blur2: f32, v: &[f32; 2]) -> Option<Rgba> {
        let d = (v[0] * v[0] + v[1] * v[1]).sqrt() * outset;
        let alpha = ((d - (inset - blur2)).min(outset - d) / blur2).clamp(0.0, 1.0);
        Some(c.map(|k| k * alpha))
    }

    #[inline]
    pub fn shade(&self, v: &[v128; 2], k: &K) -> Shaded {
        let d = f32x4_mul(f32x4_sqrt(f32x4_add(f32x4_mul(v[0], v[0]), f32x4_mul(v[1], v[1]))), self.outset);
        let alpha = clamp01(f32x4_div(min(f32x4_sub(d, self.inset), f32x4_sub(self.outset, d)), self.blur2), k);
        let uniform = self.uniform.filter(|_| u32x4_all_true(f32x4_eq(alpha, self.one)));
        Shaded { draw: u32x4_splat(u32::MAX), c: self.c.map(|c| f32x4_mul(c, alpha)), uniform }
    }
}

/// The four lanes of `v`.
#[inline]
pub fn lanes(v: v128) -> [f32; 4] {
    [f32x4_extract_lane::<0>(v), f32x4_extract_lane::<1>(v), f32x4_extract_lane::<2>(v), f32x4_extract_lane::<3>(v)]
}

/// A constant color, drawn wherever the triangle covers.
pub struct Constant {
    c: [v128; 4],
    uniform: Option<[u8; 4]>,
}

impl Constant {
    pub fn new(c: Rgba) -> Constant {
        Constant { c: c.map(|x| f32x4_splat(std::hint::black_box(x))), uniform: raster::opaque_pixel(c) }
    }

    #[inline]
    pub fn shade(&self) -> Shaded {
        Shaded { draw: u32x4_splat(u32::MAX), c: self.c, uniform: self.uniform }
    }
}
