//! Rendering into a projected CRS. Buckets are built in tile space as usual; layers drawn from
//! vertices (fill, line, circle, heatmap, symbol, background) are drawn straight into the target
//! grid with their vertices projected ([`Warp::world_to_screen`]), segments split so that they
//! curve, and pixel lengths (line widths, radii, text) extruded in screen space. Image patterns
//! and dashes repeat evenly on the output's pixels ([`Warp::pixel_origin`]). Rasters and DEMs are
//! drawn in Web Mercator and the picture is warped into the target grid ([`Warp::composite`]).
//! Tile clipping uses the Mercator position of each output pixel
//! ([`Warp::pixel_world`]), as tile boundaries curve.

use crate::mvt;
use crate::projection::Projection;
use crate::render::raster::{Rgba, Target};
use crate::tile::{self, Transform};
use crate::{Error, View};

/// A target-CRS view: what [`crate::Renderer::set_projected_view`] renders.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedView {
    /// The target CRS as PROJJSON, geographic or projected. Supported methods: Transverse Mercator,
    /// Mercator, Pseudo Mercator, Lambert Conic Conformal, Polar Stereographic, Lambert Azimuthal Equal
    /// Area, Equal Earth and PROJ's Geostationary Satellite, on any ellipsoid, without datum shifts.
    /// Coordinates are (easting, northing) / (longitude, latitude) whatever the axis order.
    pub projjson: String,
    /// `[min_x, min_y, max_x, max_y]` in CRS units (degrees for longlat).
    pub bbox: [f64; 4],
    /// CSS pixels; the output is `round(width*pixel_ratio)` × `round(height*pixel_ratio)`.
    pub width: u32,
    pub height: u32,
    pub pixel_ratio: f64,
    /// CSS pixels rendered around the view before cropping to the requested output size.
    pub margin: u32,
}

/// Largest output side, margins included, in device pixels (bounds the per-pixel buffers).
const MAX_SIZE: usize = 16384;

/// Mesh spacing in output device pixels: the Mercator position of each output pixel is
/// interpolated bilinearly between exactly inverse-projected nodes.
const GRID: usize = 8;

/// A projected view's mapping to and from the Web Mercator render.
#[derive(Debug)]
pub struct Warp {
    proj: Projection,
    bbox: [f64; 4],
    /// Rendered size in device pixels, margins included.
    pub size: [usize; 2],
    /// Rendered size in CSS pixels.
    pub css_size: [f64; 2],
    /// Device pixels cropped from each side of the rendered image.
    pub margin: usize,
    /// Mercator coordinates (world = 1, unwrapped around the reference point) of the mesh nodes,
    /// row-major; NaN outside the projection's domain. Empty when the whole view is outside.
    nodes: Vec<[f64; 2]>,
    cols: usize,
    /// Longitudes are unwrapped to within 180° of this one.
    reference_lng: f64,
    /// Mercator x (world = 1) of the projection's central meridian; `None` for longlat.
    central_x: Option<f64>,
    /// `pixel_world`, computed once.
    pixels: std::cell::OnceCell<Vec<[f64; 2]>>,
}

impl Warp {
    /// The warp for `view` and the Mercator view to render for it, at the zoom of the view's CRS
    /// units per pixel, covering the whole output. No Mercator view when nothing
    /// of `view` is in the projection's domain (the output is then transparent).
    pub fn new(view: &ProjectedView) -> Result<(Warp, Option<View>), Error> {
        let proj = Projection::parse(&view.projjson)?;
        let b = view.bbox;
        let out = [
            (view.width as f64 * view.pixel_ratio).round() as usize,
            (view.height as f64 * view.pixel_ratio).round() as usize,
        ];
        if !(b[2] > b[0] && b[3] > b[1]) || out[0] == 0 || out[1] == 0 {
            return Err(Error::Unsupported(format!("empty projected view {b:?}")));
        }
        // The margin extends the box by whole device pixels of the output's resolution.
        let margin = (view.margin as f64 * view.pixel_ratio).round() as usize;
        if out.iter().any(|&n| n.saturating_add(margin.saturating_mul(2)) > MAX_SIZE) {
            return Err(Error::Unsupported(format!("projected view over {MAX_SIZE} device pixels wide or high")));
        }
        let size = [out[0] + 2 * margin, out[1] + 2 * margin];
        let (ux, uy) = ((b[2] - b[0]) / out[0] as f64, (b[3] - b[1]) / out[1] as f64);
        let m = margin as f64;
        let b = [b[0] - m * ux, b[1] - m * uy, b[2] + m * ux, b[3] + m * uy];
        let (cols, rows) = (size[0].div_ceil(GRID) + 1, size[1].div_ceil(GRID) + 1);
        let grid: Vec<(f64, f64)> = (0..rows)
            .flat_map(|j| (0..cols).map(move |i| ((i * GRID) as f64, (j * GRID) as f64)))
            .collect();
        // The reference point longitudes are unwrapped around: the center or else, e.g. for a tile
        // on the edge of a world projection, the node nearest to it in the domain.
        let (cx, cy) = (size[0] as f64 / 2.0, size[1] as f64 / 2.0);
        let css_size = [
            size[0] as f64 / view.pixel_ratio,
            size[1] as f64 / view.pixel_ratio,
        ];
        let mut warp = Warp {
            proj,
            bbox: b,
            size,
            css_size,
            margin,
            nodes: Vec::new(),
            cols,
            reference_lng: 0.0,
            central_x: None,
            pixels: Default::default(),
        };
        let in_domain = |x: f64, y: f64| warp.lnglat(x, y);
        // Nodes by distance from the center, sorted only if the center is not in the domain.
        let candidates = std::iter::once_with(|| {
            let mut c = grid.clone();
            let d2 = |p: &(f64, f64)| (p.0 - cx).powi(2) + (p.1 - cy).powi(2);
            c.sort_by(|a, b| d2(a).total_cmp(&d2(b)));
            c
        })
        .flatten();
        let reference = std::iter::once((cx, cy))
            .chain(candidates)
            .find_map(|(x, y)| in_domain(x, y));
        let Some([reference_lng, _]) = reference else {
            return Ok((warp, None));
        };
        warp.reference_lng = reference_lng;
        // The central meridian itself (not a world copy of it): `world_to_screen` measures
        // longitudes from it.
        warp.central_x = warp.proj.central_meridian().map(tile::mercator_x);
        let merc = |x: f64, y: f64| warp.merc(x, y);
        let nodes: Vec<[f64; 2]> = grid.into_iter().map(|(x, y)| merc(x, y)).collect();
        // The zoom styles are evaluated and tiles are chosen at: CRS units per pixel read as Web
        // Mercator at the equator (OGC scale denominators), so all tiles of a grid level share it
        // wherever they are. A local scale would change across the view and seam neighboring tiles.
        let turn = if matches!(warp.proj, Projection::LongLat) { 360.0 } else { 2.0 * std::f64::consts::PI * 6378137.0 };
        let mut zoom = (turn / (512.0 * (ux * uy).sqrt() * view.pixel_ratio)).log2();
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for n in nodes.iter().filter(|n| n[0].is_finite()) {
            (x0, y0, x1, y1) = (x0.min(n[0]), y0.min(n[1]), x1.max(n[0]), y1.max(n[1]));
        }
        // ponytail: one zoom for the whole view; far from the center (e.g. continental views) the
        // picture is resampled up or down. Bound the Mercator render to 16× the output area.
        let css = |zoom: f64| {
            [
                (x1 - x0) * 512.0 * 2f64.powf(zoom),
                (y1 - y0) * 512.0 * 2f64.powf(zoom),
            ]
        };
        let [w, h] = css(zoom);
        let max_area = 16.0 * css_size[0] * css_size[1];
        if w * h > max_area {
            zoom -= 0.5 * (w * h / max_area).log2();
        }
        // Sized for the zoom the view is clamped to, or the picture would not cover the view.
        let zoom = zoom.max(tile::MIN_ZOOM);
        let [w, h] = css(zoom);
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let mercator = View {
            center: [
                mx * 360.0 - 180.0,
                tile::mercator_lat(my),
            ],
            zoom,
            bearing: 0.0,
            // A pixel of padding on each side for bilinear sampling.
            width: w.ceil() as u32 + 2,
            height: h.ceil() as u32 + 2,
            pixel_ratio: view.pixel_ratio,
            margin: 0,
        };
        warp.nodes = nodes;
        Ok((warp, Some(mercator)))
    }

    /// Longitude/latitude at an output pixel position, if Mercator has it.
    fn lnglat(&self, x: f64, y: f64) -> Option<[f64; 2]> {
        let b = self.bbox;
        self.proj
            .inverse([
                b[0] + x / self.size[0] as f64 * (b[2] - b[0]),
                b[3] - y / self.size[1] as f64 * (b[3] - b[1]),
            ])
            .filter(|p| p[1].abs() <= tile::MAX_LAT)
    }

    /// Mercator coordinates (world = 1) at an output pixel position; NaN where there are none.
    fn merc(&self, x: f64, y: f64) -> [f64; 2] {
        let Some([lng, lat]) = self.lnglat(x, y) else {
            return [f64::NAN; 2];
        };
        let lng = self.reference_lng + (lng - self.reference_lng + 540.0).rem_euclid(360.0) - 180.0;
        [tile::mercator_x(lng), tile::mercator_y(lat)]
    }

    /// Whether nothing of the view is in the projection's domain.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Mercator x of the projection's central meridian: drawn geometry is unwrapped to within
    /// 0.5 of it, where the projection is continuous. `None` for longlat (continuous anywhere).
    pub fn central_x(&self) -> Option<f64> {
        self.central_x
    }

    /// Mercator x offsets (whole worlds) that bring the range `x0..x1` within 0.5 of the central
    /// meridian, where its geometry lands on the map; two for a range across the antimeridian.
    pub fn branch_shifts(&self, x0: f64, x1: f64) -> std::ops::RangeInclusive<i32> {
        match self.central_x {
            Some(c) => (c - 0.5 - x1).ceil() as i32..=(c + 0.5 - x0).floor() as i32,
            None => 0..=0,
        }
    }

    /// Whether a Mercator point (world = 1) is on the map: within 180° of the central meridian
    /// (not a world copy past the antimeridian) and projectable.
    pub fn in_domain(&self, p: [f64; 2]) -> bool {
        self.on_map(p).is_some()
    }

    /// `world_to_screen` of a Mercator point that is `in_domain`.
    pub fn on_map(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        if self.central_x.is_some_and(|c| (p[0] - c).abs() > 0.5) {
            return None;
        }
        Some(self.world_to_screen(p)).filter(|s| s[0].is_finite())
    }

    /// The output's pixel (0, 0) in a grid of the CRS's own: x = CRS x, y = −CRS y, both in output
    /// device pixels. Anything laid out on it (e.g. image patterns) lines up across views of the
    /// same resolution, such as neighboring tiles.
    pub fn pixel_origin(&self) -> [f64; 2] {
        let b = self.bbox;
        let (ux, uy) = (
            (b[2] - b[0]) / self.size[0] as f64,
            (b[3] - b[1]) / self.size[1] as f64,
        );
        [b[0] / ux, -b[3] / uy]
    }

    /// Mercator (world = 1) → output device pixel. Longitudes are not folded: past the
    /// antimeridian, points land beyond the edge of the map.
    pub fn world_to_screen(&self, p: [f64; 2]) -> [f64; 2] {
        let lat = tile::mercator_lat(p[1]);
        let [x, y] = self.proj.forward_continuous(p[0] * 360.0 - 180.0, lat);
        let b = self.bbox;
        [
            (x - b[0]) / (b[2] - b[0]) * self.size[0] as f64,
            (b[3] - y) / (b[3] - b[1]) * self.size[1] as f64,
        ]
    }

    /// Mercator coordinates (world = 1) of each output pixel center, row-major; NaN outside the
    /// projection's domain. Interpolated in the mesh; exact in cells on the domain's edge (e.g.
    /// a globe's limb), and NaN in cells with no corner in it.
    pub fn pixel_world(&self) -> &[[f64; 2]] {
        self.pixels.get_or_init(|| {
            if self.is_empty() {
                return vec![[f64::NAN; 2]; self.size[0] * self.size[1]];
            }
            let [w, h] = self.size;
            let g = GRID as f64;
            let mut out = Vec::with_capacity(w * h);
            for y in 0..h {
                let gy = (y as f64 + 0.5) / g;
                let (j, fy) = (gy as usize, gy.fract());
                for x in 0..w {
                    let gx = (x as f64 + 0.5) / g;
                    let (i, fx) = (gx as usize, gx.fract());
                    let k = j * self.cols + i;
                    let [a, b, c, d] = [
                        self.nodes[k],
                        self.nodes[k + 1],
                        self.nodes[k + self.cols],
                        self.nodes[k + self.cols + 1],
                    ];
                    let lerp = |q: usize| {
                        (a[q] * (1.0 - fx) + b[q] * fx) * (1.0 - fy)
                            + (c[q] * (1.0 - fx) + d[q] * fx) * fy
                    };
                    let m = [lerp(0), lerp(1)];
                    // A cell across the cut where longitudes unwrap (e.g. the meridian opposite
                    // a polar view's reference point) spans about a world: exact too.
                    let xs = [a[0], b[0], c[0], d[0]];
                    let span = xs.iter().fold(f64::MIN, |s, &v| s.max(v))
                        - xs.iter().fold(f64::MAX, |s, &v| s.min(v));
                    out.push(if m[0].is_finite() && span <= 0.5 {
                        m
                    } else if [a, b, c, d].iter().any(|n| n[0].is_finite()) {
                        self.merc(x as f64 + 0.5, y as f64 + 0.5)
                    } else {
                        [f64::NAN; 2]
                    });
                }
            }
            out
        })
    }

    /// Geometry in Mercator coordinates (world = 1) in this view's output scaled to `extent` (margins
    /// excluded): clipped to the projection's side of its antimeridian, split into segments up to
    /// `segment` long so that they curve, projected, then clipped to the output with `buffer` around.
    /// Lines end where they leave the projection's domain (e.g. a geostationary view's far side), and
    /// polygons follow its edge from where they leave to where they come back (see `rejoin`).
    pub(crate) fn project(&self, ty: mvt::GeomType, mut geom: Vec<Vec<[f64; 2]>>, segment: f64, extent: f64, buffer: f64) -> Vec<Vec<[i32; 2]>> {
        use mvt::{GeomType, clip_axis};
        let m = self.margin as f64;
        let scale = [extent / (self.size[0] as f64 - 2.0 * m), extent / (self.size[1] as f64 - 2.0 * m)];
        if let Some(c) = self.central_x {
            let cut = geom.iter().flatten().any(|p| (p[0] - c).abs() > 0.5);
            geom = clip_axis(&geom, ty, c - 0.5, c + 0.5, 0);
            // `clip_axis` joins a ring's crossings in ring order, so a ring that crosses the
            // antimeridian several times runs back and forth along it. At a tile edge that is in the
            // buffer, but the antimeridian is the map's outline: its overlapping runs, densified at
            // different points, would leave slivers that readers' earcut triangulates into gaps.
            if cut && ty == GeomType::Polygon {
                geom = crate::union(geom).into_iter().flatten().collect();
            }
        }
        // Output pixels, as `world_to_screen`.
        let mut projected: Vec<Vec<[f64; 2]>> = Vec::new();
        match ty {
            GeomType::Point => projected.extend(geom.iter().flatten().filter_map(|&p| self.on_map(p)).map(|s| vec![s])),
            GeomType::LineString => {
                for line in &geom {
                    projected.extend(self.visible_runs(&crate::render::densify(line, segment), false).into_iter().map(|r| r.points));
                }
            }
            GeomType::Polygon => {
                let (mut runs, mut hidden) = (Vec::new(), Vec::new());
                for ring in &geom {
                    let ring = crate::render::densify(ring, segment);
                    let mut parts = self.visible_runs(&ring, true);
                    match parts.as_slice() {
                        [] => hidden.push(ring),
                        // Wholly visible.
                        [r] if r.enter.is_none() => projected.push(parts.pop().unwrap().points),
                        _ => runs.extend(parts),
                    }
                }
                projected.extend(self.rejoin(runs, &hidden));
            }
            GeomType::Unknown => {}
        }
        if ty == GeomType::Polygon {
            projected = mvt_order(projected);
        }
        for r in &mut projected {
            for p in r.iter_mut() {
                *p = [(p[0] - m) * scale[0], (p[1] - m) * scale[1]];
            }
        }
        let clipped = clip_axis(&clip_axis(&projected, ty, -buffer, extent + buffer, 0), ty, -buffer, extent + buffer, 1);
        clipped.iter().map(|r| r.iter().map(|p| [p[0].round() as i32, p[1].round() as i32]).collect()).collect()
    }

    /// The parts of `line` (Mercator) on the map, in output pixels, each starting and ending on the
    /// domain's edge where it enters or leaves it. A `ring` (closed) that is wholly on the map is
    /// one run with no `enter`; otherwise its runs wrap around its start.
    fn visible_runs(&self, line: &[[f64; 2]], ring: bool) -> Vec<Run> {
        let on: Vec<Option<[f64; 2]>> = line.iter().map(|&p| self.on_map(p)).collect();
        let n = if ring { line.len().saturating_sub(1) } else { line.len() };
        if ring && on[..n].iter().all(Option::is_some) {
            return vec![Run { points: on.into_iter().flatten().collect(), enter: None, leave: None }];
        }
        // Rings start at a point off the map, so that no run wraps.
        let first = if ring { (0..n).find(|&i| on[i].is_none()).unwrap_or(0) } else { 0 };
        let (mut runs, mut run): (Vec<Run>, Option<Run>) = (Vec::new(), None);
        for k in 0..if ring { n + 1 } else { n } {
            let (i, prev) = ((first + k) % n.max(1), (first + k + n.max(1) - 1) % n.max(1));
            match (on[i], run.as_mut()) {
                (Some(s), Some(r)) => r.points.push(s),
                (Some(s), None) => {
                    // Entering: from the domain's edge, unless the line starts here.
                    let enter = (ring || k > 0).then(|| self.edge_between(line[i], line[prev]));
                    run = Some(Run { points: enter.into_iter().chain([s]).collect(), enter, leave: None });
                }
                (None, Some(r)) => {
                    let leave = self.edge_between(line[prev], line[i]);
                    r.points.push(leave);
                    r.leave = Some(leave);
                    runs.extend(run.take());
                }
                (None, None) => {}
            }
        }
        runs.extend(run);
        runs.retain(|r| r.points.len() > 1);
        runs
    }

    /// Output pixel position of the domain's edge between Mercator points `inside` (on the map) and
    /// `outside`.
    fn edge_between(&self, mut inside: [f64; 2], mut outside: [f64; 2]) -> [f64; 2] {
        let mut s = self.on_map(inside).unwrap_or([f64::NAN; 2]);
        for _ in 0..30 {
            let mid = [(inside[0] + outside[0]) / 2.0, (inside[1] + outside[1]) / 2.0];
            match self.on_map(mid) {
                Some(p) => (inside, s) = (mid, p),
                None => outside = mid,
            }
        }
        s
    }

    /// The projection's center (its central meridian on the equator) in output pixels, around which
    /// its domain is taken to be star-shaped, like GL JS does for `addProjection`.
    fn domain_center(&self) -> Option<[f64; 2]> {
        let c = self.central_x?;
        Some(self.world_to_screen([c, 0.5])).filter(|s| s[0].is_finite())
    }

    /// The domain's edge in direction `angle` (radians, y down) from `center`, in output pixels.
    fn edge_at(&self, center: [f64; 2], angle: f64) -> [f64; 2] {
        let d = [angle.cos(), angle.sin()];
        let at = |r: f64| [center[0] + d[0] * r, center[1] + d[1] * r];
        let (mut lo, mut hi) = (0.0, 1.0);
        while self.lnglat(at(hi)[0], at(hi)[1]).is_some() && hi < 1e15 {
            (lo, hi) = (hi, hi * 2.0);
        }
        for _ in 0..50 {
            let mid = (lo + hi) / 2.0;
            if self.lnglat(at(mid)[0], at(mid)[1]).is_some() { lo = mid } else { hi = mid }
        }
        at(lo)
    }

    /// Rings of a polygon from the `runs` of its rings that leave the map, each joined to the next
    /// along the domain's edge, clockwise on screen (the polygon's inside stays on the right, as an
    /// MVT ring keeps it), like d3-geo's clipping. With no runs, the whole edge if the `hidden` rings
    /// (Mercator) enclose the map.
    fn rejoin(&self, runs: Vec<Run>, hidden: &[Vec<[f64; 2]>]) -> Vec<Vec<[f64; 2]>> {
        use std::f64::consts::TAU;
        let Some(center) = self.domain_center() else {
            // ponytail: no center to walk the edge around (not seen in supported projections):
            // runs closed with a chord.
            return runs.into_iter().map(|r| r.points).collect();
        };
        let angle = |p: [f64; 2]| (p[1] - center[1]).atan2(p[0] - center[0]);
        // Points along the edge from `a` to `b` (exclusive), less than half a pixel off it.
        let arc = |a: [f64; 2], b: [f64; 2]| -> Vec<[f64; 2]> {
            let (ta, sweep) = (angle(a), (angle(b) - angle(a)).rem_euclid(TAU));
            let radius = ((a[0] - center[0]).powi(2) + (a[1] - center[1]).powi(2)).sqrt();
            // ponytail: at most 16384 points a turn, half a pixel off the edge up to a radius of ~10^7 px.
            let steps = ((sweep * radius.sqrt() / 2.0).ceil() as usize).clamp(1, 16384);
            (1..steps).map(|k| self.edge_at(center, ta + sweep * k as f64 / steps as f64)).collect()
        };
        if runs.is_empty() {
            let c = [self.central_x.unwrap_or(0.5), 0.5];
            if hidden.iter().map(|r| winding(r, c)).sum::<i32>() == 0 {
                return vec![];
            }
            let start = self.edge_at(center, 0.0);
            let mut ring: Vec<[f64; 2]> = std::iter::once(start).chain((1..64).map(|k| self.edge_at(center, TAU * k as f64 / 64.0))).collect();
            // Refined like an arc between each pair.
            ring = ring.iter().zip(ring.iter().cycle().skip(1)).flat_map(|(&a, &b)| std::iter::once(a).chain(arc(a, b))).collect();
            ring.push(start);
            return vec![ring];
        }
        let mut used = vec![false; runs.len()];
        let mut rings = Vec::new();
        for start in 0..runs.len() {
            if used[start] {
                continue;
            }
            let mut ring = Vec::new();
            let mut i = start;
            loop {
                used[i] = true;
                ring.extend_from_slice(&runs[i].points);
                let leave = runs[i].leave.unwrap_or(runs[i].points[runs[i].points.len() - 1]);
                // The next run entering clockwise from here.
                let next = (0..runs.len())
                    .filter(|&k| !used[k] || k == start)
                    .filter_map(|k| Some((k, runs[k].enter?)))
                    .min_by(|a, b| {
                        let d = |p: [f64; 2]| (angle(p) - angle(leave)).rem_euclid(TAU);
                        d(a.1).total_cmp(&d(b.1))
                    });
                let Some((j, enter)) = next else { break };
                ring.extend(arc(leave, enter));
                if j == start {
                    break;
                }
                i = j;
            }
            ring.push(ring[0]);
            rings.push(ring);
        }
        rings
    }

    /// Draws the Mercator picture `src` (rendered with `t`) over `dst`, then clears `src`.
    pub fn composite(&self, src: &mut Target, t: &Transform, dst: &mut Target) {
        let ws = t.world_size;
        for (i, m) in self.pixel_world().iter().enumerate() {
            if m[0].is_nan() {
                continue;
            }
            let [sx, sy] = t.world_to_screen([m[0] * ws, m[1] * ws]);
            let px = sample(src, sx - 0.5, sy - 0.5);
            if px != [0.0; 4] {
                dst.blend(i, px);
            }
        }
        src.px.fill([0; 4]);
    }
}

/// A part of a line or ring on the map (see `Warp::visible_runs`), with where it enters and leaves
/// it (output pixels) if it does.
struct Run {
    points: Vec<[f64; 2]>,
    enter: Option<[f64; 2]>,
    leave: Option<[f64; 2]>,
}

/// Polygon rings in MVT order, each outer ring (positive area in y-down coordinates) followed by
/// the holes it contains, which readers tell apart by that order (vector-tile-js `classifyRings`).
/// Holes in no outer ring and rings without area are left out. Before clipping: rings clipped to
/// the same box share edges along it, where a point can be in either.
fn mvt_order(rings: Vec<Vec<[f64; 2]>>) -> Vec<Vec<[f64; 2]>> {
    type Ring = Vec<[f64; 2]>;
    let area = |r: &[[f64; 2]]| r.windows(2).map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1]).sum::<f64>();
    let bounds = |r: &[[f64; 2]]| r.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
    // Outer rings with their area, bounds and holes.
    let mut outers: Vec<(f64, [f64; 4], Ring, Vec<Ring>)> = Vec::new();
    let mut holes: Vec<Ring> = Vec::new();
    for r in rings {
        match area(&r) {
            a if a > 0.0 => outers.push((a, bounds(&r), r, Vec::new())),
            a if a < 0.0 => holes.push(r),
            _ => {}
        }
    }
    for hole in holes {
        // The smallest outer ring around a point of the hole (rings do not cross).
        let p = hole[0];
        let inside = (0..outers.len())
            .filter(|&i| {
                let b = outers[i].1;
                p[0] >= b[0] && p[0] <= b[2] && p[1] >= b[1] && p[1] <= b[3] && winding(&outers[i].2, p) != 0
            })
            .min_by(|&i, &j| outers[i].0.total_cmp(&outers[j].0));
        if let Some(i) = inside {
            outers[i].3.push(hole);
        }
    }
    outers.into_iter().flat_map(|(_, _, outer, holes)| std::iter::once(outer).chain(holes)).collect()
}

/// Winding number of a closed ring around `p`.
fn winding(ring: &[[f64; 2]], p: [f64; 2]) -> i32 {
    ring.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let cross = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
            match (a[1] <= p[1], b[1] <= p[1]) {
                (true, false) if cross > 0.0 => 1,
                (false, true) if cross < 0.0 => -1,
                _ => 0,
            }
        })
        .sum()
}

/// Bilinear sample of a premultiplied target at pixel-center coordinates; transparent outside.
fn sample(t: &Target, x: f64, y: f64) -> Rgba {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
    let at = |xi: f64, yi: f64| -> [f32; 4] {
        if xi < 0.0 || yi < 0.0 || xi >= t.w as f64 || yi >= t.h as f64 {
            return [0.0; 4];
        }
        t.px[yi as usize * t.w + xi as usize].map(|c| c as f32 / 255.0)
    };
    let (a, b, c, d) = (
        at(x0, y0),
        at(x0 + 1.0, y0),
        at(x0, y0 + 1.0),
        at(x0 + 1.0, y0 + 1.0),
    );
    std::array::from_fn(|k| {
        (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::test_crs;

    #[test]
    fn holes_across_the_output_edge_stay_after_their_outer_ring() {
        let view = ProjectedView { projjson: test_crs::geographic(), bbox: [0.0, 0.0, 10.0, 10.0], width: 512, height: 512, pixel_ratio: 1.0, margin: 0 };
        let (warp, _) = Warp::new(&view).unwrap();
        let m = |lng: f64, lat: f64| [tile::mercator_x(lng), tile::mercator_y(lat)];
        // MVT orientation: the outer ring clockwise on screen, the hole counterclockwise. The hole
        // starts south of the output, so clipping puts its first point on the edge the outer ring
        // is clipped along too.
        let outer = vec![m(-5.0, 15.0), m(15.0, 15.0), m(15.0, -5.0), m(-5.0, -5.0), m(-5.0, 15.0)];
        let hole = vec![m(3.0, -2.0), m(7.0, -2.0), m(7.0, 1.0), m(3.0, 1.0), m(3.0, -2.0)];
        let rings = warp.project(mvt::GeomType::Polygon, vec![outer, hole], 1.0 / 32.0, 4096.0, 128.0);
        let area = |r: &[[i32; 2]]| r.windows(2).map(|w| w[0][0] as i64 * w[1][1] as i64 - w[1][0] as i64 * w[0][1] as i64).sum::<i64>();
        assert_eq!(rings.iter().map(|r| area(r).signum()).collect::<Vec<_>>(), [1, -1], "{rings:?}");
    }

    #[test]
    fn polygons_cut_at_the_antimeridian_do_not_fold_back_along_it() {
        // A world of Equal Earth, and a ring (MVT orientation) across 180° with a bay from the west
        // that reaches past it: clipped, its crossings (60°, -60°, 20°, 30°) join back and forth
        // along the edge, where the bay cuts what is left in two.
        let view = ProjectedView { projjson: test_crs::projected("Equal Earth", test_crs::WGS84, &[]), bbox: [-1.8e7, -9e6, 1.8e7, 9e6], width: 512, height: 256, pixel_ratio: 1.0, margin: 0 };
        let (warp, _) = Warp::new(&view).unwrap();
        let m = |lng: f64, lat: f64| [tile::mercator_x(lng), tile::mercator_y(lat)];
        let ring = vec![m(170.0, 60.0), m(190.0, 60.0), m(190.0, -60.0), m(170.0, -60.0), m(170.0, 20.0), m(185.0, 20.0), m(185.0, 30.0), m(170.0, 30.0), m(170.0, 60.0)];
        let rings = warp.project(mvt::GeomType::Polygon, vec![ring], 1.0 / 512.0, 4096.0, 128.0);
        let area = |r: &[[i32; 2]]| r.windows(2).map(|w| w[0][0] as i64 * w[1][1] as i64 - w[1][0] as i64 * w[0][1] as i64).sum::<i64>();
        assert_eq!(rings.iter().map(|r| area(r).signum()).collect::<Vec<_>>(), [1, 1], "{rings:?}");
    }

    #[test]
    fn polar_views_interpolate_no_cell_across_the_unwrapping_cut() {
        // Antarctic polar stereographic: the meridian opposite the reference point runs from the
        // pole through the picture, where unwrapped longitudes jump by a world.
        let view = ProjectedView {
            projjson: test_crs::projected(
                "Polar Stereographic (variant B)",
                test_crs::WGS84,
                &[("Latitude of standard parallel", -71.0), ("Longitude of origin", 0.0)],
            ),
            bbox: [-4e6, -4e6, 4e6, 4e6],
            width: 128,
            height: 128,
            pixel_ratio: 1.0,
            margin: 0,
        };
        let (warp, _) = Warp::new(&view).unwrap();
        for (i, m) in warp.pixel_world().iter().enumerate() {
            let exact = warp.merc((i % 128) as f64 + 0.5, (i / 128) as f64 + 0.5);
            if m[0].is_finite() {
                assert!((m[0] - exact[0]).abs() < 0.01, "pixel {i}: {m:?} vs {exact:?}");
            }
        }
    }

    #[test]
    fn tiles_of_a_grid_level_share_their_zoom() {
        // Polar stereographic tiles at 70°N and 71°N: their local Mercator scales differ.
        let zoom = |y: f64| {
            let view = ProjectedView {
                projjson: test_crs::projected(
                    "Polar Stereographic (variant B)",
                    test_crs::WGS84,
                    &[("Latitude of standard parallel", 70.0), ("Longitude of origin", -45.0)],
                ),
                bbox: [500e3, y, 550e3, y + 50e3],
                width: 256,
                height: 256,
                pixel_ratio: 2.0,
                margin: 64,
            };
            Warp::new(&view).unwrap().1.unwrap().zoom
        };
        assert_eq!(zoom(-2.2e6), zoom(-2.1e6));
    }
}
