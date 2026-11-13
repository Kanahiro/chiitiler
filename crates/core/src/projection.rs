//! Map projections for `reproject`, from PROJJSON (read through the PROJ string parameters it maps
//! to): a few common methods on an ellipsoid, without datum shifts (geodetic coordinates are taken
//! as WGS84). Coordinates are always (easting, northing) / (longitude, latitude), whatever the
//! CRS's axis order.

use crate::Error;
use serde_json::Value as J;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

/// `√(a² + b²)`: libm's `hypot` rounds correctly through a software `fma` on wasm32, which costs
/// more than the rest of a projection. The arguments here are far from overflowing.
fn hypot(a: f64, b: f64) -> f64 {
    (a * a + b * b).sqrt()
}

fn unsupported(m: impl Into<String>) -> Error {
    Error::Unsupported(m.into())
}

/// A CRS's projection from and to longitude/latitude, as [`crate::reproject::ProjectedView`] uses it.
#[derive(Debug)]
pub enum Projection {
    LongLat,
    Tmerc(Tmerc),
    Merc(Merc),
    Lcc(Lcc),
    Stere(Stere),
    Laea(Laea),
    EqEarth(EqEarth),
    Geos(Geos),
}

impl Projection {
    /// PROJJSON of a geographic or projected CRS.
    pub fn parse(projjson: &str) -> Result<Projection, Error> {
        let json: J =
            serde_json::from_str(projjson).map_err(|e| unsupported(format!("PROJJSON: {e}")))?;
        Projection::from_proj_string(&proj_string(&json)?)
    }

    /// The PROJ string `proj_string` makes of a PROJJSON CRS.
    fn from_proj_string(def: &str) -> Result<Projection, Error> {
        let params: Vec<(&str, &str)> = def
            .split_whitespace()
            .map(|t| t.trim_start_matches('+'))
            .map(|t| t.split_once('=').unwrap_or((t, "")))
            .collect();
        let get = |k: &str| params.iter().find(|p| p.0 == k).map(|p| p.1);
        let num = |k: &str| {
            get(k)
                .map(|v| {
                    v.parse::<f64>()
                        .map_err(|_| unsupported(format!("+{k}={v}")))
                })
                .transpose()
        };
        let (a, f) = match (num("R")?, num("a")?) {
            (Some(r), _) => (r, 0.0),
            (_, Some(a)) => (
                a,
                match (num("rf")?, num("f")?, num("b")?) {
                    // WKT's inverse flattening 0 is a sphere.
                    (Some(rf), ..) => if rf == 0.0 { 0.0 } else { 1.0 / rf },
                    (_, Some(f), _) => f,
                    (.., Some(b)) => 1.0 - b / a,
                    _ => 0.0,
                },
            ),
            _ => return Err(unsupported("no ellipsoid")),
        };
        let el = Ellipsoid {
            a,
            e: (f * (2.0 - f)).sqrt(),
        };
        let lat_0 = num("lat_0")?.unwrap_or(0.0);
        let lon_0 = num("lon_0")?.unwrap_or(0.0);
        let k_0 = num("k_0")?.unwrap_or(1.0);
        let origin = [num("x_0")?.unwrap_or(0.0), num("y_0")?.unwrap_or(0.0)];
        let p = Origin { lon_0, origin };
        Ok(match get("proj").unwrap_or("") {
            "longlat" => Projection::LongLat,
            "tmerc" => Projection::Tmerc(Tmerc::new(el, lat_0, k_0, p)),
            "merc" => Projection::Merc(Merc::new(el, num("lat_ts")?, k_0, p)),
            // Web Mercator: spherical formulas with the ellipsoid's semi-major axis.
            "webmerc" => Projection::Merc(Merc::new(Ellipsoid { a, e: 0.0 }, None, 1.0, p)),
            "lcc" => {
                let lat_1 = num("lat_1")?.unwrap_or(lat_0);
                Projection::Lcc(Lcc::new(
                    el,
                    lat_0,
                    lat_1,
                    num("lat_2")?.unwrap_or(lat_1),
                    k_0,
                    p,
                ))
            }
            "stere" => {
                let lat_ts = num("lat_ts")?;
                // Variant B gives the pole by the sign of its standard parallel.
                let lat_0 = num("lat_0")?.or(lat_ts.map(|t| 90f64.copysign(t))).unwrap_or(0.0);
                // Within rounding: degrees converted from radians may miss 90 by an ulp or two.
                if (lat_0.abs() - 90.0).abs() > 1e-9 {
                    return Err(unsupported("oblique stereographic"));
                }
                Projection::Stere(Stere::new(el, lat_0, lat_ts, k_0, p))
            }
            "laea" => Projection::Laea(Laea::new(el, lat_0, p)),
            "eqearth" => Projection::EqEarth(EqEarth::new(el, p)),
            "geos" => {
                if lat_0 != 0.0 {
                    return Err(unsupported("geos with +lat_0 other than 0"));
                }
                let h = num("h")?.ok_or_else(|| unsupported("geos without +h"))?;
                let sweep_x = match get("sweep").unwrap_or("y") {
                    "x" => true,
                    "y" => false,
                    s => return Err(unsupported(format!("+sweep={s}"))),
                };
                Projection::Geos(Geos::new(el, h, sweep_x, p))
            }
            p => return Err(unsupported(format!("projection {p:?}"))),
        })
    }

    /// Degrees → CRS units.
    pub fn forward(&self, lng: f64, lat: f64) -> [f64; 2] {
        let lon_0 = self.lon_0();
        // Like PROJ's `adjlon`, only longitudes beyond lon_0 ± 180° fold: the antimeridian itself stays on
        // its own side (+180° on the east edge), so rings that end at ±180° do not jump across the map.
        let lng = if matches!(self, Projection::LongLat) || (lng - lon_0).abs() <= 180.0 {
            lng
        } else {
            lon_0 + (lng - lon_0 + 540.0).rem_euclid(360.0) - 180.0
        };
        self.forward_continuous(lng, lat)
    }

    /// `forward` without folding the longitude into `lon_0` ± 180°: past the projection's
    /// antimeridian its formulas continue beyond the edge of the map, so geometry crossing it
    /// stays continuous.
    pub fn forward_continuous(&self, lng: f64, lat: f64) -> [f64; 2] {
        let lat = lat.clamp(-90.0, 90.0).to_radians();
        match self {
            Projection::LongLat => [lng, lat.to_degrees()],
            Projection::Tmerc(t) => t.p.out(t.forward(t.p.lam(lng), lat)),
            Projection::Merc(m) => m.p.out(m.forward(m.p.lam(lng), lat)),
            Projection::Lcc(l) => l.p.out(l.forward(l.p.lam(lng), lat)),
            Projection::Stere(s) => s.p.out(s.forward(s.p.lam(lng), lat)),
            Projection::Laea(l) => l.p.out(l.forward(l.p.lam(lng), lat)),
            Projection::EqEarth(e) => e.p.out(e.forward(e.p.lam(lng), lat)),
            Projection::Geos(g) => g.p.out(g.forward(g.p.lam(lng), lat)),
        }
    }

    /// The center of the longitudes the projection maps (its antimeridian is 180° away), or
    /// `None` for longlat, which maps any longitude.
    pub fn central_meridian(&self) -> Option<f64> {
        (!matches!(self, Projection::LongLat)).then(|| self.lon_0())
    }

    /// CRS units → degrees, `None` outside the projection's domain.
    pub fn inverse(&self, xy: [f64; 2]) -> Option<[f64; 2]> {
        let [lam, phi] = match self {
            Projection::LongLat => return (xy[1].abs() <= 90.0 && xy[0].is_finite()).then_some(xy),
            Projection::Tmerc(t) => t.inverse(t.p.local(xy)),
            Projection::Merc(m) => m.inverse(m.p.local(xy)),
            Projection::Lcc(l) => l.inverse(l.p.local(xy)),
            Projection::Stere(s) => s.inverse(s.p.local(xy)),
            Projection::Laea(l) => l.inverse(l.p.local(xy)),
            Projection::EqEarth(e) => e.inverse(e.p.local(xy)),
            Projection::Geos(g) => g.inverse(g.p.local(xy)),
        };
        let lon_0 = self.lon_0();
        let r = [
            (lon_0 + lam.to_degrees() + 540.0).rem_euclid(360.0) - 180.0,
            phi.to_degrees(),
        ];
        (r[0].is_finite() && r[1].is_finite()).then_some(r)
    }

    fn lon_0(&self) -> f64 {
        match self {
            Projection::LongLat => 0.0,
            Projection::Tmerc(t) => t.p.lon_0,
            Projection::Merc(m) => m.p.lon_0,
            Projection::Lcc(l) => l.p.lon_0,
            Projection::Stere(s) => s.p.lon_0,
            Projection::Laea(l) => l.p.lon_0,
            Projection::EqEarth(e) => e.p.lon_0,
            Projection::Geos(g) => g.p.lon_0,
        }
    }
}

/// Central meridian and false origin, shared by the projected methods: they work in radians
/// relative to `lon_0` and in units relative to `origin`.
#[derive(Debug, Clone, Copy)]
struct Origin {
    lon_0: f64,
    origin: [f64; 2],
}

impl Origin {
    /// Longitude difference from the central meridian, in radians.
    fn lam(&self, lng: f64) -> f64 {
        (lng - self.lon_0).to_radians()
    }
    fn out(&self, p: [f64; 2]) -> [f64; 2] {
        [self.origin[0] + p[0], self.origin[1] + p[1]]
    }
    fn local(&self, p: [f64; 2]) -> [f64; 2] {
        [p[0] - self.origin[0], p[1] - self.origin[1]]
    }
}

#[derive(Debug, Clone, Copy)]
struct Ellipsoid {
    a: f64,
    e: f64,
}

impl Ellipsoid {
    /// τ′, the tangent of the conformal latitude, from τ = tan φ (Karney 2011).
    fn taup(&self, tau: f64) -> f64 {
        let s = (self.e * (self.e * tau / hypot(tau, 1.0)).atanh()).sinh();
        tau * hypot(s, 1.0) - s * hypot(tau, 1.0)
    }

    /// τ from τ′ by Newton's method (Karney 2011, eq. 19–21).
    fn tau(&self, taup: f64) -> f64 {
        let e2m = 1.0 - self.e * self.e;
        let mut tau = taup / e2m;
        for _ in 0..5 {
            let tp = self.taup(tau);
            let d =
                (taup - tp) / hypot(tp, 1.0) * (1.0 + e2m * tau * tau) / (e2m * hypot(tau, 1.0));
            tau += d;
            if d.abs() < 1e-14 * tau.abs().max(1.0) {
                break;
            }
        }
        tau
    }

    /// `tan(π/4 − χ/2)` for the conformal latitude χ of `phi` (Snyder's t).
    fn t(&self, phi: f64) -> f64 {
        let s = self.e * phi.sin();
        (FRAC_PI_4 - phi / 2.0).tan() / ((1.0 - s) / (1.0 + s)).powf(self.e / 2.0)
    }

    /// The latitude whose Snyder's t is `t`.
    fn phi_from_t(&self, t: f64) -> f64 {
        self.tau((FRAC_PI_2 - 2.0 * t.atan()).tan()).atan()
    }

    /// `cos φ / √(1 − e² sin² φ)` (Snyder's m).
    fn m(&self, phi: f64) -> f64 {
        phi.cos() / (1.0 - (self.e * phi.sin()).powi(2)).sqrt()
    }

    /// Snyder's q, for the authalic latitude.
    fn q(&self, phi: f64) -> f64 {
        let s = phi.sin();
        let e = self.e;
        let atanh_e = if e == 0.0 { s } else { (e * s).atanh() / e };
        (1.0 - e * e) * (s / (1.0 - (e * s).powi(2)) + atanh_e)
    }

    /// The latitude whose q is `q` (Snyder eq. 3-16); NaN past the poles.
    fn phi_from_q(&self, q: f64) -> f64 {
        let e = self.e;
        let qp = self.q(FRAC_PI_2);
        if (q.abs() - qp).abs() < 1e-12 {
            return FRAC_PI_2.copysign(q);
        }
        let mut phi = (q / 2.0).asin();
        if e == 0.0 || !phi.is_finite() {
            return if q.abs() > qp { f64::NAN } else { phi };
        }
        for _ in 0..15 {
            let (s, c) = phi.sin_cos();
            let w = 1.0 - (e * s).powi(2);
            let d = w * w / (2.0 * c) * (q / (1.0 - e * e) - s / w - (e * s).atanh() / e);
            phi += d;
            if d.abs() < 1e-14 {
                break;
            }
        }
        phi
    }
}

/// Ellipsoidal transverse Mercator: Krüger's series to n⁶ (Karney 2011, as PROJ's default tmerc).
#[derive(Debug)]
pub struct Tmerc {
    el: Ellipsoid,
    /// k₀ times the rectifying radius.
    k0a: f64,
    alpha: [f64; 6],
    beta: [f64; 6],
    /// ξ of the origin latitude.
    xi0: f64,
    p: Origin,
}

impl Tmerc {
    fn new(el: Ellipsoid, lat_0: f64, k_0: f64, p: Origin) -> Tmerc {
        let f = 1.0 - (1.0 - el.e * el.e).sqrt();
        let n = f / (2.0 - f);
        let [n2, n3, n4, n5, n6] = [n * n, n.powi(3), n.powi(4), n.powi(5), n.powi(6)];
        let alpha = [
            n / 2.0 - 2.0 * n2 / 3.0 + 5.0 * n3 / 16.0 + 41.0 * n4 / 180.0 - 127.0 * n5 / 288.0
                + 7891.0 * n6 / 37800.0,
            13.0 * n2 / 48.0 - 3.0 * n3 / 5.0 + 557.0 * n4 / 1440.0 + 281.0 * n5 / 630.0
                - 1983433.0 * n6 / 1935360.0,
            61.0 * n3 / 240.0 - 103.0 * n4 / 140.0
                + 15061.0 * n5 / 26880.0
                + 167603.0 * n6 / 181440.0,
            49561.0 * n4 / 161280.0 - 179.0 * n5 / 168.0 + 6601661.0 * n6 / 7257600.0,
            34729.0 * n5 / 80640.0 - 3418889.0 * n6 / 1995840.0,
            212378941.0 * n6 / 319334400.0,
        ];
        let beta = [
            n / 2.0 - 2.0 * n2 / 3.0 + 37.0 * n3 / 96.0 - n4 / 360.0 - 81.0 * n5 / 512.0
                + 96199.0 * n6 / 604800.0,
            n2 / 48.0 + n3 / 15.0 - 437.0 * n4 / 1440.0 + 46.0 * n5 / 105.0
                - 1118711.0 * n6 / 3870720.0,
            17.0 * n3 / 480.0 - 37.0 * n4 / 840.0 - 209.0 * n5 / 4480.0 + 5569.0 * n6 / 90720.0,
            4397.0 * n4 / 161280.0 - 11.0 * n5 / 504.0 - 830251.0 * n6 / 7257600.0,
            4583.0 * n5 / 161280.0 - 108847.0 * n6 / 3991680.0,
            20648693.0 * n6 / 638668800.0,
        ];
        let k0a = k_0 * el.a / (1.0 + n) * (1.0 + n2 / 4.0 + n4 / 64.0 + n6 / 256.0);
        let mut t = Tmerc {
            el,
            k0a,
            alpha,
            beta,
            xi0: 0.0,
            p,
        };
        t.xi0 = t.xi_eta(lat_0.to_radians(), 0.0)[0];
        t
    }

    fn xi_eta(&self, phi: f64, lam: f64) -> [f64; 2] {
        let taup = self
            .el
            .taup(phi.clamp(-FRAC_PI_2 + 1e-11, FRAC_PI_2 - 1e-11).tan());
        let (xi1, eta1) = (
            taup.atan2(lam.cos()),
            (lam.sin() / hypot(taup, lam.cos())).asinh(),
        );
        let [dxi, deta] = sin_series(&self.alpha, xi1, eta1);
        [xi1 + dxi, eta1 + deta]
    }

    fn forward(&self, lam: f64, phi: f64) -> [f64; 2] {
        let [xi, eta] = self.xi_eta(phi, lam);
        [self.k0a * eta, self.k0a * (xi - self.xi0)]
    }

    fn inverse(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let (xi, eta) = (y / self.k0a + self.xi0, x / self.k0a);
        let [dxi, deta] = sin_series(&self.beta, xi, eta);
        let (xi1, eta1) = (xi - dxi, eta - deta);
        let taup = xi1.sin() / hypot(eta1.sinh(), xi1.cos());
        [eta1.sinh().atan2(xi1.cos()), self.el.tau(taup).atan()]
    }
}

/// Σⱼ c[j−1]·sin(2jζ) for ζ = ξ + iη, as (real, imaginary): Krüger's series summed with
/// Clenshaw's recurrence in complex arithmetic (as PROJ does), one sin/cos and one exp in all.
fn sin_series(c: &[f64; 6], xi: f64, eta: f64) -> [f64; 2] {
    let (s, co) = (2.0 * xi).sin_cos();
    let e = (2.0 * eta).exp();
    let (sh, ch) = ((e - 1.0 / e) / 2.0, (e + 1.0 / e) / 2.0);
    // sin 2ζ and 2·cos 2ζ.
    let sin_z = [s * ch, co * sh];
    let k = [2.0 * co * ch, -2.0 * s * sh];
    // b_j = c_j + k·b_{j+1} − b_{j+2}; the sum is b_1·sin 2ζ.
    let (mut b1, mut b2) = ([0.0; 2], [0.0; 2]);
    for &a in c.iter().rev() {
        let b0 = [
            a + k[0] * b1[0] - k[1] * b1[1] - b2[0],
            k[0] * b1[1] + k[1] * b1[0] - b2[1],
        ];
        (b2, b1) = (b1, b0);
    }
    [
        b1[0] * sin_z[0] - b1[1] * sin_z[1],
        b1[0] * sin_z[1] + b1[1] * sin_z[0],
    ]
}

/// Mercator (variant A with `k_0`, B with `lat_ts`).
#[derive(Debug)]
pub struct Merc {
    el: Ellipsoid,
    ak: f64,
    p: Origin,
}

impl Merc {
    fn new(el: Ellipsoid, lat_ts: Option<f64>, k_0: f64, p: Origin) -> Merc {
        let k = lat_ts.map_or(k_0, |t| el.m(t.to_radians()));
        Merc {
            el,
            ak: el.a * k,
            p,
        }
    }
    fn forward(&self, lam: f64, phi: f64) -> [f64; 2] {
        [self.ak * lam, self.ak * self.el.taup(phi.tan()).asinh()]
    }
    fn inverse(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        [x / self.ak, self.el.tau((y / self.ak).sinh()).atan()]
    }
}

/// Lambert conformal conic (1SP: `lat_1` = `lat_2` = `lat_0` with `k_0`; 2SP), Snyder ch. 15.
#[derive(Debug)]
pub struct Lcc {
    el: Ellipsoid,
    n: f64,
    /// a·k₀·F.
    af: f64,
    rho0: f64,
    p: Origin,
}

impl Lcc {
    fn new(el: Ellipsoid, lat_0: f64, lat_1: f64, lat_2: f64, k_0: f64, p: Origin) -> Lcc {
        let [phi0, phi1, phi2] = [lat_0, lat_1, lat_2].map(f64::to_radians);
        let (m1, t1) = (el.m(phi1), el.t(phi1));
        let n = if (phi1 - phi2).abs() > 1e-10 {
            (m1.ln() - el.m(phi2).ln()) / (t1.ln() - el.t(phi2).ln())
        } else {
            phi1.sin()
        };
        let af = el.a * k_0 * m1 / (n * t1.powf(n));
        Lcc {
            el,
            n,
            af,
            rho0: af * el.t(phi0).powf(n),
            p,
        }
    }
    fn forward(&self, lam: f64, phi: f64) -> [f64; 2] {
        let rho = self.af * self.el.t(phi).powf(self.n);
        let theta = self.n * lam;
        [rho * theta.sin(), self.rho0 - rho * theta.cos()]
    }
    fn inverse(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let s = self.n.signum();
        let dy = self.rho0 - y;
        let rho = s * hypot(x, dy);
        let theta = (s * x).atan2(s * dy);
        // Points in the cone's gap (beyond lon_0 ± 180°) are off the map.
        if (theta / self.n).abs() > std::f64::consts::PI {
            return [f64::NAN; 2];
        }
        [
            theta / self.n,
            self.el.phi_from_t((rho / self.af).powf(1.0 / self.n)),
        ]
    }
}

/// Polar stereographic (variant A with `k_0`, B with `lat_ts`), Snyder ch. 21.
#[derive(Debug)]
pub struct Stere {
    el: Ellipsoid,
    /// +1 for the north pole, −1 for the south.
    s: f64,
    /// ρ / t.
    k: f64,
    p: Origin,
}

impl Stere {
    fn new(el: Ellipsoid, lat_0: f64, lat_ts: Option<f64>, k_0: f64, p: Origin) -> Stere {
        let s = lat_0.signum();
        let e = el.e;
        let k = match lat_ts
            .map(|t| (t * s).to_radians())
            .filter(|t| (t - FRAC_PI_2).abs() > 1e-10)
        {
            Some(ts) => el.a * el.m(ts) / el.t(ts),
            None => 2.0 * el.a * k_0 / ((1.0 + e).powf(1.0 + e) * (1.0 - e).powf(1.0 - e)).sqrt(),
        };
        Stere { el, s, k, p }
    }
    fn forward(&self, lam: f64, phi: f64) -> [f64; 2] {
        let rho = self.k * self.el.t(self.s * phi);
        [rho * lam.sin(), -self.s * rho * lam.cos()]
    }
    fn inverse(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let rho = hypot(x, y);
        [
            x.atan2(-self.s * y),
            self.s * self.el.phi_from_t(rho / self.k),
        ]
    }
}

/// Lambert azimuthal equal-area, Snyder ch. 24.
#[derive(Debug)]
pub struct Laea {
    el: Ellipsoid,
    qp: f64,
    /// Authalic radius.
    rq: f64,
    /// +1 / −1 at the north / south pole; 0 otherwise (oblique and equatorial).
    pole: f64,
    /// sin and cos of the origin's authalic latitude, and Snyder's D.
    sb1: f64,
    cb1: f64,
    d: f64,
    p: Origin,
}

impl Laea {
    fn new(el: Ellipsoid, lat_0: f64, p: Origin) -> Laea {
        let qp = el.q(FRAC_PI_2);
        let rq = el.a * (qp / 2.0).sqrt();
        let phi0 = lat_0.to_radians();
        let pole = if (lat_0.abs() - 90.0).abs() < 1e-10 {
            lat_0.signum()
        } else {
            0.0
        };
        let (sb1, cb1) = (el.q(phi0) / qp).clamp(-1.0, 1.0).asin().sin_cos();
        let d = if pole == 0.0 {
            el.a * el.m(phi0) / (rq * cb1)
        } else {
            1.0
        };
        Laea {
            el,
            qp,
            rq,
            pole,
            sb1,
            cb1,
            d,
            p,
        }
    }
    fn forward(&self, lam: f64, phi: f64) -> [f64; 2] {
        let q = self.el.q(phi);
        if self.pole != 0.0 {
            let rho = self.el.a * (self.qp - self.pole * q).max(0.0).sqrt();
            return [rho * lam.sin(), -self.pole * rho * lam.cos()];
        }
        let (sb, cb) = (q / self.qp).clamp(-1.0, 1.0).asin().sin_cos();
        let b = self.rq * (2.0 / (1.0 + self.sb1 * sb + self.cb1 * cb * lam.cos())).sqrt();
        [
            b * self.d * cb * lam.sin(),
            b / self.d * (self.cb1 * sb - self.sb1 * cb * lam.cos()),
        ]
    }
    fn inverse(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        if self.pole != 0.0 {
            let r = hypot(x, y) / self.el.a;
            return [
                x.atan2(-self.pole * y),
                self.el.phi_from_q(self.pole * (self.qp - r * r)),
            ];
        }
        let (x, y) = (x / self.d, y * self.d);
        let rho = hypot(x, y);
        if rho == 0.0 {
            return [0.0, self.el.phi_from_q(self.qp * self.sb1)];
        }
        let (sc, cc) = (2.0 * (rho / (2.0 * self.rq)).asin()).sin_cos();
        let sb = cc * self.sb1 + y * sc * self.cb1 / rho;
        [
            (x * sc).atan2(rho * self.cb1 * cc - y * self.sb1 * sc),
            self.el.phi_from_q(self.qp * sb),
        ]
    }
}

/// Equal Earth (Šavrič et al. 2018), on the authalic sphere of the ellipsoid like PROJ.
#[derive(Debug)]
pub struct EqEarth {
    el: Ellipsoid,
    qp: f64,
    rq: f64,
    p: Origin,
}

const EQ_A: [f64; 4] = [1.340264, -0.081106, 0.000893, 0.003796];
const EQ_M: f64 = 0.866_025_403_784_438_6; // √3 / 2

impl EqEarth {
    fn new(el: Ellipsoid, p: Origin) -> EqEarth {
        let qp = el.q(FRAC_PI_2);
        EqEarth {
            el,
            qp,
            rq: el.a * (qp / 2.0).sqrt(),
            p,
        }
    }
    /// y(θ)/θ and dy/dθ of the unit projection.
    fn poly(t: f64) -> (f64, f64) {
        let [a1, a2, a3, a4] = EQ_A;
        let (t2, t6) = (t * t, t.powi(6));
        (
            a1 + a2 * t2 + t6 * (a3 + a4 * t2),
            a1 + 3.0 * a2 * t2 + t6 * (7.0 * a3 + 9.0 * a4 * t2),
        )
    }
    fn forward(&self, lam: f64, phi: f64) -> [f64; 2] {
        let sb = (self.el.q(phi) / self.qp).clamp(-1.0, 1.0);
        let t = (EQ_M * sb).asin();
        let (p, dp) = Self::poly(t);
        [self.rq * lam * t.cos() / (EQ_M * dp), self.rq * t * p]
    }
    fn inverse(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let (x, y) = (x / self.rq, y / self.rq);
        let mut t = y;
        for _ in 0..12 {
            let (p, dp) = Self::poly(t);
            let d = (t * p - y) / dp;
            t -= d;
            if d.abs() < 1e-14 {
                break;
            }
        }
        let lam = EQ_M * x * Self::poly(t).1 / t.cos();
        let sb = t.sin() / EQ_M;
        // Past the outline: no answer.
        if lam.abs() > std::f64::consts::PI + 1e-9 || sb.abs() > 1.0 {
            return [f64::NAN; 2];
        }
        [lam, self.el.phi_from_q(self.qp * sb)]
    }
}

/// Geostationary satellite view (PROJ's geos): the scanning angles of a satellite at height `h`
/// above the equator, times `h`. NaN on the side of the Earth the satellite does not see.
#[derive(Debug)]
pub struct Geos {
    a: f64,
    /// Satellite distance from the center, in units of `a`, and `h / a`.
    radius_g: f64,
    radius_g_1: f64,
    /// b / a.
    radius_p: f64,
    /// The scanning mirror turns about x first (GOES) instead of y (Himawari, Meteosat).
    sweep_x: bool,
    p: Origin,
}

impl Geos {
    fn new(el: Ellipsoid, h: f64, sweep_x: bool, p: Origin) -> Geos {
        let radius_g_1 = h / el.a;
        Geos {
            a: el.a,
            radius_g: 1.0 + radius_g_1,
            radius_g_1,
            radius_p: (1.0 - el.e * el.e).sqrt(),
            sweep_x,
            p,
        }
    }
    fn forward(&self, lam: f64, phi: f64) -> [f64; 2] {
        let rp2 = self.radius_p * self.radius_p;
        // Geocentric latitude, and the vector from the center to the surface (units of `a`).
        let phi = (rp2 * phi.tan()).atan();
        let r = self.radius_p / hypot(self.radius_p * phi.cos(), phi.sin());
        let (vx, vy, vz) = (
            r * lam.cos() * phi.cos(),
            r * lam.sin() * phi.cos(),
            r * phi.sin(),
        );
        if (self.radius_g - vx) * vx - vy * vy - vz * vz / rp2 < 0.0 {
            return [f64::NAN; 2];
        }
        let tmp = self.radius_g - vx;
        let g = self.radius_g_1 * self.a;
        if self.sweep_x {
            [g * (vy / hypot(vz, tmp)).atan(), g * (vz / tmp).atan()]
        } else {
            [g * (vy / tmp).atan(), g * (vz / hypot(vy, tmp)).atan()]
        }
    }
    fn inverse(&self, [x, y]: [f64; 2]) -> [f64; 2] {
        let (x, y) = (x / self.a / self.radius_g_1, y / self.a / self.radius_g_1);
        let (vy, vz) = if self.sweep_x {
            let vz = y.tan();
            (x.tan() * hypot(vz, 1.0), vz)
        } else {
            let vy = x.tan();
            (vy, y.tan() * hypot(vy, 1.0))
        };
        // The nearer intersection of the line of sight with the ellipsoid.
        let a = vy * vy + (vz / self.radius_p).powi(2) + 1.0;
        let b = -2.0 * self.radius_g;
        let det = b * b - 4.0 * a * (self.radius_g * self.radius_g - 1.0);
        if det < 0.0 {
            return [f64::NAN; 2];
        }
        let k = (-b - det.sqrt()) / (2.0 * a);
        let (vx, vy, vz) = (self.radius_g - k, vy * k, vz * k);
        let lam = vy.atan2(vx);
        let phi = (vz * lam.cos() / vx).atan();
        [lam, (phi.tan() / (self.radius_p * self.radius_p)).atan()]
    }
}

/// An EPSG conversion method: code, name, PROJ name, parameters as (code, PROJ key).
type Method = (
    u64,
    &'static str,
    &'static str,
    &'static [(u64, &'static str)],
);

const METHODS: &[Method] = &[
    (
        9807,
        "Transverse Mercator",
        "tmerc",
        &[
            (8801, "lat_0"),
            (8802, "lon_0"),
            (8805, "k_0"),
            (8806, "x_0"),
            (8807, "y_0"),
        ],
    ),
    (
        9804,
        "Mercator (variant A)",
        "merc",
        &[
            (8801, "lat_0"),
            (8802, "lon_0"),
            (8805, "k_0"),
            (8806, "x_0"),
            (8807, "y_0"),
        ],
    ),
    (
        9805,
        "Mercator (variant B)",
        "merc",
        &[
            (8823, "lat_ts"),
            (8802, "lon_0"),
            (8806, "x_0"),
            (8807, "y_0"),
        ],
    ),
    (
        1024,
        "Popular Visualisation Pseudo Mercator",
        "webmerc",
        &[
            (8801, "lat_0"),
            (8802, "lon_0"),
            (8806, "x_0"),
            (8807, "y_0"),
        ],
    ),
    (
        9801,
        "Lambert Conic Conformal (1SP)",
        "lcc",
        &[
            (8801, "lat_0"),
            (8802, "lon_0"),
            (8805, "k_0"),
            (8806, "x_0"),
            (8807, "y_0"),
        ],
    ),
    (
        9802,
        "Lambert Conic Conformal (2SP)",
        "lcc",
        &[
            (8821, "lat_0"),
            (8822, "lon_0"),
            (8823, "lat_1"),
            (8824, "lat_2"),
            (8826, "x_0"),
            (8827, "y_0"),
        ],
    ),
    (
        9820,
        "Lambert Azimuthal Equal Area",
        "laea",
        &[
            (8801, "lat_0"),
            (8802, "lon_0"),
            (8806, "x_0"),
            (8807, "y_0"),
        ],
    ),
    (
        9810,
        "Polar Stereographic (variant A)",
        "stere",
        &[
            (8801, "lat_0"),
            (8802, "lon_0"),
            (8805, "k_0"),
            (8806, "x_0"),
            (8807, "y_0"),
        ],
    ),
    (
        9829,
        "Polar Stereographic (variant B)",
        "stere",
        &[
            (8832, "lat_ts"),
            (8833, "lon_0"),
            (8806, "x_0"),
            (8807, "y_0"),
        ],
    ),
    (
        1078,
        "Equal Earth",
        "eqearth",
        &[(8802, "lon_0"), (8806, "x_0"), (8807, "y_0")],
    ),
    // PROJ's own methods and parameters, without EPSG codes (0: matched by name).
    (
        0,
        "Geostationary Satellite (Sweep X)",
        "geos +sweep=x",
        &[
            (8801, "lat_0"),
            (8802, "lon_0"),
            (0, "h"),
            (8806, "x_0"),
            (8807, "y_0"),
        ],
    ),
    (
        0,
        "Geostationary Satellite (Sweep Y)",
        "geos +sweep=y",
        &[
            (8801, "lat_0"),
            (8802, "lon_0"),
            (0, "h"),
            (8806, "x_0"),
            (8807, "y_0"),
        ],
    ),
];

/// EPSG parameter names, for parameters without an `id`.
const PARAMETER_NAMES: &[(u64, &str)] = &[
    (8801, "Latitude of natural origin"),
    (8802, "Longitude of natural origin"),
    (8805, "Scale factor at natural origin"),
    (8806, "False easting"),
    (8807, "False northing"),
    (8821, "Latitude of false origin"),
    (8822, "Longitude of false origin"),
    (8823, "Latitude of 1st standard parallel"),
    (8824, "Latitude of 2nd standard parallel"),
    (8826, "Easting at false origin"),
    (8827, "Northing at false origin"),
    (8832, "Latitude of standard parallel"),
    (8833, "Longitude of origin"),
    (0, "Satellite Height"),
];

/// A PROJJSON geographic or projected CRS as the PROJ string `Projection::from_proj_string` takes.
fn proj_string(crs: &J) -> Result<String, Error> {
    let (base, conversion) = match crs["type"].as_str() {
        Some("GeographicCRS") => (crs, None),
        Some("ProjectedCRS") => (&crs["base_crs"], Some(&crs["conversion"])),
        t => {
            return Err(unsupported(format!(
                "PROJJSON type {}",
                t.unwrap_or("(none)")
            )));
        }
    };
    let datum = if base["datum"].is_object() {
        &base["datum"]
    } else {
        &base["datum_ensemble"]
    };
    if datum["prime_meridian"]["longitude"]
        .as_f64()
        .is_some_and(|l| l != 0.0)
    {
        return Err(unsupported("prime meridians other than Greenwich"));
    }
    let el = &datum["ellipsoid"];
    // ponytail: ellipsoid lengths are taken as metres.
    let len = |v: &J| v.as_f64().or(v["value"].as_f64());
    let ellipsoid = match (
        len(&el["radius"]),
        len(&el["semi_major_axis"]),
        el["inverse_flattening"].as_f64(),
        len(&el["semi_minor_axis"]),
    ) {
        (Some(r), ..) => format!("+R={r}"),
        (_, Some(a), Some(rf), _) => format!("+a={a} +rf={rf}"),
        (_, Some(a), _, Some(b)) => format!("+a={a} +b={b}"),
        _ => return Err(unsupported("PROJJSON without an ellipsoid")),
    };
    let Some(conversion) = conversion else {
        return Ok(format!("+proj=longlat {ellipsoid}"));
    };
    for axis in crs["coordinate_system"]["axis"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let unit = &axis["unit"];
        if !(unit == "metre" || unit.is_null() || unit["conversion_factor"].as_f64() == Some(1.0)) {
            return Err(unsupported(format!("axis unit {unit}")));
        }
    }
    let method = &conversion["method"];
    let name = method["name"].as_str().unwrap_or_default();
    let &(_, _, proj, keys) = METHODS
        .iter()
        .find(|m| {
            method["id"]["code"]
                .as_u64()
                .filter(|_| m.0 != 0)
                .map_or(m.1 == name, |c| c == m.0)
        })
        .ok_or_else(|| unsupported(format!("conversion method {name}")))?;
    let mut out = format!("+proj={proj} {ellipsoid}");
    for p in conversion["parameters"].as_array().into_iter().flatten() {
        let pname = p["name"].as_str().unwrap_or_default();
        let code = p["id"]["code"]
            .as_u64()
            .or_else(|| PARAMETER_NAMES.iter().find(|n| n.1 == pname).map(|n| n.0));
        let key = keys
            .iter()
            .find(|k| Some(k.0) == code)
            .ok_or_else(|| unsupported(format!("parameter {pname} of {name}")))?
            .1;
        let value = p["value"]
            .as_f64()
            .ok_or_else(|| unsupported(format!("parameter {pname}")))?;
        let value = match &p["unit"] {
            J::Null => value,
            J::String(u) if matches!(u.as_str(), "degree" | "metre" | "unity") => value,
            u if u["type"] == "AngularUnit" => {
                (value * u["conversion_factor"].as_f64().unwrap_or(f64::NAN)).to_degrees()
            }
            u if u["conversion_factor"].is_f64() || u["conversion_factor"].is_u64() => {
                value * u["conversion_factor"].as_f64().unwrap_or(1.0)
            }
            u => return Err(unsupported(format!("unit {u}"))),
        };
        out += &format!(" +{key}={value}");
    }
    Ok(out)
}

/// PROJJSON for tests.
#[cfg(test)]
pub(crate) mod test_crs {
    /// PROJJSON ellipsoid members.
    pub const WGS84: &str = r#""semi_major_axis":6378137,"inverse_flattening":298.257223563"#;
    pub const GRS80: &str = r#""semi_major_axis":6378137,"inverse_flattening":298.257222101"#;
    pub const HIMAWARI: &str = r#""semi_major_axis":6378137,"semi_minor_axis":6356752.3"#;

    /// WGS 84 longitude/latitude.
    pub fn geographic() -> String {
        format!(r#"{{"type":"GeographicCRS","datum":{{"ellipsoid":{{{WGS84}}}}}}}"#)
    }

    /// A projected CRS by `method` name, its parameters by EPSG name, in degrees and metres.
    pub fn projected(method: &str, ellipsoid: &str, params: &[(&str, f64)]) -> String {
        let params: Vec<String> = params
            .iter()
            .map(|(name, value)| format!(r#"{{"name":"{name}","value":{value}}}"#))
            .collect();
        format!(
            r#"{{"type":"ProjectedCRS","base_crs":{{"datum":{{"ellipsoid":{{{ellipsoid}}}}}}},"conversion":{{"method":{{"name":"{method}"}},"parameters":[{}]}}}}"#,
            params.join(",")
        )
    }

    /// Himawari's full disk.
    pub fn himawari() -> String {
        projected(
            "Geostationary Satellite (Sweep Y)",
            HIMAWARI,
            &[("Longitude of natural origin", 140.7), ("Satellite Height", 35785863.0)],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_crs::*;
    use super::*;

    fn close(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
        (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
    }

    #[test]
    fn projections_match_proj4js_and_roundtrip() {
        // Reference values from proj4js 2.22.0 (`proj4('EPSG:4326', def).forward`).
        let cases: &[(&str, [f64; 2], [f64; 2])] = &[
            (
                "+proj=tmerc +lon_0=-81 +k_0=0.9996 +x_0=500000 +a=6378137 +rf=298.257223563",
                [-79.387139, 43.642567],
                [630084.3008341494, 4833438.585674324],
            ),
            (
                "+proj=tmerc +lat_0=36 +lon_0=139.833333333333 +k_0=0.9999 +x_0=0 +y_0=0 +a=6378137 +rf=298.257222101",
                [139.76, 35.68],
                [-6637.949198869096, -35499.90401395519],
            ),
            (
                "+proj=merc +lon_0=0 +k_0=1 +x_0=0 +y_0=0 +a=6378137 +rf=298.257223563",
                [139.76, 35.68],
                [15558012.033267912, 4231756.054513181],
            ),
            (
                "+proj=merc +lat_ts=30 +lon_0=100 +x_0=1000 +y_0=-500 +a=6378137 +rf=298.257223563",
                [120.5, -20.25],
                [1978968.7451433796, -1983529.347369561],
            ),
            (
                "+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k_0=1 +units=m +nadgrids=@null +wktext +no_defs",
                [139.76, 35.68],
                [15558012.033267912, 4256678.731903622],
            ),
            (
                "+proj=lcc +lat_0=46.5 +lon_0=3 +lat_1=49 +lat_2=44 +x_0=700000 +y_0=6600000 +a=6378137 +rf=298.257222101 +units=m +no_defs",
                [2.35, 48.85],
                [652301.5648305281, 6861302.725899575],
            ),
            (
                "+proj=lcc +lat_1=35 +lat_0=35 +lon_0=135 +k_0=0.9999 +x_0=500000 +y_0=0 +a=6378137 +rf=298.257222101",
                [139.76, 35.68],
                [930730.4556511124, 85702.45477958511],
            ),
            (
                "+proj=lcc +lat_0=-32 +lon_0=-60 +lat_1=-20 +lat_2=-40 +x_0=0 +y_0=0 +a=6378137 +rf=298.257223563",
                [-58.4, -34.6],
                [144979.31136620985, -285470.0217958316],
            ),
            (
                "+proj=stere +lat_0=90 +lat_ts=70 +lon_0=-45 +k_0=1 +x_0=0 +y_0=0 +a=6378137 +rf=298.257223563 +units=m +no_defs",
                [-51.7, 64.2],
                [-331477.812219844, -2821736.128997682],
            ),
            (
                "+proj=stere +lat_0=-90 +lat_ts=-71 +lon_0=0 +k_0=1 +x_0=0 +y_0=0 +a=6378137 +rf=298.257223563 +units=m +no_defs",
                [166.67, -77.85],
                [305467.1627322748, -1289201.5377136339],
            ),
            (
                "+proj=stere +lat_0=90 +lon_0=0 +k_0=0.994 +x_0=2000000 +y_0=2000000 +a=6378137 +rf=298.257223563 +units=m +no_defs",
                [25.0, 78.2],
                [2555550.35298834, 808618.423324977],
            ),
            (
                "+proj=laea +lat_0=52 +lon_0=10 +x_0=4321000 +y_0=3210000 +a=6378137 +rf=298.257222101 +units=m +no_defs",
                [2.35, 48.85],
                [3760536.8229023204, 2888771.0209501884],
            ),
            (
                "+proj=laea +lat_0=90 +lon_0=-40 +x_0=0 +y_0=0 +a=6378137 +rf=298.257223563 +units=m +no_defs",
                [-51.7, 64.2],
                [-579072.5036568096, -2796235.3538357066],
            ),
            (
                "+proj=eqearth +lon_0=0 +x_0=0 +y_0=0 +a=6378137 +rf=298.257223563 +units=m +no_defs",
                [139.76, 35.68],
                [12179903.70918021, 4430861.430775947],
            ),
            (
                "+proj=eqearth +lon_0=150 +R=6371008.8",
                [-170.0, -70.0],
                [2600439.895258415, -7692885.397207941],
            ),
            (
                "+proj=laea +lat_0=-90 +lon_0=0 +x_0=0 +y_0=0 +a=6378137 +rf=298.257223563 +units=m +no_defs",
                [-60.0, -75.0],
                [-1446478.9424851353, 835125.0068209183],
            ),
            (
                "+proj=laea +lat_0=-90 +lon_0=0 +x_0=0 +y_0=0 +a=6378137 +rf=298.257222101 +units=m",
                [166.67, -77.85],
                [312255.0815240369, -1317849.4462676693],
            ),
            (
                "+proj=laea +lat_0=0 +lon_0=0 +x_0=0 +y_0=0 +R=6371000 +units=m",
                [40.0, -30.0],
                [3888844.3587193084, -3492950.553975445],
            ),
            (
                "+proj=laea +lat_0=0 +lon_0=0 +x_0=0 +y_0=0 +a=6378137 +rf=298.257223563",
                [40.0, -30.0],
                [3896680.5516183116, -3476561.355509021],
            ),
            (
                "+proj=geos +h=35785863 +lon_0=140.7 +a=6378137 +b=6356752.3 +units=m +no_defs",
                [139.76, 35.68],
                [-82347.91997331403, 3568269.3634791635],
            ),
            (
                "+proj=geos +h=35785863 +lon_0=140.7 +a=6378137 +b=6356752.3 +units=m +no_defs",
                [100.0, -40.0],
                [-2962314.9709050534, -3767986.255491857],
            ),
            (
                "+proj=geos +h=35785863 +lon_0=140.7 +a=6378137 +b=6356752.3 +sweep=x +units=m +no_defs",
                [139.76, 35.68],
                [-81938.88936006774, 3568278.7483523316],
            ),
            (
                "+proj=geos +h=35785863 +lon_0=140.7 +a=6378137 +b=6356752.3 +sweep=x +units=m +no_defs",
                [100.0, -40.0],
                [-2945871.976800097, -3780837.030351118],
            ),
        ];
        for &(def, lnglat, xy) in cases {
            let p = Projection::from_proj_string(def).unwrap();
            let f = p.forward(lnglat[0], lnglat[1]);
            assert!(
                close(f, xy, 1e-3),
                "{def}: {lnglat:?} → {f:?}, expected {xy:?}"
            );
            let i = p.inverse(xy).unwrap();
            assert!(
                close(i, lnglat, 1e-9),
                "{def}: {xy:?} → {i:?}, expected {lnglat:?}"
            );
        }
        // Himawari's far side of the Earth.
        let geos = Projection::from_proj_string("+proj=geos +h=35785863 +lon_0=140.7 +a=6378137 +b=6356752.3")
            .unwrap();
        assert!(geos.forward(-40.0, 0.0)[0].is_nan() && geos.inverse([6.0e6, 0.0]).is_none());
        assert!(Projection::parse(&projected("Robinson", WGS84, &[])).is_err());
        // Past the Equal Earth outline.
        assert!(
            Projection::from_proj_string("+proj=eqearth +a=6378137 +rf=298.257223563")
                .unwrap()
                .inverse([2.0e7, 0.0])
                .is_none()
        );
        // ±180° stay on their own edges, like PROJ; beyond them longitudes fold.
        let eqearth = Projection::from_proj_string("+proj=eqearth +a=6378137 +rf=298.257223563").unwrap();
        let [east, west] = [eqearth.forward(180.0, 60.0), eqearth.forward(-180.0, 60.0)];
        assert!(east[0] > 0.0 && east[0] == -west[0]);
        assert!(close(eqearth.forward(190.0, 60.0), eqearth.forward(-170.0, 60.0), 1e-6));
    }

    #[test]
    fn projjson_matches_proj_string() {
        let laea = r#"{"type":"ProjectedCRS","name":"ETRS89-extended / LAEA Europe","base_crs":{"type":"GeographicCRS","name":"ETRS89",
            "datum_ensemble":{"name":"ETRS89","members":[],"ellipsoid":{"name":"GRS 1980","semi_major_axis":6378137,"inverse_flattening":298.257222101}}},
            "conversion":{"name":"Europe Equal Area 2001","method":{"name":"Lambert Azimuthal Equal Area","id":{"authority":"EPSG","code":9820}},
            "parameters":[{"name":"Latitude of natural origin","value":52,"unit":"degree","id":{"authority":"EPSG","code":8801}},
            {"name":"Longitude of natural origin","value":10,"unit":"degree","id":{"authority":"EPSG","code":8802}},
            {"name":"False easting","value":4321000,"unit":"metre","id":{"authority":"EPSG","code":8806}},
            {"name":"False northing","value":3210000,"unit":"metre"}]},
            "coordinate_system":{"subtype":"Cartesian","axis":[{"name":"Northing","abbreviation":"Y","direction":"north","unit":"metre"},{"name":"Easting","abbreviation":"X","direction":"east","unit":"metre"}]}}"#;
        let a = Projection::parse(laea).unwrap().forward(2.35, 48.85);
        let b = Projection::from_proj_string(
            "+proj=laea +lat_0=52 +lon_0=10 +x_0=4321000 +y_0=3210000 +a=6378137 +rf=298.257222101",
        )
        .unwrap()
        .forward(2.35, 48.85);
        assert_eq!(a, b);
        let geographic = r#"{"type":"GeographicCRS","name":"WGS 84","datum":{"type":"GeodeticReferenceFrame","name":"WGS84","ellipsoid":{"name":"WGS 84","semi_major_axis":6378137,"inverse_flattening":298.257223563}}}"#;
        assert!(matches!(
            Projection::parse(geographic).unwrap(),
            Projection::LongLat
        ));
        assert!(Projection::parse(r#"{"type":"BoundCRS"}"#).is_err());
        // PROJ's own geos method and satellite height, matched by name.
        let himawari = r#"{"type":"ProjectedCRS","name":"Himawari","base_crs":{"name":"Himawari geographic","datum":{"type":"GeodeticReferenceFrame","name":"Himawari",
            "ellipsoid":{"name":"Himawari","semi_major_axis":6378137,"semi_minor_axis":6356752.3}}},
            "conversion":{"name":"Geostationary Satellite 140.7E","method":{"name":"Geostationary Satellite (Sweep Y)"},"parameters":[
            {"name":"Longitude of natural origin","value":140.7,"unit":"degree","id":{"authority":"EPSG","code":8802}},
            {"name":"Satellite Height","value":35785863,"unit":"metre"},
            {"name":"False easting","value":0,"unit":"metre","id":{"authority":"EPSG","code":8806}}]},
            "coordinate_system":{"subtype":"Cartesian","axis":[{"name":"Easting","direction":"east","unit":"metre"},{"name":"Northing","direction":"north","unit":"metre"}]}}"#;
        let a = Projection::parse(himawari).unwrap().forward(139.76, 35.68);
        let b = Projection::from_proj_string("+proj=geos +h=35785863 +lon_0=140.7 +a=6378137 +b=6356752.3")
            .unwrap()
            .forward(139.76, 35.68);
        assert_eq!(a, b);
        assert_eq!(Projection::parse(&test_crs::himawari()).unwrap().forward(139.76, 35.68), b);
        assert!(Projection::parse("+proj=longlat").is_err());
    }

    #[test]
    fn projjson_edge_cases() {
        use test_crs::projected;
        // WKT's inverse flattening 0: a sphere.
        let sphere = r#""semi_major_axis":6371000,"inverse_flattening":0"#;
        let p = Projection::parse(&projected("Transverse Mercator", sphere, &[])).unwrap();
        assert!(p.forward(1.0, 1.0).iter().all(|v| v.is_finite()));
        // A pole a few ulps off 90° (degrees from radians) is still polar.
        let lat = (90f64.to_radians() * 1.0000000000000002).to_degrees();
        assert!(Projection::parse(&projected("Polar Stereographic (variant A)", test_crs::WGS84, &[("Latitude of natural origin", lat)])).is_ok());
        // Lambert conformal conic: the cone's gap behind the apex is off the map.
        let lcc = Projection::parse(&projected(
            "Lambert Conic Conformal (2SP)",
            test_crs::WGS84,
            &[("Latitude of 1st standard parallel", 30.0), ("Latitude of 2nd standard parallel", 60.0), ("Latitude of false origin", 45.0)],
        ))
        .unwrap();
        let apex = lcc.forward(0.0, 90.0);
        assert_eq!(lcc.inverse([1e6, apex[1] + 5e6]), None);
        let p = lcc.inverse([1e6, 0.0]).unwrap();
        assert!(close(lcc.forward(p[0], p[1]), [1e6, 0.0], 1e-3));
    }
}
