//! Runtime values (style-spec `expression/values.ts` and `types/*`), with the JS
//! semantics the expression tests depend on (number formatting, JSON.stringify, ...).

use super::color::Color;
use super::types::Type;
use crate::hash::IndexMap;
use std::rc::Rc;

pub type Object = IndexMap<Rc<str>, Value>;

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(Rc<str>),
    Color(Color),
    Collator(Rc<super::intl::Collator>),
    Formatted(Rc<Formatted>),
    Padding([f64; 4]),
    NumberArray(Rc<Vec<f64>>),
    ColorArray(Rc<Vec<Color>>),
    /// Alternating anchor strings and `[x, y]` offsets.
    VariableAnchorOffsetCollection(Rc<Vec<Value>>),
    ResolvedImage(Rc<ResolvedImage>),
    ProjectionDefinition(Rc<ProjectionDefinition>),
    Array(Rc<Vec<Value>>),
    Object(Rc<Object>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedImage {
    pub name: String,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionDefinition {
    pub from: String,
    pub to: String,
    pub transition: f64,
}

#[derive(Debug, Clone)]
pub struct FormattedSection {
    pub text: String,
    pub image: Option<Rc<ResolvedImage>>,
    pub scale: Option<f64>,
    pub font_stack: Option<String>,
    pub text_color: Option<Color>,
    pub vertical_align: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Formatted {
    pub sections: Vec<FormattedSection>,
}

impl Formatted {
    pub fn from_string(s: String) -> Self {
        Formatted {
            sections: vec![FormattedSection {
                text: s,
                image: None,
                scale: None,
                font_stack: None,
                text_color: None,
                vertical_align: None,
            }],
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.sections.iter().any(|s| !s.text.is_empty() || s.image.as_ref().is_some_and(|i| !i.name.is_empty()))
    }
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::String(Rc::from(s))
    }

    pub fn as_f64(&self) -> Option<f64> {
        if let Value::Number(n) = self { Some(*n) } else { None }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Value::String(s) = self { Some(s) } else { None }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn from_json(v: &serde_json::Value) -> Value {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => Value::str(s),
            serde_json::Value::Array(a) => Value::Array(Rc::new(a.iter().map(Value::from_json).collect())),
            serde_json::Value::Object(o) => {
                Value::Object(Rc::new(o.iter().map(|(k, v)| (k.as_str().into(), Value::from_json(v))).collect()))
            }
        }
    }

    /// JSON view mirroring the enumerable own properties of the upstream JS objects.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::{json, Value as J};
        let num = |n: f64| serde_json::Number::from_f64(n).map_or(J::Null, J::Number);
        match self {
            Value::Null => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Number(n) => num(*n),
            Value::String(s) => J::String(s.to_string()),
            Value::Color(c) => json!({"r": num(c.r), "g": num(c.g), "b": num(c.b), "a": num(c.a)}),
            Value::Collator(c) => json!({"locale": c.locale, "sensitivity": c.sensitivity(), "collator": {}}),
            Value::Formatted(f) => json!({"sections": f.sections.iter().map(|s| json!({
                "text": s.text,
                "image": s.image.as_ref().map(|i| json!({"name": i.name, "available": i.available})),
                "scale": s.scale.map(num),
                "fontStack": s.font_stack,
                "textColor": s.text_color.map(|c| Value::Color(c).to_json()),
                "verticalAlign": s.vertical_align,
            })).collect::<Vec<_>>()}),
            Value::Padding(p) => json!({"values": p.iter().map(|&n| num(n)).collect::<Vec<_>>()}),
            Value::NumberArray(a) => json!({"values": a.iter().map(|&n| num(n)).collect::<Vec<_>>()}),
            Value::ColorArray(a) => {
                json!({"values": a.iter().map(|&c| Value::Color(c).to_json()).collect::<Vec<_>>()})
            }
            Value::VariableAnchorOffsetCollection(a) => {
                json!({"values": a.iter().map(Value::to_json).collect::<Vec<_>>()})
            }
            Value::ResolvedImage(i) => json!({"name": i.name, "available": i.available}),
            Value::ProjectionDefinition(p) => json!({"from": p.from, "to": p.to, "transition": num(p.transition)}),
            Value::Array(a) => J::Array(a.iter().map(Value::to_json).collect()),
            Value::Object(o) => J::Object(o.iter().map(|(k, v)| (k.to_string(), v.to_json())).collect()),
        }
    }

    /// `typeOf` from values.ts.
    pub fn type_of(&self) -> Type {
        match self {
            Value::Null => Type::Null,
            Value::Bool(_) => Type::Boolean,
            Value::Number(_) => Type::Number,
            Value::String(_) => Type::String,
            Value::Color(_) => Type::Color,
            Value::Collator(_) => Type::Collator,
            Value::Formatted(_) => Type::Formatted,
            Value::Padding(_) => Type::Padding,
            Value::NumberArray(_) => Type::NumberArray,
            Value::ColorArray(_) => Type::ColorArray,
            Value::VariableAnchorOffsetCollection(_) => Type::VariableAnchorOffsetCollection,
            Value::ResolvedImage(_) => Type::ResolvedImage,
            Value::ProjectionDefinition(_) => Type::ProjectionDefinition,
            Value::Object(_) => Type::Object,
            Value::Array(items) => {
                let mut item: Option<Type> = None;
                for v in items.iter() {
                    let t = v.type_of();
                    match &item {
                        None => item = Some(t),
                        Some(it) if it.identical(&t) => {}
                        Some(_) => {
                            item = Some(Type::Value);
                            break;
                        }
                    }
                }
                Type::array(item.unwrap_or(Type::Value), Some(items.len()))
            }
        }
    }

    /// `valueToString` from values.ts.
    pub fn to_js_string(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::String(s) => s.to_string(),
            Value::Number(n) => js_number_to_string(*n),
            Value::Bool(b) => b.to_string(),
            Value::Color(c) => c.to_css_string(),
            Value::Formatted(f) => f.sections.iter().map(|s| s.text.as_str()).collect(),
            Value::ResolvedImage(i) => i.name.clone(),
            Value::ProjectionDefinition(p) if p.from == p.to && p.transition == 1.0 => p.from.clone(),
            Value::ProjectionDefinition(p) => {
                format!("[{},{},{}]", json_string(&p.from), json_string(&p.to), js_number_to_string(p.transition))
            }
            Value::Padding(_) | Value::NumberArray(_) | Value::ColorArray(_) | Value::VariableAnchorOffsetCollection(_) => {
                js_stringify(&self.to_json()["values"])
            }
            _ => js_stringify(&self.to_json()),
        }
    }

    /// JS truthiness (`Boolean(v)`).
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    /// JS strict equality (`===`); objects compare by identity.
    pub fn strict_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// `Number(v)`.
    pub fn to_js_number(&self) -> f64 {
        match self {
            Value::Null => 0.0,
            Value::Bool(b) => *b as u8 as f64,
            Value::Number(n) => *n,
            Value::String(s) => js_string_to_number(s),
            Value::Array(a) => match a.len() {
                0 => 0.0,
                1 => js_string_to_number(&a[0].to_js_string()),
                _ => f64::NAN,
            },
            _ => f64::NAN,
        }
    }
}

/// `String(n)` for numbers (ECMAScript Number::toString).
pub fn js_number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let sign = if n < 0.0 { "-" } else { "" };
    // Shortest round-trip digits and exponent, e.g. "1.2345e-7".
    let e = format!("{:e}", n.abs());
    let (mantissa, exp) = e.split_once('e').unwrap();
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n_exp = exp.parse::<i32>().unwrap() + 1; // position of the decimal point
    let body = if k <= n_exp && n_exp <= 21 {
        format!("{digits}{}", "0".repeat((n_exp - k) as usize))
    } else if 0 < n_exp && n_exp <= 21 {
        format!("{}.{}", &digits[..n_exp as usize], &digits[n_exp as usize..])
    } else if -6 < n_exp && n_exp <= 0 {
        format!("0.{}{digits}", "0".repeat((-n_exp) as usize))
    } else {
        let e = n_exp - 1;
        let es = if e >= 0 { format!("+{e}") } else { e.to_string() };
        if k == 1 { format!("{digits}e{es}") } else { format!("{}.{}e{es}", &digits[..1], &digits[1..]) }
    };
    format!("{sign}{body}")
}

/// `Number(s)` for strings.
pub fn js_string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return u64::from_str_radix(rest, radix).map_or(f64::NAN, |v| v as f64);
        }
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // Rust accepts "inf"/"nan"/"infinity", JS does not.
    if t.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// `JSON.stringify` with JS number formatting.
pub fn js_stringify(v: &serde_json::Value) -> String {
    use serde_json::Value as J;
    match v {
        J::Null => "null".into(),
        J::Bool(b) => b.to_string(),
        J::Number(n) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() { js_number_to_string(f) } else { "null".into() }
        }
        J::String(s) => json_string(s),
        J::Array(a) => format!("[{}]", a.iter().map(js_stringify).collect::<Vec<_>>().join(",")),
        J::Object(o) => format!(
            "{{{}}}",
            o.iter().map(|(k, v)| format!("{}:{}", json_string(k), js_stringify(v))).collect::<Vec<_>>().join(",")
        ),
    }
}

pub fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// `Math.round`: halves round toward +∞.
pub fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string() {
        for (n, s) in [
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (123456.789, "123456.789"),
        ] {
            assert_eq!(js_number_to_string(n), s);
        }
        assert_eq!(js_string_to_number(" 12 "), 12.0);
        assert!(js_string_to_number("abc").is_nan());
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
    }
}
