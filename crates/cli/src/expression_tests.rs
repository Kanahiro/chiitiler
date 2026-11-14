//! Runs maplibre-style-spec expression tests (fixtures/expression), mirroring
//! test/integration/expression/expression.test.ts upstream.

use crate::{find_files, read_json, Tally};
use ml2drender::expression::{self as ex, create_property_expression};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const ROOT_KEY: &str = "layers[0].paint.some-property";
const DECIMAL_SIGNIFICANT_FIGURES: i32 = 6;

pub fn run(root: &Path, filter: Option<&str>) {
    let tests_dir = root.join("fixtures/expression/tests");
    let out_dir = root.join("target/test-report/expression");
    fs::create_dir_all(&out_dir).unwrap();

    let mut overall = Tally::default();
    let mut by_group: BTreeMap<String, Tally> = BTreeMap::new();
    let mut results = Vec::new();
    for path in find_files(&tests_dir, "test.json") {
        let id = path.parent().unwrap().strip_prefix(&tests_dir).unwrap().to_string_lossy().replace('\\', "/");
        if filter.is_some_and(|f| !id.contains(f)) {
            continue;
        }
        let fixture = read_json(&path);
        let actual = evaluate_fixture(&fixture);
        let expected = &fixture["expected"];
        let compile_ok = deep_equal(&actual["compiled"], &expected["compiled"]);
        let ok = compile_ok && deep_equal(&actual["outputs"], &expected["outputs"]);
        let status = if ok { "pass" } else { "fail" };
        overall.add(status);
        by_group.entry(id.split('/').next().unwrap().to_string()).or_default().add(status);
        if !ok {
            let stage = if compile_ok { "evaluation" } else { "compilation" };
            results.push(json!({"id": id, "stage": stage, "expected": expected, "actual": actual}));
        }
    }

    let mut md = format!("## Expression tests\n\n{}\n\n| group | pass | fail | skip | pass rate |\n|---|---|---|---|---|\n", overall.line());
    for (g, tally) in &by_group {
        md += &tally.row(g);
    }
    println!("## Expression tests\n\n{}", overall.line());
    fs::write(out_dir.join("summary.md"), &md).unwrap();
    fs::write(out_dir.join("failures.json"), serde_json::to_string_pretty(&results).unwrap()).unwrap();
    eprintln!("report: {}", out_dir.display());
}

fn evaluate_fixture(fixture: &Value) -> Value {
    let mut spec = fixture["propertySpec"].as_object().cloned().unwrap_or_default();
    spec.entry("property-type").or_insert(json!("data-driven"));
    spec.entry("expression").or_insert(json!({"interpolated": true, "parameters": ["zoom", "feature"]}));
    let spec = Value::Object(spec);

    match create_property_expression(&fixture["expression"], &spec, fixture.get("globalState")) {
        Err(errors) => json!({"compiled": {
            "result": "error",
            "errors": errors.iter().map(|e| json!({"key": e.key, "error": e.message})).collect::<Vec<_>>(),
        }}),
        Ok(expr) => {
            let is_color = expr.type_name() == "color";
            let outputs: Vec<Value> = fixture["inputs"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|input| {
                    let (feature, canonical) = fixture_feature(&input[1], &input[0]["canonicalID"]);
                    let feature_state = match ex::Value::from_json(&input[1]["featureState"]) {
                        ex::Value::Object(o) => Some(o),
                        _ => None,
                    };
                    let images: Option<Vec<String>> = input[0]["availableImages"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
                    let g = &input[0];
                    let mut ctx = ex::EvalCtx {
                        globals: ex::Globals {
                            zoom: g["zoom"].as_f64().unwrap_or(f64::NAN),
                            heatmap_density: g["heatmapDensity"].as_f64(),
                            elevation: g["elevation"].as_f64(),
                            line_progress: g["lineProgress"].as_f64(),
                            ..Default::default()
                        },
                        feature: Some(&feature),
                        feature_state: feature_state.as_deref(),
                        canonical,
                        available_images: images.as_deref(),
                    };
                    match expr.evaluate_without_error_handling(&mut ctx) {
                        Ok(ex::Value::Color(c)) if is_color => json!([c.r, c.g, c.b, c.a]),
                        Ok(v) => v.to_json(),
                        Err(e) => json!({"error": format!("{ROOT_KEY}{}: {}", e.path, e.message)}),
                    }
                })
                .collect();
            json!({
                "compiled": {
                    "result": "success",
                    "isFeatureConstant": expr.is_feature_constant(),
                    "isZoomConstant": expr.is_zoom_constant(),
                    "type": expr.type_name(),
                },
                "outputs": outputs,
            })
        }
    }
}

/// Mirrors the upstream harness: GeoJSON geometry becomes tile coordinates when a
/// canonical tile ID is given, otherwise only the geometry type is set.
fn fixture_feature(f: &Value, canonical: &Value) -> (ex::Feature, Option<ex::CanonicalTileId>) {
    let canonical = canonical.as_object().map(|c| ex::CanonicalTileId {
        z: c["z"].as_u64().unwrap_or(0) as u8,
        x: c["x"].as_u64().unwrap_or(0) as u32,
        y: c["y"].as_u64().unwrap_or(0) as u32,
    });
    let properties = match ex::Value::from_json(&f["properties"]) {
        ex::Value::Object(o) => o,
        _ => Default::default(),
    };
    let mut feature = ex::Feature { id: f.get("id").map(ex::Value::from_json), properties: ex::Properties::Object(properties), ..Default::default() };
    let geom = &f["geometry"];
    let name = |t: &str| -> &'static str {
        match t {
            "Point" => "Point",
            "MultiPoint" => "MultiPoint",
            "LineString" => "LineString",
            "MultiLineString" => "MultiLineString",
            "Polygon" => "Polygon",
            "MultiPolygon" => "MultiPolygon",
            _ => "Unknown",
        }
    };
    if let Some(t) = geom["type"].as_str() {
        match canonical {
            None => feature.geom_type = Some(name(t)),
            Some(c) if geom.get("coordinates").is_some() => {
                let point = |p: &Value| -> [f64; 2] {
                    let tiles = 2f64.powi(c.z as i32);
                    let (lng, lat) = (p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0));
                    let x = (180.0 + lng) / 360.0;
                    let y = (180.0 - (180.0 / std::f64::consts::PI) * (std::f64::consts::PI / 4.0 + lat * std::f64::consts::PI / 360.0).tan().ln()) / 360.0;
                    let round = |v: f64| { let f = v.floor(); if v - f >= 0.5 { f + 1.0 } else { f } };
                    [round(x * tiles * 8192.0) - c.x as f64 * 8192.0, round(y * tiles * 8192.0) - c.y as f64 * 8192.0]
                };
                let line = |l: &Value| -> Vec<[f64; 2]> { l.as_array().into_iter().flatten().map(point).collect() };
                let lines = |ls: &Value| -> Vec<Vec<[f64; 2]>> { ls.as_array().into_iter().flatten().map(line).collect() };
                let c = &geom["coordinates"];
                let (ty, g) = match t {
                    "Point" => ("Point", vec![vec![point(c)]]),
                    "MultiPoint" => ("Point", c.as_array().into_iter().flatten().map(|p| vec![point(p)]).collect()),
                    "LineString" => ("LineString", vec![line(c)]),
                    "MultiLineString" => ("LineString", lines(c)),
                    "Polygon" => ("Polygon", lines(c)),
                    "MultiPolygon" => ("Polygon", c.as_array().into_iter().flatten().flat_map(lines).collect()),
                    _ => ("Unknown", vec![]),
                };
                feature.geom_type = Some(ty);
                feature.geometry = Some(g);
            }
            Some(_) => {}
        }
    }
    (feature, canonical)
}

/// Upstream `deepEqual` with numbers compared after `stripPrecision` to 6 significant figures.
fn deep_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => strip_precision(x.as_f64().unwrap()) == strip_precision(y.as_f64().unwrap()),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| deep_equal(a, b)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| deep_equal(v, w))),
        _ => a == b,
    }
}

fn strip_precision(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    // `powf`, not `powi`: must match JS `Math.pow` bit for bit.
    let m = 10f64.powf((DECIMAL_SIGNIFICANT_FIGURES - x.abs().log10().ceil() as i32).max(0) as f64);
    let first = (x * m).floor() / m;
    (first * m).floor() / m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precision() {
        assert!(deep_equal(&json!([1.0000001, {"a": 2}]), &json!([1.0000004, {"a": 2.0}])));
        assert!(!deep_equal(&json!(1.0001), &json!(1.0003)));
        assert!(!deep_equal(&json!({"a": 1}), &json!({"a": 1, "b": 2})));
    }
}
