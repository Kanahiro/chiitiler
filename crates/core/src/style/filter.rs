//! Layer filters (style-spec `feature_filter/index.ts`): legacy syntax is converted to the
//! internal `filter-*` operators, then compiled like a boolean data-driven property.

use crate::expression::{self as ex, Expression};
use serde_json::{json, Value as J};

#[derive(PartialEq)]
enum Class {
    Expression,
    Legacy,
    Neutral,
}

fn classify(f: &J) -> Class {
    if f.is_boolean() {
        return Class::Neutral;
    }
    let Some(a) = f.as_array().filter(|a| !a.is_empty()) else { return Class::Legacy };
    let arr = |i: usize| a.get(i).is_some_and(J::is_array);
    match a[0].as_str() {
        Some("has") => {
            if a.len() < 2 || a[1] == "$id" || a[1] == "$type" {
                Class::Legacy
            } else if a.len() == 2 {
                Class::Neutral
            } else {
                Class::Expression
            }
        }
        Some("in") => {
            if a.len() >= 3 && (!a[1].is_string() || arr(2)) {
                Class::Expression
            } else {
                Class::Legacy
            }
        }
        Some("!in" | "!has" | "none") => Class::Legacy,
        Some("==" | "!=" | ">" | ">=" | "<" | "<=") => {
            if a.len() != 3 || arr(1) || arr(2) {
                Class::Expression
            } else {
                Class::Legacy
            }
        }
        Some("any" | "all") => {
            let mut saw_legacy = false;
            for c in &a[1..] {
                match classify(c) {
                    Class::Expression => return Class::Expression,
                    Class::Legacy => saw_legacy = true,
                    Class::Neutral => {}
                }
            }
            if saw_legacy { Class::Legacy } else { Class::Neutral }
        }
        _ => Class::Expression,
    }
}

fn js_cmp(a: &J, b: &J) -> std::cmp::Ordering {
    match (a, b) {
        (J::Number(x), J::Number(y)) => x.as_f64().partial_cmp(&y.as_f64()).unwrap_or(std::cmp::Ordering::Equal),
        (J::String(x), J::String(y)) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    }
}

fn convert(f: &J) -> J {
    let Some(a) = f.as_array() else { return json!(true) };
    let Some(op) = a.first().and_then(J::as_str) else { return json!(true) };
    if a.len() <= 1 {
        return json!(op != "any");
    }
    let cmp = |op: &str| -> J {
        match a[1].as_str() {
            Some("$type") => json!([format!("filter-type-{op}"), a[2]]),
            Some("$id") => json!([format!("filter-id-{op}"), a[2]]),
            _ => json!([format!("filter-{op}"), a[1], a.get(2).cloned().unwrap_or(J::Null)]),
        }
    };
    let in_op = || -> J {
        let values: Vec<J> = a[2..].to_vec();
        if values.is_empty() {
            return json!(false);
        }
        match a[1].as_str() {
            Some("$type") => json!(["filter-type-in", ["literal", values]]),
            Some("$id") => json!(["filter-id-in", ["literal", values]]),
            _ => {
                let same_type = values.iter().all(|v| std::mem::discriminant(v) == std::mem::discriminant(&values[0]));
                if values.len() > 200 && same_type {
                    let mut sorted = values;
                    sorted.sort_by(js_cmp);
                    json!(["filter-in-large", a[1], ["literal", sorted]])
                } else {
                    json!(["filter-in-small", a[1], ["literal", values]])
                }
            }
        }
    };
    let has_op = || -> J {
        match a[1].as_str() {
            Some("$type") => json!(true),
            Some("$id") => json!(["filter-has-id"]),
            _ => json!(["filter-has", a[1]]),
        }
    };
    let not = |v: J| json!(["!", v]);
    match op {
        "==" => cmp("=="),
        "!=" => not(cmp("==")),
        "<" | ">" | "<=" | ">=" => cmp(op),
        "any" => J::Array(std::iter::once(json!("any")).chain(a[1..].iter().map(convert)).collect()),
        "all" => J::Array(std::iter::once(json!("all")).chain(a[1..].iter().map(convert)).collect()),
        "none" => J::Array(std::iter::once(json!("all")).chain(a[1..].iter().map(|c| not(convert(c)))).collect()),
        "in" => in_op(),
        "!in" => not(in_op()),
        "has" => has_op(),
        "!has" => not(has_op()),
        _ => json!(true),
    }
}

pub fn compile(filter: &J, global_state: &J) -> Option<Expression> {
    if filter.is_null() {
        return None;
    }
    let expr = if classify(filter) == Class::Legacy { convert(filter) } else { filter.clone() };
    let spec = json!({
        "type": "boolean",
        "default": false,
        "property-type": "data-driven",
        "expression": {"interpolated": false, "parameters": ["zoom", "feature"]}
    });
    ex::create_expression(&expr, &spec, Some(global_state)).ok()
}
