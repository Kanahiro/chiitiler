//! Parsing and type checking (style-spec `parsing_context.ts` and each definition's `parse`).

use super::color::ColorSpace;
use super::compound::{self, Sig};
use super::expr::*;
use super::geometry;
use super::types::{check_subtype, Type};
use super::value::Value;
use serde_json::Value as J;
use std::cell::RefCell;
use crate::hash::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub key: String,
    pub message: String,
}

struct Scope {
    parent: Option<Rc<Scope>>,
    bindings: Vec<(String, Rc<Expr>)>,
}

impl Scope {
    fn get(scope: &Option<Rc<Scope>>, name: &str) -> Option<Rc<Expr>> {
        let s = scope.as_ref()?;
        s.bindings.iter().rev().find(|(n, _)| n == name).map(|(_, e)| e.clone()).or_else(|| Scope::get(&s.parent, name))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Annotation {
    Assert,
    Coerce,
    Omit,
}

#[derive(Clone)]
pub struct ParseCtx {
    path: Vec<usize>,
    pub key: String,
    scope: Option<Rc<Scope>>,
    pub errors: Rc<RefCell<Vec<ParseError>>>,
    pub expected: Option<Type>,
}

pub const SPECIAL_FORMS: &[&str] = &[
    "==", "!=", ">", "<", ">=", "<=", "array", "at", "boolean", "case", "coalesce", "collator", "format", "image", "in", "index-of",
    "interpolate", "interpolate-hcl", "interpolate-lab", "length", "let", "literal", "match", "number", "number-format", "object",
    "semiliteral", "slice", "step", "string", "to-boolean", "to-color", "to-number", "to-string", "var", "within", "distance",
    "global-state",
];

pub fn is_expression_name(name: &str) -> bool {
    SPECIAL_FORMS.contains(&name) || compound::definition(name).is_some()
}

fn js_typeof(v: &J) -> &'static str {
    match v {
        J::Null | J::Array(_) | J::Object(_) => "object",
        J::Bool(_) => "boolean",
        J::Number(_) => "number",
        J::String(_) => "string",
    }
}

fn num(v: &J) -> Option<f64> {
    v.as_f64()
}

impl ParseCtx {
    pub fn new(expected: Option<Type>) -> ParseCtx {
        ParseCtx { path: vec![], key: String::new(), scope: None, errors: Default::default(), expected }
    }

    fn concat(&self, index: Option<usize>, expected: Option<Type>, bindings: Option<Vec<(String, Rc<Expr>)>>) -> ParseCtx {
        let mut path = self.path.clone();
        if let Some(i) = index {
            path.push(i);
        }
        let key = path.iter().map(|p| format!("[{p}]")).collect();
        let scope = match bindings {
            Some(b) => Some(Rc::new(Scope { parent: self.scope.clone(), bindings: b })),
            None => self.scope.clone(),
        };
        ParseCtx { path, key, scope, errors: self.errors.clone(), expected }
    }

    /// Same path and scope, fresh error list, no expected type.
    fn fresh(&self) -> ParseCtx {
        ParseCtx { path: self.path.clone(), key: self.key.clone(), scope: self.scope.clone(), errors: Default::default(), expected: None }
    }

    pub fn error(&self, message: impl Into<String>, keys: &[usize]) {
        let key = format!("{}{}", self.key, keys.iter().map(|k| format!("[{k}]")).collect::<String>());
        self.errors.borrow_mut().push(ParseError { key, message: message.into() });
    }

    fn fail<T>(&self, message: impl Into<String>, keys: &[usize]) -> Option<T> {
        self.error(message, keys);
        None
    }

    fn check_subtype(&self, expected: &Type, t: &Type) -> bool {
        match check_subtype(expected, t) {
            Some(e) => {
                self.error(e, &[]);
                true
            }
            None => false,
        }
    }

    pub fn parse_at(&self, expr: &J, index: usize, expected: Option<Type>) -> Option<Expr> {
        self.concat(Some(index), expected, None).parse_impl(expr, None)
    }

    fn parse_with(&self, expr: &J, index: usize, expected: Option<Type>, bindings: Option<Vec<(String, Rc<Expr>)>>, ann: Option<Annotation>) -> Option<Expr> {
        self.concat(Some(index), expected, bindings).parse_impl(expr, ann)
    }

    pub fn parse_impl(&self, expr: &J, ann: Option<Annotation>) -> Option<Expr> {
        let wrapped;
        let expr = match expr {
            J::Null | J::Bool(_) | J::Number(_) | J::String(_) => {
                wrapped = J::Array(vec![J::String("literal".into()), expr.clone()]);
                &wrapped
            }
            e => e,
        };
        match expr {
            J::Array(args) => {
                if args.is_empty() {
                    return self.fail("Expected an array with at least one element. If you wanted a literal array, use [\"literal\", []].", &[]);
                }
                let Some(op) = args[0].as_str() else {
                    return self.fail(
                        format!(
                            "Expression name must be a string, but found {} instead. If you wanted a literal array, use [\"literal\", [...]].",
                            js_typeof(&args[0])
                        ),
                        &[0],
                    );
                };
                if !is_expression_name(op) {
                    return self.fail(format!("Unknown expression \"{op}\". If you wanted a literal array, use [\"literal\", [...]]."), &[0]);
                }
                let mut parsed = self.parse_form(op, args)?;
                if let Some(expected) = &self.expected {
                    let actual = &parsed.ty;
                    let ek = expected.kind();
                    let ak = actual.kind();
                    if matches!(ek, "string" | "number" | "boolean" | "object" | "array") && ak == "value" {
                        parsed = annotate(parsed, expected, ann.unwrap_or(Annotation::Assert), &self.key);
                    } else if (ek == "projectionDefinition" && matches!(ak, "string" | "array" | "value"))
                        || (matches!(ek, "color" | "formatted" | "resolvedImage") && matches!(ak, "value" | "string"))
                        || (matches!(ek, "padding" | "numberArray") && matches!(ak, "value" | "number" | "array"))
                        || (ek == "colorArray" && matches!(ak, "value" | "string" | "array"))
                        || (ek == "variableAnchorOffsetCollection" && matches!(ak, "value" | "array"))
                    {
                        parsed = annotate(parsed, expected, ann.unwrap_or(Annotation::Coerce), &self.key);
                    } else if self.check_subtype(expected, actual) {
                        return None;
                    }
                }
                if !matches!(parsed.node, Node::Literal(_)) && parsed.ty != Type::ResolvedImage && is_constant(&parsed) {
                    match parsed.evaluate(&EvalCtx { globals: Globals { zoom: f64::NAN, ..Default::default() }, ..Default::default() }) {
                        Ok(v) => parsed = Expr::literal(parsed.ty, v),
                        Err(e) => return self.fail(e.message, &[]),
                    }
                }
                Some(parsed)
            }
            J::Object(_) => self.fail("Bare objects invalid. Use [\"literal\", {...}] instead.", &[]),
            _ => self.fail(format!("Expected an array, but found {} instead.", js_typeof(expr)), &[]),
        }
    }

    fn parse_form(&self, op: &str, args: &[J]) -> Option<Expr> {
        match op {
            "literal" => self.parse_literal(args),
            "==" | "!=" | ">" | "<" | ">=" | "<=" => self.parse_comparison(op, args),
            "array" | "boolean" | "number" | "object" | "string" => self.parse_assertion(op, args),
            "to-boolean" | "to-color" | "to-number" | "to-string" => self.parse_coercion(op, args),
            "at" => self.parse_at_expr(args),
            "case" => self.parse_case(args),
            "coalesce" => self.parse_coalesce(args),
            "collator" => self.parse_collator(args),
            "format" => self.parse_format(args),
            "image" => self.parse_image(args),
            "in" | "index-of" => self.parse_in(op, args),
            "interpolate" | "interpolate-hcl" | "interpolate-lab" => self.parse_interpolate(op, args),
            "length" => self.parse_length(args),
            "let" => self.parse_let(args),
            "match" => self.parse_match(args),
            "number-format" => self.parse_number_format(args),
            "semiliteral" => self.parse_semiliteral(args),
            "slice" => self.parse_slice(args),
            "step" => self.parse_step(args),
            "var" => self.parse_var(args),
            "within" => self.parse_within(args),
            "distance" => self.parse_distance(args),
            "global-state" => self.parse_global_state(args),
            _ => self.parse_compound(op, args),
        }
    }

    fn parse_literal(&self, args: &[J]) -> Option<Expr> {
        if args.len() != 2 {
            return self.fail(format!("'literal' expression requires exactly one argument, but found {} instead.", args.len() - 1), &[]);
        }
        let v = Value::from_json(&args[1]);
        let mut ty = v.type_of();
        if let (Type::Array(_, Some(0)), Some(Type::Array(_, n))) = (&ty, &self.expected)
            && (n.is_none() || *n == Some(0))
        {
            ty = self.expected.clone().unwrap();
        }
        Some(Expr::literal(ty, v))
    }

    fn parse_semiliteral(&self, args: &[J]) -> Option<Expr> {
        if args.len() != 2 {
            return self.fail(format!("'semiliteral' expression requires exactly one argument, but found {} instead.", args.len() - 1), &[]);
        }
        let J::Array(items) = &args[1] else {
            let v = Value::from_json(&args[1]);
            return Some(Expr::literal(v.type_of(), v));
        };
        let ctx = self.concat(Some(1), None, None);
        let mut parsed = Vec::new();
        for (i, item) in items.iter().enumerate() {
            parsed.push(ctx.parse_at(item, i, Some(Type::Value))?);
        }
        let mut el: Option<Type> = None;
        for e in &parsed {
            match &el {
                None => el = Some(e.ty.clone()),
                Some(t) if t.identical(&e.ty) => {}
                Some(_) => {
                    el = Some(Type::Value);
                    break;
                }
            }
        }
        Some(Expr { ty: Type::array(el.unwrap_or(Type::Value), Some(parsed.len())), node: Node::Semiliteral(parsed) })
    }

    fn parse_assertion(&self, name: &str, args: &[J]) -> Option<Expr> {
        if args.len() < 2 {
            return self.fail("Expected at least one argument.", &[]);
        }
        let mut i = 1;
        let base = |n: &str| match n {
            "string" => Some(Type::String),
            "number" => Some(Type::Number),
            "boolean" => Some(Type::Boolean),
            "object" => Some(Type::Object),
            _ => None,
        };
        let ty = if name == "array" {
            let item = if args.len() > 2 {
                match args[1].as_str().and_then(|s| if s == "object" { None } else { base(s) }) {
                    Some(t) => {
                        i += 1;
                        t
                    }
                    None => return self.fail("The item type argument of \"array\" must be one of string, number, boolean", &[1]),
                }
            } else {
                Type::Value
            };
            let mut n = None;
            if args.len() > 3 {
                match &args[2] {
                    J::Null => {}
                    J::Number(x) if x.as_f64().is_some_and(|f| f >= 0.0 && f == f.floor()) => n = Some(x.as_f64().unwrap() as usize),
                    _ => return self.fail("The length argument to \"array\" must be a positive integer literal", &[2]),
                }
                i += 1;
            }
            Type::array(item, n)
        } else {
            base(name)?
        };
        let mut parsed = Vec::new();
        while i < args.len() {
            parsed.push(self.parse_at(&args[i], i, Some(Type::Value))?);
            i += 1;
        }
        Some(Expr { ty, node: Node::Assertion { args: parsed, key: self.key.clone() } })
    }

    fn parse_coercion(&self, name: &str, args: &[J]) -> Option<Expr> {
        if args.len() < 2 {
            return self.fail("Expected at least one argument.", &[]);
        }
        if (name == "to-boolean" || name == "to-string") && args.len() != 2 {
            return self.fail("Expected one argument.", &[]);
        }
        let ty = match name {
            "to-boolean" => Type::Boolean,
            "to-color" => Type::Color,
            "to-number" => Type::Number,
            _ => Type::String,
        };
        let mut parsed = Vec::new();
        for (i, a) in args.iter().enumerate().skip(1) {
            parsed.push(self.parse_at(a, i, Some(Type::Value))?);
        }
        Some(Expr { ty, node: Node::Coercion { args: parsed, key: self.key.clone() } })
    }

    fn parse_comparison(&self, op: &str, args: &[J]) -> Option<Expr> {
        let op = match op {
            "==" => CmpOp::Eq,
            "!=" => CmpOp::Ne,
            "<" => CmpOp::Lt,
            ">" => CmpOp::Gt,
            "<=" => CmpOp::Le,
            _ => CmpOp::Ge,
        };
        if args.len() != 3 && args.len() != 4 {
            return self.fail("Expected two or three arguments.", &[]);
        }
        let comparable = |t: &Type| match op {
            CmpOp::Eq | CmpOp::Ne => matches!(t, Type::Boolean | Type::String | Type::Number | Type::Null | Type::Value),
            _ => matches!(t, Type::String | Type::Number | Type::Value),
        };
        let mut lhs = self.parse_at(&args[1], 1, Some(Type::Value))?;
        if !comparable(&lhs.ty) {
            return self.concat(Some(1), None, None).fail(format!("\"{}\" comparisons are not supported for type '{}'.", op.name(), lhs.ty), &[]);
        }
        let mut rhs = self.parse_at(&args[2], 2, Some(Type::Value))?;
        if !comparable(&rhs.ty) {
            return self.concat(Some(2), None, None).fail(format!("\"{}\" comparisons are not supported for type '{}'.", op.name(), rhs.ty), &[]);
        }
        if lhs.ty.kind() != rhs.ty.kind() && lhs.ty != Type::Value && rhs.ty != Type::Value {
            return self.fail(format!("Cannot compare types '{}' and '{}'.", lhs.ty, rhs.ty), &[]);
        }
        if op.is_order() {
            if lhs.ty == Type::Value && rhs.ty != Type::Value {
                lhs = Expr { ty: rhs.ty.clone(), node: Node::Assertion { args: vec![lhs], key: self.key.clone() } };
            } else if lhs.ty != Type::Value && rhs.ty == Type::Value {
                rhs = Expr { ty: lhs.ty.clone(), node: Node::Assertion { args: vec![rhs], key: self.key.clone() } };
            }
        }
        let mut collator = None;
        if args.len() == 4 {
            let s = |t: &Type| matches!(t, Type::String | Type::Value);
            if !s(&lhs.ty) && !s(&rhs.ty) {
                return self.fail("Cannot use collator to compare non-string types.", &[]);
            }
            collator = Some(Box::new(self.parse_at(&args[3], 3, Some(Type::Collator))?));
        }
        Some(Expr {
            ty: Type::Boolean,
            node: Node::Comparison { op, lhs: Box::new(lhs), rhs: Box::new(rhs), collator, key: self.key.clone() },
        })
    }

    fn parse_at_expr(&self, args: &[J]) -> Option<Expr> {
        if args.len() != 3 {
            return self.fail(format!("Expected 2 arguments, but found {} instead.", args.len() - 1), &[]);
        }
        let index = self.parse_at(&args[1], 1, Some(Type::Number));
        let input = self.parse_at(&args[2], 2, Some(Type::array(self.expected.clone().unwrap_or(Type::Value), None)));
        let (index, input) = (index?, input?);
        let Type::Array(item, _) = &input.ty else { return None };
        Some(Expr { ty: (**item).clone(), node: Node::At { index: Box::new(index), input: Box::new(input), key: self.key.clone() } })
    }

    fn parse_in(&self, op: &str, args: &[J]) -> Option<Expr> {
        let is_index_of = op == "index-of";
        if is_index_of {
            if args.len() <= 2 || args.len() >= 5 {
                return self.fail(format!("Expected 2 or 3 arguments, but found {} instead.", args.len() - 1), &[]);
            }
        } else if args.len() != 3 {
            return self.fail(format!("Expected 2 arguments, but found {} instead.", args.len() - 1), &[]);
        }
        let needle = self.parse_at(&args[1], 1, Some(Type::Value));
        let haystack = self.parse_at(&args[2], 2, Some(Type::Value));
        let (needle, haystack) = (needle?, haystack?);
        if !matches!(needle.ty, Type::Boolean | Type::String | Type::Number | Type::Null | Type::Value) {
            return self.fail(
                format!("Expected first argument to be of type boolean, string, number or null, but found {} instead", needle.ty),
                &[],
            );
        }
        let key = self.key.clone();
        if !is_index_of {
            return Some(Expr { ty: Type::Boolean, node: Node::In { needle: Box::new(needle), haystack: Box::new(haystack), key } });
        }
        let from = if args.len() == 4 { Some(Box::new(self.parse_at(&args[3], 3, Some(Type::Number))?)) } else { None };
        Some(Expr { ty: Type::Number, node: Node::IndexOf { needle: Box::new(needle), haystack: Box::new(haystack), from, key } })
    }

    fn parse_slice(&self, args: &[J]) -> Option<Expr> {
        if args.len() <= 2 || args.len() >= 5 {
            return self.fail(format!("Expected 2 or 3 arguments, but found {} instead.", args.len() - 1), &[]);
        }
        let input = self.parse_at(&args[1], 1, Some(Type::Value));
        let begin = self.parse_at(&args[2], 2, Some(Type::Number));
        let (input, begin) = (input?, begin?);
        if !matches!(input.ty, Type::Array(..) | Type::String | Type::Value) {
            return self.fail(format!("Expected first argument to be of type array or string, but found {} instead", input.ty), &[]);
        }
        let end = if args.len() == 4 { Some(Box::new(self.parse_at(&args[3], 3, Some(Type::Number))?)) } else { None };
        Some(Expr { ty: input.ty.clone(), node: Node::Slice { input: Box::new(input), begin: Box::new(begin), end, key: self.key.clone() } })
    }

    fn parse_length(&self, args: &[J]) -> Option<Expr> {
        if args.len() != 2 {
            return self.fail(format!("Expected 1 argument, but found {} instead.", args.len() - 1), &[]);
        }
        let input = self.parse_at(&args[1], 1, None)?;
        if !matches!(input.ty, Type::Array(..) | Type::String | Type::Value) {
            return self.fail(format!("Expected argument of type string or array, but found {} instead.", input.ty), &[]);
        }
        Some(Expr { ty: Type::Number, node: Node::Length { input: Box::new(input), key: self.key.clone() } })
    }

    fn output_type(&self) -> Option<Type> {
        self.expected.clone().filter(|t| *t != Type::Value)
    }

    fn parse_case(&self, args: &[J]) -> Option<Expr> {
        if args.len() < 4 {
            return self.fail(format!("Expected at least 3 arguments, but found only {}.", args.len() - 1), &[]);
        }
        if !args.len().is_multiple_of(2) {
            return self.fail("Expected an odd number of arguments.", &[]);
        }
        let mut out = self.output_type();
        let mut branches = Vec::new();
        let mut i = 1;
        while i < args.len() - 1 {
            let test = self.parse_at(&args[i], i, Some(Type::Boolean))?;
            let result = self.parse_at(&args[i + 1], i + 1, out.clone())?;
            out = out.or(Some(result.ty.clone()));
            branches.push((test, result));
            i += 2;
        }
        let otherwise = self.parse_at(&args[args.len() - 1], args.len() - 1, out.clone())?;
        Some(Expr { ty: out?, node: Node::Case { branches, otherwise: Box::new(otherwise) } })
    }

    fn parse_coalesce(&self, args: &[J]) -> Option<Expr> {
        if args.len() < 2 {
            return self.fail("Expected at least one argument.", &[]);
        }
        let expected = self.expected.clone();
        let mut out = self.output_type();
        let mut parsed = Vec::new();
        for (i, a) in args.iter().enumerate().skip(1) {
            let p = self.parse_with(a, i, out.clone(), None, Some(Annotation::Omit))?;
            out = out.or(Some(p.ty.clone()));
            parsed.push(p);
        }
        let out = out?;
        let needs_annotation = expected.as_ref().is_some_and(|e| parsed.iter().any(|a| check_subtype(e, &a.ty).is_some()));
        Some(Expr { ty: if needs_annotation { Type::Value } else { out }, node: Node::Coalesce(parsed) })
    }

    fn parse_let(&self, args: &[J]) -> Option<Expr> {
        if args.len() < 4 {
            return self.fail(format!("Expected at least 3 arguments, but found {} instead.", args.len() - 1), &[]);
        }
        let mut bindings = Vec::new();
        let mut i = 1;
        while i < args.len() - 1 {
            let Some(name) = args[i].as_str() else {
                return self.fail(format!("Expected string, but found {} instead.", js_typeof(&args[i])), &[i]);
            };
            if name.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '_')) {
                return self.fail("Variable names must contain only alphanumeric characters or '_'.", &[i]);
            }
            let value = self.parse_at(&args[i + 1], i + 1, None)?;
            bindings.push((name.to_string(), Rc::new(value)));
            i += 2;
        }
        let result = self.parse_with(&args[args.len() - 1], args.len() - 1, self.expected.clone(), Some(bindings.clone()), None)?;
        Some(Expr { ty: result.ty.clone(), node: Node::Let { bindings, result: Box::new(result) } })
    }

    fn parse_var(&self, args: &[J]) -> Option<Expr> {
        let (2, Some(name)) = (args.len(), args.get(1).and_then(J::as_str)) else {
            return self.fail("'var' expression requires exactly one string literal argument.", &[]);
        };
        match Scope::get(&self.scope, name) {
            Some(bound) => Some(Expr { ty: bound.ty.clone(), node: Node::Var { bound } }),
            None => self.fail(
                format!("Unknown variable \"{name}\". Make sure \"{name}\" has been bound in an enclosing \"let\" expression before using it."),
                &[1],
            ),
        }
    }

    fn parse_match(&self, args: &[J]) -> Option<Expr> {
        if args.len() < 5 {
            return self.fail(format!("Expected at least 4 arguments, but found only {}.", args.len() - 1), &[]);
        }
        if args.len() % 2 != 1 {
            return self.fail("Expected an even number of arguments.", &[]);
        }
        let mut input_type: Option<Type> = None;
        let mut out = self.output_type();
        let mut cases = HashMap::default();
        let mut outputs = Vec::new();
        let mut i = 2;
        while i < args.len() - 1 {
            let labels: Vec<&J> = match &args[i] {
                J::Array(a) => a.iter().collect(),
                l => vec![l],
            };
            let lctx = self.concat(Some(i), None, None);
            if labels.is_empty() {
                return lctx.fail("Expected at least one branch label.", &[]);
            }
            for label in labels {
                let key = match label {
                    J::Number(n) => {
                        let n = n.as_f64().unwrap();
                        if n.abs() > 9007199254740991.0 {
                            return lctx.fail("Branch labels must be integers no larger than 9007199254740991.", &[]);
                        }
                        if n.floor() != n {
                            return lctx.fail("Numeric branch labels must be integer values.", &[]);
                        }
                        super::value::js_number_to_string(n)
                    }
                    J::String(s) => s.clone(),
                    _ => return lctx.fail("Branch labels must be numbers or strings.", &[]),
                };
                let lt = Value::from_json(label).type_of();
                match &input_type {
                    None => input_type = Some(lt),
                    Some(t) => {
                        if lctx.check_subtype(t, &lt) {
                            return None;
                        }
                    }
                }
                if cases.contains_key(&key) {
                    return lctx.fail("Branch labels must be unique.", &[]);
                }
                cases.insert(key, outputs.len());
            }
            let result = self.parse_at(&args[i + 1], i, out.clone())?;
            out = out.or(Some(result.ty.clone()));
            outputs.push(result);
            i += 2;
        }
        let input = self.parse_at(&args[1], 1, Some(Type::Value))?;
        let otherwise = self.parse_at(&args[args.len() - 1], args.len() - 1, out.clone())?;
        let input_type = input_type?;
        if input.ty != Type::Value && self.concat(Some(1), None, None).check_subtype(&input_type, &input.ty) {
            return None;
        }
        Some(Expr { ty: out?, node: Node::Match { input_type, input: Box::new(input), cases, outputs, otherwise: Box::new(otherwise) } })
    }

    fn parse_step(&self, args: &[J]) -> Option<Expr> {
        if args.len() - 1 < 4 {
            return self.fail(format!("Expected at least 4 arguments, but found only {}.", args.len() - 1), &[]);
        }
        if !(args.len() - 1).is_multiple_of(2) {
            return self.fail("Expected an even number of arguments.", &[]);
        }
        let input = self.parse_at(&args[1], 1, Some(Type::Number))?;
        let mut out = self.output_type();
        let (mut labels, mut outputs) = (Vec::new(), Vec::new());
        let mut i = 1;
        while i < args.len() {
            let label = if i == 1 { Some(f64::NEG_INFINITY) } else { num(&args[i]) };
            let Some(label) = label else {
                return self.fail(
                    "Input/output pairs for \"step\" expressions must be defined using literal numeric values (not computed expressions) for the input values.",
                    &[i],
                );
            };
            if labels.last().is_some_and(|&l: &f64| l >= label) {
                return self.fail(
                    "Input/output pairs for \"step\" expressions must be arranged with input values in strictly ascending order.",
                    &[i],
                );
            }
            let parsed = self.parse_at(&args[i + 1], i + 1, out.clone())?;
            out = out.or(Some(parsed.ty.clone()));
            labels.push(label);
            outputs.push(parsed);
            i += 2;
        }
        Some(Expr { ty: out?, node: Node::Step { input: Box::new(input), labels, outputs, key: self.key.clone() } })
    }

    fn parse_interpolate(&self, op: &str, args: &[J]) -> Option<Expr> {
        let interpolation = match args.get(1) {
            Some(J::Array(i)) if !i.is_empty() => i,
            _ => return self.fail("Expected an interpolation type expression.", &[1]),
        };
        let interpolation = match interpolation[0].as_str() {
            Some("linear") => Interpolation::Linear,
            Some("exponential") => match interpolation.get(1).and_then(num) {
                Some(base) => Interpolation::Exponential(base),
                None => return self.fail("Exponential interpolation requires a numeric base.", &[1, 1]),
            },
            Some("cubic-bezier") => {
                let cp: Vec<f64> = interpolation[1..].iter().filter_map(|v| num(v).filter(|t| (0.0..=1.0).contains(t))).collect();
                if interpolation.len() != 5 || cp.len() != 4 {
                    return self.fail("Cubic bezier interpolation requires four numeric arguments with values between 0 and 1.", &[1]);
                }
                Interpolation::CubicBezier([cp[0], cp[1], cp[2], cp[3]])
            }
            _ => {
                let name = match &interpolation[0] {
                    J::String(s) => s.clone(),
                    J::Null => "null".into(),
                    v => super::value::Value::from_json(v).to_js_string(),
                };
                return self.fail(format!("Unknown interpolation type {name}"), &[1, 0]);
            }
        };
        if args.len() - 1 < 4 {
            return self.fail(format!("Expected at least 4 arguments, but found only {}.", args.len() - 1), &[]);
        }
        if !(args.len() - 1).is_multiple_of(2) {
            return self.fail("Expected an even number of arguments.", &[]);
        }
        let input = self.parse_at(&args[2], 2, Some(Type::Number))?;
        let space = match op {
            "interpolate-hcl" => ColorSpace::Hcl,
            "interpolate-lab" => ColorSpace::Lab,
            _ => ColorSpace::Rgb,
        };
        let mut out = if space != ColorSpace::Rgb && self.expected != Some(Type::ColorArray) { Some(Type::Color) } else { self.output_type() };
        let (mut labels, mut outputs) = (Vec::new(), Vec::new());
        let mut i = 3;
        while i < args.len() {
            let Some(label) = num(&args[i]) else {
                return self.fail(
                    "Input/output pairs for \"interpolate\" expressions must be defined using literal numeric values (not computed expressions) for the input values.",
                    &[i],
                );
            };
            if labels.last().is_some_and(|&l: &f64| l >= label) {
                return self.fail(
                    "Input/output pairs for \"interpolate\" expressions must be arranged with input values in strictly ascending order.",
                    &[i],
                );
            }
            let parsed = self.parse_at(&args[i + 1], i + 1, out.clone())?;
            out = out.or(Some(parsed.ty.clone()));
            labels.push(label);
            outputs.push(parsed);
            i += 2;
        }
        let out = out?;
        let ok = match &out {
            Type::Number | Type::ProjectionDefinition | Type::Color | Type::Padding | Type::NumberArray | Type::ColorArray | Type::VariableAnchorOffsetCollection => true,
            Type::Array(item, Some(_)) => **item == Type::Number,
            _ => false,
        };
        if !ok {
            return self.fail(format!("Type {out} is not interpolatable."), &[]);
        }
        Some(Expr { ty: out, node: Node::Interpolate { space, interpolation, input: Box::new(input), labels, outputs, key: self.key.clone() } })
    }

    fn parse_collator(&self, args: &[J]) -> Option<Expr> {
        if args.len() != 2 {
            return self.fail("Expected one argument.", &[]);
        }
        let J::Object(o) = &args[1] else { return self.fail("Collator options argument must be an object.", &[]) };
        let f = J::Bool(false);
        let case_sensitive = self.parse_at(o.get("case-sensitive").unwrap_or(&f), 1, Some(Type::Boolean))?;
        let diacritic_sensitive = self.parse_at(o.get("diacritic-sensitive").unwrap_or(&f), 1, Some(Type::Boolean))?;
        let locale = match o.get("locale").filter(|l| js_truthy(l)) {
            Some(l) => Some(Box::new(self.parse_at(l, 1, Some(Type::String))?)),
            None => None,
        };
        Some(Expr {
            ty: Type::Collator,
            node: Node::Collator { case_sensitive: Box::new(case_sensitive), diacritic_sensitive: Box::new(diacritic_sensitive), locale },
        })
    }

    fn parse_number_format(&self, args: &[J]) -> Option<Expr> {
        if args.len() != 3 {
            return self.fail("Expected two arguments.", &[]);
        }
        let number = self.parse_at(&args[1], 1, Some(Type::Number))?;
        let J::Object(o) = &args[2] else { return self.fail("NumberFormat options argument must be an object.", &[]) };
        let opt = |k: &str, t: Type| -> Result<Option<Box<Expr>>, ()> {
            match o.get(k).filter(|v| js_truthy(v)) {
                Some(v) => self.parse_at(v, 1, Some(t)).map(|e| Some(Box::new(e))).ok_or(()),
                None => Ok(None),
            }
        };
        let locale = opt("locale", Type::String).ok()?;
        let currency = opt("currency", Type::String).ok()?;
        let unit = opt("unit", Type::String).ok()?;
        if currency.is_some() && unit.is_some() {
            return self.fail("NumberFormat options `currency` and `unit` are mutually exclusive", &[]);
        }
        let min = opt("min-fraction-digits", Type::Number).ok()?;
        let max = opt("max-fraction-digits", Type::Number).ok()?;
        Some(Expr { ty: Type::String, node: Node::NumberFormat { number: Box::new(number), locale, currency, unit, min, max } })
    }

    fn parse_format(&self, args: &[J]) -> Option<Expr> {
        if args.len() < 2 {
            return self.fail("Expected at least one argument.", &[]);
        }
        if matches!(args[1], J::Object(_)) {
            return self.fail("First argument must be an image or text section.", &[]);
        }
        let mut sections: Vec<FormatSection> = Vec::new();
        let mut next_may_be_object = false;
        for (i, arg) in args.iter().enumerate().skip(1) {
            if let (true, J::Object(o)) = (next_may_be_object, arg) {
                next_may_be_object = false;
                let opt = |k: &str, t: Type| -> Result<Option<Expr>, ()> {
                    match o.get(k).filter(|v| js_truthy(v)) {
                        Some(v) => self.parse_at(v, 1, Some(t)).map(Some).ok_or(()),
                        None => Ok(None),
                    }
                };
                let scale = opt("font-scale", Type::Number).ok()?;
                let font = opt("text-font", Type::array(Type::String, None)).ok()?;
                let text_color = opt("text-color", Type::Color).ok()?;
                if let Some(J::String(va)) = o.get("vertical-align")
                    && !["bottom", "center", "top"].contains(&va.as_str())
                {
                    return self.fail(format!("'vertical-align' must be one of: 'bottom', 'center', 'top' but found '{va}' instead."), &[]);
                }
                let vertical_align = opt("vertical-align", Type::String).ok()?;
                let last = sections.last_mut().unwrap();
                last.scale = scale;
                last.font = font;
                last.text_color = text_color;
                last.vertical_align = vertical_align;
            } else {
                let _ = i;
                let content = self.parse_at(arg, 1, Some(Type::Value))?;
                if !matches!(content.ty, Type::String | Type::Value | Type::Null | Type::ResolvedImage) {
                    return self.fail("Formatted text type must be 'string', 'value', 'image' or 'null'.", &[]);
                }
                next_may_be_object = true;
                sections.push(FormatSection { content, scale: None, font: None, text_color: None, vertical_align: None });
            }
        }
        Some(Expr { ty: Type::Formatted, node: Node::Format(sections) })
    }

    fn parse_image(&self, args: &[J]) -> Option<Expr> {
        if args.len() != 2 {
            return self.fail("Expected two arguments.", &[]);
        }
        match self.parse_at(&args[1], 1, Some(Type::String)) {
            Some(name) => Some(Expr { ty: Type::ResolvedImage, node: Node::Image(Box::new(name)) }),
            None => self.fail("No image name provided.", &[]),
        }
    }

    fn parse_global_state(&self, args: &[J]) -> Option<Expr> {
        if args.len() != 2 {
            return self.fail(format!("Expected 1 argument, but found {} instead.", args.len() - 1), &[]);
        }
        match &args[1] {
            J::Null => self.fail("Global state property must be defined.", &[]),
            J::String(s) => Some(Expr { ty: Type::Value, node: Node::GlobalState(s.clone()) }),
            v => self.fail(format!("Global state property must be string, but found {} instead.", js_typeof(v)), &[]),
        }
    }

    fn parse_within(&self, args: &[J]) -> Option<Expr> {
        if args.len() != 2 {
            return self.fail(format!("'within' expression requires exactly one argument, but found {} instead.", args.len() - 1), &[]);
        }
        match geometry::within_polygons(&args[1]) {
            Some(p) => Some(Expr { ty: Type::Boolean, node: Node::Within(p) }),
            None => self.fail("'within' expression requires valid geojson object that contains polygon geometry type.", &[]),
        }
    }

    fn parse_distance(&self, args: &[J]) -> Option<Expr> {
        if args.len() != 2 {
            return self.fail(format!("'distance' expression requires exactly one argument, but found {} instead.", args.len() - 1), &[]);
        }
        match geometry::distance_geometries(&args[1]) {
            Some(g) => Some(Expr { ty: Type::Number, node: Node::Distance(g) }),
            None => self.fail("'distance' expression requires valid geojson object that contains polygon geometry type.", &[]),
        }
    }

    fn parse_compound(&self, op: &str, args: &[J]) -> Option<Expr> {
        let def = compound::definition(op)?;
        let available = &def.overloads;
        let overloads: Vec<&(Sig, CompoundFn)> = available
            .iter()
            .filter(|(sig, _)| match sig {
                Sig::Varargs(_) => true,
                Sig::Fixed(p) => p.len() == args.len() - 1,
            })
            .collect();
        let mut sig_ctx: Option<ParseCtx> = None;
        for (sig, f) in &overloads {
            let ctx = self.fresh();
            sig_ctx = Some(ctx.clone());
            let mut parsed = Vec::new();
            let mut failed = false;
            for (i, a) in args.iter().enumerate().skip(1) {
                let expected = match sig {
                    Sig::Fixed(p) => p[i - 1].clone(),
                    Sig::Varargs(t) => t.clone(),
                };
                match ctx.parse_at(a, 1 + parsed.len(), Some(expected)) {
                    Some(p) => parsed.push(p),
                    None => {
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                continue;
            }
            for (i, a) in parsed.iter().enumerate() {
                let expected = match sig {
                    Sig::Fixed(p) => &p[i],
                    Sig::Varargs(t) => t,
                };
                ctx.concat(Some(i + 1), None, None).check_subtype(expected, &a.ty);
            }
            if ctx.errors.borrow().is_empty() {
                return Some(Expr { ty: def.ty.clone(), node: Node::Compound { name: op.to_string(), f: *f, args: parsed, key: self.key.clone() } });
            }
        }
        if overloads.len() == 1 {
            let errs = sig_ctx.unwrap().errors.borrow().clone();
            self.errors.borrow_mut().extend(errs);
        } else {
            let shown: Vec<&Sig> = if overloads.is_empty() { available.iter().map(|(s, _)| s).collect() } else { overloads.iter().map(|(s, _)| s).collect() };
            let signatures = shown.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(" | ");
            let mut actual = Vec::new();
            for (i, a) in args.iter().enumerate().skip(1) {
                let _ = i;
                let p = self.parse_at(a, 1 + actual.len(), None)?;
                actual.push(p.ty.to_string());
            }
            self.error(format!("Expected arguments of type {signatures}, but found ({}) instead.", actual.join(", ")), &[]);
        }
        None
    }
}

fn js_truthy(v: &J) -> bool {
    match v {
        J::Null => false,
        J::Bool(b) => *b,
        J::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        J::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn annotate(parsed: Expr, ty: &Type, ann: Annotation, key: &str) -> Expr {
    match ann {
        Annotation::Assert => Expr { ty: ty.clone(), node: Node::Assertion { args: vec![parsed], key: key.into() } },
        Annotation::Coerce => Expr { ty: ty.clone(), node: Node::Coercion { args: vec![parsed], key: key.into() } },
        Annotation::Omit => parsed,
    }
}

pub fn is_constant(e: &Expr) -> bool {
    match &e.node {
        Node::Var { bound, .. } => return is_constant(bound),
        Node::Compound { name, .. } if name == "error" => return false,
        Node::Collator { .. } | Node::Within(_) | Node::Distance(_) | Node::GlobalState(_) => return false,
        _ => {}
    }
    let annotation = matches!(e.node, Node::Coercion { .. } | Node::Assertion { .. });
    let mut children = true;
    e.each_child(&mut |c| {
        children = children && if annotation { is_constant(c) } else { matches!(c.node, Node::Literal(_)) };
    });
    children
        && is_feature_constant(e)
        && is_global_property_constant(e, &["zoom", "heatmap-density", "elevation", "line-progress", "accumulated", "is-supported-script"])
}

pub fn is_feature_constant(e: &Expr) -> bool {
    match &e.node {
        Node::Compound { name, args, .. } => {
            if (name == "get" || name == "has") && args.len() == 1 {
                return false;
            }
            if matches!(name.as_str(), "feature-state" | "properties" | "geometry-type" | "id") || name.starts_with("filter-") {
                return false;
            }
        }
        Node::Within(_) | Node::Distance(_) => return false,
        _ => {}
    }
    let mut r = true;
    e.each_child(&mut |c| r = r && is_feature_constant(c));
    r
}

pub fn is_state_constant(e: &Expr) -> bool {
    if e.compound_name() == Some("feature-state") {
        return false;
    }
    let mut r = true;
    e.each_child(&mut |c| r = r && is_state_constant(c));
    r
}

pub fn is_global_property_constant(e: &Expr, props: &[&str]) -> bool {
    if e.compound_name().is_some_and(|n| props.contains(&n)) {
        return false;
    }
    let mut r = true;
    e.each_child(&mut |c| r = r && is_global_property_constant(c, props));
    r
}
