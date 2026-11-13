//! Style-spec expressions: parsing, type checking and evaluation, ported from
//! maplibre-style-spec `src/expression`.

pub mod color;
mod compound;
pub mod convert;
mod expr;
pub mod function;
mod geometry;
pub mod intl;
mod parse;
pub mod types;
pub mod value;

pub use expr::{CanonicalTileId, EvalCtx, Feature, Globals, Interpolation, Properties, RuntimeError, TagTable};
pub use parse::ParseError;
pub use value::{Object, Value};

use expr::{Expr, Node};
use parse::{is_feature_constant, is_global_property_constant, Annotation, ParseCtx};
use serde_json::Value as J;
use types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Constant,
    Source,
    Camera,
    Composite,
}

/// A compiled property expression (`StylePropertyExpression` upstream).
#[derive(Debug, Clone)]
pub struct Expression {
    expr: Expr,
    pub kind: Kind,
    /// Zoom stops of the top-level zoom curve, if any.
    pub zoom_stops: Vec<f64>,
    pub interpolation: Option<Interpolation>,
    default: Value,
    enum_values: Option<Vec<String>>,
    global_state: Option<std::rc::Rc<Object>>,
}

impl Expression {
    pub fn is_feature_constant(&self) -> bool {
        matches!(self.kind, Kind::Constant | Kind::Camera)
    }

    pub fn is_zoom_constant(&self) -> bool {
        matches!(self.kind, Kind::Constant | Kind::Source)
    }

    pub fn is_state_dependent(&self) -> bool {
        !matches!(self.kind, Kind::Constant | Kind::Camera) && !parse::is_state_constant(&self.expr)
    }

    /// Whether the top-level expression is a `step` (MapLibre's "step interpolant").
    pub fn is_step(&self) -> bool {
        matches!(self.expr.node, Node::Step { .. })
    }

    /// Result type as printed by style-spec `toString(type)`, e.g. `array<number, 2>`.
    pub fn type_name(&self) -> String {
        self.expr.ty.to_string()
    }

    pub fn evaluate_without_error_handling(&self, ctx: &mut EvalCtx) -> Result<Value, RuntimeError> {
        if let Some(gs) = &self.global_state {
            ctx.globals.global_state = Some(gs.clone());
        }
        self.expr.evaluate(ctx)
    }

    /// Evaluate, falling back to the property default on errors / null / NaN.
    pub fn evaluate(&self, ctx: &mut EvalCtx) -> Value {
        match self.evaluate_without_error_handling(ctx) {
            Ok(Value::Null) | Err(_) => self.default.clone(),
            Ok(Value::Number(n)) if n.is_nan() => self.default.clone(),
            Ok(Value::String(s)) if self.enum_values.as_ref().is_some_and(|e| !e.iter().any(|v| **v == *s)) => self.default.clone(),
            Ok(v) => v,
        }
    }

    /// Stop inputs of a top-level `interpolate` (e.g. the elevation stops of `color-relief-color`).
    pub fn interpolate_labels(&self) -> Option<&[f64]> {
        match &self.expr.node {
            Node::Interpolate { labels, .. } => Some(labels),
            _ => None,
        }
    }

    pub fn interpolation_factor(&self, input: f64, lower: f64, upper: f64) -> f64 {
        self.interpolation.map_or(0.0, |i| i.factor(input, lower, upper))
    }
}

fn expected_type(spec: &J) -> Option<Type> {
    let base = |t: &str| -> Option<Type> {
        Some(match t {
            "color" => Type::Color,
            "string" | "enum" => Type::String,
            "number" => Type::Number,
            "boolean" => Type::Boolean,
            "formatted" => Type::Formatted,
            "padding" => Type::Padding,
            "numberArray" => Type::NumberArray,
            "colorArray" => Type::ColorArray,
            "projectionDefinition" => Type::ProjectionDefinition,
            "resolvedImage" => Type::ResolvedImage,
            "variableAnchorOffsetCollection" => Type::VariableAnchorOffsetCollection,
            _ => return None,
        })
    };
    match spec["type"].as_str()? {
        "array" => Some(Type::array(spec["value"].as_str().and_then(base).unwrap_or(Type::Value), spec["length"].as_u64().map(|n| n as usize))),
        t => base(t),
    }
}

/// Spec default as a runtime value.
pub fn default_value(spec: &J) -> Value {
    let d = &spec["default"];
    let v = Value::from_json(d);
    match spec["type"].as_str() {
        Some("color") if d.is_object() => Value::Color(color::Color::TRANSPARENT),
        Some("color") => d.as_str().and_then(color::Color::parse).map_or(Value::Null, Value::Color),
        Some("padding") => expr::parse_padding(&v).unwrap_or(Value::Null),
        Some("numberArray") => expr::parse_number_array(&v).unwrap_or(Value::Null),
        Some("colorArray") => expr::parse_color_array(&v).unwrap_or(Value::Null),
        Some("variableAnchorOffsetCollection") => expr::parse_variable_anchor_offset_collection(&v).unwrap_or(Value::Null),
        _ => v,
    }
}

enum ZoomCurve<'a> {
    None,
    Found(&'a Expr),
    Error(&'static str),
}

const ZOOM_CURVE_ERROR: &str = "\"zoom\" expression may only be used as input to a top-level \"step\" or \"interpolate\" expression.";

fn find_zoom_curve(e: &Expr) -> ZoomCurve<'_> {
    let mut result = match &e.node {
        Node::Let { result, .. } => find_zoom_curve(result),
        Node::Coalesce(args) => {
            let mut r = ZoomCurve::None;
            for a in args {
                r = find_zoom_curve(a);
                if !matches!(r, ZoomCurve::None) {
                    break;
                }
            }
            r
        }
        Node::Step { input, .. } | Node::Interpolate { input, .. } if input.compound_name() == Some("zoom") => ZoomCurve::Found(e),
        _ => ZoomCurve::None,
    };
    if matches!(result, ZoomCurve::Error(_)) {
        return result;
    }
    e.each_child(&mut |child| {
        let c = find_zoom_curve(child);
        result = match (std::mem::replace(&mut result, ZoomCurve::None), c) {
            (_, ZoomCurve::Error(m)) => ZoomCurve::Error(m),
            (ZoomCurve::None, ZoomCurve::Found(_)) => ZoomCurve::Error(ZOOM_CURVE_ERROR),
            (ZoomCurve::Found(a), ZoomCurve::Found(b)) if !std::ptr::eq(a, b) => {
                ZoomCurve::Error("Only one zoom-based \"step\" or \"interpolate\" subexpression may be used in an expression.")
            }
            (r, _) => r,
        };
    });
    result
}

/// `createPropertyExpression`: accepts expressions and legacy functions.
/// `spec` is the property specification JSON (`type`, `property-type`, `expression`, `default`, ...).
pub fn create_property_expression(input: &J, spec: &J, global_state: Option<&J>) -> Result<Expression, Vec<ParseError>> {
    create(input, spec, global_state, true)
}

/// `createExpression`: like [`create_property_expression`] but without the property-type
/// and zoom-curve restrictions (used for filters).
pub fn create_expression(input: &J, spec: &J, global_state: Option<&J>) -> Result<Expression, Vec<ParseError>> {
    create(input, spec, global_state, false)
}

fn create(input: &J, spec: &J, global_state: Option<&J>, property: bool) -> Result<Expression, Vec<ParseError>> {
    let converted;
    let input = if convert::is_function(input) {
        converted = convert::convert_function(input, spec);
        &converted
    } else {
        input
    };
    let ctx = ParseCtx::new(expected_type(spec));
    let ann = (spec["type"] == "string").then_some(Annotation::Coerce);
    let Some(parsed) = ctx.parse_impl(input, ann) else {
        return Err(ctx.errors.borrow().clone());
    };
    let error = |m: &str| Err(vec![ParseError { key: String::new(), message: m.into() }]);

    let feature_constant = is_feature_constant(&parsed);
    if !property {
        let kind = match (feature_constant, is_global_property_constant(&parsed, &["zoom"])) {
            (true, true) => Kind::Constant,
            (false, true) => Kind::Source,
            (true, false) => Kind::Camera,
            (false, false) => Kind::Composite,
        };
        let global_state = global_state.map(|g| match Value::from_json(g) {
            Value::Object(o) => o,
            _ => Default::default(),
        });
        return Ok(Expression { expr: parsed, kind, zoom_stops: vec![], interpolation: None, default: default_value(spec), enum_values: None, global_state });
    }
    let property_type = spec["property-type"].as_str().unwrap_or("");
    if !feature_constant && !matches!(property_type, "data-driven" | "cross-faded-data-driven") {
        return error("data expressions not supported");
    }
    let zoom_constant = is_global_property_constant(&parsed, &["zoom"]);
    let params = spec["expression"]["parameters"].as_array();
    if !zoom_constant && !params.is_some_and(|p| p.iter().any(|v| v == "zoom")) {
        return error("zoom expressions not supported");
    }
    let (zoom_stops, interpolation) = match find_zoom_curve(&parsed) {
        ZoomCurve::Error(m) => return error(m),
        ZoomCurve::None if !zoom_constant => return error(ZOOM_CURVE_ERROR),
        ZoomCurve::None => (vec![], None),
        ZoomCurve::Found(curve) => match &curve.node {
            Node::Interpolate { labels, interpolation, .. } => {
                if spec["expression"]["interpolated"].as_bool() != Some(true) {
                    return error("\"interpolate\" expressions cannot be used with this property");
                }
                (labels.clone(), Some(*interpolation))
            }
            Node::Step { labels, .. } => (labels.clone(), None),
            _ => unreachable!(),
        },
    };
    let kind = match (feature_constant, zoom_stops.is_empty() && zoom_constant) {
        (true, true) => Kind::Constant,
        (false, true) => Kind::Source,
        (true, false) => Kind::Camera,
        (false, false) => Kind::Composite,
    };
    let enum_values = (spec["type"] == "enum").then(|| match &spec["values"] {
        J::Object(o) => o.keys().cloned().collect(),
        J::Array(a) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        _ => vec![],
    });
    let global_state = global_state.map(|g| match Value::from_json(g) {
        Value::Object(o) => o,
        _ => Default::default(),
    });
    Ok(Expression { expr: parsed, kind, zoom_stops, interpolation, default: default_value(spec), enum_values, global_state })
}

/// Upstream `isExpression`: an array whose first element names a known operator.
pub fn is_expression(v: &J) -> bool {
    v.as_array().and_then(|a| a.first()).and_then(J::as_str).is_some_and(parse::is_expression_name)
}
