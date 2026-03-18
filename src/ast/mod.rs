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

use crate::types::{DomainBridgeKind, GridPos};

// ---------------------------------------------------------------------------
// Optimization level
// ---------------------------------------------------------------------------

/// Controls which optimization passes run on compiled expressions.
///
/// Higher levels include all passes from lower levels plus additional
/// transformations.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum OptLevel {
    /// No optimization — emit the AST exactly as pieces produce it.
    None,
    /// Constant folding + algebraic simplification (identity / absorbing
    /// elements, double negation).
    #[default]
    Basic,
    /// Basic + conditional simplification + common sub-expression elimination.
    Full,
}

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

    /// A block of let-bindings followed by a result expression.
    ///
    /// Produced by the CSE (common sub-expression elimination) pass.
    /// Backends render this as a sequence of local variable declarations
    /// followed by the result value.
    ///
    /// ```text
    /// // JS:   (() => { let _t0 = <value>; return <result>; })()
    /// // Lua:  (function() local _t0 = <value>; return <result> end)()
    /// ```
    Block {
        bindings: Vec<(String, Expr)>,
        result: Box<Expr>,
    },

    /// A reference to a host-managed delay buffer (previous frame's value).
    ///
    /// Produced by the `core.delay` piece. The `slot` identifies which
    /// frame buffer the host should read from; `default` provides the
    /// initial value for frame 0 before any history exists.
    ///
    /// ```text
    /// // JS:   __delay('d_3_2', <default>)
    /// // Lua:  __delay('d_3_2', <default>)
    /// ```
    DelayRef { slot: String, default: Box<Expr> },

    /// An implicit execution-domain bridge inserted by the compiler.
    ///
    /// Backends lower this to a host-provided runtime helper.
    DomainConvert {
        kind: DomainBridgeKind,
        input: Box<Expr>,
    },

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

    pub fn block(bindings: Vec<(String, Expr)>, result: Expr) -> Self {
        Self::new(ExprKind::Block {
            bindings,
            result: Box::new(result),
        })
    }

    pub fn delay_ref(slot: impl Into<String>, default: Expr) -> Self {
        Self::new(ExprKind::DelayRef {
            slot: slot.into(),
            default: Box::new(default),
        })
    }

    pub fn domain_convert(kind: DomainBridgeKind, expr: Expr) -> Self {
        Self::new(ExprKind::DomainConvert {
            kind,
            input: Box::new(expr),
        })
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
                    // u64 that didn't fit in i64 — precision may be lost in f64
                    // representation. All current scripting targets (JS, Lua, Python)
                    // use f64 for JSON numbers anyway, so this is semantically correct
                    // even if not bit-exact.
                    #[cfg(debug_assertions)]
                    if u as f64 as u64 != u {
                        eprintln!(
                            "tessera: precision loss converting {u} to f64 in from_json_value"
                        );
                    }
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

    // -- Optimization passes ------------------------------------------------

    /// Run optimization passes at the given level.
    ///
    /// - [`OptLevel::None`] — return a clone with no transformations.
    /// - [`OptLevel::Basic`] — constant folding → algebraic simplification.
    /// - [`OptLevel::Full`] — Basic + conditional simplification.
    ///
    /// CSE (common sub-expression elimination) is a *program-level* pass that
    /// operates on terminal expression lists; see
    /// [`Expr::hoist_common_subexprs`].
    pub fn optimize_at(&self, level: OptLevel) -> Expr {
        match level {
            OptLevel::None => self.clone(),
            OptLevel::Basic => self.fold_constants().eliminate_dead_code(),
            OptLevel::Full => self
                .fold_constants()
                .eliminate_dead_code()
                .simplify_conditionals(),
        }
    }

    /// Run all canonical optimization passes in sequence
    /// (equivalent to [`OptLevel::Basic`]).
    pub fn optimize(&self) -> Expr {
        self.optimize_at(OptLevel::Basic)
    }

    /// Constant-fold literal sub-expressions bottom-up.
    ///
    /// Evaluates arithmetic, comparison, logical, string-concat, conditional,
    /// and unary operations when all operands are known literals.
    pub fn fold_constants(&self) -> Expr {
        opt::fold_expr(self)
    }

    /// Simplify by removing algebraic identity / absorbing elements and
    /// collapsing double negation.
    ///
    /// **Identity elements** — return the other operand:
    /// `x+0`, `0+x`, `x-0`, `x*1`, `1*x`, `x/1`, `x++""`, `""++x`,
    /// `x&&true`, `true&&x`, `x||false`, `false||x`.
    ///
    /// **Absorbing elements** — short-circuit to a constant:
    /// `x*0`, `0*x` → `0`; `x&&false` → `false`; `x||true` → `true`.
    ///
    /// **Double negation**: `!!x` → `x`, `--x` → `x`.
    pub fn eliminate_dead_code(&self) -> Expr {
        opt::dce_expr(self)
    }

    /// Simplify conditional expressions.
    ///
    /// - `cond ? x : x` → `x` (identical branches).
    /// - Nested conditionals with matching branches are collapsed.
    pub fn simplify_conditionals(&self) -> Expr {
        opt::simplify_conditionals_expr(self)
    }

    /// Hoist common sub-expressions shared across a set of terminal
    /// expressions into `Block` let-bindings.
    ///
    /// Returns a new list of expressions where duplicated sub-trees are
    /// replaced by identifier references, with each terminal wrapped in a
    /// `Block` that binds the shared values.
    ///
    /// This is a **program-level** pass — call it on the final terminal
    /// expression list, not on individual per-node expressions.
    pub fn hoist_common_subexprs(terminals: &[Expr]) -> Vec<Expr> {
        opt::hoist_common_subexprs(terminals)
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
            ExprKind::Block { bindings, result } => bindings
                .iter()
                .find_map(|(_, v)| v.first_error())
                .or_else(|| result.first_error()),
            ExprKind::DelayRef { default, .. } => default.first_error(),
            ExprKind::DomainConvert { input, .. } => input.first_error(),
        }
    }
}

mod opt;

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
        assert_eq!(Expr::from_json_value(&json!(2.5)), Expr::float(2.5));
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

    // -- fold_constants tests -----------------------------------------------

    fn test_origin() -> Origin {
        Origin {
            node: GridPos { col: 3, row: 2 },
            param: Some("value".into()),
        }
    }

    #[test]
    fn folds_arithmetic_literals() {
        assert_eq!(
            Expr::bin_op(BinOp::Add, Expr::int(1), Expr::int(2)).fold_constants(),
            Expr::int(3)
        );
        assert_eq!(
            Expr::bin_op(BinOp::Add, Expr::float(1.0), Expr::int(2)).fold_constants(),
            Expr::float(3.0)
        );
        assert_eq!(
            Expr::bin_op(BinOp::Div, Expr::int(10), Expr::int(3)).fold_constants(),
            Expr::float(10.0 / 3.0)
        );
        assert_eq!(
            Expr::bin_op(BinOp::Div, Expr::int(9), Expr::int(3)).fold_constants(),
            Expr::int(3)
        );
        assert_eq!(
            Expr::bin_op(BinOp::Mod, Expr::int(7), Expr::int(3)).fold_constants(),
            Expr::int(1)
        );
    }

    #[test]
    fn division_by_zero_stays_unchanged() {
        let expr = Expr::bin_op(BinOp::Div, Expr::int(1), Expr::int(0));
        assert_eq!(expr.fold_constants(), expr);
    }

    #[test]
    fn folds_numeric_comparisons() {
        assert_eq!(
            Expr::bin_op(BinOp::Lt, Expr::int(1), Expr::int(2)).fold_constants(),
            Expr::bool_lit(true)
        );
        assert_eq!(
            Expr::bin_op(BinOp::Eq, Expr::float(3.0), Expr::int(3)).fold_constants(),
            Expr::bool_lit(true)
        );
    }

    #[test]
    fn folds_logical_ops_and_short_circuits() {
        assert_eq!(
            Expr::bin_op(BinOp::And, Expr::bool_lit(true), Expr::bool_lit(false)).fold_constants(),
            Expr::bool_lit(false)
        );

        let ident = Expr::ident("x");
        assert_eq!(
            Expr::bin_op(BinOp::And, Expr::bool_lit(true), ident.clone()).fold_constants(),
            ident
        );
        assert_eq!(
            Expr::bin_op(BinOp::Or, Expr::bool_lit(false), ident.clone()).fold_constants(),
            ident
        );
    }

    #[test]
    fn folds_string_concat_to_default_syntax() {
        let expr = Expr::bin_op(BinOp::Concat, Expr::pattern("a"), Expr::pattern("b"));
        assert_eq!(
            expr.fold_constants(),
            Expr::new(ExprKind::Lit {
                value: Lit::Str {
                    value: "ab".into(),
                    syntax: StringSyntax::Default,
                },
            })
        );
    }

    #[test]
    fn folds_unary_ops() {
        assert_eq!(
            Expr::unary_op(UnaryOp::Neg, Expr::int(5)).fold_constants(),
            Expr::int(-5)
        );
        assert_eq!(
            Expr::unary_op(UnaryOp::Not, Expr::bool_lit(true)).fold_constants(),
            Expr::bool_lit(false)
        );
    }

    #[test]
    fn folds_conditionals() {
        let expr = Expr::conditional(Expr::bool_lit(true), Expr::int(1), Expr::int(2));
        assert_eq!(expr.fold_constants(), Expr::int(1));
    }

    #[test]
    fn folds_nested_expressions_bottom_up() {
        let expr = Expr::bin_op(
            BinOp::Mul,
            Expr::bin_op(BinOp::Add, Expr::int(1), Expr::int(2)),
            Expr::int(3),
        );
        assert_eq!(expr.fold_constants(), Expr::int(9));
    }

    #[test]
    fn leaves_non_foldable_expressions_alone() {
        let expr = Expr::bin_op(BinOp::Add, Expr::ident("x"), Expr::int(1));
        assert_eq!(expr.fold_constants(), expr);
    }

    #[test]
    fn preserves_origin_for_literal_folds_and_child_rewrites() {
        let origin = test_origin();
        let folded = Expr::bin_op(BinOp::Add, Expr::int(1), Expr::int(2))
            .with_origin(origin.clone())
            .fold_constants();
        assert_eq!(folded.origin, Some(origin.clone()));

        let child_origin = Origin {
            node: GridPos { col: 1, row: 1 },
            param: None,
        };
        let logical = Expr::bin_op(
            BinOp::And,
            Expr::bool_lit(true),
            Expr::ident("rhs").with_origin(child_origin.clone()),
        )
        .with_origin(origin.clone())
        .fold_constants();
        assert_eq!(logical.origin, Some(child_origin));

        let conditional =
            Expr::conditional(Expr::bool_lit(false), Expr::int(1), Expr::ident("fallback"))
                .with_origin(origin.clone())
                .fold_constants();
        assert_eq!(conditional.origin, Some(origin));
    }

    // -- eliminate_dead_code tests ------------------------------------------

    #[test]
    fn dce_add_zero_identity() {
        let x = Expr::ident("x");
        assert_eq!(
            Expr::bin_op(BinOp::Add, x.clone(), Expr::int(0)).eliminate_dead_code(),
            x
        );
        assert_eq!(
            Expr::bin_op(BinOp::Add, Expr::int(0), x.clone()).eliminate_dead_code(),
            x
        );
        assert_eq!(
            Expr::bin_op(BinOp::Add, x.clone(), Expr::float(0.0)).eliminate_dead_code(),
            x
        );
    }

    #[test]
    fn dce_sub_zero_identity() {
        let x = Expr::ident("x");
        assert_eq!(
            Expr::bin_op(BinOp::Sub, x.clone(), Expr::int(0)).eliminate_dead_code(),
            x
        );
        let expr = Expr::bin_op(BinOp::Sub, Expr::int(0), x.clone());
        assert_eq!(expr.eliminate_dead_code(), expr);
    }

    #[test]
    fn dce_mul_one_identity() {
        let x = Expr::ident("x");
        assert_eq!(
            Expr::bin_op(BinOp::Mul, x.clone(), Expr::int(1)).eliminate_dead_code(),
            x
        );
        assert_eq!(
            Expr::bin_op(BinOp::Mul, Expr::int(1), x.clone()).eliminate_dead_code(),
            x
        );
        assert_eq!(
            Expr::bin_op(BinOp::Mul, x.clone(), Expr::float(1.0)).eliminate_dead_code(),
            x
        );
    }

    #[test]
    fn dce_div_one_identity() {
        let x = Expr::ident("x");
        assert_eq!(
            Expr::bin_op(BinOp::Div, x.clone(), Expr::int(1)).eliminate_dead_code(),
            x
        );
    }

    #[test]
    fn dce_mul_zero_absorbing() {
        let x = Expr::ident("x");
        assert_eq!(
            Expr::bin_op(BinOp::Mul, x.clone(), Expr::int(0)).eliminate_dead_code(),
            Expr::int(0)
        );
        assert_eq!(
            Expr::bin_op(BinOp::Mul, Expr::int(0), x.clone()).eliminate_dead_code(),
            Expr::int(0)
        );
    }

    #[test]
    fn dce_concat_empty_identity() {
        let x = Expr::ident("x");
        assert_eq!(
            Expr::bin_op(BinOp::Concat, x.clone(), Expr::str_lit("")).eliminate_dead_code(),
            x
        );
        assert_eq!(
            Expr::bin_op(BinOp::Concat, Expr::str_lit(""), x.clone()).eliminate_dead_code(),
            x
        );
    }

    #[test]
    fn dce_and_identity_and_absorbing() {
        let x = Expr::ident("x");
        assert_eq!(
            Expr::bin_op(BinOp::And, x.clone(), Expr::bool_lit(true)).eliminate_dead_code(),
            x
        );
        assert_eq!(
            Expr::bin_op(BinOp::And, Expr::bool_lit(true), x.clone()).eliminate_dead_code(),
            x
        );
        assert_eq!(
            Expr::bin_op(BinOp::And, x.clone(), Expr::bool_lit(false)).eliminate_dead_code(),
            Expr::bool_lit(false)
        );
    }

    #[test]
    fn dce_or_identity_and_absorbing() {
        let x = Expr::ident("x");
        assert_eq!(
            Expr::bin_op(BinOp::Or, x.clone(), Expr::bool_lit(false)).eliminate_dead_code(),
            x
        );
        assert_eq!(
            Expr::bin_op(BinOp::Or, Expr::bool_lit(false), x.clone()).eliminate_dead_code(),
            x
        );
        assert_eq!(
            Expr::bin_op(BinOp::Or, x.clone(), Expr::bool_lit(true)).eliminate_dead_code(),
            Expr::bool_lit(true)
        );
    }

    #[test]
    fn dce_double_not() {
        let x = Expr::ident("x");
        assert_eq!(
            Expr::unary_op(UnaryOp::Not, Expr::unary_op(UnaryOp::Not, x.clone()))
                .eliminate_dead_code(),
            x
        );
    }

    #[test]
    fn dce_double_neg() {
        let x = Expr::ident("x");
        assert_eq!(
            Expr::unary_op(UnaryOp::Neg, Expr::unary_op(UnaryOp::Neg, x.clone()))
                .eliminate_dead_code(),
            x
        );
    }

    #[test]
    fn dce_mismatched_double_unary_not_simplified() {
        let x = Expr::ident("x");
        let expr = Expr::unary_op(UnaryOp::Neg, Expr::unary_op(UnaryOp::Not, x));
        assert_eq!(expr.eliminate_dead_code(), expr);
    }

    #[test]
    fn dce_leaves_non_simplifiable_alone() {
        let expr = Expr::bin_op(BinOp::Add, Expr::ident("x"), Expr::ident("y"));
        assert_eq!(expr.eliminate_dead_code(), expr);

        let expr = Expr::bin_op(BinOp::Mul, Expr::ident("x"), Expr::int(2));
        assert_eq!(expr.eliminate_dead_code(), expr);
    }

    #[test]
    fn dce_nested_bottom_up() {
        let x = Expr::ident("x");
        let expr = Expr::bin_op(
            BinOp::Add,
            Expr::bin_op(BinOp::Mul, x.clone(), Expr::int(1)),
            Expr::int(0),
        );
        assert_eq!(expr.eliminate_dead_code(), x);
    }

    #[test]
    fn dce_preserves_origins() {
        let origin = test_origin();
        let child_origin = Origin {
            node: GridPos { col: 1, row: 1 },
            param: None,
        };

        let x = Expr::ident("x");
        let result = Expr::bin_op(BinOp::Add, x.clone(), Expr::int(0))
            .with_origin(origin.clone())
            .eliminate_dead_code();
        assert_eq!(result, x.clone().with_origin(origin.clone()));

        let result = Expr::bin_op(
            BinOp::Add,
            x.clone().with_origin(child_origin.clone()),
            Expr::int(0),
        )
        .with_origin(origin.clone())
        .eliminate_dead_code();
        assert_eq!(result.origin, Some(child_origin));

        let result = Expr::bin_op(BinOp::Mul, x.clone(), Expr::int(0))
            .with_origin(origin.clone())
            .eliminate_dead_code();
        assert_eq!(result, Expr::int(0).with_origin(origin));
    }

    // -- optimize (combined pass) -------------------------------------------

    #[test]
    fn optimize_chains_fold_then_dce() {
        // fold: 1+2 → 3, then dce: x * 1 → x won't apply (3 is not 1)
        // but: fold: true → true, then dce: x && true → x does chain
        let x = Expr::ident("x");
        let expr = Expr::bin_op(
            BinOp::And,
            x.clone(),
            Expr::bin_op(BinOp::Eq, Expr::int(1), Expr::int(1)),
        );
        assert_eq!(expr.optimize(), x);
    }

    #[test]
    fn full_opt_simplifies_matching_conditional_branches() {
        let expr = Expr::conditional(
            Expr::ident("cond"),
            Expr::method_call(Expr::ident("x"), "fast", Vec::new()),
            Expr::method_call(Expr::ident("x"), "fast", Vec::new()),
        );

        assert_eq!(
            expr.optimize_at(OptLevel::Full),
            Expr::method_call(Expr::ident("x"), "fast", Vec::new())
        );
    }

    #[test]
    fn cse_hoists_common_subexpressions_without_self_references() {
        let terminals = vec![
            Expr::method_call(Expr::str_lit("bd"), "fast", Vec::new()),
            Expr::method_call(Expr::str_lit("bd"), "fast", Vec::new()),
        ];

        let hoisted = Expr::hoist_common_subexprs(&terminals);
        assert_eq!(hoisted.len(), 2);

        for expr in hoisted {
            match expr.kind {
                ExprKind::Block { bindings, result } => {
                    assert_eq!(bindings.len(), 1);
                    assert_eq!(bindings[0].0, "_t0");
                    assert_eq!(*result, Expr::ident("_t0"));
                    assert_ne!(bindings[0].1, Expr::ident("_t0"));
                }
                other => panic!("expected hoisted block, got {other:?}"),
            }
        }
    }
}
