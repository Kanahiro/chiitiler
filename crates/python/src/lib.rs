//! Python bindings of the `ml2drender` renderer (the same sans-IO core as `crates/napi` and
//! `crates/wasm`). `render` runs the whole `needs` → load → `provide` loop with a Python callable
//! for loading; `Renderer` exposes the steps for hosts that fetch concurrently.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

fn error(e: ml2drender::Error) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

/// A Web Mercator camera: `center` (lng, lat) in degrees, MapLibre `zoom` (the world is
/// 512 * 2^zoom CSS pixels wide), `bearing` in degrees clockwise, the output size in CSS pixels
/// (times `pixel_ratio` in pixels) and `margin` CSS pixels rendered around it and cropped.
#[pyclass(frozen, from_py_object)]
#[derive(Clone)]
struct View(ml2drender::View);

#[pymethods]
impl View {
    #[new]
    #[pyo3(signature = (center, zoom, width, height, bearing = 0.0, pixel_ratio = 1.0, margin = 0))]
    fn new(center: (f64, f64), zoom: f64, width: u32, height: u32, bearing: f64, pixel_ratio: f64, margin: u32) -> Self {
        View(ml2drender::View { center: [center.0, center.1], zoom, bearing, width, height, pixel_ratio, margin })
    }

    /// The XYZ tile `z/x/y` of the Web Mercator grid, `tile_size` CSS pixels square.
    #[staticmethod]
    #[pyo3(signature = (z, x, y, tile_size = 256))]
    fn tile(z: u8, x: u32, y: u32, tile_size: u32) -> Self {
        View(ml2drender::View::tile(z, x, y, tile_size))
    }

    /// Output width and height in pixels.
    #[getter]
    fn size(&self) -> (u32, u32) {
        let v = &self.0;
        ((v.width as f64 * v.pixel_ratio).round() as u32, (v.height as f64 * v.pixel_ratio).round() as u32)
    }
}

/// A style fixed at creation, drawn for a view once the resources it `needs` are provided.
#[pyclass(unsendable)]
struct Renderer(ml2drender::Renderer);

#[pymethods]
impl Renderer {
    #[new]
    fn new(style: &str, view: View) -> PyResult<Self> {
        Ok(Renderer(ml2drender::Renderer::new(style, view.0).map_err(error)?))
    }

    /// Point the renderer at another camera, keeping the resources it has.
    fn set_view(&mut self, view: View) {
        self.0.set_view(view.0);
    }

    /// URLs of the resources still needed before `render`.
    fn needs(&self) -> Vec<String> {
        self.0.needs().into_iter().map(|r| r.url).collect()
    }

    /// The bytes of `url`, or None for a missing resource (like HTTP 404).
    #[pyo3(signature = (url, data))]
    fn provide(&mut self, url: &str, data: Option<&[u8]>) {
        self.0.provide(url, data);
    }

    /// RGBA8 pixels, row by row (see `View.size`).
    fn render<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        Ok(PyBytes::new(py, &self.0.render().map_err(error)?))
    }
}

/// RGBA8 pixels of `style` (JSON) in `view`, loading each resource with `load(url)`, which returns
/// its bytes or None when missing (like HTTP 404: a missing vector tile is empty, a missing
/// raster tile is replaced by its parent). Exceptions from `load` propagate.
#[pyfunction]
fn render<'py>(py: Python<'py>, style: &str, view: View, load: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyBytes>> {
    let mut r = ml2drender::Renderer::new(style, view.0).map_err(error)?;
    // Resources come in stages (style sources, tiles, glyph ranges), each found from the last.
    for round in 0.. {
        let needs = r.needs();
        if needs.is_empty() {
            break;
        }
        if round >= 32 {
            return Err(PyRuntimeError::new_err("resource requests did not settle"));
        }
        for res in needs {
            let data = load.call1((res.url.as_str(),))?;
            if data.is_none() {
                r.provide(&res.url, None);
            } else {
                r.provide(&res.url, Some(data.cast::<PyBytes>()?.as_bytes()));
            }
        }
    }
    Ok(PyBytes::new(py, &r.render().map_err(error)?))
}

/// Lossless PNG of RGBA8 pixels.
#[pyfunction]
fn encode_png<'py>(py: Python<'py>, rgba: &[u8], width: u32, height: u32) -> PyResult<Bound<'py, PyBytes>> {
    Ok(PyBytes::new(py, &ml2drender::encode_png(rgba, width, height).map_err(error)?))
}

/// Lossless WebP of RGBA8 pixels.
#[pyfunction]
fn encode_webp<'py>(py: Python<'py>, rgba: &[u8], width: u32, height: u32) -> PyResult<Bound<'py, PyBytes>> {
    Ok(PyBytes::new(py, &ml2drender::encode_webp(rgba, width, height).map_err(error)?))
}

/// JPEG of RGBA8 pixels at `quality` (1–100), flattened over black.
#[pyfunction]
#[pyo3(signature = (rgba, width, height, quality = 90))]
fn encode_jpeg<'py>(py: Python<'py>, rgba: &[u8], width: u32, height: u32, quality: u8) -> PyResult<Bound<'py, PyBytes>> {
    Ok(PyBytes::new(py, &ml2drender::encode_jpeg(rgba, width, height, quality).map_err(error)?))
}

#[pymodule]
#[pyo3(name = "ml2drender")]
fn ml2drender_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<View>()?;
    m.add_class::<Renderer>()?;
    m.add_function(wrap_pyfunction!(render, m)?)?;
    m.add_function(wrap_pyfunction!(encode_png, m)?)?;
    m.add_function(wrap_pyfunction!(encode_webp, m)?)?;
    m.add_function(wrap_pyfunction!(encode_jpeg, m)?)?;
    Ok(())
}
