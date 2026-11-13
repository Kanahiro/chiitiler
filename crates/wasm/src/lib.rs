//! Plain C-ABI exports for wasm32-unknown-unknown (no wasm-bindgen).
//!
//! Byte results (needs list, RGBA, PNG, error messages) go to a shared output buffer read via
//! `out_ptr`/`out_len`; it stays valid until the next call. Inputs are copied in via `alloc`.

use ml2drender::{reproject::ProjectedView, Projection, Renderer, View};
use std::cell::RefCell;

thread_local! {
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// The point `projection_forward` / `projection_inverse` computed, read via `xy_ptr`.
    static XY: RefCell<[f64; 2]> = const { RefCell::new([0.0; 2]) };
}

fn set_out(bytes: Vec<u8>) {
    OUT.with(|o| *o.borrow_mut() = bytes);
}

unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(ptr, len) } }
}

#[unsafe(no_mangle)]
pub extern "C" fn out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr`/`len` must come from `alloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// Returns a renderer handle, or null with the error message in the output buffer.
///
/// # Safety
/// `style_ptr`/`style_len` must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_new(
    style_ptr: *const u8,
    style_len: usize,
    lng: f64,
    lat: f64,
    zoom: f64,
    bearing: f64,
    width: u32,
    height: u32,
    pixel_ratio: f64,
    margin: u32,
) -> *mut Renderer {
    let style = String::from_utf8_lossy(unsafe { bytes(style_ptr, style_len) });
    let view = View { center: [lng, lat], zoom, bearing, width, height, pixel_ratio, margin };
    match Renderer::new(&style, view) {
        Ok(r) => Box::into_raw(Box::new(r)),
        Err(e) => {
            set_out(e.to_string().into_bytes());
            std::ptr::null_mut()
        }
    }
}

/// Moves the camera of a live renderer, keeping its fetched and decoded resources.
///
/// # Safety
/// `r` must be a live handle from `renderer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_set_view(
    r: *mut Renderer,
    lng: f64,
    lat: f64,
    zoom: f64,
    bearing: f64,
    width: u32,
    height: u32,
    pixel_ratio: f64,
    margin: u32,
) {
    unsafe { &mut *r }.set_view(View { center: [lng, lat], zoom, bearing, width, height, pixel_ratio, margin });
}

/// Renders a box of a projected CRS instead of a Mercator camera (see
/// `Renderer::set_projected_view`). Returns 1, or 0 with an error message in the output buffer.
///
/// # Safety
/// `r` must be a live handle from `renderer_new`; `projjson_ptr`/`projjson_len` must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_set_projected_view(
    r: *mut Renderer,
    projjson_ptr: *const u8,
    projjson_len: usize,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    width: u32,
    height: u32,
    pixel_ratio: f64,
    margin: u32,
) -> u32 {
    let projjson = String::from_utf8_lossy(unsafe { bytes(projjson_ptr, projjson_len) }).into_owned();
    let view = ProjectedView { projjson, bbox: [min_x, min_y, max_x, max_y], width, height, pixel_ratio, margin };
    match unsafe { &mut *r }.set_projected_view(&view) {
        Ok(()) => 1,
        Err(e) => {
            set_out(e.to_string().into_bytes());
            0
        }
    }
}

/// See `Renderer::set_keep_buckets`.
///
/// # Safety
/// `r` must be a live handle from `renderer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_set_keep_buckets(r: *mut Renderer, keep: u32) {
    unsafe { &mut *r }.set_keep_buckets(keep != 0);
}

/// Tiles needed or provided from now on are kept for render `ticket` (see
/// `Renderer::set_ticket`); a JS number, an integer below 2^53.
///
/// # Safety
/// `r` must be a live handle from `renderer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_set_ticket(r: *mut Renderer, ticket: f64) {
    unsafe { &mut *r }.set_ticket(ticket as u64);
}

/// Writes the needed URLs, newline-separated, to the output buffer; returns their count.
///
/// # Safety
/// `r` must be a live handle from `renderer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_needs(r: *mut Renderer) -> usize {
    let needs = unsafe { &*r }.needs();
    set_out(needs.iter().map(|n| n.url.as_str()).collect::<Vec<_>>().join("\n").into_bytes());
    needs.len()
}

/// `found == 0` means the host could not fetch the URL.
///
/// # Safety
/// `r` must be a live handle; pointers must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_provide(
    r: *mut Renderer,
    url_ptr: *const u8,
    url_len: usize,
    data_ptr: *const u8,
    data_len: usize,
    found: u32,
) {
    let url = String::from_utf8_lossy(unsafe { bytes(url_ptr, url_len) });
    let data = (found != 0).then(|| unsafe { bytes(data_ptr, data_len) });
    unsafe { &mut *r }.provide(&url, data);
}

/// Returns 1 with RGBA in the output buffer, or 0 with an error message. Keeps the tiles of
/// tickets `oldest` and later (see `renderer_set_ticket`).
///
/// # Safety
/// `r` must be a live handle from `renderer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_render(r: *mut Renderer, oldest: f64) -> u32 {
    match unsafe { &mut *r }.render_retaining(oldest as u64) {
        Ok(rgba) => {
            set_out(rgba);
            1
        }
        Err(e) => {
            set_out(e.to_string().into_bytes());
            0
        }
    }
}

/// See `Renderer::set_export_source`; an empty name is `None`. Returns 1, or 0 with an error
/// message in the output buffer.
///
/// # Safety
/// `r` must be a live handle; `ptr`/`len` must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_set_export_source(r: *mut Renderer, ptr: *const u8, len: usize) -> u32 {
    let source = String::from_utf8_lossy(unsafe { bytes(ptr, len) });
    match unsafe { &mut *r }.set_export_source(Some(&*source).filter(|s| !s.is_empty())) {
        Ok(()) => 1,
        Err(e) => {
            set_out(e.to_string().into_bytes());
            0
        }
    }
}

/// Returns 1 with the export source's tile in the output buffer (see `Renderer::export_tile`), or 0
/// with an error message.
///
/// # Safety
/// `r` must be a live handle from `renderer_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_export_tile(r: *mut Renderer, oldest: f64) -> u32 {
    match unsafe { &mut *r }.export_tile(oldest as u64) {
        Ok(tile) => {
            set_out(tile);
            1
        }
        Err(e) => {
            set_out(e.to_string().into_bytes());
            0
        }
    }
}

/// Encodes RGBA8 pixels; returns 1 with PNG in the output buffer, or 0 with an error message.
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn encode_png(ptr: *const u8, len: usize, width: u32, height: u32) -> u32 {
    match ml2drender::encode_png(unsafe { bytes(ptr, len) }, width, height) {
        Ok(png) => {
            set_out(png);
            1
        }
        Err(e) => {
            set_out(e.to_string().into_bytes());
            0
        }
    }
}

/// Encodes RGBA8 pixels; returns 1 with lossless WebP in the output buffer, or 0 with an error message.
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn encode_webp(ptr: *const u8, len: usize, width: u32, height: u32) -> u32 {
    match ml2drender::encode_webp(unsafe { bytes(ptr, len) }, width, height) {
        Ok(webp) => {
            set_out(webp);
            1
        }
        Err(e) => {
            set_out(e.to_string().into_bytes());
            0
        }
    }
}

/// Encodes RGBA8 pixels; returns 1 with a JPEG (flattened over black) in the output buffer, or 0 with an error message.
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn encode_jpeg(ptr: *const u8, len: usize, width: u32, height: u32, quality: u32) -> u32 {
    match ml2drender::encode_jpeg(unsafe { bytes(ptr, len) }, width, height, quality.min(100) as u8) {
        Ok(jpeg) => {
            set_out(jpeg);
            1
        }
        Err(e) => {
            set_out(e.to_string().into_bytes());
            0
        }
    }
}

/// Returns a projection handle for a PROJJSON CRS, or null with the error message in
/// the output buffer.
///
/// # Safety
/// `ptr`/`len` must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn projection_new(ptr: *const u8, len: usize) -> *mut Projection {
    match Projection::parse(&String::from_utf8_lossy(unsafe { bytes(ptr, len) })) {
        Ok(p) => Box::into_raw(Box::new(p)),
        Err(e) => {
            set_out(e.to_string().into_bytes());
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn xy_ptr() -> *const f64 {
    XY.with(|xy| xy.as_ptr() as *const f64)
}

fn set_xy(p: [f64; 2]) -> u32 {
    XY.with(|xy| *xy.borrow_mut() = p);
    (p[0].is_finite() && p[1].is_finite()) as u32
}

/// Degrees → CRS units at `xy_ptr`; returns 0 where the CRS has no point (e.g. the far side of a
/// geostationary view).
///
/// # Safety
/// `p` must be a live handle from `projection_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn projection_forward(p: *const Projection, lng: f64, lat: f64) -> u32 {
    set_xy(unsafe { &*p }.forward(lng, lat))
}

/// CRS units → degrees at `xy_ptr`; returns 0 outside the projection's domain.
///
/// # Safety
/// `p` must be a live handle from `projection_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn projection_inverse(p: *const Projection, x: f64, y: f64) -> u32 {
    set_xy(unsafe { &*p }.inverse([x, y]).unwrap_or([f64::NAN; 2]))
}

/// # Safety
/// `p` must be a live handle from `projection_new`; it is invalid afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn projection_free(p: *mut Projection) {
    drop(unsafe { Box::from_raw(p) });
}

/// # Safety
/// `r` must be a live handle from `renderer_new`; it is invalid afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn renderer_free(r: *mut Renderer) {
    drop(unsafe { Box::from_raw(r) });
}
