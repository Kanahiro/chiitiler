//! Runs maplibre-gl-js render tests (fixtures/render) and reports pass rates.

use crate::{find_files, read_json, Tally};
use ml2drender::{Renderer, View};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// Operations with no effect on a static render.
const NOOP_OPERATIONS: &[&str] = &["wait", "idle", "sleep", "redraw"];

enum Outcome {
    Pass(f64),
    Fail(String),
    Skip(String),
}

pub fn run(root: &Path, filter: Option<&str>) {
    let tests_dir = root.join("fixtures/render/tests");
    let assets = root.join("fixtures/render/assets");
    let exclusions = read_json(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/exclusions.json"));
    let out_dir = root.join("target/test-report/render");
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).unwrap();

    let mut overall = Tally::default();
    let mut by_layer: BTreeMap<String, Tally> = BTreeMap::new();
    let mut skip_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut fail_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut results = Vec::new();

    for style_path in find_files(&tests_dir, "style.json") {
        let dir = style_path.parent().unwrap();
        let id = dir.strip_prefix(&tests_dir).unwrap().to_string_lossy().replace('\\', "/");
        if filter.is_some_and(|f| !id.contains(f)) {
            continue;
        }
        let style = read_json(&style_path);
        let layer_types: BTreeSet<&str> =
            style["layers"].as_array().into_iter().flatten().filter_map(|l| l["type"].as_str()).collect();

        let (outcome, hash) = match exclusion(&exclusions, &style) {
            Some(reason) => (Outcome::Skip(reason), None),
            None => run_one(dir, &style, &assets, &out_dir.join(&id)),
        };
        let (status, detail) = match &outcome {
            Outcome::Pass(d) => ("pass", json!(d)),
            Outcome::Fail(e) => ("fail", json!(e)),
            Outcome::Skip(r) => ("skip", json!(r)),
        };
        results.push(json!({"id": id, "status": status, "detail": detail, "layers": layer_types, "hash": hash}));
        match &outcome {
            Outcome::Skip(r) => *skip_reasons.entry(r.clone()).or_default() += 1,
            Outcome::Fail(e) => {
                let key = if e.starts_with("diff ") { "pixel diff above allowed".to_string() } else { e.lines().next().unwrap_or("").to_string() };
                *fail_reasons.entry(key).or_default() += 1
            }
            Outcome::Pass(_) => {}
        }
        overall.add(status);
        for t in &layer_types {
            by_layer.entry(t.to_string()).or_default().add(status);
        }
    }

    let mut md = format!("## Render tests\n\n{}\n\n| layer type | pass | fail | skip | pass rate (excl. skip) |\n|---|---|---|---|---|\n", overall.line());
    for (t, tally) in &by_layer {
        md += &tally.row(t);
    }
    md += "\n(a test counts under every layer type it uses)\n\n### Skip reasons\n\n";
    for (r, n) in &skip_reasons {
        md += &format!("- {n}: {r}\n");
    }
    md += "\n### Fail reasons\n\n";
    for (r, n) in &fail_reasons {
        md += &format!("- {n}: {r}\n");
    }
    print!("{md}");
    fs::write(out_dir.join("summary.md"), &md).unwrap();
    fs::write(out_dir.join("results.json"), serde_json::to_string_pretty(&results).unwrap()).unwrap();
    eprintln!("report: {}", out_dir.display());
}

/// The outcome, and a hash of the compared RGBA (to check that an optimization keeps the output
/// bit for bit: compare `hash` in results.json before and after).
fn run_one(dir: &Path, style: &Value, assets: &Path, out_dir: &Path) -> (Outcome, Option<String>) {
    let mut hash = None;
    (compare_one(dir, style, assets, out_dir, &mut hash), hash)
}

fn compare_one(dir: &Path, style: &Value, assets: &Path, out_dir: &Path, hash: &mut Option<String>) -> Outcome {
    let t = &style["metadata"]["test"];
    let width = t["width"].as_u64().unwrap_or(512) as u32;
    let height = t["height"].as_u64().unwrap_or(512) as u32;
    // The harness passes `maxCanvasSize: [8192, 8192]`; MapLibre lowers the pixel ratio to fit.
    let requested_ratio = t["pixelRatio"].as_f64().unwrap_or(1.0);
    let fit = |size: u32| (8192.0 / (size as f64 * requested_ratio)).min(1.0);
    let pixel_ratio = requested_ratio * fit(width).min(fit(height));
    let allowed = t["allowed"].as_f64().unwrap_or(0.00025);
    let threshold = t["threshold"].as_f64().unwrap_or(0.1285);

    let num = |v: &Value| v.as_f64().unwrap_or(0.0);
    let view = View {
        center: [num(&style["center"][0]), num(&style["center"][1])],
        zoom: num(&style["zoom"]),
        bearing: num(&style["bearing"]),
        width,
        height,
        pixel_ratio,
        margin: 0,
    };
    let max_zoom = t["maxZoom"].as_f64();
    let cross_source = t["crossSourceCollisions"].as_bool().unwrap_or(true);
    let rgba = match render(style, view, max_zoom, cross_source, assets) {
        Ok(rgba) => rgba,
        Err(e) => return Outcome::Fail(e),
    };
    let (cw, ch) = ((width as f64 * pixel_ratio).round() as usize, (height as f64 * pixel_ratio).round() as usize);
    if rgba.len() != cw * ch * 4 {
        return Outcome::Fail(format!("renderer returned {} bytes, expected {cw}x{ch} RGBA", rgba.len()));
    }
    // `readPixels(0, 0, w, h)` reads the bottom-left corner (GL's origin).
    let w = t["reportWidth"].as_u64().map_or(cw, |v| v as usize).min(cw);
    let h = t["reportHeight"].as_u64().map_or(ch, |v| v as usize).min(ch);
    let rgba: Vec<u8> = (ch - h..ch).flat_map(|y| rgba[(y * cw) * 4..(y * cw + w) * 4].to_vec()).collect();
    *hash = Some({
        use std::hash::{Hash, Hasher};
        // SipHash with fixed keys: stable across runs and builds of the same toolchain.
        #[allow(deprecated)]
        let mut h = std::hash::SipHasher::new();
        rgba.hash(&mut h);
        format!("{:016x}", h.finish())
    });

    // Like upstream: compare against every expected*.png, keep the closest.
    let mut best: Option<(f64, Vec<u8>, Vec<u8>)> = None;
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !(name.starts_with("expected") && name.ends_with(".png")) {
            continue;
        }
        let Some((ew, eh, expected)) = decode_png(&entry.path()) else { continue };
        if (ew, eh) != (w, h) {
            continue;
        }
        let mut diff_img = vec![0; w * h * 4];
        let diff = crate::pixelmatch::pixelmatch(&rgba, &expected, &mut diff_img, w, h, threshold) as f64 / (w * h) as f64;
        if best.as_ref().is_none_or(|b| diff < b.0) {
            best = Some((diff, diff_img, expected));
        }
    }
    let Some((diff, diff_img, expected)) = best else {
        return Outcome::Fail(format!("no expected*.png of size {w}x{h}"));
    };
    if diff <= allowed {
        return Outcome::Pass(diff);
    }
    fs::create_dir_all(out_dir).unwrap();
    for (name, img) in [("actual.png", &rgba), ("expected.png", &expected), ("diff.png", &diff_img)] {
        encode_png(&out_dir.join(name), w, h, img);
    }
    Outcome::Fail(format!("diff {diff:.5} > allowed {allowed}"))
}

/// The host loop: answer `needs()` from the local assets until it is empty, then render.
fn render(style: &Value, view: View, max_zoom: Option<f64>, cross_source_collisions: bool, assets: &Path) -> Result<Vec<u8>, String> {
    let mut r = Renderer::new(&style.to_string(), view).map_err(|e| e.to_string())?;
    r.set_cross_source_collisions(cross_source_collisions);
    if let Some(z) = max_zoom {
        r.set_max_zoom(z);
    }
    let mut rounds = 0;
    loop {
        let needs = r.needs();
        if needs.is_empty() {
            break;
        }
        rounds += 1;
        if rounds > 32 {
            return Err(format!("needs() not settling after {rounds} rounds: {:?}", needs[0].url));
        }
        for res in needs {
            // The upstream test server answers 204 (empty body) for these, 404 otherwise.
            let bytes = if res.url.contains("/sparse204/1-") { Some(vec![]) } else { local_path(&res.url, assets).and_then(|p| fs::read(p).ok()) };
            r.provide(&res.url, bytes.as_deref());
        }
    }
    r.render().map_err(|e| e.to_string())
}

/// `local://x` and the upstream harness server `http://localhost:2900/x` both map to assets/x.
fn local_path(url: &str, assets: &Path) -> Option<std::path::PathBuf> {
    let rel = url.strip_prefix("local://").or_else(|| url.strip_prefix("http://localhost:2900/"))?;
    let rel = rel.split(['?', '#']).next().unwrap();
    Some(assets.join(percent_decode(rel)))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn exclusion(ex: &Value, style: &Value) -> Option<String> {
    let rule = |table: &str, key: &str| ex[table][key].as_str().map(|r| format!("{table}.{key}: {r}"));
    let nonzero = |v: &Value| v.as_f64().is_some_and(|x| x != 0.0);
    if nonzero(&style["pitch"]) {
        return Some("pitch != 0 (3D)".into());
    }
    if nonzero(&style["roll"]) {
        return Some("roll != 0 (3D)".into());
    }
    if style.get("projection").is_some_and(|p| p["type"] != "mercator") {
        return Some("non-mercator projection".into());
    }
    let t = &style["metadata"]["test"];
    let ops = t["operations"].as_array().into_iter().flatten().filter_map(|o| o[0].as_str());
    if let Some(op) = ops.into_iter().find(|op| !NOOP_OPERATIONS.contains(op)) {
        return Some(format!("operation `{op}`: runtime style/camera changes out of scope"));
    }
    let layers = style["layers"].as_array().into_iter().flatten();
    let sources = style["sources"].as_object().into_iter().flat_map(|o| o.values());
    let checks = [
        layers.clone().find_map(|l| rule("layer_types", l["type"].as_str()?)),
        sources.clone().find_map(|s| rule("source_types", s["type"].as_str()?)),
        sources.clone().find_map(|s| rule("source_encodings", s["encoding"].as_str()?)),
        style.as_object()?.keys().find_map(|k| rule("style_properties", k)),
        t.as_object().into_iter().flat_map(|o| o.keys()).find_map(|k| rule("test_options", k)),
    ];
    checks.into_iter().flatten().next()
}

/// Mismatched pixel count between two same-sized PNGs.
pub fn compare(a: &Path, b: &Path, threshold: f64) -> usize {
    let (w, h, a) = decode_png(a).expect("decode a");
    let (_, _, b) = decode_png(b).expect("decode b");
    crate::pixelmatch::pixelmatch(&a, &b, &mut vec![0; w * h * 4], w, h, threshold)
}

fn decode_png(path: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).ok()?));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None, // EXPAND removes palettes
    };
    Some((info.width as usize, info.height as usize, rgba))
}

fn encode_png(path: &Path, w: usize, h: usize, rgba: &[u8]) {
    let mut enc = png::Encoder::new(std::io::BufWriter::new(fs::File::create(path).unwrap()), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}
