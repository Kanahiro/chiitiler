//! Expression AST and evaluation (style-spec `expression/definitions/*`).

use super::color::{Color, ColorSpace};
use super::geometry;
use super::intl::{self, Collator};
use super::types::{check_subtype, Type};
use super::value::*;
use crate::hash::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub message: String,
    /// Index path of the throwing sub-expression, e.g. `[3][0]`; empty for the root.
    pub path: String,
}

pub type EvalResult = Result<Value, RuntimeError>;

fn err(message: impl Into<String>, path: &str) -> RuntimeError {
    RuntimeError { message: message.into(), path: path.into() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalTileId {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Globals {
    /// NaN when absent.
    pub zoom: f64,
    pub heatmap_density: Option<f64>,
    pub elevation: Option<f64>,
    pub line_progress: Option<f64>,
    pub accumulated: Option<Value>,
    pub global_state: Option<Rc<Object>>,
}

#[derive(Debug, Clone, Default)]
pub struct Feature {
    pub id: Option<Value>,
    /// "Point", "LineString", "Polygon", "Unknown" (or GeoJSON Multi* names in tests).
    pub geom_type: Option<&'static str>,
    pub properties: Properties,
    /// Tile coordinates (extent 8192 in the expression tests, the tile extent when rendering).
    pub geometry: Option<Vec<Vec<[f64; 2]>>>,
}

/// A feature's properties: an object, or an MVT feature's tags (pairs of indices into its
/// layer's key and value tables), which take 8 bytes per property instead of a map entry.
#[derive(Debug, Clone)]
pub enum Properties {
    Object(Rc<Object>),
    /// The feature's tags are `table.tags[start..end]`.
    Tags(Rc<TagTable>, u32, u32),
}

/// An MVT layer's keys and values, and the tags of all its features.
#[derive(Debug, Default)]
pub struct TagTable {
    pub keys: Vec<Rc<str>>,
    pub values: Vec<Value>,
    pub tags: Vec<u32>,
}

impl Default for Properties {
    fn default() -> Self {
        Properties::Object(Rc::default())
    }
}

impl Properties {
    /// The value of `key`, as in the object built by `to_object` (the last tag of a key wins;
    /// tags with indices out of range are skipped).
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Properties::Object(o) => o.get(key),
            Properties::Tags(t, start, end) => t.tags[*start as usize..*end as usize]
                .chunks_exact(2)
                .rev()
                .find_map(|kv| (t.keys.get(kv[0] as usize).is_some_and(|k| &**k == key)).then(|| t.values.get(kv[1] as usize)).flatten()),
        }
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn to_object(&self) -> Rc<Object> {
        match self {
            Properties::Object(o) => o.clone(),
            Properties::Tags(t, start, end) => {
                let tags = &t.tags[*start as usize..*end as usize];
                let mut o = Object::with_capacity_and_hasher(tags.len() / 2, Default::default());
                for kv in tags.chunks_exact(2) {
                    if let (Some(k), Some(v)) = (t.keys.get(kv[0] as usize), t.values.get(kv[1] as usize)) {
                        o.insert(k.clone(), v.clone());
                    }
                }
                Rc::new(o)
            }
        }
    }
}

#[derive(Default)]
pub struct EvalCtx<'a> {
    pub globals: Globals,
    pub feature: Option<&'a Feature>,
    pub feature_state: Option<&'a Object>,
    pub canonical: Option<CanonicalTileId>,
    pub available_images: Option<&'a [String]>,
}

impl EvalCtx<'_> {
    fn geometry_type(&self) -> Option<&'static str> {
        self.feature.and_then(|f| f.geom_type)
    }
}

pub type CompoundFn = fn(&EvalCtx, &[Expr], &str) -> EvalResult;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Interpolation {
    Linear,
    Exponential(f64),
    CubicBezier([f64; 4]),
}

impl Interpolation {
    pub fn factor(&self, input: f64, lower: f64, upper: f64) -> f64 {
        match *self {
            Interpolation::Linear => exponential_interpolation(input, 1.0, lower, upper),
            Interpolation::Exponential(base) => exponential_interpolation(input, base, lower, upper),
            Interpolation::CubicBezier(c) => {
                unit_bezier(c[0], c[1], c[2], c[3], exponential_interpolation(input, 1.0, lower, upper))
            }
        }
    }
}

fn exponential_interpolation(input: f64, base: f64, lower: f64, upper: f64) -> f64 {
    let difference = upper - lower;
    let progress = input - lower;
    if difference == 0.0 {
        0.0
    } else if base == 1.0 {
        progress / difference
    } else {
        (base.powf(progress) - 1.0) / (base.powf(difference) - 1.0)
    }
}

/// `@mapbox/unitbezier` solve(x).
fn unit_bezier(p1x: f64, p1y: f64, p2x: f64, p2y: f64, x: f64) -> f64 {
    let cx = 3.0 * p1x;
    let bx = 3.0 * (p2x - p1x) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * p1y;
    let by = 3.0 * (p2y - p1y) - cy;
    let ay = 1.0 - cy - by;
    let sample_x = |t: f64| ((ax * t + bx) * t + cx) * t;
    let sample_y = |t: f64| ((ay * t + by) * t + cy) * t;
    let deriv_x = |t: f64| (3.0 * ax * t + 2.0 * bx) * t + cx;
    let solve_curve_x = |x: f64| {
        let epsilon = 1e-6;
        if x < 0.0 {
            return 0.0;
        }
        if x > 1.0 {
            return 1.0;
        }
        let mut t = x;
        for _ in 0..8 {
            let x2 = sample_x(t) - x;
            if x2.abs() < epsilon {
                return t;
            }
            let d2 = deriv_x(t);
            if d2.abs() < 1e-6 {
                break;
            }
            t -= x2 / d2;
        }
        let (mut t0, mut t1) = (0.0, 1.0);
        t = x;
        for _ in 0..20 {
            let x2 = sample_x(t);
            if (x2 - x).abs() < epsilon {
                break;
            }
            if x > x2 {
                t0 = t;
            } else {
                t1 = t;
            }
            t = (t1 - t0) * 0.5 + t0;
        }
        t
    };
    sample_y(solve_curve_x(x))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

impl CmpOp {
    pub fn name(self) -> &'static str {
        match self {
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
            CmpOp::Lt => "<",
            CmpOp::Gt => ">",
            CmpOp::Le => "<=",
            CmpOp::Ge => ">=",
        }
    }

    pub fn is_order(self) -> bool {
        !matches!(self, CmpOp::Eq | CmpOp::Ne)
    }
}

#[derive(Debug, Clone)]
pub struct FormatSection {
    pub content: Expr,
    pub scale: Option<Expr>,
    pub font: Option<Expr>,
    pub text_color: Option<Expr>,
    pub vertical_align: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub ty: Type,
    pub node: Node,
}

#[derive(Debug, Clone)]
pub enum Node {
    Literal(Value),
    Compound { name: String, f: CompoundFn, args: Vec<Expr>, key: String },
    Assertion { args: Vec<Expr>, key: String },
    Coercion { args: Vec<Expr>, key: String },
    Let { bindings: Vec<(String, Rc<Expr>)>, result: Box<Expr> },
    Var { bound: Rc<Expr> },
    At { index: Box<Expr>, input: Box<Expr>, key: String },
    In { needle: Box<Expr>, haystack: Box<Expr>, key: String },
    IndexOf { needle: Box<Expr>, haystack: Box<Expr>, from: Option<Box<Expr>>, key: String },
    Slice { input: Box<Expr>, begin: Box<Expr>, end: Option<Box<Expr>>, key: String },
    Length { input: Box<Expr>, key: String },
    Case { branches: Vec<(Expr, Expr)>, otherwise: Box<Expr> },
    Match { input_type: Type, input: Box<Expr>, cases: HashMap<String, usize>, outputs: Vec<Expr>, otherwise: Box<Expr> },
    Coalesce(Vec<Expr>),
    Step { input: Box<Expr>, labels: Vec<f64>, outputs: Vec<Expr>, key: String },
    Interpolate { space: ColorSpace, interpolation: Interpolation, input: Box<Expr>, labels: Vec<f64>, outputs: Vec<Expr>, key: String },
    Comparison { op: CmpOp, lhs: Box<Expr>, rhs: Box<Expr>, collator: Option<Box<Expr>>, key: String },
    Collator { case_sensitive: Box<Expr>, diacritic_sensitive: Box<Expr>, locale: Option<Box<Expr>> },
    Format(Vec<FormatSection>),
    NumberFormat { number: Box<Expr>, locale: Option<Box<Expr>>, currency: Option<Box<Expr>>, unit: Option<Box<Expr>>, min: Option<Box<Expr>>, max: Option<Box<Expr>> },
    Image(Box<Expr>),
    GlobalState(String),
    /// Multipolygon in lng/lat.
    Within(Vec<Vec<Vec<[f64; 2]>>>),
    Distance(Vec<geometry::SimpleGeometry>),
    Semiliteral(Vec<Expr>),
}

impl Expr {
    pub fn literal(ty: Type, v: Value) -> Expr {
        Expr { ty, node: Node::Literal(v) }
    }

    pub fn each_child(&self, f: &mut dyn FnMut(&Expr)) {
        match &self.node {
            Node::Literal(_) | Node::Var { .. } | Node::GlobalState(_) | Node::Within(_) | Node::Distance(_) => {}
            Node::Compound { args, .. } | Node::Assertion { args, .. } | Node::Coercion { args, .. } | Node::Coalesce(args) | Node::Semiliteral(args) => {
                args.iter().for_each(f)
            }
            Node::Let { bindings, result } => {
                for (_, b) in bindings {
                    f(b);
                }
                f(result);
            }
            Node::At { index, input, .. } => {
                f(index);
                f(input);
            }
            Node::In { needle, haystack, .. } => {
                f(needle);
                f(haystack);
            }
            Node::IndexOf { needle, haystack, from, .. } => {
                f(needle);
                f(haystack);
                if let Some(e) = from {
                    f(e);
                }
            }
            Node::Slice { input, begin, end, .. } => {
                f(input);
                f(begin);
                if let Some(e) = end {
                    f(e);
                }
            }
            Node::Length { input, .. } => f(input),
            Node::Case { branches, otherwise } => {
                for (t, o) in branches {
                    f(t);
                    f(o);
                }
                f(otherwise);
            }
            Node::Match { input, outputs, otherwise, .. } => {
                f(input);
                outputs.iter().for_each(&mut *f);
                f(otherwise);
            }
            Node::Step { input, outputs, .. } | Node::Interpolate { input, outputs, .. } => {
                f(input);
                outputs.iter().for_each(f);
            }
            Node::Comparison { lhs, rhs, collator, .. } => {
                f(lhs);
                f(rhs);
                if let Some(c) = collator {
                    f(c);
                }
            }
            Node::Collator { case_sensitive, diacritic_sensitive, locale } => {
                f(case_sensitive);
                f(diacritic_sensitive);
                if let Some(l) = locale {
                    f(l);
                }
            }
            Node::Format(sections) => {
                for s in sections {
                    f(&s.content);
                    for e in [&s.scale, &s.font, &s.text_color, &s.vertical_align].into_iter().flatten() {
                        f(e);
                    }
                }
            }
            Node::NumberFormat { number, locale, currency, unit, min, max } => {
                f(number);
                for e in [locale, currency, unit, min, max].into_iter().flatten() {
                    f(e);
                }
            }
            Node::Image(e) => f(e),
        }
    }

    pub fn compound_name(&self) -> Option<&str> {
        if let Node::Compound { name, .. } = &self.node { Some(name) } else { None }
    }

    pub fn evaluate(&self, ctx: &EvalCtx) -> EvalResult {
        match &self.node {
            Node::Literal(v) => Ok(v.clone()),
            Node::Compound { f, args, key, .. } => f(ctx, args, key),
            Node::Var { bound, .. } => bound.evaluate(ctx),
            Node::Let { result, .. } => result.evaluate(ctx),
            Node::Assertion { args, key } => {
                for (i, a) in args.iter().enumerate() {
                    let v = a.evaluate(ctx)?;
                    let t = v.type_of();
                    if check_subtype(&self.ty, &t).is_none() {
                        return Ok(v);
                    } else if i == args.len() - 1 {
                        return Err(err(format!("Expected value to be of type {}, but found {t} instead.", self.ty), key));
                    }
                }
                Err(err("", ""))
            }
            Node::Coercion { args, key } => self.eval_coercion(ctx, args, key),
            Node::At { index, input, key } => {
                let i = index.evaluate(ctx)?.as_f64().unwrap_or(f64::NAN);
                let arr = input.evaluate(ctx)?;
                let len = match &arr {
                    Value::Array(a) => a.len(),
                    _ => 0,
                };
                if i < 0.0 {
                    return Err(err(format!("Array index out of bounds: {} < 0.", js_number_to_string(i)), key));
                }
                if i >= len as f64 {
                    return Err(err(
                        format!("Array index out of bounds: {} > {}.", js_number_to_string(i), len as i64 - 1),
                        key,
                    ));
                }
                if i != i.floor() {
                    return Err(err(format!("Array index must be an integer, but found {} instead.", js_number_to_string(i)), key));
                }
                match arr {
                    Value::Array(a) => Ok(a[i as usize].clone()),
                    _ => Ok(Value::Null),
                }
            }
            Node::In { needle, haystack, key } => {
                let n = needle.evaluate(ctx)?;
                let h = haystack.evaluate(ctx)?;
                if !h.truthy() {
                    return Ok(Value::Bool(false));
                }
                check_needle(&n, key)?;
                match &h {
                    Value::String(s) => Ok(Value::Bool(s.contains(&*js_string_coerce(&n)))),
                    Value::Array(a) => Ok(Value::Bool(a.iter().any(|v| v.strict_eq(&n)))),
                    _ => Err(haystack_error("second argument", &h, key)),
                }
            }
            Node::IndexOf { needle, haystack, from, key } => {
                let n = needle.evaluate(ctx)?;
                let h = haystack.evaluate(ctx)?;
                check_needle(&n, key)?;
                let from = match from {
                    Some(e) => Some(e.evaluate(ctx)?.as_f64().unwrap_or(f64::NAN)),
                    None => None,
                };
                match &h {
                    Value::String(s) => Ok(Value::Number(string_index_of(s, &js_string_coerce(&n), from))),
                    Value::Array(a) => {
                        let start = relative_index(from, a.len());
                        Ok(Value::Number(
                            a.iter().enumerate().skip(start).find(|(_, v)| v.strict_eq(&n)).map_or(-1.0, |(i, _)| i as f64),
                        ))
                    }
                    _ => Err(haystack_error("second argument", &h, key)),
                }
            }
            Node::Slice { input, begin, end, key } => {
                let v = input.evaluate(ctx)?;
                let b = begin.evaluate(ctx)?.as_f64().unwrap_or(f64::NAN);
                let e = match end {
                    Some(e) => Some(e.evaluate(ctx)?.as_f64().unwrap_or(f64::NAN)),
                    None => None,
                };
                match &v {
                    Value::String(s) => {
                        let chars: Vec<char> = s.chars().collect();
                        let (b, e) = slice_range(b, e, chars.len());
                        Ok(Value::str(&chars[b..e].iter().collect::<String>()))
                    }
                    Value::Array(a) => {
                        let (b, e) = slice_range(b, e, a.len());
                        Ok(Value::Array(Rc::new(a[b..e].to_vec())))
                    }
                    _ => Err(err(
                        format!("Expected first argument to be of type array or string, but found {} instead.", v.type_of()),
                        key,
                    )),
                }
            }
            Node::Length { input, key } => match input.evaluate(ctx)? {
                Value::String(s) => Ok(Value::Number(s.chars().count() as f64)),
                Value::Array(a) => Ok(Value::Number(a.len() as f64)),
                v => Err(err(format!("Expected value to be of type string or array, but found {} instead.", v.type_of()), key)),
            },
            Node::Case { branches, otherwise } => {
                for (test, out) in branches {
                    if test.evaluate(ctx)?.truthy() {
                        return out.evaluate(ctx);
                    }
                }
                otherwise.evaluate(ctx)
            }
            Node::Match { input_type, input, cases, outputs, otherwise } => {
                let v = input.evaluate(ctx)?;
                let label = match &v {
                    Value::Number(n) => Some(js_number_to_string(*n)),
                    Value::String(s) => Some(s.to_string()),
                    _ => None,
                };
                let out = (v.type_of().identical(input_type))
                    .then(|| label.and_then(|l| cases.get(&l)).map(|&i| &outputs[i]))
                    .flatten()
                    .unwrap_or(otherwise);
                out.evaluate(ctx)
            }
            Node::Coalesce(args) => {
                let mut requested: Option<String> = None;
                let mut result = Value::Null;
                for (i, a) in args.iter().enumerate() {
                    result = a.evaluate(ctx)?;
                    if let Value::ResolvedImage(img) = &result
                        && !img.available
                    {
                        if requested.is_none() {
                            requested = Some(img.name.clone());
                        }
                        result = Value::Null;
                        if i == args.len() - 1 {
                            result = requested.as_deref().map_or(Value::Null, Value::str);
                        }
                    }
                    if !result.is_null() {
                        break;
                    }
                }
                Ok(result)
            }
            Node::Step { input, labels, outputs, key } => {
                if labels.len() == 1 {
                    return outputs[0].evaluate(ctx);
                }
                let v = input.evaluate(ctx)?.as_f64().unwrap_or(f64::NAN);
                if v <= labels[0] {
                    return outputs[0].evaluate(ctx);
                }
                if v >= labels[labels.len() - 1] {
                    return outputs[labels.len() - 1].evaluate(ctx);
                }
                outputs[find_stop(labels, v, key)?].evaluate(ctx)
            }
            Node::Interpolate { space, interpolation, input, labels, outputs, key } => {
                if labels.len() == 1 {
                    return outputs[0].evaluate(ctx);
                }
                let v = input.evaluate(ctx)?.as_f64().unwrap_or(f64::NAN);
                if v <= labels[0] {
                    return outputs[0].evaluate(ctx);
                }
                let n = labels.len();
                if v >= labels[n - 1] {
                    return outputs[n - 1].evaluate(ctx);
                }
                let i = find_stop(labels, v, key)?;
                let t = interpolation.factor(v, labels[i], labels[i + 1]);
                let lo = outputs[i].evaluate(ctx)?;
                let hi = outputs[i + 1].evaluate(ctx)?;
                interpolate_values(&self.ty, *space, &lo, &hi, t, key)
            }
            Node::Comparison { op, lhs, rhs, collator, key } => {
                let l = lhs.evaluate(ctx)?;
                let r = rhs.evaluate(ctx)?;
                let untyped = lhs.ty == Type::Value || rhs.ty == Type::Value;
                if op.is_order() && untyped {
                    let (lt, rt) = (l.type_of(), r.type_of());
                    if lt.kind() != rt.kind() || !matches!(lt, Type::String | Type::Number) {
                        return Err(err(
                            format!(
                                "Expected arguments for \"{}\" to be (string, string) or (number, number), but found ({}, {}) instead.",
                                op.name(),
                                lt.kind(),
                                rt.kind()
                            ),
                            key,
                        ));
                    }
                }
                if let Some(c) = collator {
                    let both_strings = matches!((&l, &r), (Value::String(_), Value::String(_)));
                    if !(!op.is_order() && untyped && !both_strings) {
                        let Value::Collator(c) = c.evaluate(ctx)? else { return Ok(Value::Bool(false)) };
                        let ord = c.compare(l.as_str().unwrap_or(""), r.as_str().unwrap_or(""));
                        use std::cmp::Ordering::*;
                        return Ok(Value::Bool(match op {
                            CmpOp::Eq => ord == Equal,
                            CmpOp::Ne => ord != Equal,
                            CmpOp::Lt => ord == Less,
                            CmpOp::Gt => ord == Greater,
                            CmpOp::Le => ord != Greater,
                            CmpOp::Ge => ord != Less,
                        }));
                    }
                }
                Ok(Value::Bool(compare_basic(*op, &l, &r)))
            }
            Node::Collator { case_sensitive, diacritic_sensitive, locale } => Ok(Value::Collator(Rc::new(Collator {
                case_sensitive: case_sensitive.evaluate(ctx)?.truthy(),
                diacritic_sensitive: diacritic_sensitive.evaluate(ctx)?.truthy(),
                locale: match locale {
                    Some(l) => Some(l.evaluate(ctx)?.to_js_string()),
                    None => None,
                },
            }))),
            Node::Format(sections) => {
                let mut out = Vec::new();
                for s in sections {
                    let content = s.content.evaluate(ctx)?;
                    let vertical_align = match &s.vertical_align {
                        Some(e) => Some(e.evaluate(ctx)?.to_js_string()),
                        None => None,
                    };
                    if let Value::ResolvedImage(img) = content {
                        out.push(FormattedSection { text: String::new(), image: Some(img), scale: None, font_stack: None, text_color: None, vertical_align });
                        continue;
                    }
                    let scale = match &s.scale {
                        Some(e) => Some(e.evaluate(ctx)?.as_f64().unwrap_or(f64::NAN)),
                        None => None,
                    };
                    let font_stack = match &s.font {
                        Some(e) => Some(match e.evaluate(ctx)? {
                            Value::Array(a) => a.iter().map(Value::to_js_string).collect::<Vec<_>>().join(","),
                            v => v.to_js_string(),
                        }),
                        None => None,
                    };
                    let text_color = match &s.text_color {
                        Some(e) => match e.evaluate(ctx)? {
                            Value::Color(c) => Some(c),
                            _ => None,
                        },
                        None => None,
                    };
                    out.push(FormattedSection { text: content.to_js_string(), image: None, scale, font_stack, text_color, vertical_align });
                }
                Ok(Value::Formatted(Rc::new(Formatted { sections: out })))
            }
            Node::NumberFormat { number, locale, currency, unit, min, max } => {
                let opt_s = |e: &Option<Box<Expr>>| -> Result<Option<String>, RuntimeError> {
                    e.as_ref().map(|e| e.evaluate(ctx).map(|v| v.to_js_string())).transpose()
                };
                let opt_n = |e: &Option<Box<Expr>>| -> Result<Option<f64>, RuntimeError> {
                    e.as_ref().map(|e| e.evaluate(ctx).map(|v| v.as_f64().unwrap_or(f64::NAN))).transpose()
                };
                let (locale, currency, unit) = (opt_s(locale)?, opt_s(currency)?, opt_s(unit)?);
                let (min, max) = (opt_n(min)?, opt_n(max)?);
                let n = number.evaluate(ctx)?.as_f64().unwrap_or(f64::NAN);
                Ok(Value::str(&intl::format_number(
                    n,
                    &intl::NumberFormatOptions {
                        locale: locale.as_deref(),
                        currency: currency.as_deref(),
                        unit: unit.as_deref(),
                        min_fraction_digits: min,
                        max_fraction_digits: max,
                    },
                )))
            }
            Node::Image(e) => {
                let name = e.evaluate(ctx)?.to_js_string();
                if name.is_empty() {
                    return Ok(Value::Null);
                }
                let available = ctx.available_images.is_some_and(|imgs| imgs.contains(&name));
                Ok(Value::ResolvedImage(Rc::new(ResolvedImage { name, available })))
            }
            Node::GlobalState(key) => Ok(ctx
                .globals
                .global_state
                .as_ref()
                .and_then(|s| s.get(key.as_str()).cloned())
                .unwrap_or(Value::Null)),
            Node::Within(polygons) => Ok(Value::Bool(match (ctx.feature.and_then(|f| f.geometry.as_ref()), ctx.canonical) {
                (Some(g), Some(c)) => geometry::within(ctx.geometry_type(), g, c, polygons),
                _ => false,
            })),
            Node::Distance(geoms) => Ok(Value::Number(match (ctx.feature.and_then(|f| f.geometry.as_ref()), ctx.canonical) {
                (Some(g), Some(c)) => geometry::distance(ctx.geometry_type(), g, c, geoms),
                _ => f64::NAN,
            })),
            Node::Semiliteral(items) => Ok(Value::Array(Rc::new(items.iter().map(|e| e.evaluate(ctx)).collect::<Result<_, _>>()?))),
        }
    }

    fn eval_coercion(&self, ctx: &EvalCtx, args: &[Expr], key: &str) -> EvalResult {
        let describe = |v: &Value| match v {
            Value::String(s) => s.to_string(),
            v => js_stringify(&v.to_json()),
        };
        match &self.ty {
            Type::Boolean => Ok(Value::Bool(args[0].evaluate(ctx)?.truthy())),
            Type::Color => {
                let mut input = Value::Null;
                let mut error = None;
                for a in args {
                    input = a.evaluate(ctx)?;
                    error = None;
                    match &input {
                        Value::Color(c) => return Ok(Value::Color(*c)),
                        Value::String(s) => {
                            if let Some(c) = Color::parse(s) {
                                return Ok(Value::Color(c));
                            }
                        }
                        Value::Array(arr) => {
                            if arr.len() < 3 || arr.len() > 4 {
                                error = Some(format!(
                                    "Invalid rgba value {}: expected an array containing either three or four numeric values.",
                                    js_stringify(&input.to_json())
                                ));
                            } else {
                                error = validate_rgba(&arr[0], &arr[1], &arr[2], arr.get(3));
                            }
                            if error.is_none() {
                                let n = |i: usize| arr[i].as_f64().unwrap();
                                let a = arr.get(3).map_or(1.0, |v| v.as_f64().unwrap());
                                return Ok(Value::Color(Color::new(n(0) / 255.0, n(1) / 255.0, n(2) / 255.0, a)));
                            }
                        }
                        _ => {}
                    }
                }
                Err(err(error.unwrap_or_else(|| format!("Could not parse color from value '{}'", describe(&input))), key))
            }
            Type::Padding | Type::NumberArray | Type::ColorArray | Type::VariableAnchorOffsetCollection => {
                let mut input = Value::Null;
                for a in args {
                    input = a.evaluate(ctx)?;
                    let parsed = match self.ty {
                        Type::Padding => parse_padding(&input),
                        Type::NumberArray => parse_number_array(&input),
                        Type::ColorArray => parse_color_array(&input),
                        _ => parse_variable_anchor_offset_collection(&input),
                    };
                    if let Some(v) = parsed {
                        return Ok(v);
                    }
                }
                Err(err(format!("Could not parse {} from value '{}'", self.ty.kind(), describe(&input)), key))
            }
            Type::Number => {
                let mut value = Value::Null;
                for a in args {
                    value = a.evaluate(ctx)?;
                    if value.is_null() {
                        return Ok(Value::Number(0.0));
                    }
                    let n = value.to_js_number();
                    if !n.is_nan() {
                        return Ok(Value::Number(n));
                    }
                }
                Err(err(format!("Could not convert {} to number.", js_stringify(&value.to_json())), key))
            }
            Type::Formatted => Ok(Value::Formatted(Rc::new(Formatted::from_string(args[0].evaluate(ctx)?.to_js_string())))),
            Type::ResolvedImage => {
                let name = args[0].evaluate(ctx)?.to_js_string();
                Ok(if name.is_empty() { Value::Null } else { Value::ResolvedImage(Rc::new(ResolvedImage { name, available: false })) })
            }
            Type::ProjectionDefinition => {
                let input = args[0].evaluate(ctx)?;
                if parse_projection_definition(&input).is_some() {
                    Ok(input)
                } else {
                    Err(err(format!("Could not parse projectionDefinition from value '{}'", describe(&input)), key))
                }
            }
            _ => Ok(Value::str(&args[0].evaluate(ctx)?.to_js_string())),
        }
    }
}

/// JS `String(v)` (unlike `valueToString`, null becomes "null").
fn js_string_coerce(v: &Value) -> String {
    if v.is_null() { "null".into() } else { v.to_js_string() }
}

fn check_needle(n: &Value, key: &str) -> Result<(), RuntimeError> {
    if matches!(n, Value::Bool(_) | Value::String(_) | Value::Number(_) | Value::Null) {
        Ok(())
    } else {
        Err(err(
            format!("Expected first argument to be of type boolean, string, number or null, but found {} instead.", n.type_of()),
            key,
        ))
    }
}

fn haystack_error(which: &str, h: &Value, key: &str) -> RuntimeError {
    err(format!("Expected {which} to be of type array or string, but found {} instead.", h.type_of()), key)
}

/// JS `ToIntegerOrInfinity` + relative index clamping as in `Array.prototype.indexOf`/`slice`.
fn relative_index(i: Option<f64>, len: usize) -> usize {
    let Some(i) = i else { return 0 };
    let i = if i.is_nan() { 0.0 } else { i.trunc() };
    if i < 0.0 { (len as f64 + i).max(0.0) as usize } else { (i as usize).min(len) }
}

fn slice_range(b: f64, e: Option<f64>, len: usize) -> (usize, usize) {
    let b = relative_index(Some(b), len);
    let e = match e {
        None => len,
        Some(e) => relative_index(Some(e), len),
    };
    (b, e.max(b))
}

/// `String.prototype.indexOf` on UTF-16, returning the code point index of the match.
fn string_index_of(hay: &str, needle: &str, from: Option<f64>) -> f64 {
    let h: Vec<u16> = hay.encode_utf16().collect();
    let n: Vec<u16> = needle.encode_utf16().collect();
    let start = from.map_or(0.0, |f| if f.is_nan() { 0.0 } else { f.trunc() }).clamp(0.0, h.len() as f64) as usize;
    let found = (start..=h.len()).find(|&i| h.len() - i >= n.len() && h[i..i + n.len()] == n[..]);
    match found {
        None => -1.0,
        Some(i) => String::from_utf16_lossy(&h[..i]).chars().count() as f64,
    }
}

fn find_stop(labels: &[f64], input: f64, key: &str) -> Result<usize, RuntimeError> {
    let last = labels.len() - 1;
    let (mut lo, mut hi) = (0i64, last as i64);
    while lo <= hi {
        let cur = ((lo + hi) / 2) as usize;
        let v = labels[cur];
        if v <= input {
            if cur == last || input < labels[cur + 1] {
                return Ok(cur);
            }
            lo = cur as i64 + 1;
        } else if v > input {
            hi = cur as i64 - 1;
        } else {
            return Err(err("Input is not a number.", key));
        }
    }
    Ok(0)
}

fn compare_basic(op: CmpOp, l: &Value, r: &Value) -> bool {
    match op {
        CmpOp::Eq => l.strict_eq(r),
        CmpOp::Ne => !l.strict_eq(r),
        _ => {
            let ord = match (l, r) {
                (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
                (Value::String(a), Value::String(b)) => Some(js_string_cmp(a, b)),
                _ => None,
            };
            use std::cmp::Ordering::*;
            match (op, ord) {
                (_, None) => false,
                (CmpOp::Lt, Some(o)) => o == Less,
                (CmpOp::Gt, Some(o)) => o == Greater,
                (CmpOp::Le, Some(o)) => o != Greater,
                (CmpOp::Ge, Some(o)) => o != Less,
                _ => false,
            }
        }
    }
}

/// JS string ordering compares UTF-16 code units.
pub fn js_string_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn interpolate_values(ty: &Type, space: ColorSpace, lo: &Value, hi: &Value, t: f64, key: &str) -> EvalResult {
    let lerp = |a: f64, b: f64| a + t * (b - a);
    let lerp_arr = |a: &[f64], b: &[f64]| -> Vec<f64> { a.iter().enumerate().map(|(i, &x)| lerp(x, b.get(i).copied().unwrap_or(f64::NAN))).collect() };
    Ok(match (ty, lo, hi) {
        (Type::Number, Value::Number(a), Value::Number(b)) => Value::Number(lerp(*a, *b)),
        (Type::Color, Value::Color(a), Value::Color(b)) => Value::Color(Color::interpolate(a, b, t, space)),
        (Type::Padding, Value::Padding(a), Value::Padding(b)) => Value::Padding(std::array::from_fn(|i| lerp(a[i], b[i]))),
        (Type::NumberArray, Value::NumberArray(a), Value::NumberArray(b)) => Value::NumberArray(Rc::new(lerp_arr(a, b))),
        (Type::ColorArray, Value::ColorArray(a), Value::ColorArray(b)) => {
            if a.len() != b.len() {
                return Err(err(
                    format!("colorArray: Arrays have mismatched length ({} vs. {}), cannot interpolate.", a.len(), b.len()),
                    "",
                ));
            }
            Value::ColorArray(Rc::new(a.iter().zip(b.iter()).map(|(x, y)| Color::interpolate(x, y, t, space)).collect()))
        }
        (Type::VariableAnchorOffsetCollection, Value::VariableAnchorOffsetCollection(a), Value::VariableAnchorOffsetCollection(b)) => {
            if a.len() != b.len() {
                return Err(err(
                    format!(
                        "Cannot interpolate values of different length. from: {}, to: {}",
                        lo.to_js_string(),
                        hi.to_js_string()
                    ),
                    key,
                ));
            }
            let mut out = Vec::new();
            for i in (0..a.len()).step_by(2) {
                if !a[i].strict_eq(&b[i]) {
                    return Err(err(
                        format!(
                            "Cannot interpolate values containing mismatched anchors. from[{i}]: {}, to[{i}]: {}",
                            a[i].to_js_string(),
                            b[i].to_js_string()
                        ),
                        key,
                    ));
                }
                out.push(a[i].clone());
                let xy = |v: &Value| -> [f64; 2] {
                    match v {
                        Value::Array(p) => [p[0].as_f64().unwrap_or(f64::NAN), p[1].as_f64().unwrap_or(f64::NAN)],
                        _ => [f64::NAN; 2],
                    }
                };
                let (f, o) = (xy(&a[i + 1]), xy(&b[i + 1]));
                out.push(Value::Array(Rc::new(vec![Value::Number(lerp(f[0], o[0])), Value::Number(lerp(f[1], o[1]))])));
            }
            Value::VariableAnchorOffsetCollection(Rc::new(out))
        }
        (Type::Array(..), Value::Array(a), Value::Array(b)) => Value::Array(Rc::new(
            a.iter()
                .enumerate()
                .map(|(i, x)| Value::Number(lerp(x.as_f64().unwrap_or(f64::NAN), b.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN))))
                .collect(),
        )),
        (Type::ProjectionDefinition, a, b) => {
            let s = |v: &Value| v.as_str().map_or_else(|| v.to_js_string(), str::to_string);
            Value::ProjectionDefinition(Rc::new(ProjectionDefinition { from: s(a), to: s(b), transition: t }))
        }
        _ => Value::Null,
    })
}

pub fn validate_rgba(r: &Value, g: &Value, b: &Value, a: Option<&Value>) -> Option<String> {
    let in_range = |v: &Value, max: f64| v.as_f64().is_some_and(|n| (0.0..=max).contains(&n));
    let join = |vals: &[&Value]| vals.iter().map(|v| v.to_js_string()).collect::<Vec<_>>().join(", ");
    if !(in_range(r, 255.0) && in_range(g, 255.0) && in_range(b, 255.0)) {
        let vals: Vec<&Value> = match a {
            Some(a @ Value::Number(_)) => vec![r, g, b, a],
            _ => vec![r, g, b],
        };
        return Some(format!("Invalid rgba value [{}]: 'r', 'g', and 'b' must be between 0 and 255.", join(&vals)));
    }
    if let Some(a) = a
        && !in_range(a, 1.0)
    {
        return Some(format!("Invalid rgba value [{}]: 'a' must be between 0 and 1.", join(&[r, g, b, a])));
    }
    None
}

pub fn parse_padding(v: &Value) -> Option<Value> {
    match v {
        Value::Padding(_) => Some(v.clone()),
        Value::Number(n) => Some(Value::Padding([*n; 4])),
        Value::Array(a) => {
            let n: Vec<f64> = a.iter().map(Value::as_f64).collect::<Option<_>>()?;
            Some(Value::Padding(match n.len() {
                1 => [n[0]; 4],
                2 => [n[0], n[1], n[0], n[1]],
                3 => [n[0], n[1], n[2], n[1]],
                4 => [n[0], n[1], n[2], n[3]],
                _ => return None,
            }))
        }
        _ => None,
    }
}

pub fn parse_number_array(v: &Value) -> Option<Value> {
    match v {
        Value::NumberArray(_) => Some(v.clone()),
        Value::Number(n) => Some(Value::NumberArray(Rc::new(vec![*n]))),
        Value::Array(a) => Some(Value::NumberArray(Rc::new(a.iter().map(Value::as_f64).collect::<Option<_>>()?))),
        _ => None,
    }
}

pub fn parse_color_array(v: &Value) -> Option<Value> {
    match v {
        Value::ColorArray(_) => Some(v.clone()),
        Value::String(s) => Some(Value::ColorArray(Rc::new(vec![Color::parse(s)?]))),
        Value::Array(a) => Some(Value::ColorArray(Rc::new(a.iter().map(|c| c.as_str().and_then(Color::parse)).collect::<Option<_>>()?))),
        _ => None,
    }
}

pub fn parse_variable_anchor_offset_collection(v: &Value) -> Option<Value> {
    const ANCHORS: [&str; 9] = ["center", "left", "right", "top", "bottom", "top-left", "top-right", "bottom-left", "bottom-right"];
    match v {
        Value::VariableAnchorOffsetCollection(_) => Some(v.clone()),
        Value::Array(a) if !a.is_empty() && a.len() % 2 == 0 => {
            for pair in a.chunks(2) {
                if !pair[0].as_str().is_some_and(|s| ANCHORS.contains(&s)) {
                    return None;
                }
                match &pair[1] {
                    Value::Array(o) if o.len() == 2 && o[0].as_f64().is_some() && o[1].as_f64().is_some() => {}
                    _ => return None,
                }
            }
            Some(Value::VariableAnchorOffsetCollection(a.clone()))
        }
        _ => None,
    }
}

pub fn parse_projection_definition(v: &Value) -> Option<ProjectionDefinition> {
    match v {
        Value::ProjectionDefinition(p) => Some((**p).clone()),
        Value::Array(a) if a.len() == 3 => Some(ProjectionDefinition {
            from: a[0].as_str()?.into(),
            to: a[1].as_str()?.into(),
            transition: a[2].as_f64()?,
        }),
        Value::Object(o) => Some(ProjectionDefinition {
            from: o.get("from")?.as_str()?.into(),
            to: o.get("to")?.as_str()?.into(),
            transition: o.get("transition")?.as_f64()?,
        }),
        Value::String(s) => Some(ProjectionDefinition { from: s.to_string(), to: s.to_string(), transition: 1.0 }),
        _ => None,
    }
}
