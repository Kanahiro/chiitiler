//! Image decoding and GL-like textures (premultiplied on upload, optional mipmaps,
//! LINEAR / NEAREST filtering with clamp-to-edge).

/// Non-premultiplied RGBA8.
#[derive(Debug, Clone)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl Image {
    /// PNG, JPEG or WebP, by magic bytes.
    pub fn decode(bytes: &[u8]) -> Option<Image> {
        if bytes.starts_with(b"\x89PNG") {
            decode_png(bytes)
        } else if bytes.starts_with(&[0xff, 0xd8]) {
            decode_jpeg(bytes)
        } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            decode_webp(bytes)
        } else {
            None
        }
    }

    /// Copy a `w`×`h` region at `(x, y)` (out-of-range pixels are transparent).
    pub fn sub_image(&self, x: usize, y: usize, w: usize, h: usize) -> Image {
        let mut data = vec![0; w * h * 4];
        for row in 0..h {
            for col in 0..w {
                let (sx, sy) = (x + col, y + row);
                if sx < self.w && sy < self.h {
                    let s = (sy * self.w + sx) * 4;
                    let d = (row * w + col) * 4;
                    data[d..d + 4].copy_from_slice(&self.data[s..s + 4]);
                }
            }
        }
        Image { w, h, data }
    }

    /// `ImageAtlas` icon entry: the image with a 1px transparent border.
    pub fn with_padding(&self) -> Image {
        let (w, h) = (self.w + 2, self.h + 2);
        let mut data = vec![0; w * h * 4];
        for y in 0..self.h {
            let d = ((y + 1) * w + 1) * 4;
            data[d..d + self.w * 4].copy_from_slice(&self.data[y * self.w * 4..(y + 1) * self.w * 4]);
        }
        Image { w, h, data }
    }

    /// `ImageAtlas` pattern entry: the image with a 1px border of wrapped edge pixels
    /// (corners stay transparent).
    pub fn with_wrapped_padding(&self) -> Image {
        let (w, h) = (self.w + 2, self.h + 2);
        let mut data = vec![0; w * h * 4];
        let mut put = |dx: usize, dy: usize, sx: usize, sy: usize| {
            let s = (sy * self.w + sx) * 4;
            let d = (dy * w + dx) * 4;
            data[d..d + 4].copy_from_slice(&self.data[s..s + 4]);
        };
        for y in 0..self.h {
            for x in 0..self.w {
                put(x + 1, y + 1, x, y);
            }
        }
        for x in 0..self.w {
            put(x + 1, 0, x, self.h - 1);
            put(x + 1, h - 1, x, 0);
        }
        for y in 0..self.h {
            put(0, y + 1, self.w - 1, y);
            put(w - 1, y + 1, 0, y);
        }
        Image { w, h, data }
    }
}

fn decode_jpeg(bytes: &[u8]) -> Option<Image> {
    use zune_jpeg::zune_core::{colorspace::ColorSpace, options::DecoderOptions};
    let mut dec = zune_jpeg::JpegDecoder::new_with_options(bytes, DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA));
    let data = dec.decode().ok()?;
    let (w, h) = dec.dimensions()?;
    Some(Image { w, h, data })
}

fn decode_webp(bytes: &[u8]) -> Option<Image> {
    let mut dec = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let (w, h) = dec.dimensions();
    let mut buf = vec![0; dec.output_buffer_size()?];
    dec.read_image(&mut buf).ok()?;
    let data = if dec.has_alpha() { buf } else { buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() };
    Some(Image { w: w as usize, h: h as usize, data })
}

fn decode_png(bytes: &[u8]) -> Option<Image> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let data = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(Image { w: info.width as usize, h: info.height as usize, data })
}

/// One mip level: premultiplied RGBA8.
struct Level {
    w: usize,
    h: usize,
    px: Vec<[u8; 4]>,
}

pub struct Texture {
    levels: Vec<Level>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Linear,
    Nearest,
}

impl Texture {
    /// `premultiplyAlpha` (rounded) on upload; `mipmaps` like `gl.generateMipmap` (box filter).
    pub fn new(img: &Image, mipmaps: bool) -> Texture {
        let px = img
            .data
            .chunks_exact(4)
            .map(|p| {
                // round(c·a / 255) in integers: c·a / 255 is never a tie.
                let m = |c: u8| ((c as u32 * p[3] as u32 + 127) / 255) as u8;
                [m(p[0]), m(p[1]), m(p[2]), p[3]]
            })
            .collect();
        let mut levels = vec![Level { w: img.w.max(1), h: img.h.max(1), px }];
        while mipmaps && levels.last().is_some_and(|l| l.w > 1 || l.h > 1) {
            let l = levels.last().unwrap();
            let (w, h) = ((l.w / 2).max(1), (l.h / 2).max(1));
            let mut px = Vec::with_capacity(w * h);
            for y in 0..h {
                for x in 0..w {
                    let get = |dx: usize, dy: usize| l.px[(y * 2 + dy).min(l.h - 1) * l.w + (x * 2 + dx).min(l.w - 1)];
                    let s = [get(0, 0), get(1, 0), get(0, 1), get(1, 1)];
                    px.push(std::array::from_fn(|k| ((s.iter().map(|p| p[k] as u32).sum::<u32>() + 2) / 4) as u8));
                }
            }
            levels.push(Level { w, h, px });
        }
        Texture { levels }
    }

    pub fn size(&self) -> (usize, usize) {
        (self.levels[0].w, self.levels[0].h)
    }

    /// Sample at normalized `(u, v)`. `lambda` is log2 of texels per pixel: `> 0` minifies with
    /// `min` (and picks the nearest mip level, `*_MIPMAP_NEAREST`), otherwise `mag` applies.
    #[inline(always)]
    pub fn sample(&self, u: f32, v: f32, mag: Filter, min: Filter, lambda: f32) -> [f32; 4] {
        let level = if lambda <= 0.5 || self.levels.len() == 1 {
            0
        } else {
            ((lambda + 0.5).ceil() as usize - 1).min(self.levels.len() - 1)
        };
        let filter = if lambda > 0.0 { min } else { mag };
        let l = &self.levels[level];
        #[inline(always)]
        fn texel(l: &Level, x: i64, y: i64) -> [f32; 4] {
            let p = l.px[(y.clamp(0, l.h as i64 - 1) as usize) * l.w + x.clamp(0, l.w as i64 - 1) as usize];
            p.map(|c| c as f32 / 255.0)
        }
        let (tx, ty) = (u * l.w as f32, v * l.h as f32);
        match filter {
            Filter::Nearest => texel(l, tx.floor() as i64, ty.floor() as i64),
            Filter::Linear => {
                let (x, y) = (tx - 0.5, ty - 0.5);
                let (x0, y0) = (x.floor(), y.floor());
                let (fx, fy) = (x - x0, y - y0);
                let (x0, y0) = (x0 as i64, y0 as i64);
                // The same per-channel operations as below, four lanes at once.
                #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
                {
                    use core::arch::wasm32::*;
                    #[inline(always)]
                    fn texel(l: &Level, x: i64, y: i64) -> v128 {
                        let p = l.px[(y.clamp(0, l.h as i64 - 1) as usize) * l.w + x.clamp(0, l.w as i64 - 1) as usize];
                        f32x4_div(f32x4_convert_u32x4(u32x4_extend_low_u16x8(u16x8_extend_low_u8x16(u32x4(u32::from_ne_bytes(p), 0, 0, 0)))), f32x4_splat(255.0))
                    }
                    let (a, b, c, d) = (texel(l, x0, y0), texel(l, x0 + 1, y0), texel(l, x0, y0 + 1), texel(l, x0 + 1, y0 + 1));
                    let (gx, hx, gy, hy) = (f32x4_splat(1.0 - fx), f32x4_splat(fx), f32x4_splat(1.0 - fy), f32x4_splat(fy));
                    let lerp = |p, q| f32x4_add(f32x4_mul(p, gx), f32x4_mul(q, hx));
                    let r = f32x4_add(f32x4_mul(lerp(a, b), gy), f32x4_mul(lerp(c, d), hy));
                    return [f32x4_extract_lane::<0>(r), f32x4_extract_lane::<1>(r), f32x4_extract_lane::<2>(r), f32x4_extract_lane::<3>(r)];
                }
                #[allow(unreachable_code)]
                let (a, b, c, d) = (texel(l, x0, y0), texel(l, x0 + 1, y0), texel(l, x0, y0 + 1), texel(l, x0 + 1, y0 + 1));
                std::array::from_fn(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodes whatever files `MLR_DECODE_FILES` (comma-separated) points at.
    #[test]
    fn decode_files() {
        for path in std::env::var("MLR_DECODE_FILES").unwrap_or_default().split(',').filter(|p| !p.is_empty()) {
            let img = Image::decode(&std::fs::read(path).unwrap()).unwrap_or_else(|| panic!("decode {path}"));
            assert_eq!(img.data.len(), img.w * img.h * 4);
            eprintln!("{path}: {}x{}", img.w, img.h);
        }
    }
}
