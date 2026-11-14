//! Node-API bindings of the `ml2drender` renderer: the same sans-IO `Renderer` as the WASM
//! bindings (`crates/wasm`), for Node. The host loops `needs` → fetch → `provide` and then
//! `render`s. The work (deciding needs, decoding provided resources, drawing, encoding) runs on
//! libuv's thread pool, so the JavaScript thread stays free and renderers run in parallel.

use std::sync::{Arc, Mutex};

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

/// A renderer that moves between threads as a whole.
struct Movable(ml2drender::Renderer);

// SAFETY: the renderer's `Rc`s and `RefCell`s point only into the renderer itself (the core keeps
// no thread-locals and only an immutable `OnceLock` static), so it can move to another thread as
// a whole; the mutex lets one thread use it at a time.
unsafe impl Send for Movable {}

type Shared = Arc<Mutex<Movable>>;

/// Runs `f` on the renderer, turning a panic into an error (it must not unwind out of a
/// thread-pool callback). A renderer that panicked may be inconsistent: the host drops a core
/// whose operation failed.
fn with<T>(shared: &Shared, f: impl FnOnce(&mut ml2drender::Renderer) -> T) -> Result<T> {
    let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut guard.0))).map_err(|panic| {
        let message = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned());
        Error::from_reason(format!("renderer panicked: {}", message.unwrap_or_default()))
    })
}

/// Calls must not overlap on one renderer (the host queues them); the mutex only makes an
/// overlap wait instead of racing.
#[napi]
pub struct Renderer {
    inner: Shared,
}

#[napi]
impl Renderer {
    #[napi(constructor)]
    pub fn new(style: String, view: View) -> Result<Self> {
        let renderer = ml2drender::Renderer::new(&style, view.into()).map_err(error)?;
        Ok(Renderer { inner: Arc::new(Mutex::new(Movable(renderer))) })
    }

    #[napi]
    pub fn set_view(&self, view: View) -> Result<()> {
        with(&self.inner, |r| r.set_view(view.into()))
    }

    #[napi]
    pub fn set_ticket(&self, ticket: f64) -> Result<()> {
        with(&self.inner, |r| r.set_ticket(ticket as u64))
    }

    #[napi]
    pub fn set_keep_buckets(&self, keep: bool) -> Result<()> {
        with(&self.inner, |r| r.set_keep_buckets(keep))
    }

    /// URLs of the resources still needed before `render`.
    #[napi(ts_return_type = "Promise<string[]>")]
    pub fn needs(&self) -> AsyncTask<Needs> {
        AsyncTask::new(Needs(self.inner.clone()))
    }

    /// The bytes of `url` (copied), or null for a missing resource (HTTP 404).
    #[napi(ts_return_type = "Promise<void>")]
    pub fn provide(&self, url: String, bytes: Option<Uint8Array>) -> AsyncTask<Provide> {
        AsyncTask::new(Provide { inner: self.inner.clone(), url, bytes: bytes.map(|b| b.to_vec()) })
    }

    /// RGBA8 pixels, keeping the tiles of renders with tickets from `oldest` on.
    #[napi(ts_return_type = "Promise<Uint8Array>")]
    pub fn render(&self, oldest: f64) -> AsyncTask<Render> {
        AsyncTask::new(Render { inner: self.inner.clone(), oldest: oldest as u64 })
    }
}

pub struct Needs(Shared);

impl Task for Needs {
    type Output = Vec<String>;
    type JsValue = Vec<String>;

    fn compute(&mut self) -> Result<Self::Output> {
        with(&self.0, |r| r.needs().into_iter().map(|r| r.url).collect())
    }

    fn resolve(&mut self, _: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct Provide {
    inner: Shared,
    url: String,
    bytes: Option<Vec<u8>>,
}

impl Task for Provide {
    type Output = ();
    type JsValue = ();

    fn compute(&mut self) -> Result<Self::Output> {
        let bytes = self.bytes.take();
        with(&self.inner, |r| r.provide(&self.url, bytes.as_deref()))
    }

    fn resolve(&mut self, _: Env, _: Self::Output) -> Result<Self::JsValue> {
        Ok(())
    }
}

pub struct Render {
    inner: Shared,
    oldest: u64,
}

impl Task for Render {
    type Output = Vec<u8>;
    type JsValue = Uint8Array;

    fn compute(&mut self) -> Result<Self::Output> {
        with(&self.inner, |r| r.render_retaining(self.oldest))?.map_err(error)
    }

    fn resolve(&mut self, _: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output.into())
    }
}

#[derive(Clone, Copy)]
enum Format {
    Png,
    Webp,
    Jpeg(u8),
}

pub struct Encode {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    format: Format,
}

impl Task for Encode {
    type Output = Vec<u8>;
    type JsValue = Uint8Array;

    fn compute(&mut self) -> Result<Self::Output> {
        let (rgba, w, h) = (&self.rgba, self.width, self.height);
        let format = self.format;
        std::panic::catch_unwind(|| match format {
            Format::Png => ml2drender::encode_png(rgba, w, h),
            Format::Webp => ml2drender::encode_webp(rgba, w, h),
            Format::Jpeg(q) => ml2drender::encode_jpeg(rgba, w, h, q),
        })
        .map_err(|_| Error::from_reason("encoder panicked"))?
        .map_err(error)
    }

    fn resolve(&mut self, _: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output.into())
    }
}

fn encode(rgba: Uint8Array, width: u32, height: u32, format: Format) -> AsyncTask<Encode> {
    AsyncTask::new(Encode { rgba: rgba.to_vec(), width, height, format })
}

#[napi(ts_return_type = "Promise<Uint8Array>")]
pub fn encode_png(rgba: Uint8Array, width: u32, height: u32) -> AsyncTask<Encode> {
    encode(rgba, width, height, Format::Png)
}

#[napi(ts_return_type = "Promise<Uint8Array>")]
pub fn encode_webp(rgba: Uint8Array, width: u32, height: u32) -> AsyncTask<Encode> {
    encode(rgba, width, height, Format::Webp)
}

#[napi(ts_return_type = "Promise<Uint8Array>")]
pub fn encode_jpeg(rgba: Uint8Array, width: u32, height: u32, quality: u32) -> AsyncTask<Encode> {
    encode(rgba, width, height, Format::Jpeg(quality.min(100) as u8))
}
