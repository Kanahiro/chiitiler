//! Tile IDs, Web Mercator math, visible tile covering and tile URL templates.

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl TileId {
    pub fn parent_at(&self, z: u8) -> TileId {
        let d = self.z - z;
        TileId { z, x: self.x >> d, y: self.y >> d }
    }
}

/// A tile as rendered: `canonical` data drawn at `overscaled_z`, shifted by `wrap` worlds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RenderTile {
    pub overscaled_z: u8,
    pub wrap: i32,
    pub canonical: TileId,
}

pub fn mercator_x(lng: f64) -> f64 {
    (180.0 + lng) / 360.0
}

pub fn mercator_y(lat: f64) -> f64 {
    (180.0 - (180.0 / PI) * (PI / 4.0 + lat * PI / 360.0).tan().ln()) / 360.0
}

/// Latitude (degrees) at Mercator y (world = 1), the inverse of `mercator_y`.
pub fn mercator_lat(y: f64) -> f64 {
    (PI * (1.0 - 2.0 * y)).sinh().atan().to_degrees()
}

pub const MAX_LAT: f64 = 85.051129;
/// MapLibre's minimum zoom: views are clamped to it.
pub const MIN_ZOOM: f64 = -2.0;

/// Camera for a 2D Web Mercator map: world coordinates are in CSS pixels at `zoom`
/// (world is `512 * 2^zoom` wide), screen coordinates in device pixels.
#[derive(Debug, Clone)]
pub struct Transform {
    pub zoom: f64,
    pub world_size: f64,
    /// World-pixel position of the screen center.
    pub center: [f64; 2],
    pub bearing_rad: f64,
    pub width: f64,
    pub height: f64,
    pub pixel_ratio: f64,
    /// Original viewport size, preserved for raster pixel alignment when adding coverage.
    viewport_size: [f64; 2],
    /// `sin_cos` of `bearing_rad` and of `-bearing_rad`, for per-point projections.
    rot: [(f64, f64); 2],
    /// Set on the transform symbols are placed and drawn with in a projected view: world
    /// positions then map to the projected output instead (see `crate::reproject`).
    warp: Option<std::rc::Rc<crate::reproject::Warp>>,
}

impl Transform {
    pub fn new(center: [f64; 2], zoom: f64, bearing_deg: f64, width: f64, height: f64, pixel_ratio: f64) -> Transform {
        let world_size = 512.0 * 2f64.powf(zoom);
        let lat = center[1].clamp(-MAX_LAT, MAX_LAT);
        let mut c = [mercator_x(center[0]) * world_size, mercator_y(lat) * world_size];
        // Like MapLibre's `_constrain` for an unrotated mercator map: keep the world filling
        // the viewport vertically when it is tall enough.
        if world_size >= height {
            let half = height / 2.0;
            c[1] = c[1].clamp(half, world_size - half);
        } else {
            c[1] = world_size / 2.0;
        }
        let bearing_rad = -bearing_deg.to_radians();
        let rot = [bearing_rad.sin_cos(), (-bearing_rad).sin_cos()];
        Transform { zoom, world_size, center: c, bearing_rad, width, height, pixel_ratio, viewport_size: [width, height], rot, warp: None }
    }

    /// Extend coverage without constraining the camera again or shifting its pixel grid.
    pub fn set_margin(&mut self, margin: u32) {
        let margin = (margin as f64 * self.pixel_ratio).round() / self.pixel_ratio;
        self.width = self.viewport_size[0] + 2.0 * margin;
        self.height = self.viewport_size[1] + 2.0 * margin;
    }

    /// This Mercator camera's world drawn into `warp`'s projected output, for the layers drawn
    /// from vertices (background, fill, line, circle, heatmap, symbol).
    pub fn reprojected(&self, warp: std::rc::Rc<crate::reproject::Warp>) -> Transform {
        let [width, height] = warp.css_size;
        Transform {
            width,
            height,
            viewport_size: [width, height],
            warp: Some(warp),
            ..self.clone()
        }
    }

    /// Whether world positions map to a projected output (`reprojected`), where straight
    /// segments curve and pixel lengths need `local_frame`.
    pub fn is_projected(&self) -> bool {
        self.warp.is_some()
    }

    /// Mercator coordinates (world = 1) of each output pixel center of a projected output.
    pub fn pixel_world(&self) -> Option<&[[f64; 2]]> {
        self.warp.as_ref().map(|w| w.pixel_world())
    }

    /// See `Warp::pixel_origin`.
    pub fn pixel_origin(&self) -> Option<[f64; 2]> {
        self.warp.as_ref().map(|w| w.pixel_origin())
    }

    /// See `Warp::central_x`.
    pub fn central_x(&self) -> Option<f64> {
        self.warp.as_ref().and_then(|w| w.central_x())
    }

    /// Whether world point `p` is on a projected output's map (see `Warp::in_domain`); always for
    /// Mercator.
    pub fn in_domain(&self, p: [f64; 2]) -> bool {
        self.warp
            .as_ref()
            .is_none_or(|w| w.in_domain([p[0] / self.world_size, p[1] / self.world_size]))
    }

    /// `world_to_screen` of world point `p` if it is `in_domain`, projecting it once.
    pub fn on_map(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        match &self.warp {
            Some(w) => w.on_map([p[0] / self.world_size, p[1] / self.world_size]),
            None => Some(self.world_to_screen(p)),
        }
    }

    /// Screen position of world point `p` and the rotation (sin, cos) from world to screen
    /// directions there: in a projected output, the rotation nearest the projection's local
    /// Jacobian, for offsets given in screen pixels (line widths) around projected points.
    pub fn local_frame(&self, p: [f64; 2]) -> ([f64; 2], (f64, f64)) {
        let s = self.world_to_screen(p);
        if self.warp.is_none() {
            return (s, self.rot[0]);
        }
        let (ex, ey) = (
            self.world_to_screen([p[0] + 1.0, p[1]]),
            self.world_to_screen([p[0], p[1] + 1.0]),
        );
        let (a, c, b, d) = (ex[0] - s[0], ex[1] - s[1], ey[0] - s[0], ey[1] - s[1]);
        let angle = (c - b).atan2(a + d);
        (
            s,
            if angle.is_finite() {
                angle.sin_cos()
            } else {
                (0.0, 1.0)
            },
        )
    }

    /// World pixel (CSS) → screen device pixel.
    pub fn world_to_screen(&self, p: [f64; 2]) -> [f64; 2] {
        if let Some(w) = &self.warp {
            return w.world_to_screen([p[0] / self.world_size, p[1] / self.world_size]);
        }
        let dx = p[0] - self.center[0];
        let dy = p[1] - self.center[1];
        let (s, c) = self.rot[0];
        [(dx * c - dy * s + self.width / 2.0) * self.pixel_ratio, (dx * s + dy * c + self.height / 2.0) * self.pixel_ratio]
    }

    /// World-space shift of MapLibre's pixel-aligned projection (`_alignedProjMatrix`), used
    /// for rasters: rounds the center to the pixel grid, with half-pixel corrections for odd
    /// viewport sizes rotated by the bearing.
    pub fn aligned_offset(&self) -> [f64; 2] {
        let round = crate::expression::value::js_round;
        let (x, y) = (self.center[0], self.center[1]);
        let (x_shift, y_shift) = ((self.viewport_size[0] % 2.0) / 2.0, (self.viewport_size[1] % 2.0) / 2.0);
        let bearing = -self.bearing_rad;
        let (cos, sin) = (bearing.cos(), (-bearing).sin());
        let dx = x - round(x) + cos * x_shift + sin * y_shift;
        let dy = y - round(y) + cos * y_shift + sin * x_shift;
        [if dx > 0.5 { dx - 1.0 } else { dx }, if dy > 0.5 { dy - 1.0 } else { dy }]
    }

    /// Screen device pixel → world pixel (CSS).
    pub fn screen_to_world(&self, p: [f64; 2]) -> [f64; 2] {
        let x = p[0] / self.pixel_ratio - self.width / 2.0;
        let y = p[1] / self.pixel_ratio - self.height / 2.0;
        let (s, c) = self.rot[1];
        [x * c - y * s + self.center[0], x * s + y * c + self.center[1]]
    }

    /// Tile zoom for a source with `tile_size` (MapLibre `coveringZoomLevel`).
    pub fn covering_zoom(&self, tile_size: f64, round: bool) -> i32 {
        let z = self.zoom + (512.0 / tile_size).log2();
        (if round { z.round() } else { z.floor() }).max(0.0) as i32
    }

    /// Tiles at `z` (unclamped, possibly wrapped x) intersecting the viewport.
    pub fn covering_tiles(&self, z: i32) -> Vec<(i32, i32, i32)> {
        let corners = [[0.0, 0.0], [self.width, 0.0], [0.0, self.height], [self.width, self.height]]
            .map(|[x, y]| self.screen_to_world([x * self.pixel_ratio, y * self.pixel_ratio]));
        let scale = 2f64.powi(z) / self.world_size;
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for c in corners {
            x0 = x0.min(c[0] * scale);
            y0 = y0.min(c[1] * scale);
            x1 = x1.max(c[0] * scale);
            y1 = y1.max(c[1] * scale);
        }
        let n = 1i32 << z;
        let mut out = Vec::new();
        for ty in (y0.floor() as i32).max(0)..=(y1.ceil() as i32 - 1).min(n - 1) {
            for tx in x0.floor() as i32..=x1.ceil() as i32 - 1 {
                let wrap = tx.div_euclid(n);
                out.push((tx.rem_euclid(n), ty, wrap));
            }
        }
        out
    }
}

/// Fill a tile URL template.
pub fn tile_url(templates: &[String], id: TileId, scheme_tms: bool) -> String {
    let t = &templates[((id.x + id.y) as usize) % templates.len().max(1)];
    let y = if scheme_tms { (1u32 << id.z) - 1 - id.y } else { id.y };
    let mut quadkey = String::new();
    for z in (1..=id.z).rev() {
        let mask = 1 << (z - 1);
        quadkey.push(char::from(b'0' + ((id.x & mask != 0) as u8) + 2 * ((id.y & mask != 0) as u8)));
    }
    let bbox = {
        let size = 2.0 * PI * 6378137.0 / (1u64 << id.z) as f64;
        let min_x = id.x as f64 * size - PI * 6378137.0;
        let max_y = PI * 6378137.0 - id.y as f64 * size;
        format!("{},{},{},{}", min_x, max_y - size, min_x + size, max_y)
    };
    t.replace("{prefix}", &format!("{:x}{:x}", id.x % 16, id.y % 16))
        .replace("{z}", &id.z.to_string())
        .replace("{x}", &id.x.to_string())
        .replace("{y}", &y.to_string())
        .replace("{quadkey}", &quadkey)
        .replace("{bbox-epsg-3857}", &bbox)
        .replace("{ratio}", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_roundtrip() {
        let t = Transform::new([10.0, 20.0], 3.5, 30.0, 512.0, 256.0, 2.0);
        let w = [1234.5, 987.25];
        let s = t.world_to_screen(w);
        let back = t.screen_to_world(s);
        assert!((back[0] - w[0]).abs() < 1e-6 && (back[1] - w[1]).abs() < 1e-6);
        let c = t.world_to_screen(t.center);
        assert!((c[0] - 512.0).abs() < 1e-9 && (c[1] - 256.0).abs() < 1e-9);
    }

    #[test]
    fn margin_preserves_projection_and_raster_alignment() {
        for ratio in [1.0, 1.25, 1.5, 2.0] {
            for bearing in [0.0, 30.0] {
                let plain = Transform::new([10.0, 85.0], 3.5, bearing, 511.0, 257.0, ratio);
                let mut padded = plain.clone();
                padded.set_margin(31);
                assert_eq!(plain.center, padded.center);
                assert_eq!(plain.aligned_offset(), padded.aligned_offset());
                let point = [1234.5, 987.25];
                let a = plain.world_to_screen(point);
                let b = padded.world_to_screen(point);
                let offset = (31.0 * ratio).round();
                for axis in 0..2 {
                    assert!((b[axis] - a[axis] - offset).abs() < 1e-9);
                }
            }
        }
    }

    #[test]
    fn urls() {
        let id = TileId { z: 3, x: 1, y: 2 };
        assert_eq!(tile_url(&["t/{z}/{x}/{y}.pbf".into()], id, false), "t/3/1/2.pbf");
        assert_eq!(tile_url(&["{z}-{x}-{y}".into()], id, true), "3-1-5");
        assert_eq!(tile_url(&["{quadkey}".into()], id, false), "021");
    }
}
