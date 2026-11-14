//! Legacy function → expression conversion (style-spec `function/convert.ts`).

use serde_json::{json, Value as J};

pub fn is_function(v: &J) -> bool {
    v.is_object()
}

fn literal(v: &J) -> J {
    if v.is_object() || v.is_array() { json!(["literal", v]) } else { v.clone() }
}

pub fn convert_function(params: &J, spec: &J) -> J {
    let Some(stops) = params["stops"].as_array() else {
        return convert_identity(params, spec);
    };
    let zoom_and_feature = stops.first().is_some_and(|s| s[0].is_object());
    let feature_dependent = zoom_and_feature || params.get("property").is_some();
    let zoom_dependent = zoom_and_feature || !feature_dependent;
    let stops: Vec<(J, J)> = stops
        .iter()
        .map(|s| {
            if !feature_dependent
                && spec["tokens"].as_bool() == Some(true)
                && let Some(t) = s[1].as_str()
            {
                return (s[0].clone(), convert_token_string(t));
            }
            (s[0].clone(), literal(&s[1]))
        })
        .collect();
    if zoom_and_feature {
        convert_zoom_and_property(params, spec, &stops)
    } else if zoom_dependent {
        convert_zoom(params, spec, &stops)
    } else {
        convert_property(params, spec, &stops)
    }
}

fn convert_identity(params: &J, spec: &J) -> J {
    let get = json!(["get", params["property"]]);
    let ty = spec["type"].as_str().unwrap_or("");
    match params.get("default") {
        None => {
            if ty == "string" {
                json!(["string", get])
            } else {
                get
            }
        }
        Some(default) if ty == "enum" => {
            let keys: Vec<J> = match &spec["values"] {
                J::Object(o) => o.keys().map(|k| J::String(k.clone())).collect(),
                J::Array(a) => a.clone(),
                _ => vec![],
            };
            json!(["match", get, keys, get, default])
        }
        Some(default) => {
            let mut e = vec![json!(if ty == "color" { "to-color" } else { ty }), get, literal(default)];
            if ty == "array" {
                e.splice(1..1, [spec["value"].clone(), spec.get("length").cloned().unwrap_or(J::Null)]);
            }
            J::Array(e)
        }
    }
}

fn interpolate_operator(params: &J) -> &'static str {
    match params["colorSpace"].as_str() {
        Some("hcl") => "interpolate-hcl",
        Some("lab") => "interpolate-lab",
        _ => "interpolate",
    }
}

fn function_type(params: &J, spec: &J) -> String {
    match params["type"].as_str() {
        Some(t) => t.to_string(),
        None if spec["expression"]["interpolated"].as_bool() == Some(true) => "exponential".into(),
        None => "interval".into(),
    }
}

fn append_stop(curve: &mut Vec<J>, input: J, output: J, is_step: bool) {
    if curve.len() > 3 && curve[curve.len() - 2] == input {
        return;
    }
    if !(is_step && curve.len() == 2) {
        curve.push(input);
    }
    curve.push(output);
}

fn fixup_degenerate_step(e: &mut Vec<J>) {
    if e[0] == "step" && e.len() == 3 {
        e.push(json!(0));
        let v = e[3].clone();
        e.push(v);
    }
}

fn convert_zoom_and_property(params: &J, spec: &J, stops: &[(J, J)]) -> J {
    let mut zooms: Vec<J> = Vec::new();
    let mut by_zoom: Vec<Vec<(J, J)>> = Vec::new();
    for (input, output) in stops {
        let z = input["zoom"].clone();
        let i = match zooms.iter().position(|v| *v == z) {
            Some(i) => i,
            None => {
                zooms.push(z);
                by_zoom.push(vec![]);
                zooms.len() - 1
            }
        };
        by_zoom[i].push((input["value"].clone(), output.clone()));
    }
    let mut inner = serde_json::Map::new();
    for k in ["type", "property", "default"] {
        if let Some(v) = params.get(k) {
            inner.insert(k.into(), v.clone());
        }
    }
    let inner = J::Object(inner);
    if function_type(&json!({}), spec) == "exponential" {
        let mut e = vec![json!(interpolate_operator(params)), json!(["linear"]), json!(["zoom"])];
        for (z, s) in zooms.iter().zip(&by_zoom) {
            let out = convert_property(&inner, spec, s);
            append_stop(&mut e, z.clone(), out, false);
        }
        J::Array(e)
    } else {
        let mut e = vec![json!("step"), json!(["zoom"])];
        for (z, s) in zooms.iter().zip(&by_zoom) {
            let out = convert_property(&inner, spec, s);
            append_stop(&mut e, z.clone(), out, true);
        }
        fixup_degenerate_step(&mut e);
        J::Array(e)
    }
}

fn fallback(params: &J, spec: &J) -> J {
    match params.get("default").or_else(|| spec.get("default")) {
        Some(v) => literal(v),
        None if spec["type"] == "resolvedImage" => json!(""),
        None => J::Null,
    }
}

fn convert_property(params: &J, spec: &J, stops: &[(J, J)]) -> J {
    let ty = function_type(params, spec);
    let get = json!(["get", params["property"]]);
    let with_default = |e: J| match params.get("default") {
        None => e,
        Some(d) => json!(["case", ["==", ["typeof", get], "number"], e, literal(d)]),
    };
    match ty.as_str() {
        "categorical" if stops.first().is_some_and(|s| s.0.is_boolean()) => {
            let mut e = vec![json!("case")];
            for (i, o) in stops {
                e.push(json!(["==", get, i]));
                e.push(o.clone());
            }
            e.push(fallback(params, spec));
            J::Array(e)
        }
        "categorical" => {
            let mut e = vec![json!("match"), get.clone()];
            for (i, o) in stops {
                append_stop(&mut e, i.clone(), o.clone(), false);
            }
            e.push(fallback(params, spec));
            J::Array(e)
        }
        "interval" => {
            let mut e = vec![json!("step"), json!(["number", get])];
            for (i, o) in stops {
                append_stop(&mut e, i.clone(), o.clone(), true);
            }
            fixup_degenerate_step(&mut e);
            with_default(J::Array(e))
        }
        "exponential" => {
            let base = params["base"].as_f64().unwrap_or(1.0);
            let interp = if base == 1.0 { json!(["linear"]) } else { json!(["exponential", base]) };
            let mut e = vec![json!(interpolate_operator(params)), interp, json!(["number", get])];
            for (i, o) in stops {
                append_stop(&mut e, i.clone(), o.clone(), false);
            }
            with_default(J::Array(e))
        }
        _ => J::Null,
    }
}

fn convert_zoom(params: &J, spec: &J, stops: &[(J, J)]) -> J {
    let ty = function_type(params, spec);
    let (mut e, is_step) = match ty.as_str() {
        "interval" => (vec![json!("step"), json!(["zoom"])], true),
        "exponential" => {
            let base = params["base"].as_f64().unwrap_or(1.0);
            let interp = if base == 1.0 { json!(["linear"]) } else { json!(["exponential", base]) };
            (vec![json!(interpolate_operator(params)), interp, json!(["zoom"])], false)
        }
        _ => return J::Null,
    };
    for (i, o) in stops {
        append_stop(&mut e, i.clone(), o.clone(), is_step);
    }
    fixup_degenerate_step(&mut e);
    J::Array(e)
}

/// "String with {name} token" → `["concat", "String with ", ["get", "name"], " token"]`.
pub fn convert_token_string(s: &str) -> J {
    let mut result = vec![json!("concat")];
    let mut pos = 0;
    let mut rest = s;
    let mut offset = 0;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(['{', '}']) else { break };
        if after.as_bytes()[close] == b'{' || close == 0 {
            offset += open + 1;
            rest = &s[offset..];
            continue;
        }
        let start = offset + open;
        let end = start + 1 + close + 1;
        if start > pos {
            result.push(json!(&s[pos..start]));
        }
        result.push(json!(["get", &s[start + 1..end - 1]]));
        pos = end;
        offset = end;
        rest = &s[offset..];
    }
    if result.len() == 1 {
        return json!(s);
    }
    if pos < s.len() {
        result.push(json!(&s[pos..]));
    } else if result.len() == 2 {
        return json!(["to-string", result[1]]);
    }
    J::Array(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert_eq!(convert_token_string("a {b} c"), json!(["concat", "a ", ["get", "b"], " c"]));
        assert_eq!(convert_token_string("{b}"), json!(["to-string", ["get", "b"]]));
        assert_eq!(convert_token_string("plain"), json!("plain"));
        assert_eq!(convert_token_string("{a}{b}"), json!(["concat", ["get", "a"], ["get", "b"]]));
    }
}
