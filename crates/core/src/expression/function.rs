//! Legacy style functions as MapLibre evaluates them at runtime (style-spec
//! `function/index.ts` `createFunction`). The expression conversion in `convert.rs` is only
//! what the expression tests exercise; rendering uses these semantics.

use super::color::{Color, ColorSpace};
use super::value::{js_number_to_string, Value};
use super::Feature;
use serde_json::Value as J;
use crate::hash::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ty {
    Exponential,
    Interval,
    Categorical,
    Identity,
}

#[derive(Debug, Clone)]
enum Output {
    Value(Value),
    /// Inner per-zoom function of a zoom-and-property function.
    Function(Rc<LegacyFunction>),
}

#[derive(Debug, Clone)]
pub struct LegacyFunction {
    ty: Ty,
    property: Option<String>,
    base: f64,
    default: Option<Value>,
    spec_default: Option<Value>,
    spec_type: String,
    enum_values: Vec<String>,
    space: ColorSpace,
    stops: Vec<(f64, Output)>,
    /// Categorical stops keyed by `String(key)`, and the JS type of the keys.
    categories: HashMap<String, Value>,
    key_type: &'static str,
    zoom_dependent: bool,
    composite: bool,
}

fn js_type(v: &J) -> &'static str {
    match v {
        J::Number(_) => "number",
        J::String(_) => "string",
        J::Bool(_) => "boolean",
        J::Null => "object",
        _ => "object",
    }
}

fn value_type(v: &Value) -> &'static str {
    match v {
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Bool(_) => "boolean",
        Value::Array(_) => "array",
        Value::Null => "null",
        _ => "object",
    }
}

fn key(v: &J) -> String {
    match v {
        J::String(s) => s.clone(),
        J::Number(n) => js_number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        J::Bool(b) => b.to_string(),
        J::Null => "null".into(),
        _ => String::new(),
    }
}

/// `getParseFunction`: colors and the composite array types are parsed up front.
fn parse_output(v: &J, spec: &J) -> Option<Value> {
    let raw = Value::from_json(v);
    match spec["type"].as_str() {
        Some("color") => v.as_str().and_then(Color::parse).map(Value::Color),
        Some("padding") => super::expr::parse_padding(&raw),
        Some("numberArray") => super::expr::parse_number_array(&raw),
        Some("colorArray") => super::expr::parse_color_array(&raw),
        _ => Some(raw),
    }
}

fn has_parse_fn(spec: &J) -> bool {
    matches!(spec["type"].as_str(), Some("color" | "padding" | "numberArray" | "colorArray"))
}

impl LegacyFunction {
    pub fn new(params: &J, spec: &J) -> Option<LegacyFunction> {
        let stops_json = params["stops"].as_array();
        let zoom_and_feature = stops_json.and_then(|s| s.first()).is_some_and(|s| s[0].is_object());
        let feature_dependent = zoom_and_feature || params.get("property").is_some();
        let interpolated = spec["expression"]["interpolated"].as_bool() == Some(true);
        let ty = match params["type"].as_str() {
            Some("exponential") => Ty::Exponential,
            Some("interval") => Ty::Interval,
            Some("categorical") => Ty::Categorical,
            Some("identity") => Ty::Identity,
            Some(_) => return None,
            None if interpolated => Ty::Exponential,
            None => Ty::Interval,
        };
        let space = match params["colorSpace"].as_str() {
            None | Some("rgb") => ColorSpace::Rgb,
            Some("hcl") => ColorSpace::Hcl,
            Some("lab") => ColorSpace::Lab,
            Some(_) => return None,
        };
        let parse = has_parse_fn(spec);
        let spec_default = spec.get("default").and_then(|d| if parse { parse_output(d, spec) } else { Some(Value::from_json(d)) });
        let default = match params.get("default") {
            Some(d) if parse && is_truthy(d) => parse_output(d, spec),
            Some(d) if !parse => Some(Value::from_json(d)),
            _ if parse => spec_default.clone(),
            _ => None,
        };
        let mut f = LegacyFunction {
            ty,
            property: params["property"].as_str().map(String::from),
            base: params["base"].as_f64().unwrap_or(1.0),
            default,
            spec_default,
            spec_type: spec["type"].as_str().unwrap_or("").to_string(),
            enum_values: match &spec["values"] {
                J::Object(o) => o.keys().cloned().collect(),
                J::Array(a) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
                _ => vec![],
            },
            space,
            stops: vec![],
            categories: HashMap::default(),
            key_type: "string",
            zoom_dependent: zoom_and_feature || !feature_dependent,
            composite: zoom_and_feature,
        };
        let stops = stops_json.map(Vec::as_slice).unwrap_or_default();
        if zoom_and_feature {
            let mut zooms: Vec<f64> = Vec::new();
            let mut groups: Vec<Vec<J>> = Vec::new();
            for s in stops {
                let z = s[0]["zoom"].as_f64().unwrap_or(0.0);
                let i = match zooms.iter().position(|&v| v == z) {
                    Some(i) => i,
                    None => {
                        zooms.push(z);
                        groups.push(vec![]);
                        zooms.len() - 1
                    }
                };
                groups[i].push(serde_json::json!([s[0]["value"], s[1]]));
            }
            for (z, g) in zooms.iter().zip(groups) {
                let mut inner = serde_json::Map::new();
                for k in ["type", "property", "default"] {
                    if let Some(v) = params.get(k) {
                        inner.insert(k.into(), v.clone());
                    }
                }
                inner.insert("stops".into(), J::Array(g));
                f.stops.push((*z, Output::Function(Rc::new(LegacyFunction::new(&J::Object(inner), spec)?))));
            }
            return Some(f);
        }
        if ty == Ty::Categorical {
            f.key_type = stops.first().map_or("string", |s| js_type(&s[0]));
        }
        for s in stops {
            let out = if parse { parse_output(&s[1], spec).unwrap_or(Value::Null) } else { Value::from_json(&s[1]) };
            if ty == Ty::Categorical {
                f.categories.insert(key(&s[0]), out.clone());
            }
            f.stops.push((s[0].as_f64().unwrap_or(f64::NAN), Output::Value(out)));
        }
        Some(f)
    }

    pub fn is_zoom_dependent(&self) -> bool {
        self.zoom_dependent
    }

    pub fn is_composite(&self) -> bool {
        self.composite
    }

    /// Zoom stops and interpolation (`StylePropertyFunction.zoomStops/interpolationType`) of a
    /// zoom-dependent function; `None` interpolation for step (interval) functions.
    pub fn zoom_curve(&self) -> (Vec<f64>, Option<super::expr::Interpolation>) {
        let interp = (self.ty == Ty::Exponential).then_some(super::expr::Interpolation::Exponential(self.base));
        (self.stops.iter().map(|s| s.0).collect(), interp)
    }

    fn fallback(&self) -> Value {
        self.default.clone().or_else(|| self.spec_default.clone()).unwrap_or(Value::Null)
    }

    pub fn evaluate(&self, zoom: f64, feature: Option<&Feature>) -> Value {
        if self.composite {
            return self.exponential(zoom, feature);
        }
        if self.zoom_dependent {
            return self.inner(&Value::Number(zoom), feature);
        }
        let props = feature.map(|f| &f.properties);
        match self.property.as_ref().and_then(|p| props.and_then(|o| o.get(p.as_str()))) {
            None => self.fallback(),
            Some(v) => self.inner(v, feature),
        }
    }

    fn inner(&self, input: &Value, feature: Option<&Feature>) -> Value {
        match self.ty {
            Ty::Categorical => {
                if value_type(input) != self.key_type {
                    return self.fallback();
                }
                let k = match input {
                    Value::Number(n) => js_number_to_string(*n),
                    v => v.to_js_string(),
                };
                self.categories.get(&k).cloned().unwrap_or_else(|| self.fallback())
            }
            Ty::Interval => {
                let Value::Number(x) = *input else { return self.fallback() };
                let n = self.stops.len();
                if n == 0 {
                    return self.fallback();
                }
                let i = if n == 1 || x <= self.stops[0].0 {
                    0
                } else if x >= self.stops[n - 1].0 {
                    n - 1
                } else {
                    self.stops.iter().rposition(|s| s.0 <= x).unwrap_or(0)
                };
                self.output(&self.stops[i].1, feature)
            }
            Ty::Exponential => match input {
                Value::Number(x) => self.exponential(*x, feature),
                _ => self.fallback(),
            },
            Ty::Identity => {
                let v = match (self.spec_type.as_str(), input) {
                    ("color", Value::String(s)) => Color::parse(s).map(Value::Color),
                    ("color", _) => None,
                    ("formatted", v) => Some(Value::Formatted(Rc::new(super::value::Formatted::from_string(v.to_js_string())))),
                    ("resolvedImage", v) => {
                        let name = v.to_js_string();
                        (!name.is_empty()).then(|| Value::ResolvedImage(Rc::new(super::value::ResolvedImage { name, available: false })))
                    }
                    ("padding", v) => super::expr::parse_padding(v),
                    ("colorArray", v) => super::expr::parse_color_array(v),
                    ("numberArray", v) => super::expr::parse_number_array(v),
                    ("enum", Value::String(s)) if self.enum_values.iter().any(|e| **e == **s) => Some(input.clone()),
                    (t, v) if value_type(v) == t => Some(v.clone()),
                    _ => None,
                };
                v.unwrap_or_else(|| self.fallback())
            }
        }
    }

    fn output(&self, o: &Output, feature: Option<&Feature>) -> Value {
        match o {
            Output::Value(v) => v.clone(),
            Output::Function(f) => f.evaluate(f64::NAN, feature),
        }
    }

    /// `evaluateExponentialFunction` over `self.stops` at `x`; for composite functions the
    /// outputs are inner feature functions (linear in zoom).
    fn exponential(&self, x: f64, feature: Option<&Feature>) -> Value {
        let base = self.base;
        let n = self.stops.len();
        if n == 0 {
            return self.fallback();
        }
        let eval = |o: &Output| match o {
            Output::Value(v) => v.clone(),
            Output::Function(f) => f.evaluate(x, feature),
        };
        if n == 1 || x <= self.stops[0].0 {
            return eval(&self.stops[0].1);
        }
        if x >= self.stops[n - 1].0 {
            return eval(&self.stops[n - 1].1);
        }
        let i = self.stops.iter().rposition(|s| s.0 <= x).unwrap_or(0);
        let (lo, hi) = (self.stops[i].0, self.stops[i + 1].0);
        let t = super::expr::Interpolation::Exponential(base).factor(x, lo, hi);
        let (a, b) = (eval(&self.stops[i].1), eval(&self.stops[i + 1].1));
        if a.is_null() || b.is_null() {
            return Value::Null;
        }
        interpolate(&self.spec_type, &a, &b, t, self.space)
    }
}

fn is_truthy(v: &J) -> bool {
    !matches!(v, J::Null | J::Bool(false)) && v != &J::String(String::new()) && v.as_f64() != Some(0.0)
}

/// `interpolateFactory[spec.type] || identity` (identity returns the lower value).
fn interpolate(spec_type: &str, a: &Value, b: &Value, t: f64, space: ColorSpace) -> Value {
    let lerp = |x: f64, y: f64| x + t * (y - x);
    match (spec_type, a, b) {
        ("number", Value::Number(x), Value::Number(y)) => Value::Number(lerp(*x, *y)),
        ("color", Value::Color(x), Value::Color(y)) => Value::Color(Color::interpolate(x, y, t, space)),
        ("padding", Value::Padding(x), Value::Padding(y)) => Value::Padding(std::array::from_fn(|i| lerp(x[i], y[i]))),
        ("numberArray", Value::NumberArray(x), Value::NumberArray(y)) if x.len() == y.len() => {
            Value::NumberArray(Rc::new(x.iter().zip(y.iter()).map(|(p, q)| lerp(*p, *q)).collect()))
        }
        ("colorArray", Value::ColorArray(x), Value::ColorArray(y)) if x.len() == y.len() => {
            Value::ColorArray(Rc::new(x.iter().zip(y.iter()).map(|(p, q)| Color::interpolate(p, q, t, space)).collect()))
        }
        ("array", Value::Array(x), Value::Array(y)) => Value::Array(Rc::new(
            x.iter().zip(y.iter()).map(|(p, q)| Value::Number(lerp(p.as_f64().unwrap_or(f64::NAN), q.as_f64().unwrap_or(f64::NAN)))).collect(),
        )),
        _ => a.clone(),
    }
}
