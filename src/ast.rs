//! Language-neutral expression AST.
//!
//! This module defines the intermediate representation that pieces compile
//! into. Target-specific backends (see [`crate::backend`]) convert `Expr`
//! trees into output strings for JavaScript, Lua, or other scripting
//! languages.
//!
//! # Canonical AST shapes
//!
//! - **`MethodCall`** carries real method semantics; backends may render it
//!   with `.` (JS) or `:` (Lua) syntax.
//! - **`Call(Field(…), args)`** means "access the property, then call the
//!   result" — no method semantics implied.
//! - **`Field`** is pure access only, never invocation.
//!
//! # Origin propagation
//!
//! - Compiler-created root nodes: `origin = Some(Origin { node, param: None })`
//! - Inline / default param values: `origin = Some(Origin { node, param: Some(id) })`
//! - Child expressions retain their own origins.
//! - Transform passes should preserve origins.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::GridPos;

// ---------------------------------------------------------------------------
// Source origin tracking
// ---------------------------------------------------------------------------

/// Tracks which graph element produced an AST node.
///
/// Tessera's "source" is a graph, not a text file, so positions are grid
/// cells and parameter ids rather than byte offsets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// The graph node position that produced this expression.
    pub node: GridPos,
    /// The specific parameter, if the expression came from a parameter value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
}

// ---------------------------------------------------------------------------
// String syntax hint
// ---------------------------------------------------------------------------

/// Rendering hint for string literals.
///
/// The backend decides how each variant is rendered — the AST expresses
/// intent, not quoting style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StringSyntax {
    /// Backend's normal string quoting (e.g. single-quoted in JS).
    #[default]
    Default,
    /// Domain-specific pattern syntax (e.g. double-quoted mini-notation in JS).
    Pattern,
}

// ---------------------------------------------------------------------------
// Literal values
// ---------------------------------------------------------------------------

/// Language-neutral literal value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Lit {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str { value: String, syntax: StringSyntax },
}

// ---------------------------------------------------------------------------
// Operators
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical
    And,
    Or,
    // String
    Concat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaryOp {
    Neg,
    Not,
}

// ---------------------------------------------------------------------------
// The expression AST
// ---------------------------------------------------------------------------

/// A spanned expression node: the expression itself plus optional origin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expr {
    pub kind: ExprKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
}

/// The core expression variants.
///
/// This is the intermediate representation that pieces compile into.
/// Target-specific backends convert `ExprKind` trees into output strings.
///
/// # Error node policy
///
/// `Error` nodes are allowed in **preview** compilation output (rendered as
/// comments or sentinels). **Runtime** compilation must reject any program
/// whose final terminals still contain `Error` nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "expr", rename_all = "snake_case")]
pub enum ExprKind {
    /// A literal value: number, string, bool, or nil.
    Lit { value: Lit },

    /// A variable or identifier reference.
    Ident { name: String },

    /// An expression-level conditional: `cond ? then_expr : else_expr`.
    Conditional {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },

    /// A function call: `func(args...)`.
    ///
    /// `func` is an expression to support higher-order patterns such as
    /// `Call(Field(obj, "prop"), args)` — "access the property and invoke
    /// the result".
    Call { func: Box<Expr>, args: Vec<Expr> },

    /// A method call with receiver semantics: `receiver.method(args...)`.
    ///
    /// Backends may render this with `.` (JS) or `:` (Lua) syntax.
    /// This is semantically distinct from `Call(Field(…), args)` which
    /// carries no method semantics.
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },

    /// Property / member access: `object.field`.
    ///
    /// Pure access only — never invocation.
    Field { object: Box<Expr>, field: String },

    /// Index access: `object[index]`.
    Index { object: Box<Expr>, index: Box<Expr> },

    /// Binary operation: `lhs op rhs`.
    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },

    /// Unary operation: `op operand`.
    UnaryOp { op: UnaryOp, operand: Box<Expr> },

    /// Lambda / closure: `|params| body`.
    ///
    /// Renders as `(a, b) => body` in JS, `function(a, b) return body end`
    /// in Lua, etc.
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
    },

    /// Array / list literal: `[a, b, c]`.
    Array { elements: Vec<Expr> },

    /// String-keyed record literal: `{k1: v1, k2: v2}`.
    ///
    /// If arbitrary-key maps are needed later, add a separate `Map` variant
    /// with `Vec<(Expr, Expr)>` entries.
    ///
    /// `Record` intentionally preserves insertion order and allows duplicate
    /// keys. Converting through JSON will normalize duplicates because JSON
    /// object parsing is map-based.
    Record { entries: Vec<(String, Expr)> },

    /// An error placeholder that replaces a missing or invalid expression.
    ///
    /// Allowed in preview output; runtime compilation must reject programs
    /// that still contain `Error` nodes in their final terminals.
    Error { message: String },
}

// ---------------------------------------------------------------------------
// Identifier validation
// ---------------------------------------------------------------------------

/// Check whether `s` matches the v1 bare-identifier grammar:
/// `[a-zA-Z_][a-zA-Z0-9_]*`.
pub fn is_valid_ident_segment(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Check whether `s` is a valid dotted identifier path where every segment
/// matches the v1 bare-identifier grammar.
pub fn is_valid_ident_path(s: &str) -> bool {
    let mut segments = s.split('.');
    let Some(first) = segments.next() else {
        return false;
    };
    if !is_valid_ident_segment(first) {
        return false;
    }
    segments.all(is_valid_ident_segment)
}

/// Parse a dotted identifier path into a structural AST expression.
pub fn parse_ident_path(s: &str) -> Option<Expr> {
    let mut segments = s.split('.');
    let first = segments.next()?;
    if !is_valid_ident_segment(first) {
        return None;
    }

    let mut expr = Expr::ident(first);
    for segment in segments {
        if !is_valid_ident_segment(segment) {
            return None;
        }
        expr = Expr::field(expr, segment);
    }

    Some(expr)
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

impl Expr {
    /// Create an expression with no origin tracking.
    pub fn new(kind: ExprKind) -> Self {
        Self { kind, origin: None }
    }

    /// Attach an origin to this expression.
    pub fn with_origin(mut self, origin: Origin) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Attach an origin only when this node does not already have one.
    pub fn with_origin_if_missing(mut self, origin: Origin) -> Self {
        if self.origin.is_none() {
            self.origin = Some(origin);
        }
        self
    }

    // -- Convenience constructors --

    pub fn nil() -> Self {
        Self::new(ExprKind::Lit { value: Lit::Nil })
    }

    pub fn bool_lit(v: bool) -> Self {
        Self::new(ExprKind::Lit {
            value: Lit::Bool(v),
        })
    }

    pub fn int(v: i64) -> Self {
        Self::new(ExprKind::Lit { value: Lit::Int(v) })
    }

    pub fn float(v: f64) -> Self {
        assert!(v.is_finite(), "AST floats must be finite, got {v}");
        Self::new(ExprKind::Lit {
            value: Lit::Float(v),
        })
    }

    pub fn str_lit(v: impl Into<String>) -> Self {
        Self::new(ExprKind::Lit {
            value: Lit::Str {
                value: v.into(),
                syntax: StringSyntax::Default,
            },
        })
    }

    pub fn pattern(v: impl Into<String>) -> Self {
        Self::new(ExprKind::Lit {
            value: Lit::Str {
                value: v.into(),
                syntax: StringSyntax::Pattern,
            },
        })
    }

    pub fn ident(name: impl Into<String>) -> Self {
        Self::new(ExprKind::Ident { name: name.into() })
    }

    pub fn conditional(cond: Expr, then_expr: Expr, else_expr: Expr) -> Self {
        Self::new(ExprKind::Conditional {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
        })
    }

    pub fn call(func: Expr, args: Vec<Expr>) -> Self {
        Self::new(ExprKind::Call {
            func: Box::new(func),
            args,
        })
    }

    /// Shorthand: call a function by name.
    pub fn call_named(name: impl Into<String>, args: Vec<Expr>) -> Self {
        Self::call(Self::ident(name), args)
    }

    pub fn method_call(receiver: Expr, method: impl Into<String>, args: Vec<Expr>) -> Self {
        Self::new(ExprKind::MethodCall {
            receiver: Box::new(receiver),
            method: method.into(),
            args,
        })
    }

    pub fn field(object: Expr, field: impl Into<String>) -> Self {
        Self::new(ExprKind::Field {
            object: Box::new(object),
            field: field.into(),
        })
    }

    pub fn index(object: Expr, index: Expr) -> Self {
        Self::new(ExprKind::Index {
            object: Box::new(object),
            index: Box::new(index),
        })
    }

    pub fn bin_op(op: BinOp, lhs: Expr, rhs: Expr) -> Self {
        Self::new(ExprKind::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }

    pub fn unary_op(op: UnaryOp, operand: Expr) -> Self {
        Self::new(ExprKind::UnaryOp {
            op,
            operand: Box::new(operand),
        })
    }

    pub fn lambda(params: Vec<String>, body: Expr) -> Self {
        Self::new(ExprKind::Lambda {
            params,
            body: Box::new(body),
        })
    }

    pub fn array(elements: Vec<Expr>) -> Self {
        Self::new(ExprKind::Array { elements })
    }

    pub fn record(entries: Vec<(String, Expr)>) -> Self {
        Self::new(ExprKind::Record { entries })
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::new(ExprKind::Error {
            message: message.into(),
        })
    }

    /// Convert a JSON value into an AST expression.
    ///
    /// JSON objects become `Record` values in insertion order as exposed by
    /// `serde_json`. Duplicate keys cannot survive this conversion because
    /// JSON object parsing is map-based.
    pub fn from_json_value(value: &Value) -> Self {
        match value {
            Value::Null => Expr::nil(),
            Value::Bool(b) => Expr::bool_lit(*b),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Expr::int(i)
                } else if let Some(u) = n.as_u64() {
                    // u64 > i64::MAX: convert to float with potential precision loss
                    Expr::float(u as f64)
                } else {
                    Expr::float(n.as_f64().unwrap_or(0.0))
                }
            }
            Value::String(s) => Expr::str_lit(s.as_str()),
            Value::Array(arr) => Expr::array(arr.iter().map(Expr::from_json_value).collect()),
            Value::Object(obj) => Expr::record(
                obj.iter()
                    .map(|(k, v)| (k.clone(), Expr::from_json_value(v)))
                    .collect(),
            ),
        }
    }

    /// Returns `true` if this expression or any sub-expression is an `Error` node.
    pub fn contains_error(&self) -> bool {
        self.first_error().is_some()
    }

    /// Return the first nested `Error` node in a depth-first walk.
    pub fn first_error(&self) -> Option<&Expr> {
        match &self.kind {
            ExprKind::Error { .. } => Some(self),
            ExprKind::Lit { .. } | ExprKind::Ident { .. } => None,
            ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => cond
                .first_error()
                .or_else(|| then_expr.first_error())
                .or_else(|| else_expr.first_error()),
            ExprKind::Call { func, args } => func
                .first_error()
                .or_else(|| args.iter().find_map(Expr::first_error)),
            ExprKind::MethodCall { receiver, args, .. } => receiver
                .first_error()
                .or_else(|| args.iter().find_map(Expr::first_error)),
            ExprKind::Field { object, .. } => object.first_error(),
            ExprKind::Index { object, index } => {
                object.first_error().or_else(|| index.first_error())
            }
            ExprKind::BinOp { lhs, rhs, .. } => lhs.first_error().or_else(|| rhs.first_error()),
            ExprKind::UnaryOp { operand, .. } => operand.first_error(),
            ExprKind::Lambda { body, .. } => body.first_error(),
            ExprKind::Array { elements } => elements.iter().find_map(Expr::first_error),
            ExprKind::Record { entries } => entries.iter().find_map(|(_, v)| v.first_error()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn is_valid_ident_segment_accepts_simple() {
        assert!(is_valid_ident_segment("foo"));
        assert!(is_valid_ident_segment("_bar"));
        assert!(is_valid_ident_segment("Baz_123"));
    }

    #[test]
    fn is_valid_ident_segment_rejects_invalid() {
        assert!(!is_valid_ident_segment(""));
        assert!(!is_valid_ident_segment("123abc"));
        assert!(!is_valid_ident_segment("foo bar"));
        assert!(!is_valid_ident_segment(".foo"));
        assert!(!is_valid_ident_segment("a.b"));
    }

    #[test]
    fn is_valid_ident_path_accepts_dotted_paths() {
        assert!(is_valid_ident_path("foo"));
        assert!(is_valid_ident_path("foo.bar"));
        assert!(is_valid_ident_path("_a.b2.c3"));
    }

    #[test]
    fn is_valid_ident_path_rejects_invalid_segments() {
        assert!(!is_valid_ident_path(""));
        assert!(!is_valid_ident_path(".foo"));
        assert!(!is_valid_ident_path("foo."));
        assert!(!is_valid_ident_path("foo..bar"));
        assert!(!is_valid_ident_path("foo.123"));
    }

    #[test]
    fn parse_ident_path_builds_field_chain() {
        let expr = parse_ident_path("foo.bar.baz").expect("path expr");
        assert_eq!(
            expr,
            Expr::field(Expr::field(Expr::ident("foo"), "bar"), "baz")
        );
    }

    #[test]
    fn parse_ident_path_rejects_invalid_input() {
        assert!(parse_ident_path("foo..bar").is_none());
    }

    #[test]
    fn conditional_reports_nested_errors() {
        let expr = Expr::conditional(Expr::bool_lit(true), Expr::error("missing"), Expr::int(0));
        assert!(expr.contains_error());
        assert!(matches!(
            expr.first_error().map(|expr| &expr.kind),
            Some(ExprKind::Error { .. })
        ));
    }

    #[test]
    fn from_json_value_basic() {
        assert_eq!(Expr::from_json_value(&json!(null)), Expr::nil());
        assert_eq!(Expr::from_json_value(&json!(true)), Expr::bool_lit(true));
        assert_eq!(Expr::from_json_value(&json!(42)), Expr::int(42));
        assert_eq!(Expr::from_json_value(&json!(3.14)), Expr::float(3.14));
        assert_eq!(
            Expr::from_json_value(&json!("hello")),
            Expr::str_lit("hello")
        );
    }

    #[test]
    fn from_json_value_array() {
        let expr = Expr::from_json_value(&json!([1, "a"]));
        assert_eq!(expr, Expr::array(vec![Expr::int(1), Expr::str_lit("a")]));
    }

    #[test]
    fn from_json_value_object() {
        let expr = Expr::from_json_value(&json!({"x": 1}));
        assert_eq!(expr, Expr::record(vec![("x".into(), Expr::int(1))]));
    }

    #[test]
    fn contains_error_detects_nested() {
        let expr = Expr::call_named("foo", vec![Expr::error("missing")]);
        assert!(expr.contains_error());
    }

    #[test]
    fn contains_error_false_for_clean() {
        let expr = Expr::call_named("foo", vec![Expr::int(1)]);
        assert!(!expr.contains_error());
    }

    #[test]
    #[should_panic(expected = "AST floats must be finite")]
    fn float_rejects_non_finite() {
        let _ = Expr::float(f64::NAN);
    }

    #[test]
    fn lit_serde_round_trips_primitives() {
        let bool_json = serde_json::to_string(&Lit::Bool(true)).expect("serialize bool");
        assert_eq!(bool_json, r#"{"kind":"bool","value":true}"#);
        let bool_lit: Lit = serde_json::from_str(&bool_json).expect("deserialize bool");
        assert_eq!(bool_lit, Lit::Bool(true));

        let int_json = serde_json::to_string(&Lit::Int(42)).expect("serialize int");
        assert_eq!(int_json, r#"{"kind":"int","value":42}"#);
        let int_lit: Lit = serde_json::from_str(&int_json).expect("deserialize int");
        assert_eq!(int_lit, Lit::Int(42));
    }
}
