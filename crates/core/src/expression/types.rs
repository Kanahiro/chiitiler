//! Expression type system (style-spec `expression/types.ts`).

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Null,
    Number,
    String,
    Boolean,
    Color,
    ProjectionDefinition,
    Object,
    Value,
    Array(Box<Type>, Option<usize>),
    Error,
    Collator,
    Formatted,
    Padding,
    NumberArray,
    ColorArray,
    ResolvedImage,
    VariableAnchorOffsetCollection,
}

impl Type {
    pub fn array(item: Type, n: Option<usize>) -> Type {
        Type::Array(Box::new(item), n)
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Type::Null => "null",
            Type::Number => "number",
            Type::String => "string",
            Type::Boolean => "boolean",
            Type::Color => "color",
            Type::ProjectionDefinition => "projectionDefinition",
            Type::Object => "object",
            Type::Value => "value",
            Type::Array(..) => "array",
            Type::Error => "error",
            Type::Collator => "collator",
            Type::Formatted => "formatted",
            Type::Padding => "padding",
            Type::NumberArray => "numberArray",
            Type::ColorArray => "colorArray",
            Type::ResolvedImage => "resolvedImage",
            Type::VariableAnchorOffsetCollection => "variableAnchorOffsetCollection",
        }
    }

    pub fn same_kind(&self, other: &Type) -> bool {
        self.kind() == other.kind()
    }

    /// Upstream compares type objects by identity in a few places; non-array types are
    /// singletons there while array types are fresh objects.
    pub fn identical(&self, other: &Type) -> bool {
        !matches!(self, Type::Array(..)) && self == other
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Array(item, Some(n)) => write!(f, "array<{item}, {n}>"),
            Type::Array(item, None) if **item == Type::Value => write!(f, "array"),
            Type::Array(item, None) => write!(f, "array<{item}>"),
            t => f.write_str(t.kind()),
        }
    }
}

const VALUE_MEMBERS: [Type; 13] = [
    Type::Null,
    Type::Number,
    Type::String,
    Type::Boolean,
    Type::Color,
    Type::ProjectionDefinition,
    Type::Formatted,
    Type::Object,
    Type::Padding,
    Type::NumberArray,
    Type::ColorArray,
    Type::ResolvedImage,
    Type::VariableAnchorOffsetCollection,
];

/// `None` if `t` is a subtype of `expected`, otherwise the error message.
pub fn check_subtype(expected: &Type, t: &Type) -> Option<String> {
    let ok = match (expected, t) {
        (_, Type::Error) => true,
        (Type::Array(eitem, en), Type::Array(titem, tn)) => {
            ((*tn == Some(0) && **titem == Type::Value) || check_subtype(eitem, titem).is_none())
                && (en.is_none() || en == tn)
        }
        (Type::Array(..), _) => false,
        (e, t) if e.same_kind(t) => true,
        (Type::Value, t) => {
            VALUE_MEMBERS.iter().any(|m| check_subtype(m, t).is_none())
                || check_subtype(&Type::array(Type::Value, None), t).is_none()
        }
        _ => false,
    };
    if ok { None } else { Some(format!("Expected {expected} but found {t} instead.")) }
}
