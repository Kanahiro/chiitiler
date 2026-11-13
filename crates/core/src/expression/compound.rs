//! Built-in operators (style-spec `compound_expression.ts`).

use super::color::Color;
use super::expr::*;
use super::types::Type;
use super::value::*;
use std::rc::Rc;

pub enum Sig {
    Fixed(Vec<Type>),
    Varargs(Type),
}

impl std::fmt::Display for Sig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Sig::Fixed(p) => write!(f, "({})", p.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")),
            Sig::Varargs(t) => write!(f, "({t}...)"),
        }
    }
}

pub struct Definition {
    pub ty: Type,
    pub overloads: Vec<(Sig, CompoundFn)>,
}

fn rt(message: impl Into<String>, key: &str) -> RuntimeError {
    RuntimeError { message: message.into(), path: key.into() }
}

fn n(ctx: &EvalCtx, e: &Expr) -> Result<f64, RuntimeError> {
    Ok(e.evaluate(ctx)?.as_f64().unwrap_or(f64::NAN))
}

fn s(ctx: &EvalCtx, e: &Expr) -> Result<String, RuntimeError> {
    Ok(e.evaluate(ctx)?.to_js_string())
}

fn lit(e: &Expr) -> Value {
    if let Node::Literal(v) = &e.node { v.clone() } else { Value::Null }
}

/// `s` for a property name, borrowed when it is a literal string (most are: evaluated per feature).
fn key<'a>(ctx: &EvalCtx, e: &'a Expr) -> Result<std::borrow::Cow<'a, str>, RuntimeError> {
    match &e.node {
        Node::Literal(Value::String(k)) => Ok(std::borrow::Cow::Borrowed(k)),
        _ => Ok(std::borrow::Cow::Owned(s(ctx, e)?)),
    }
}

/// `lit(e).to_js_string()`, borrowed for a literal string.
fn lit_key(e: &Expr) -> std::borrow::Cow<'_, str> {
    match &e.node {
        Node::Literal(Value::String(k)) => std::borrow::Cow::Borrowed(k),
        _ => std::borrow::Cow::Owned(lit(e).to_js_string()),
    }
}

macro_rules! math1 {
    ($f:expr) => {
        |ctx: &EvalCtx, a: &[Expr], _: &str| -> EvalResult { Ok(Value::Number($f(n(ctx, &a[0])?))) }
    };
}

fn js_pow(b: f64, e: f64) -> f64 {
    if e.is_nan() || (b.abs() == 1.0 && e.is_infinite()) { f64::NAN } else { b.powf(e) }
}

fn get_prop(obj: &Object, key: &str) -> Value {
    obj.get(key).cloned().unwrap_or(Value::Null)
}

fn rgba(ctx: &EvalCtx, a: &[Expr], key: &str) -> EvalResult {
    let r = a[0].evaluate(ctx)?;
    let g = a[1].evaluate(ctx)?;
    let b = a[2].evaluate(ctx)?;
    let alpha = match a.get(3) {
        Some(e) => e.evaluate(ctx)?,
        None => Value::Number(1.0),
    };
    if let Some(e) = validate_rgba(&r, &g, &b, Some(&alpha)) {
        return Err(rt(e, key));
    }
    let f = |v: &Value| v.as_f64().unwrap();
    Ok(Value::Color(Color::new(f(&r) / 255.0, f(&g) / 255.0, f(&b) / 255.0, f(&alpha))))
}

/// Typed comparison for the legacy `filter-<` family: false unless both are the same JS type.
fn filter_cmp(a: &Value, b: &Value, accept: fn(std::cmp::Ordering) -> bool) -> bool {
    let ord = match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y),
        (Value::String(x), Value::String(y)) => Some(js_string_cmp(x, y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    };
    ord.is_some_and(accept)
}

fn filter_prop(ctx: &EvalCtx, k: &Expr) -> Value {
    feature_prop(ctx, &lit_key(k))
}

fn feature_prop(ctx: &EvalCtx, key: &str) -> Value {
    ctx.feature.and_then(|f| f.properties.get(key)).cloned().unwrap_or(Value::Null)
}

fn has_prop(ctx: &EvalCtx, key: &str) -> bool {
    ctx.feature.is_some_and(|f| f.properties.contains_key(key))
}

fn feature_id(ctx: &EvalCtx) -> Value {
    ctx.feature.and_then(|f| f.id.clone()).unwrap_or(Value::Null)
}

fn geometry_type(ctx: &EvalCtx) -> Value {
    ctx.feature.and_then(|f| f.geom_type).map_or(Value::Null, Value::str)
}

fn props(ctx: &EvalCtx) -> Rc<Object> {
    ctx.feature.map(|f| f.properties.to_object()).unwrap_or_default()
}

const NUMBER: Type = Type::Number;
const STRING: Type = Type::String;
const BOOLEAN: Type = Type::Boolean;
const VALUE: Type = Type::Value;
const OBJECT: Type = Type::Object;
const COLOR: Type = Type::Color;
const ERROR: Type = Type::Error;
const COLLATOR: Type = Type::Collator;

pub fn definition(name: &str) -> Option<Definition> {
    let one = |ty: Type, sig: Vec<Type>, f: CompoundFn| Some(Definition { ty, overloads: vec![(Sig::Fixed(sig), f)] });
    let va = |ty: Type, t: Type, f: CompoundFn| Some(Definition { ty, overloads: vec![(Sig::Varargs(t), f)] });
    match name {
        "error" => one(ERROR, vec![STRING], |ctx, a, key| Err(rt(s(ctx, &a[0])?, key))),
        "typeof" => one(STRING, vec![VALUE], |ctx, a, _| Ok(Value::str(&a[0].evaluate(ctx)?.type_of().to_string()))),
        "to-rgba" => one(Type::array(NUMBER, Some(4)), vec![COLOR], |ctx, a, _| {
            let Value::Color(c) = a[0].evaluate(ctx)? else { return Ok(Value::Null) };
            let [r, g, b, al] = c.rgb();
            Ok(Value::Array(Rc::new(vec![Value::Number(r * 255.0), Value::Number(g * 255.0), Value::Number(b * 255.0), Value::Number(al)])))
        }),
        "rgb" => one(COLOR, vec![NUMBER, NUMBER, NUMBER], rgba),
        "rgba" => one(COLOR, vec![NUMBER, NUMBER, NUMBER, NUMBER], rgba),
        "has" => Some(Definition {
            ty: BOOLEAN,
            overloads: vec![
                (Sig::Fixed(vec![STRING]), |ctx, a, _| {
                    let k = key(ctx, &a[0])?;
                    Ok(Value::Bool(has_prop(ctx, &k)))
                }),
                (Sig::Fixed(vec![STRING, OBJECT]), |ctx, a, _| {
                    let k = key(ctx, &a[0])?;
                    Ok(Value::Bool(matches!(a[1].evaluate(ctx)?, Value::Object(o) if o.contains_key(&*k))))
                }),
            ],
        }),
        "get" => Some(Definition {
            ty: VALUE,
            overloads: vec![
                (Sig::Fixed(vec![STRING]), |ctx, a, _| {
                    let k = key(ctx, &a[0])?;
                    Ok(feature_prop(ctx, &k))
                }),
                (Sig::Fixed(vec![STRING, OBJECT]), |ctx, a, _| {
                    let k = key(ctx, &a[0])?;
                    Ok(match a[1].evaluate(ctx)? {
                        Value::Object(o) => get_prop(&o, &k),
                        _ => Value::Null,
                    })
                }),
            ],
        }),
        "feature-state" => one(VALUE, vec![STRING], |ctx, a, _| {
            let k = key(ctx, &a[0])?;
            Ok(ctx.feature_state.map_or(Value::Null, |st| get_prop(st, &k)))
        }),
        "properties" => one(OBJECT, vec![], |ctx, _, _| Ok(Value::Object(props(ctx)))),
        "geometry-type" => one(STRING, vec![], |ctx, _, _| Ok(geometry_type(ctx))),
        "id" => one(VALUE, vec![], |ctx, _, _| Ok(feature_id(ctx))),
        "zoom" => one(NUMBER, vec![], |ctx, _, _| Ok(Value::Number(ctx.globals.zoom))),
        "heatmap-density" => one(NUMBER, vec![], |ctx, _, _| Ok(Value::Number(ctx.globals.heatmap_density.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0)))),
        "elevation" => one(NUMBER, vec![], |ctx, _, _| Ok(Value::Number(ctx.globals.elevation.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0)))),
        "line-progress" => one(NUMBER, vec![], |ctx, _, _| Ok(Value::Number(ctx.globals.line_progress.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0)))),
        "accumulated" => one(VALUE, vec![], |ctx, _, _| Ok(ctx.globals.accumulated.clone().unwrap_or(Value::Null))),
        "+" => va(NUMBER, NUMBER, |ctx, a, _| {
            let mut r = 0.0;
            for e in a {
                r += n(ctx, e)?;
            }
            Ok(Value::Number(r))
        }),
        "*" => va(NUMBER, NUMBER, |ctx, a, _| {
            let mut r = 1.0;
            for e in a {
                r *= n(ctx, e)?;
            }
            Ok(Value::Number(r))
        }),
        "-" => Some(Definition {
            ty: NUMBER,
            overloads: vec![
                (Sig::Fixed(vec![NUMBER, NUMBER]), |ctx, a, _| Ok(Value::Number(n(ctx, &a[0])? - n(ctx, &a[1])?))),
                (Sig::Fixed(vec![NUMBER]), |ctx, a, _| Ok(Value::Number(-n(ctx, &a[0])?))),
            ],
        }),
        "/" => one(NUMBER, vec![NUMBER, NUMBER], |ctx, a, _| Ok(Value::Number(n(ctx, &a[0])? / n(ctx, &a[1])?))),
        "%" => one(NUMBER, vec![NUMBER, NUMBER], |ctx, a, _| Ok(Value::Number(n(ctx, &a[0])? % n(ctx, &a[1])?))),
        "ln2" => one(NUMBER, vec![], |_, _, _| Ok(Value::Number(std::f64::consts::LN_2))),
        "pi" => one(NUMBER, vec![], |_, _, _| Ok(Value::Number(std::f64::consts::PI))),
        "e" => one(NUMBER, vec![], |_, _, _| Ok(Value::Number(std::f64::consts::E))),
        "^" => one(NUMBER, vec![NUMBER, NUMBER], |ctx, a, _| Ok(Value::Number(js_pow(n(ctx, &a[0])?, n(ctx, &a[1])?)))),
        "sqrt" => one(NUMBER, vec![NUMBER], math1!(f64::sqrt)),
        "log10" => one(NUMBER, vec![NUMBER], math1!(|x: f64| x.ln() / std::f64::consts::LN_10)),
        "ln" => one(NUMBER, vec![NUMBER], math1!(f64::ln)),
        "log2" => one(NUMBER, vec![NUMBER], math1!(|x: f64| x.ln() / std::f64::consts::LN_2)),
        "sin" => one(NUMBER, vec![NUMBER], math1!(f64::sin)),
        "cos" => one(NUMBER, vec![NUMBER], math1!(f64::cos)),
        "tan" => one(NUMBER, vec![NUMBER], math1!(f64::tan)),
        "asin" => one(NUMBER, vec![NUMBER], math1!(f64::asin)),
        "acos" => one(NUMBER, vec![NUMBER], math1!(f64::acos)),
        "atan" => one(NUMBER, vec![NUMBER], math1!(f64::atan)),
        "min" => va(NUMBER, NUMBER, |ctx, a, _| {
            let mut r = f64::INFINITY;
            for e in a {
                let v = n(ctx, e)?;
                r = if v.is_nan() || r.is_nan() { f64::NAN } else { r.min(v) };
            }
            Ok(Value::Number(r))
        }),
        "max" => va(NUMBER, NUMBER, |ctx, a, _| {
            let mut r = f64::NEG_INFINITY;
            for e in a {
                let v = n(ctx, e)?;
                r = if v.is_nan() || r.is_nan() { f64::NAN } else { r.max(v) };
            }
            Ok(Value::Number(r))
        }),
        "abs" => one(NUMBER, vec![NUMBER], math1!(f64::abs)),
        "round" => one(NUMBER, vec![NUMBER], math1!(|v: f64| if v < 0.0 { -js_round(-v) } else { js_round(v) })),
        "floor" => one(NUMBER, vec![NUMBER], math1!(f64::floor)),
        "ceil" => one(NUMBER, vec![NUMBER], math1!(f64::ceil)),
        "filter-==" => one(BOOLEAN, vec![STRING, VALUE], |ctx, a, _| Ok(Value::Bool(filter_prop(ctx, &a[0]).strict_eq(&lit(&a[1]))))),
        "filter-id-==" => one(BOOLEAN, vec![VALUE], |ctx, a, _| Ok(Value::Bool(feature_id(ctx).strict_eq(&lit(&a[0]))))),
        "filter-type-==" => one(BOOLEAN, vec![STRING], |ctx, a, _| Ok(Value::Bool(geometry_type(ctx).strict_eq(&lit(&a[0]))))),
        "filter-<" => one(BOOLEAN, vec![STRING, VALUE], |ctx, a, _| Ok(Value::Bool(filter_cmp(&filter_prop(ctx, &a[0]), &lit(&a[1]), |o| o.is_lt())))),
        "filter-id-<" => one(BOOLEAN, vec![VALUE], |ctx, a, _| Ok(Value::Bool(filter_cmp(&feature_id(ctx), &lit(&a[0]), |o| o.is_lt())))),
        "filter->" => one(BOOLEAN, vec![STRING, VALUE], |ctx, a, _| Ok(Value::Bool(filter_cmp(&filter_prop(ctx, &a[0]), &lit(&a[1]), |o| o.is_gt())))),
        "filter-id->" => one(BOOLEAN, vec![VALUE], |ctx, a, _| Ok(Value::Bool(filter_cmp(&feature_id(ctx), &lit(&a[0]), |o| o.is_gt())))),
        "filter-<=" => one(BOOLEAN, vec![STRING, VALUE], |ctx, a, _| Ok(Value::Bool(filter_cmp(&filter_prop(ctx, &a[0]), &lit(&a[1]), |o| o.is_le())))),
        "filter-id-<=" => one(BOOLEAN, vec![VALUE], |ctx, a, _| Ok(Value::Bool(filter_cmp(&feature_id(ctx), &lit(&a[0]), |o| o.is_le())))),
        "filter->=" => one(BOOLEAN, vec![STRING, VALUE], |ctx, a, _| Ok(Value::Bool(filter_cmp(&filter_prop(ctx, &a[0]), &lit(&a[1]), |o| o.is_ge())))),
        "filter-id->=" => one(BOOLEAN, vec![VALUE], |ctx, a, _| Ok(Value::Bool(filter_cmp(&feature_id(ctx), &lit(&a[0]), |o| o.is_ge())))),
        "filter-has" => one(BOOLEAN, vec![VALUE], |ctx, a, _| Ok(Value::Bool(has_prop(ctx, &lit_key(&a[0]))))),
        "filter-has-id" => one(BOOLEAN, vec![], |ctx, _, _| Ok(Value::Bool(!feature_id(ctx).is_null()))),
        "filter-type-in" => one(BOOLEAN, vec![Type::array(STRING, None)], |ctx, a, _| {
            let t = geometry_type(ctx);
            Ok(Value::Bool(matches!(lit(&a[0]), Value::Array(v) if v.iter().any(|x| x.strict_eq(&t)))))
        }),
        "filter-id-in" => one(BOOLEAN, vec![Type::array(VALUE, None)], |ctx, a, _| {
            let id = feature_id(ctx);
            Ok(Value::Bool(matches!(lit(&a[0]), Value::Array(v) if v.iter().any(|x| x.strict_eq(&id)))))
        }),
        "filter-in-small" | "filter-in-large" => one(BOOLEAN, vec![STRING, Type::array(VALUE, None)], |ctx, a, _| {
            let p = filter_prop(ctx, &a[0]);
            Ok(Value::Bool(matches!(lit(&a[1]), Value::Array(v) if v.iter().any(|x| x.strict_eq(&p)))))
        }),
        "all" => Some(Definition {
            ty: BOOLEAN,
            overloads: vec![
                (Sig::Fixed(vec![BOOLEAN, BOOLEAN]), |ctx, a, _| Ok(Value::Bool(a[0].evaluate(ctx)?.truthy() && a[1].evaluate(ctx)?.truthy()))),
                (Sig::Varargs(BOOLEAN), |ctx, a, _| {
                    for e in a {
                        if !e.evaluate(ctx)?.truthy() {
                            return Ok(Value::Bool(false));
                        }
                    }
                    Ok(Value::Bool(true))
                }),
            ],
        }),
        "any" => Some(Definition {
            ty: BOOLEAN,
            overloads: vec![
                (Sig::Fixed(vec![BOOLEAN, BOOLEAN]), |ctx, a, _| Ok(Value::Bool(a[0].evaluate(ctx)?.truthy() || a[1].evaluate(ctx)?.truthy()))),
                (Sig::Varargs(BOOLEAN), |ctx, a, _| {
                    for e in a {
                        if e.evaluate(ctx)?.truthy() {
                            return Ok(Value::Bool(true));
                        }
                    }
                    Ok(Value::Bool(false))
                }),
            ],
        }),
        "!" => one(BOOLEAN, vec![BOOLEAN], |ctx, a, _| Ok(Value::Bool(!a[0].evaluate(ctx)?.truthy()))),
        // Every script is supported: there is no RTL plugin and glyphs come from PBF ranges.
        "is-supported-script" => one(BOOLEAN, vec![STRING], |_, _, _| Ok(Value::Bool(true))),
        "upcase" => one(STRING, vec![STRING], |ctx, a, _| Ok(Value::str(&s(ctx, &a[0])?.to_uppercase()))),
        "downcase" => one(STRING, vec![STRING], |ctx, a, _| Ok(Value::str(&s(ctx, &a[0])?.to_lowercase()))),
        "concat" => va(STRING, VALUE, |ctx, a, _| {
            let mut out = std::string::String::new();
            for e in a {
                out.push_str(&e.evaluate(ctx)?.to_js_string());
            }
            Ok(Value::str(&out))
        }),
        "split" => one(Type::array(STRING, None), vec![STRING, STRING], |ctx, a, _| {
            let (text, delim) = (s(ctx, &a[0])?, s(ctx, &a[1])?);
            let parts: Vec<Value> = if delim.is_empty() {
                text.chars().map(|c| Value::str(&c.to_string())).collect()
            } else {
                text.split(delim.as_str()).map(Value::str).collect()
            };
            Ok(Value::Array(Rc::new(parts)))
        }),
        "join" => one(STRING, vec![Type::array(STRING, None), STRING], |ctx, a, _| {
            let arr = a[0].evaluate(ctx)?;
            let delim = s(ctx, &a[1])?;
            Ok(Value::str(&match arr {
                Value::Array(v) => v.iter().map(Value::to_js_string).collect::<Vec<_>>().join(&delim),
                _ => std::string::String::new(),
            }))
        }),
        "resolved-locale" => one(STRING, vec![COLLATOR], |ctx, a, _| match a[0].evaluate(ctx)? {
            Value::Collator(c) => Ok(Value::str(&c.resolved_locale())),
            _ => Ok(Value::Null),
        }),
        _ => None,
    }
}
