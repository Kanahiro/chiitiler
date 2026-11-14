//! Node-API bindings of the `ml2drender` renderer: the same sans-IO `Renderer` as the WASM
//! bindings (`crates/wasm`), for Node. The host loops `needs` → fetch → `provide` and then
//! `render`s.

use napi::bindgen_prelude::*;
use napi_derive::napi;

/// A Web Mercator camera (see `ml2drender::View`).
#[napi(object)]
pub struct View {
    pub lng: f64,
    pub lat: f64,
    pub zoom: f64,
    pub bearing: f64,
    pub width: u32,
    pub height: u32,
    pub pixel_ratio: f64,
    pub margin: u32,
}

impl From<View> for ml2drender::View {
    fn from(v: View) -> Self {
        ml2drender::View { center: [v.lng, v.lat], zoom: v.zoom, bearing: v.bearing, width: v.width, height: v.height, pixel_ratio: v.pixel_ratio, margin: v.margin }
    }
}

fn error(e: ml2drender::Error) -> Error {
    Error::from_reason(e.to_string())
}

#[napi]
pub struct Renderer {
    inner: ml2drender::Renderer,
}

#[napi]
impl Renderer {
    #[napi(constructor)]
    pub fn new(style: String, view: View) -> Result<Self> {
        Ok(Renderer { inner: ml2drender::Renderer::new(&style, view.into()).map_err(error)? })
    }

    #[napi]
    pub fn set_view(&mut self, view: View) {
        self.inner.set_view(view.into());
    }

    #[napi]
    pub fn set_ticket(&mut self, ticket: f64) {
        self.inner.set_ticket(ticket as u64);
    }

    #[napi]
    pub fn set_keep_buckets(&mut self, keep: bool) {
        self.inner.set_keep_buckets(keep);
    }

    /// URLs of the resources still needed before `render`.
    #[napi]
    pub fn needs(&self) -> Vec<String> {
        self.inner.needs().into_iter().map(|r| r.url).collect()
    }

    /// The bytes of `url`, or null for a missing resource (HTTP 404).
    #[napi]
    pub fn provide(&mut self, url: String, bytes: Option<Uint8Array>) {
        self.inner.provide(&url, bytes.as_deref());
    }

    /// RGBA8 pixels, keeping the tiles of renders with tickets from `oldest` on.
    #[napi]
    pub fn render(&mut self, oldest: f64) -> Result<Uint8Array> {
        Ok(self.inner.render_retaining(oldest as u64).map_err(error)?.into())
    }
}

#[napi]
pub fn encode_png(rgba: Uint8Array, width: u32, height: u32) -> Result<Uint8Array> {
    Ok(ml2drender::encode_png(&rgba, width, height).map_err(error)?.into())
}

#[napi]
pub fn encode_webp(rgba: Uint8Array, width: u32, height: u32) -> Result<Uint8Array> {
    Ok(ml2drender::encode_webp(&rgba, width, height).map_err(error)?.into())
}

#[napi]
pub fn encode_jpeg(rgba: Uint8Array, width: u32, height: u32, quality: u32) -> Result<Uint8Array> {
    Ok(ml2drender::encode_jpeg(&rgba, width, height, quality.min(100) as u8).map_err(error)?.into())
}
