//! Raster layers (raster tiles and image sources): MapLibre's `raster` shaders.

use super::raster::{self, Mask, Target};
use super::{num, Ctx, EXTENT};
use crate::image::{Filter, Texture};
use crate::style::Layer;
use crate::tile::RenderTile;

type P = [f64; 2];

/// Raster paint uniforms (`rasterUniformValues`).
pub struct RasterParams {
    opacity: f32,
    brightness_low: f32,
    brightness_high: f32,
    saturation: f32,
    contrast: f32,
    spin: [f32; 3],
    mag: Filter,
}

impl RasterParams {
    pub fn new(layer: &Layer, ctx: &Ctx) -> RasterParams {
        let t = super::dummy_tile();
        let p = |n: &str| ctx.paint(layer, n, None, &t);
        let hue = num(&p("raster-hue-rotate")).to_radians();
        let (s, c) = hue.sin_cos();
        let sat = num(&p("raster-saturation"));
        let con = num(&p("raster-contrast"));
        let nearest = p("resampling").as_str() == Some("nearest") || p("raster-resampling").as_str() == Some("nearest");
        RasterParams {
            opacity: num(&p("raster-opacity")) as f32,
            brightness_low: num(&p("raster-brightness-min")) as f32,
            brightness_high: num(&p("raster-brightness-max")) as f32,
            saturation: (if sat > 0.0 { 1.0 - 1.0 / (1.001 - sat) } else { -sat }) as f32,
            contrast: (if con > 0.0 { 1.0 / (1.0 - con) } else { 1.0 + con }) as f32,
            spin: [((2.0 * c + 1.0) / 3.0) as f32, ((-(3f64.sqrt()) * s - c + 1.0) / 3.0) as f32, ((3f64.sqrt() * s - c + 1.0) / 3.0) as f32],
            mag: if nearest { Filter::Nearest } else { Filter::Linear },
        }
    }

    /// Fragment shader on a premultiplied texel.
    fn shade(&self, texel: [f32; 4]) -> [f32; 4] {
        let a = texel[3];
        let mut c = [texel[0], texel[1], texel[2]];
        if a > 0.0 {
            c = c.map(|v| v / a);
        }
        let alpha = a * self.opacity;
        let w = self.spin;
        let mut rgb = [
            c[0] * w[0] + c[1] * w[1] + c[2] * w[2],
            c[0] * w[2] + c[1] * w[0] + c[2] * w[1],
            c[0] * w[1] + c[1] * w[2] + c[2] * w[0],
        ];
        let average = (c[0] + c[1] + c[2]) / 3.0;
        rgb = rgb.map(|v| {
            let v = v + (average - v) * self.saturation;
            let v = (v - 0.5) * self.contrast + 0.5;
            (self.brightness_low + (self.brightness_high - self.brightness_low) * v) * alpha
        });
        [rgb[0], rgb[1], rgb[2], alpha]
    }
}

/// A textured quad in tile units of `tile`: corners top-left, top-right, bottom-right,
/// bottom-left, and the image warp (`[perspective x, perspective y, bilinear blend]`).
pub struct Quad {
    pub tile: RenderTile,
    pub corners: [P; 4],
    pub warp: [f64; 3],
    pub subdivided: bool,
}

impl Quad {
    pub fn tile(tile: RenderTile) -> Quad {
        Quad { tile, corners: [[0.0, 0.0], [EXTENT, 0.0], [EXTENT, EXTENT], [0.0, EXTENT]], warp: [0.0, 0.0, 1.0], subdivided: false }
    }
}

pub fn draw_quad(target: &mut Target, ctx: &Ctx, q: &Quad, tex: &Texture, params: &RasterParams, mask: Option<&Mask>) {
    let to_world = ctx.tile_to_world(&q.tile);
    let off = ctx.transform.aligned_offset();
    let [tl, tr, br, bl] = q.corners;
    let w = q.warp;
    // Vertex shader for a mesh vertex at fractional tile position `f`.
    let vertex = |f: P| -> (P, [f32; 3]) {
        let lerp = |a: P, b: P, t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let bilinear = lerp(lerp(tl, tr, f[0]), lerp(bl, br, f[0]), f[1]);
        let den = w[0] * f[0] + w[1] * f[1] + 1.0;
        let across = [tr[0] - tl[0] + w[0] * tr[0], tr[1] - tl[1] + w[0] * tr[1]];
        let down = [bl[0] - tl[0] + w[1] * bl[0], bl[1] - tl[1] + w[1] * bl[1]];
        let projective = [(across[0] * f[0] + down[0] * f[1] + tl[0]) / den, (across[1] * f[0] + down[1] * f[1] + tl[1]) / den];
        let pos = lerp(projective, bilinear, w[2]);
        let wp = to_world(pos);
        let s = ctx.transform.world_to_screen([wp[0] + off[0], wp[1] + off[1]]);
        let ratio = 1.0 / den + (1.0 - 1.0 / den) * w[2];
        (s, [(f[0] * ratio) as f32, (f[1] * ratio) as f32, ratio as f32])
    };
    let g = if q.subdivided { 16 } else { 1 };
    let verts: Vec<(P, [f32; 3])> = (0..=g).flat_map(|y| (0..=g).map(move |x| [x as f64 / g as f64, y as f64 / g as f64])).map(vertex).collect();
    let (tw, th) = tex.size();
    for y in 0..g {
        for x in 0..g {
            let v0 = y * (g + 1) + x;
            let (v1, v2, v3) = (v0 + 1, v0 + g + 1, v0 + g + 2);
            for tri in [[v1, v0, v2], [v1, v2, v3]] {
                let p = tri.map(|i| verts[i].0);
                let v = tri.map(|i| verts[i].1);
                let lambda = mip_lambda(p, v, tw as f64, th as f64);
                raster::triangle(target, mask, p, v, &mut |t: &[f32; 3]| {
                    let texel = tex.sample(t[0] / t[2], t[1] / t[2], params.mag, Filter::Linear, lambda);
                    Some(params.shade(texel))
                });
            }
        }
    }
}

/// log2 of texels per screen pixel for an (affine) triangle.
fn mip_lambda(p: [P; 3], v: [[f32; 3]; 3], tw: f64, th: f64) -> f32 {
    let t = v.map(|v| [v[0] as f64 / v[2] as f64 * tw, v[1] as f64 / v[2] as f64 * th]);
    let (e1, e2) = ([p[1][0] - p[0][0], p[1][1] - p[0][1]], [p[2][0] - p[0][0], p[2][1] - p[0][1]]);
    let (t1, t2) = ([t[1][0] - t[0][0], t[1][1] - t[0][1]], [t[2][0] - t[0][0], t[2][1] - t[0][1]]);
    let det = e1[0] * e2[1] - e1[1] * e2[0];
    if det == 0.0 {
        return 0.0;
    }
    // d(tex)/d(screen x) and d(tex)/d(screen y).
    let dx = [(t1[0] * e2[1] - t2[0] * e1[1]) / det, (t1[1] * e2[1] - t2[1] * e1[1]) / det];
    let dy = [(t2[0] * e1[0] - t1[0] * e2[0]) / det, (t2[1] * e1[0] - t1[1] * e2[0]) / det];
    let rho = (dx[0].hypot(dx[1])).max(dy[0].hypot(dy[1]));
    rho.log2() as f32
}
