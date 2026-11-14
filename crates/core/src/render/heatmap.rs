//! Heatmap layers (`draw_heatmap.ts`): Gaussian kernels added into a quarter-resolution RGBA16F
//! framebuffer (`ONE, ONE` blending, half-float storage), then upsampled with LINEAR filtering
//! through the `heatmap-color` ramp.

use super::dem::ramp_texel;
use super::raster::{self, Rgba, Target};
use super::{num, Ctx, TileFeatures, EXTENT};
use crate::expression::{Globals, Value};
use crate::style::Layer;

const GAUSS_COEF: f32 = 0.398_942_3;
const ZERO: f32 = 1.0 / 255.0 / 16.0;

/// Round to the nearest half float (ties to even), as an RGBA16F attachment stores it.
fn f16(x: f32) -> f32 {
    let a = x.abs();
    if !a.is_finite() || a >= 65520.0 {
        return if x.is_nan() { x } else { f32::INFINITY.copysign(x) };
    }
    let step = if a < 2f32.powi(-14) { 2f32.powi(-24) } else { 2f32.powi(a.log2().floor() as i32 - 10) };
    (x / step).round_ties_even() * step
}

/// The `heatmap-color` ramp: 256 texels over `heatmap-density` 0..1 (`renderColorRamp`).
fn color_ramp(layer: &Layer, ctx: &Ctx) -> Vec<[u8; 4]> {
    let prop = layer.paint("heatmap-color");
    (0..256)
        .map(|i| {
            let g = Globals { zoom: ctx.transform.zoom, heatmap_density: Some(i as f64 / 255.0), global_state: Some(ctx.global_state.clone()), ..Default::default() };
            match prop.eval_globals(g) {
                Value::Color(c) => ramp_texel(&c),
                _ => [0; 4],
            }
        })
        .collect()
}

/// The heatmap's premultiplied color per canvas pixel (to blend in layer order, and again over
/// later opaque-pass pixels).
pub fn draw_heatmap(target: &Target, layer: &Layer, ctx: &Ctx, tiles: &[TileFeatures]) -> Vec<Rgba> {
    let t = ctx.transform;
    let dummy = super::dummy_tile();
    let mut out = vec![[0.0; 4]; target.w * target.h];
    let opacity = num(&ctx.paint(layer, "heatmap-opacity", None, &dummy)) as f32;
    if opacity == 0.0 {
        return out;
    }
    let intensity = num(&ctx.paint(layer, "heatmap-intensity", None, &dummy)) as f32;
    // The offscreen framebuffer is a quarter of the canvas (sizes truncated like GLsizei).
    let (w4, h4) = (target.w / 4, target.h / 4);
    if w4 == 0 || h4 == 0 {
        return out;
    }
    let (sx, sy) = (w4 as f64 / target.w as f64, h4 as f64 / target.h as f64);
    let mut density = vec![0f32; w4 * h4];
    for tf in tiles {
        let to_world = ctx.tile_to_world(&tf.tile);
        let extrude_scale = EXTENT / ctx.tile_px(&tf.tile);
        for f in ctx.bucket_features(layer, tf, None) {
            let weight = num(&ctx.paint(layer, "heatmap-weight", Some(&f), &tf.tile)) as f32;
            let radius = num(&ctx.paint(layer, "heatmap-radius", Some(&f), &tf.tile)) as f32;
            // Kernel extent where it falls to `ZERO`, in units of the radius.
            let s = (-2.0 * (ZERO / weight / intensity / GAUSS_COEF).ln()).sqrt() / 3.0;
            if !s.is_finite() {
                continue;
            }
            let val0 = weight * intensity * GAUSS_COEF;
            for p in f.geometry.iter().flatten() {
                if p[0] < 0.0 || p[0] >= EXTENT || p[1] < 0.0 || p[1] >= EXTENT {
                    continue;
                }
                // Quad vertices (extrude -1/1 per axis), indices (0, 3, 1), (0, 2, 3).
                let corners = [[-1.0f32, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]];
                // Projected: the kernel is extruded in screen pixels around the projected point.
                let center = match t.is_projected() {
                    true => match t.on_map(to_world(*p)) {
                        Some(c) => Some(c),
                        None => continue,
                    },
                    false => None,
                };
                let pos = corners.map(|e| {
                    let d = match center {
                        Some(c) => {
                            let off = (s * radius) as f64 * t.pixel_ratio;
                            [c[0] + e[0] as f64 * off, c[1] + e[1] as f64 * off]
                        }
                        None => {
                            let off = (s * radius) as f64 * extrude_scale;
                            t.world_to_screen(to_world([
                                p[0] + e[0] as f64 * off,
                                p[1] + e[1] as f64 * off,
                            ]))
                        }
                    };
                    [d[0] * sx, d[1] * sy]
                });
                let vary = corners.map(|e| [e[0] * s, e[1] * s]);
                for tri in [[0, 3, 1], [0, 2, 3]] {
                    raster::for_each_pixel(w4, h4, tri.map(|i| pos[i]), |x, y, b| {
                        let v = [0, 1].map(|k| (vary[tri[0]][k] as f64 * b[0] + vary[tri[1]][k] as f64 * b[1] + vary[tri[2]][k] as f64 * b[2]) as f32);
                        let d = -0.5 * 3.0 * 3.0 * (v[0] * v[0] + v[1] * v[1]);
                        let i = y * w4 + x;
                        density[i] = f16(density[i] + f16(val0 * d.exp()));
                    });
                }
            }
        }
    }
    // `heatmap_texture`: upsample the density (LINEAR, clamp) and look up the ramp.
    let ramp = color_ramp(layer, ctx);
    let texel = |x: i64, y: i64| density[(y.clamp(0, h4 as i64 - 1) as usize) * w4 + x.clamp(0, w4 as i64 - 1) as usize];
    let ramp_at = |u: f32| -> Rgba {
        let tx = u * 256.0 - 0.5;
        let i0 = tx.floor();
        let f = tx - i0;
        let c = |i: i64| ramp[i.clamp(0, 255) as usize].map(|v| v as f32 / 255.0);
        let (a, b) = (c(i0 as i64), c(i0 as i64 + 1));
        std::array::from_fn(|k| a[k] * (1.0 - f) + b[k] * f)
    };
    for y in 0..target.h {
        for x in 0..target.w {
            let u = (x as f32 + 0.5) / target.w as f32 * w4 as f32 - 0.5;
            let v = (y as f32 + 0.5) / target.h as f32 * h4 as f32 - 0.5;
            let (x0, y0) = (u.floor(), v.floor());
            let (fx, fy) = (u - x0, v - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let d = (texel(x0, y0) * (1.0 - fx) + texel(x0 + 1, y0) * fx) * (1.0 - fy) + (texel(x0, y0 + 1) * (1.0 - fx) + texel(x0 + 1, y0 + 1) * fx) * fy;
            out[y * target.w + x] = ramp_at(d).map(|v| v * opacity);
        }
    }
    out
}

/// Blend a heatmap's colors, at all pixels or only where `only` is set.
pub fn blend(target: &mut Target, colors: &[Rgba], only: Option<&[bool]>) {
    for (i, c) in colors.iter().enumerate() {
        if only.is_none_or(|o| o[i]) && c.iter().any(|&v| v != 0.0) {
            target.blend(i, *c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::f16;

    #[test]
    fn half_float_rounding() {
        assert_eq!(f16(1.0), 1.0);
        assert_eq!(f16(1.0 + 1.0 / 4096.0), 1.0);
        assert_eq!(f16(1.0 + 3.0 / 2048.0), 1.0 + 2.0 / 1024.0);
        assert_eq!(f16(1e-9), 0.0);
    }
}

/// Blend the heatmaps again over the pixels the last layer wrote in the opaque pass.
pub fn reapply(target: &mut Target, heatmaps: &[Vec<Rgba>]) {
    let only = std::mem::take(&mut target.opaque);
    if only.iter().any(|&o| o) {
        for colors in heatmaps {
            blend(target, colors, Some(&only));
        }
    }
}
