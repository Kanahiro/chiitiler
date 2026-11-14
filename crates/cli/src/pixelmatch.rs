//! Port of pixelmatch 8.0.0 (ISC, Mapbox), the comparator used by maplibre-gl-js render tests.
//! Defaults as used there: checkerboard blending, AA pixels excluded, no window/ignore mask.

use std::sync::LazyLock;

/// Returns the number of mismatched pixels; writes a visual diff into `output`.
pub fn pixelmatch(img1: &[u8], img2: &[u8], output: &mut [u8], width: usize, height: usize, threshold: f64) -> usize {
    let len = width * height;
    assert!(img1.len() == len * 4 && img2.len() == len * 4 && output.len() == len * 4);
    let a32: Vec<u32> = img1.chunks_exact(4).map(|p| u32::from_le_bytes(p.try_into().unwrap())).collect();
    let b32: Vec<u32> = img2.chunks_exact(4).map(|p| u32::from_le_bytes(p.try_into().unwrap())).collect();

    let mut diff = 0;
    for i in 0..len {
        let pos = i * 4;
        let delta = if a32[i] == b32[i] { 0 } else { color_delta(img1, img2, pos, threshold) };
        if delta != 0 {
            let (x, y) = (i % width, i / width);
            if antialiased(img1, x, y, width, height, &a32, &b32) || antialiased(img2, x, y, width, height, &b32, &a32) {
                draw(output, pos, 255, 255, 0);
            } else {
                draw(output, pos, 255, 0, 0);
                diff += 1;
            }
        } else {
            let v = 255.0
                + (img1[pos] as f64 * 0.29889531 + img1[pos + 1] as f64 * 0.58662247 + img1[pos + 2] as f64 * 0.11448223 - 255.0)
                    * 0.1
                    * img1[pos + 3] as f64
                    / 255.0;
            let v = v as u8;
            draw(output, pos, v, v, v);
        }
    }
    diff
}

fn draw(out: &mut [u8], pos: usize, r: u8, g: u8, b: u8) {
    out[pos..pos + 4].copy_from_slice(&[r, g, b, 255]);
}

fn antialiased(img: &[u8], x1: usize, y1: usize, width: usize, height: usize, a32: &[u32], b32: &[u32]) -> bool {
    let (x0, y0) = (x1.saturating_sub(1), y1.saturating_sub(1));
    let (x2, y2) = ((x1 + 1).min(width - 1), (y1 + 1).min(height - 1));
    let c = &img[(y1 * width + x1) * 4..][..4];
    let mut zeroes = (x1 == x0 || x1 == x2 || y1 == y0 || y1 == y2) as u32;
    let (mut min, mut max) = (0.0, 0.0);
    let (mut min_xy, mut max_xy) = ((0, 0), (0, 0));
    for x in x0..=x2 {
        for y in y0..=y2 {
            if x == x1 && y == y1 {
                continue;
            }
            let delta = brightness_delta(&img[(y * width + x) * 4..][..4], c);
            if delta == 0.0 {
                zeroes += 1;
                if zeroes > 2 {
                    return false;
                }
            } else if delta < min {
                min = delta;
                min_xy = (x, y);
            } else if delta > max {
                max = delta;
                max_xy = (x, y);
            }
        }
    }
    if min == 0.0 || max == 0.0 {
        return false;
    }
    let many = |(x, y)| has_many_siblings(a32, x, y, width, height) && has_many_siblings(b32, x, y, width, height);
    many(min_xy) || many(max_xy)
}

fn has_many_siblings(img: &[u32], x1: usize, y1: usize, width: usize, height: usize) -> bool {
    let (x0, y0) = (x1.saturating_sub(1), y1.saturating_sub(1));
    let (x2, y2) = ((x1 + 1).min(width - 1), (y1 + 1).min(height - 1));
    let val = img[y1 * width + x1];
    let mut zeroes = (x1 == x0 || x1 == x2 || y1 == y0 || y1 == y2) as u32;
    for x in x0..=x2 {
        for y in y0..=y2 {
            if x == x1 && y == y1 {
                continue;
            }
            zeroes += (val == img[y * width + x]) as u32;
            if zeroes > 2 {
                return true;
            }
        }
    }
    false
}

/// Gamma-space Rec.601 luma delta (center `c` minus neighbor `n`), alpha composited over white.
fn brightness_delta(n: &[u8], c: &[u8]) -> f64 {
    let [r1, g1, b1, a1] = [c[0], c[1], c[2], c[3]].map(f64::from);
    let [r2, g2, b2, a2] = [n[0], n[1], n[2], n[3]].map(f64::from);
    let (mut dr, mut dg, mut db, da) = (r1 - r2, g1 - g2, b1 - b2, a1 - a2);
    if dr == 0.0 && dg == 0.0 && db == 0.0 && da == 0.0 {
        return 0.0;
    }
    if a1 < 255.0 || a2 < 255.0 {
        dr = (r1 * a1 - r2 * a2 - 255.0 * da) / 255.0;
        dg = (g1 * a1 - g2 * a2 - 255.0 * da) / 255.0;
        db = (b1 * a1 - b2 * a2 - 255.0 * da) / 255.0;
        let d = dr * 0.29889531 + dg * 0.58662247 + db * 0.11448223;
        return if d == 0.0 && da != 0.0 { da / 2.0 } else { d };
    }
    dr * 0.29889531 + dg * 0.58662247 + db * 0.11448223
}

// sRGB byte -> linear, padded with a 257th entry for interpolation.
static LIN: LazyLock<[f64; 257]> = LazyLock::new(|| {
    let mut t = [0.0; 257];
    for (i, v) in t.iter_mut().enumerate().take(256) {
        let c = i as f64 / 255.0;
        *v = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    }
    t[256] = t[255];
    t
});

const CBRT_N: usize = 4096;
static CBRT: LazyLock<Vec<f64>> = LazyLock::new(|| (0..=CBRT_N + 1).map(|i| (i as f64 / CBRT_N as f64).cbrt()).collect());

fn lin_lut(x: f64) -> f64 {
    let i = x as usize;
    LIN[i] + (LIN[i + 1] - LIN[i]) * (x - i as f64)
}

fn cbrt_lut(x: f64) -> f64 {
    let t = x * CBRT_N as f64;
    let i = t as i64;
    if i < 8 {
        return x.cbrt();
    }
    let i = i as usize;
    CBRT[i] + (CBRT[i + 1] - CBRT[i]) * (t - i as f64)
}

fn lms(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    (
        cbrt_lut(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b),
        cbrt_lut(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b),
        cbrt_lut(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b),
    )
}

/// 0 if within threshold, else ±1 (negative if img2 is darker). OKLab HyAB distance.
fn color_delta(img1: &[u8], img2: &[u8], k: usize, max_delta: f64) -> i32 {
    let [r1, g1, b1, a1] = [img1[k], img1[k + 1], img1[k + 2], img1[k + 3]].map(f64::from);
    let [r2, g2, b2, a2] = [img2[k], img2[k + 1], img2[k + 2], img2[k + 3]].map(f64::from);
    let (p1, p2) = if a1 == 255.0 && a2 == 255.0 {
        // Exact table lookups, same as upstream's opaque fast path.
        let lin = |v: f64| LIN[v as usize];
        (lms(lin(r1), lin(g1), lin(b1)), lms(lin(r2), lin(g2), lin(b2)))
    } else {
        // Blend over a per-pixel pseudo-random checkerboard (Fibonacci hash of the pixel index).
        let h = ((k >> 2) as u32).wrapping_mul(0x9e3779b1);
        let rb = 48.0 + 159.0 * (h >> 31) as f64;
        let gb = 48.0 + 159.0 * ((h >> 30) & 1) as f64;
        let bb = 48.0 + 159.0 * ((h >> 29) & 1) as f64;
        let bl = |c: f64, a: f64, bg: f64| lin_lut((c * a + bg * (255.0 - a)) / 255.0);
        (
            lms(bl(r1, a1, rb), bl(g1, a1, gb), bl(b1, a1, bb)),
            lms(bl(r2, a2, rb), bl(g2, a2, gb), bl(b2, a2, bb)),
        )
    };
    let (l1, m1, s1) = p1;
    let (l2, m2, s2) = p2;
    let d_lr = lightness(l1, m1, s1) - lightness(l2, m2, s2);
    let (dl, dm, ds) = (l1 - l2, m1 - m2, s1 - s2);
    let rest = max_delta - d_lr.abs();
    if rest >= 0.0 {
        let da = 1.9779984951 * dl - 2.4285922050 * dm + 0.4505937099 * ds;
        let db = 0.0259040371 * dl + 0.7827717662 * dm - 0.8086757660 * ds;
        if da * da + db * db <= rest * rest {
            return 0;
        }
    }
    if d_lr > 0.0 { -1 } else { 1 }
}

/// Toe-corrected OKLab lightness from cube-rooted LMS.
fn lightness(l: f64, m: f64, s: f64) -> f64 {
    const K1: f64 = 0.206;
    const K2: f64 = 0.03;
    const K3: f64 = (1.0 + K1) / (1.0 + K2);
    let ll = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s;
    let x = K3 * ll - K1;
    0.5 * (x + (x * x + 4.0 * K2 * K3 * ll).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_and_different() {
        let (w, h) = (4, 4);
        let a = vec![255u8; w * h * 4];
        let mut out = vec![0u8; w * h * 4];
        assert_eq!(pixelmatch(&a, &a, &mut out, w, h, 0.1), 0);
        let mut b = a.clone();
        // A solid black block (not AA: neighbors have many equal siblings) differs.
        for i in [5, 6, 9, 10] {
            b[i * 4..i * 4 + 3].fill(0);
        }
        assert_eq!(pixelmatch(&a, &b, &mut out, w, h, 0.1), 4);
        // Tiny change stays under threshold.
        let mut c = a.clone();
        c[0] = 250;
        assert_eq!(pixelmatch(&a, &c, &mut out, w, h, 0.1), 0);
    }
}
