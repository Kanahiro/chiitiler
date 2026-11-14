//! Style parsing: layers, compiled layout/paint properties and filters.

mod filter;

use crate::expression::{self as ex, CanonicalTileId, EvalCtx, Expression, Feature, Globals, Object, Value};
use serde_json::Value as J;
use crate::hash::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

fn spec() -> &'static J {
    static SPEC: OnceLock<J> = OnceLock::new();
    SPEC.get_or_init(|| serde_json::from_str(include_str!("spec.json")).expect("spec.json"))
}

#[derive(Debug, Clone)]
pub enum PropValue {
    Constant(Value),
    Expr(Rc<Expression>),
    /// Legacy (pre-expression) function.
    Function(Rc<ex::function::LegacyFunction>),
}

impl PropValue {
    pub fn is_constant(&self) -> bool {
        matches!(self, PropValue::Constant(_)) || matches!(self, PropValue::Expr(e) if e.kind == ex::Kind::Constant)
    }

    /// Evaluate a color ramp (`line-gradient`) at a line progress.
    pub fn eval_line_progress(&self, progress: f64, global_state: &Rc<Object>) -> Value {
        match self {
            PropValue::Constant(v) => v.clone(),
            PropValue::Expr(e) => {
                let mut ctx = EvalCtx {
                    globals: Globals { zoom: f64::NAN, line_progress: Some(progress), global_state: Some(global_state.clone()), ..Default::default() },
                    ..Default::default()
                };
                e.evaluate(&mut ctx)
            }
            PropValue::Function(f) => f.evaluate(f64::NAN, None),
        }
    }

    /// Expression kind with the top-level zoom stops and interpolation (camera/composite).
    pub fn curve(&self) -> (ex::Kind, Vec<f64>, Option<ex::Interpolation>) {
        match self {
            PropValue::Constant(_) => (ex::Kind::Constant, vec![], None),
            PropValue::Expr(e) => (e.kind, e.zoom_stops.clone(), e.interpolation),
            PropValue::Function(f) => {
                let kind = match (f.is_zoom_dependent(), f.is_composite()) {
                    (_, true) => ex::Kind::Composite,
                    (true, false) => ex::Kind::Camera,
                    (false, _) => ex::Kind::Source,
                };
                let (stops, interp) = f.zoom_curve();
                (kind, stops, interp)
            }
        }
    }

    /// Evaluate with given globals (e.g. `heatmap-density`, `elevation`).
    pub fn eval_globals(&self, mut globals: Globals) -> Value {
        match self {
            PropValue::Constant(v) => v.clone(),
            PropValue::Expr(e) => e.evaluate(&mut EvalCtx { globals: { globals.global_state.get_or_insert_with(Default::default); globals }, ..Default::default() }),
            PropValue::Function(f) => f.evaluate(globals.zoom, None),
        }
    }

    pub fn is_feature_constant(&self) -> bool {
        match self {
            PropValue::Constant(_) => true,
            PropValue::Expr(e) => e.is_feature_constant(),
            PropValue::Function(f) => f.is_zoom_dependent() && !f.is_composite(),
        }
    }

    pub fn eval(&self, zoom: f64, feature: Option<&Feature>, canonical: Option<CanonicalTileId>, global_state: &Rc<Object>) -> Value {
        match self {
            PropValue::Constant(v) => v.clone(),
            PropValue::Expr(e) => {
                let mut ctx = EvalCtx {
                    globals: Globals { zoom, global_state: Some(global_state.clone()), ..Default::default() },
                    feature,
                    canonical,
                    ..Default::default()
                };
                e.evaluate(&mut ctx)
            }
            PropValue::Function(f) => f.evaluate(zoom, feature),
        }
    }
}

/// `normalizePropertyExpression`: constants are coerced to the spec type up front.
fn compile_property(value: &J, spec: &J, global_state: &J) -> Option<PropValue> {
    if ex::convert::is_function(value) {
        return ex::function::LegacyFunction::new(value, spec).map(|f| PropValue::Function(Rc::new(f)));
    }
    if ex::is_expression(value) {
        return ex::create_property_expression(value, spec, Some(global_state)).ok().map(|e| PropValue::Expr(Rc::new(e)));
    }
    let v = Value::from_json(value);
    let v = match (spec["type"].as_str(), &v) {
        (Some("color"), Value::String(s)) => Value::Color(ex::color::Color::parse(s)?),
        (Some("padding" | "numberArray" | "colorArray" | "variableAnchorOffsetCollection"), _) => {
            // Reuse expression coercion for the composite types.
            let e = ex::create_property_expression(&serde_json::json!(["literal", value]), spec, None).ok()?;
            e.evaluate(&mut EvalCtx { globals: Globals { zoom: 0.0, ..Default::default() }, ..Default::default() })
        }
        (Some("formatted"), Value::String(s)) => Value::Formatted(Rc::new(ex::value::Formatted::from_string(s.to_string()))),
        _ => v,
    };
    Some(PropValue::Constant(v))
}

#[derive(Debug)]
pub struct Layer {
    pub id: String,
    pub ty: String,
    pub source: Option<String>,
    pub source_layer: Option<String>,
    pub minzoom: f64,
    pub maxzoom: f64,
    pub filter: Option<Rc<Expression>>,
    pub layout: HashMap<String, PropValue>,
    pub paint: HashMap<String, PropValue>,
    /// Whether expressions need feature geometry (`within` / `distance`).
    pub needs_geometry: bool,
    /// Id of the first layer with the same source data, filter and layout (like
    /// `groupByLayout`, but ignoring zoom ranges); layers in one group build identical buckets.
    pub layout_group: String,
}

impl Layer {
    pub fn paint(&self, name: &str) -> &PropValue {
        &self.paint[name]
    }

    pub fn layout(&self, name: &str) -> &PropValue {
        &self.layout[name]
    }
}

pub struct Style {
    pub json: J,
    pub layers: Vec<Layer>,
    pub global_state: Rc<Object>,
}

fn contains_op(v: &J, ops: &[&str]) -> bool {
    match v {
        J::Array(a) => a.first().and_then(J::as_str).is_some_and(|s| ops.contains(&s)) || a.iter().any(|x| contains_op(x, ops)),
        J::Object(o) => o.values().any(|x| contains_op(x, ops)),
        _ => false,
    }
}

/// Resolve deprecated `ref` layers (style-spec `deref`).
fn deref_layers(layers: &[J]) -> Vec<J> {
    let by_id: HashMap<&str, &J> = layers.iter().filter_map(|l| Some((l["id"].as_str()?, l))).collect();
    layers
        .iter()
        .map(|l| match l["ref"].as_str().and_then(|r| by_id.get(r)) {
            Some(parent) => {
                let mut out = l.clone();
                for k in ["type", "source", "source-layer", "minzoom", "maxzoom", "filter", "layout"] {
                    if let Some(v) = parent.get(k) {
                        out[k] = v.clone();
                    }
                }
                out
            }
            None => l.clone(),
        })
        .collect()
}

impl Style {
    pub fn parse(json: J) -> Style {
        let mut state = Object::default();
        for (k, v) in json["state"].as_object().into_iter().flatten() {
            state.insert(k.as_str().into(), Value::from_json(&v["default"]));
        }
        let state_json = J::Object(state.iter().map(|(k, v)| (k.to_string(), v.to_json())).collect());
        let mut layers: Vec<Layer> = Vec::new();
        let derefed = deref_layers(json["layers"].as_array().map(Vec::as_slice).unwrap_or_default());
        let mut grouped: Vec<&J> = Vec::new();
        for l in &derefed {
            let Some(ty) = l["type"].as_str() else { continue };
            let mut layout = HashMap::default();
            let mut paint = HashMap::default();
            for (kind, map) in [("layout", &mut layout), ("paint", &mut paint)] {
                let Some(props) = spec().get(format!("{kind}_{ty}")).and_then(J::as_object) else { continue };
                for (name, pspec) in props {
                    let given = l[kind].get(name);
                    let compiled = given.and_then(|v| compile_property(v, pspec, &state_json));
                    // Some defaults are expressions themselves (`heatmap-color`).
                    let default = || match &pspec["default"] {
                        d if ex::is_expression(d) => compile_property(d, pspec, &state_json),
                        _ => None,
                    };
                    let value = compiled.or_else(default).unwrap_or_else(|| PropValue::Constant(ex::default_value(pspec)));
                    map.insert(name.clone(), value);
                }
            }
            let filter = l.get("filter").and_then(|f| filter::compile(f, &state_json)).map(Rc::new);
            let id = l["id"].as_str().unwrap_or("").to_string();
            let needs_geometry = contains_op(l, &["within", "distance"]);
            // Zoom ranges only decide visibility. Paint expressions see the bucket's features, so
            // `needs_geometry` must agree.
            const GROUP: [&str; 5] = ["type", "source", "source-layer", "filter", "layout"];
            let layout_group = layers
                .iter()
                .zip(&grouped)
                .find(|(g, gl)| g.needs_geometry == needs_geometry && GROUP.iter().all(|k| gl.get(*k) == l.get(*k)))
                .map_or_else(|| id.clone(), |(g, _)| g.layout_group.clone());
            grouped.push(l);
            layers.push(Layer {
                id,
                ty: ty.to_string(),
                source: l["source"].as_str().map(String::from),
                source_layer: l["source-layer"].as_str().map(String::from),
                minzoom: l["minzoom"].as_f64().unwrap_or(f64::NEG_INFINITY),
                maxzoom: l["maxzoom"].as_f64().unwrap_or(f64::INFINITY),
                filter,
                layout,
                paint,
                needs_geometry,
                layout_group,
            });
        }
        Style { json, layers, global_state: Rc::new(state) }
    }

    /// Raw JSON of layer `i` (for checks like "was this property set").
    pub fn layer_json(&self, id: &str) -> &J {
        self.json["layers"].as_array().and_then(|ls| ls.iter().find(|l| l["id"] == id)).unwrap_or(&J::Null)
    }
}

impl Layer {
    /// Hidden by `visibility` or zoom range at `zoom`.
    pub fn is_hidden(&self, zoom: f64, global_state: &Rc<Object>) -> bool {
        if zoom < self.minzoom || zoom >= self.maxzoom {
            return true;
        }
        matches!(self.layout.get("visibility").map(|v| v.eval(zoom, None, None, global_state)), Some(Value::String(s)) if &*s == "none")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_state_layout() {
        let s = Style::parse(serde_json::json!({
            "version": 8, "state": {"limit": {"default": 5}}, "sources": {},
            "layers": [{"id": "l", "type": "line", "source": "x", "layout": {"line-miter-limit": ["global-state", "limit"]}}]
        }));
        let v = s.layers[0].layout("line-miter-limit").eval(5.0, None, None, &s.global_state);
        assert_eq!(v.as_f64(), Some(5.0));
    }
}
