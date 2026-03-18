//! Target-language rendering backends.
//!
//! The [`Backend`] trait defines the language-specific decisions a renderer
//! must make. The default method implementations provide a recursive walk
//! over the AST, delegating per-node rendering to overridable methods.
//!
//! Built-in backends:
//! - [`JsBackend`] — JavaScript-style output.
//! - [`LuaBackend`] — Lua 5.3+ output.
//!
//! Behavior note:
//! Empty and whitespace-only strings render literally. Hosts that want to
//! suppress or reinterpret blank strings should do that in a target-specific
//! adaptation pass before backend rendering.

use crate::ast::{BinOp, Expr, ExprKind, Lit, StringSyntax, UnaryOp};
use crate::types::DomainBridgeKind;

// ---------------------------------------------------------------------------
// Precedence levels (higher = tighter binding)
// ---------------------------------------------------------------------------

/// Default precedence values used by the built-in backends.
/// Backends may override [`Backend::precedence`] for different models.
pub const PREC_LAMBDA: u8 = 1;
pub const PREC_CONDITIONAL: u8 = 2;
pub const PREC_OR: u8 = 3;
pub const PREC_AND: u8 = 4;
pub const PREC_EQUALITY: u8 = 5; // ==, !=
pub const PREC_COMPARISON: u8 = 6; // <, <=, >, >=
pub const PREC_CONCAT: u8 = 7;
pub const PREC_ADD: u8 = 8; // +, -
pub const PREC_MUL: u8 = 9; // *, /, %
pub const PREC_UNARY: u8 = 10;
pub const PREC_CALL: u8 = 11; // call, method, field, index
pub const PREC_ATOM: u8 = 12; // literals, idents

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

/// Trait for rendering an AST to a target language string.
///
/// Override individual methods to customise syntax. The default
/// implementations produce JavaScript-like output so that existing
/// behaviour is preserved without a dedicated backend.
pub trait Backend {
    // -- Language-specific methods (override these) --

    /// The nil / null / None keyword for the target language.
    fn nil_keyword(&self) -> &str {
        "null"
    }

    /// Render a quoted string literal with the given syntax hint.
    fn render_string(&self, value: &str, syntax: StringSyntax) -> String {
        match syntax {
            StringSyntax::Default => render_quoted(value, '\''),
            StringSyntax::Pattern => render_quoted(value, '"'),
        }
    }

    /// Render a lambda / closure expression.
    fn render_lambda(&self, params: &[String], body: &str) -> String {
        let params_str = params.join(", ");
        format!("({params_str}) => {body}")
    }

    /// Render a method call. `recv`, `args` are already-rendered strings.
    fn render_method_call(&self, recv: &str, method: &str, args: &str) -> String {
        format!("{recv}.{method}({args})")
    }

    /// Render an expression-level conditional.
    fn render_conditional_expr(&self, cond: &str, then_expr: &str, else_expr: &str) -> String {
        format!("{cond} ? {then_expr} : {else_expr}")
    }

    /// Render an array literal. `inner` is the already-rendered, comma-separated elements.
    fn render_array_literal(&self, inner: &str) -> String {
        format!("[{inner}]")
    }

    /// Render a record literal. Each entry is `(key, rendered_value)`.
    fn render_record_literal(&self, entries: &[(String, String)]) -> String {
        let inner = entries
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{{inner}}}")
    }

    /// Render a block of let-bindings followed by a result expression.
    ///
    /// `bindings` is a list of `(name, rendered_value)` pairs.
    /// `result` is the already-rendered result expression.
    fn render_block(&self, bindings: &[(String, String)], result: &str) -> String {
        let mut parts = Vec::with_capacity(bindings.len() + 1);
        for (name, value) in bindings {
            parts.push(format!("let {name} = {value}"));
        }
        parts.push(format!("return {result}"));
        let body = parts.join("; ");
        format!("(() => {{ {body}; }})()")
    }

    /// Render a delay buffer reference.
    ///
    /// `slot` is the unique buffer identifier; `default` is the already-rendered
    /// initial value for frame 0. The host runtime must provide a `__delay`
    /// function (or equivalent) that reads from the named frame buffer.
    fn render_delay_ref(&self, slot: &str, default: &str) -> String {
        format!("__delay('{slot}', {default})")
    }

    /// Render an implicit execution-domain conversion.
    ///
    /// The default form expects the host runtime to provide a
    /// `__domain_convert(kind, value)` helper that performs the conversion.
    fn render_domain_convert(&self, kind: DomainBridgeKind, expr: &str) -> String {
        format!("__domain_convert('{}', {expr})", kind.as_str())
    }

    /// Render an error placeholder.
    fn render_error(&self, message: &str) -> String {
        format!("/* {message} */")
    }

    /// Return the symbol for a binary operator.
    fn bin_op_symbol(&self, op: BinOp) -> &str {
        match op {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::Concat => "+",
        }
    }

    /// Return the symbol for a unary operator.
    fn unary_op_symbol(&self, op: UnaryOp) -> &str {
        match op {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
        }
    }

    /// Return the associativity for a binary operator in this backend.
    fn bin_op_is_right_associative(&self, _op: BinOp) -> bool {
        false
    }

    /// Return the precedence level for an expression kind.
    /// Higher values bind tighter.
    fn precedence(&self, kind: &ExprKind) -> u8 {
        match kind {
            ExprKind::Lambda { .. } => PREC_LAMBDA,
            ExprKind::Conditional { .. } => PREC_CONDITIONAL,
            ExprKind::BinOp { op, .. } => match op {
                BinOp::Or => PREC_OR,
                BinOp::And => PREC_AND,
                BinOp::Eq | BinOp::Ne => PREC_EQUALITY,
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => PREC_COMPARISON,
                BinOp::Concat => PREC_CONCAT,
                BinOp::Add | BinOp::Sub => PREC_ADD,
                BinOp::Mul | BinOp::Div | BinOp::Mod => PREC_MUL,
            },
            ExprKind::UnaryOp { .. } => PREC_UNARY,
            ExprKind::Call { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::Field { .. }
            | ExprKind::Index { .. } => PREC_CALL,
            ExprKind::Lit { .. }
            | ExprKind::Ident { .. }
            | ExprKind::Array { .. }
            | ExprKind::Record { .. }
            | ExprKind::Block { .. }
            | ExprKind::DelayRef { .. }
            | ExprKind::DomainConvert { .. }
            | ExprKind::Error { .. } => PREC_ATOM,
        }
    }

    /// Render a literal value.
    fn render_lit(&self, lit: &Lit) -> String {
        match lit {
            Lit::Nil => self.nil_keyword().to_string(),
            Lit::Bool(true) => "true".to_string(),
            Lit::Bool(false) => "false".to_string(),
            Lit::Int(v) => v.to_string(),
            Lit::Float(v) => format_float(*v),
            Lit::Str { value, syntax } => self.render_string(value, *syntax),
        }
    }

    // -- Entry point (default recursive dispatch) --

    /// Render a complete expression to a string.
    fn render(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Lit { value } => self.render_lit(value),
            ExprKind::Ident { name } => name.clone(),
            ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                let cond_str = self.render_conditional_operand(cond, &expr.kind);
                let then_str = self.render_conditional_operand(then_expr, &expr.kind);
                let else_str = self.render_conditional_operand(else_expr, &expr.kind);
                self.render_conditional_expr(&cond_str, &then_str, &else_str)
            }
            ExprKind::Call { func, args } => {
                let func_str = self.render_in_access_position(func);
                let args_str = self.render_args(args);
                format!("{func_str}({args_str})")
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                let recv_str = self.render_in_access_position(receiver);
                let args_str = self.render_args(args);
                self.render_method_call(&recv_str, method, &args_str)
            }
            ExprKind::Field { object, field } => {
                let obj_str = self.render_in_access_position(object);
                format!("{obj_str}.{field}")
            }
            ExprKind::Index { object, index } => {
                let obj_str = self.render_in_access_position(object);
                let idx_str = self.render(index);
                format!("{obj_str}[{idx_str}]")
            }
            ExprKind::BinOp { op, lhs, rhs } => {
                let op_str = self.bin_op_symbol(*op);
                let lhs_str = self.render_bin_operand(lhs, &expr.kind, false);
                let rhs_str = self.render_bin_operand(rhs, &expr.kind, true);
                format!("{lhs_str} {op_str} {rhs_str}")
            }
            ExprKind::UnaryOp { op, operand } => self.render_unary_expr(*op, operand, &expr.kind),
            ExprKind::Lambda { params, body } => {
                let body_str = self.render_lambda_body(body);
                self.render_lambda(params, &body_str)
            }
            ExprKind::Array { elements } => {
                let inner = self.render_args(elements);
                self.render_array_literal(&inner)
            }
            ExprKind::Record { entries } => {
                let rendered: Vec<(String, String)> = entries
                    .iter()
                    .map(|(k, v)| (k.clone(), self.render(v)))
                    .collect();
                self.render_record_literal(&rendered)
            }
            ExprKind::Block { bindings, result } => {
                let rendered_bindings: Vec<(String, String)> = bindings
                    .iter()
                    .map(|(name, value)| (name.clone(), self.render(value)))
                    .collect();
                let result_str = self.render(result);
                self.render_block(&rendered_bindings, &result_str)
            }
            ExprKind::DelayRef { slot, default } => {
                let default_str = self.render(default);
                self.render_delay_ref(slot, &default_str)
            }
            ExprKind::DomainConvert { kind, input } => {
                let rendered = self.render(input);
                self.render_domain_convert(*kind, &rendered)
            }
            ExprKind::Error { message } => self.render_error(message),
        }
    }

    // -- Shared helpers (not intended for override) --

    /// Render a comma-separated argument list.
    fn render_args(&self, args: &[Expr]) -> String {
        args.iter()
            .map(|a| self.render(a))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn render_wrapped(&self, child: &Expr, needs_parens: bool) -> String {
        let rendered = self.render(child);
        if needs_parens {
            format!("({rendered})")
        } else {
            rendered
        }
    }

    fn render_in_access_position(&self, child: &Expr) -> String {
        let needs_parens = self.precedence(&child.kind) < PREC_CALL
            || matches!(child.kind, ExprKind::Record { .. });
        self.render_wrapped(child, needs_parens)
    }

    fn render_bin_operand(&self, child: &Expr, parent_kind: &ExprKind, is_rhs: bool) -> String {
        let child_prec = self.precedence(&child.kind);
        let parent_prec = self.precedence(parent_kind);
        let needs_parens = child_prec < parent_prec
            || (child_prec == parent_prec
                && matches!(child.kind, ExprKind::BinOp { .. })
                && match parent_kind {
                    ExprKind::BinOp { op, .. } => {
                        if self.bin_op_is_right_associative(*op) {
                            !is_rhs
                        } else {
                            is_rhs
                        }
                    }
                    _ => true,
                });
        self.render_wrapped(child, needs_parens)
    }

    fn render_conditional_operand(&self, child: &Expr, parent_kind: &ExprKind) -> String {
        let child_prec = self.precedence(&child.kind);
        let parent_prec = self.precedence(parent_kind);
        let needs_parens = child_prec <= parent_prec;
        self.render_wrapped(child, needs_parens)
    }

    fn render_unary_operand(&self, child: &Expr, parent_kind: &ExprKind) -> String {
        let child_prec = self.precedence(&child.kind);
        let parent_prec = self.precedence(parent_kind);
        let needs_parens = child_prec < parent_prec
            || (child_prec == parent_prec && matches!(child.kind, ExprKind::UnaryOp { .. }));
        self.render_wrapped(child, needs_parens)
    }

    fn render_unary_expr(&self, op: UnaryOp, operand: &Expr, parent_kind: &ExprKind) -> String {
        let op_str = self.unary_op_symbol(op);
        let operand_str = self.render_unary_operand(operand, parent_kind);
        format!("{op_str}{operand_str}")
    }

    fn render_lambda_body(&self, body: &Expr) -> String {
        self.render_wrapped(body, matches!(body.kind, ExprKind::Record { .. }))
    }
}

// ---------------------------------------------------------------------------
// Shared utilities
// ---------------------------------------------------------------------------

/// Render a quoted string with escape handling.
pub fn render_quoted(value: &str, quote: char) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' if quote == '\'' => out.push_str("\\'"),
            '"' if quote == '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push(quote);
    out
}

fn format_float(v: f64) -> String {
    let s = v.to_string();
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

// ---------------------------------------------------------------------------
// Built-in backends
// ---------------------------------------------------------------------------

/// JavaScript backend.
///
/// Produces JavaScript-style output for the Tessera AST.
pub struct JsBackend;

impl Backend for JsBackend {}

/// Lua 5.3+ backend.
pub struct LuaBackend;

impl Backend for LuaBackend {
    fn nil_keyword(&self) -> &str {
        "nil"
    }

    fn render_string(&self, value: &str, syntax: StringSyntax) -> String {
        match syntax {
            StringSyntax::Default | StringSyntax::Pattern => render_quoted(value, '\''),
        }
    }

    fn bin_op_symbol(&self, op: BinOp) -> &str {
        match op {
            BinOp::Ne => "~=",
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::Concat => "..",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
        }
    }

    fn unary_op_symbol(&self, op: UnaryOp) -> &str {
        match op {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "not",
        }
    }

    fn bin_op_is_right_associative(&self, op: BinOp) -> bool {
        matches!(op, BinOp::Concat)
    }

    fn render_lambda(&self, params: &[String], body: &str) -> String {
        let params_str = params.join(", ");
        format!("function({params_str}) return {body} end")
    }

    fn render_method_call(&self, recv: &str, method: &str, args: &str) -> String {
        format!("{recv}:{method}({args})")
    }

    fn render_conditional_expr(&self, cond: &str, then_expr: &str, else_expr: &str) -> String {
        format!("(function() if {cond} then return {then_expr} else return {else_expr} end end)()")
    }

    fn render_array_literal(&self, inner: &str) -> String {
        format!("{{{inner}}}")
    }

    fn render_record_literal(&self, entries: &[(String, String)]) -> String {
        let inner = entries
            .iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{{inner}}}")
    }

    fn render_block(&self, bindings: &[(String, String)], result: &str) -> String {
        let mut parts = Vec::with_capacity(bindings.len() + 1);
        for (name, value) in bindings {
            parts.push(format!("local {name} = {value}"));
        }
        parts.push(format!("return {result}"));
        let body = parts.join("; ");
        format!("(function() {body} end)()")
    }

    fn render_delay_ref(&self, slot: &str, default: &str) -> String {
        format!("__delay('{slot}', {default})")
    }

    fn render_error(&self, message: &str) -> String {
        format!("--[[ {message} ]]")
    }

    fn render_unary_expr(&self, op: UnaryOp, operand: &Expr, parent_kind: &ExprKind) -> String {
        match op {
            UnaryOp::Neg => {
                let operand_str = self.render_unary_operand(operand, parent_kind);
                format!("-{operand_str}")
            }
            UnaryOp::Not => {
                let operand_str = self.render_unary_operand(operand, parent_kind);
                format!("not {operand_str}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Expr;
    use crate::types::DomainBridgeKind;

    fn js() -> &'static dyn Backend {
        &JsBackend
    }

    fn lua() -> &'static dyn Backend {
        &LuaBackend
    }

    // -- JS backend tests --

    #[test]
    fn js_string_literal() {
        assert_eq!(js().render(&Expr::str_lit("bd")), "'bd'");
    }

    #[test]
    fn js_empty_string_renders_literal() {
        assert_eq!(js().render(&Expr::str_lit("")), "''");
    }

    #[test]
    fn js_whitespace_string_renders_literal() {
        assert_eq!(js().render(&Expr::str_lit("  ")), "'  '");
    }

    #[test]
    fn js_pattern_string() {
        assert_eq!(js().render(&Expr::pattern("bd sd")), "\"bd sd\"");
    }

    #[test]
    fn js_number() {
        assert_eq!(js().render(&Expr::float(0.5)), "0.5");
    }

    #[test]
    fn js_integer() {
        assert_eq!(js().render(&Expr::int(42)), "42");
    }

    #[test]
    fn js_bool() {
        assert_eq!(js().render(&Expr::bool_lit(true)), "true");
        assert_eq!(js().render(&Expr::bool_lit(false)), "false");
    }

    #[test]
    fn js_nil() {
        assert_eq!(js().render(&Expr::nil()), "null");
    }

    #[test]
    fn js_ident() {
        assert_eq!(js().render(&Expr::ident("foo")), "foo");
    }

    #[test]
    fn js_conditional_expr() {
        let expr = Expr::conditional(Expr::ident("flag"), Expr::int(1), Expr::int(0));
        assert_eq!(js().render(&expr), "flag ? 1 : 0");
    }

    #[test]
    fn js_call() {
        let expr = Expr::call_named("foo", vec![Expr::int(1), Expr::int(2)]);
        assert_eq!(js().render(&expr), "foo(1, 2)");
    }

    #[test]
    fn js_method_call() {
        let expr = Expr::method_call(Expr::str_lit("bd"), "fast", vec![]);
        assert_eq!(js().render(&expr), "'bd'.fast()");
    }

    #[test]
    fn js_lambda() {
        let expr = Expr::lambda(
            vec!["pattern".into()],
            Expr::call_named("shimmer", vec![Expr::ident("pattern"), Expr::float(0.5)]),
        );
        assert_eq!(js().render(&expr), "(pattern) => shimmer(pattern, 0.5)");
    }

    #[test]
    fn js_error() {
        assert_eq!(
            js().render(&Expr::error("missing subgraph output")),
            "/* missing subgraph output */"
        );
    }

    #[test]
    fn js_array() {
        let expr = Expr::array(vec![Expr::int(1), Expr::int(2)]);
        assert_eq!(js().render(&expr), "[1, 2]");
    }

    #[test]
    fn js_record() {
        let expr = Expr::record(vec![("x".into(), Expr::int(1))]);
        assert_eq!(js().render(&expr), "{x: 1}");
    }

    #[test]
    fn js_block() {
        let expr = Expr::block(vec![("_t0".into(), Expr::int(3))], Expr::ident("_t0"));
        assert_eq!(js().render(&expr), "(() => { let _t0 = 3; return _t0; })()");
    }

    #[test]
    fn js_field() {
        let expr = Expr::field(Expr::ident("obj"), "prop");
        assert_eq!(js().render(&expr), "obj.prop");
    }

    #[test]
    fn js_index() {
        let expr = Expr::index(Expr::ident("arr"), Expr::int(0));
        assert_eq!(js().render(&expr), "arr[0]");
    }

    #[test]
    fn js_bin_op() {
        let expr = Expr::bin_op(BinOp::Add, Expr::int(1), Expr::int(2));
        assert_eq!(js().render(&expr), "1 + 2");
    }

    #[test]
    fn js_unary_op() {
        let expr = Expr::unary_op(UnaryOp::Neg, Expr::int(5));
        assert_eq!(js().render(&expr), "-5");
    }

    #[test]
    fn js_parens_for_precedence() {
        // (1 + 2) * 3 — add has lower precedence than mul
        let expr = Expr::bin_op(
            BinOp::Mul,
            Expr::bin_op(BinOp::Add, Expr::int(1), Expr::int(2)),
            Expr::int(3),
        );
        assert_eq!(js().render(&expr), "(1 + 2) * 3");
    }

    #[test]
    fn js_no_parens_when_not_needed() {
        // 1 * 2 + 3 — mul has higher precedence, rendered left-to-right
        let expr = Expr::bin_op(
            BinOp::Add,
            Expr::bin_op(BinOp::Mul, Expr::int(1), Expr::int(2)),
            Expr::int(3),
        );
        assert_eq!(js().render(&expr), "1 * 2 + 3");
    }

    #[test]
    fn js_left_associative_chain_avoids_redundant_parens() {
        let expr = Expr::bin_op(
            BinOp::Add,
            Expr::bin_op(BinOp::Add, Expr::int(1), Expr::int(2)),
            Expr::int(3),
        );
        assert_eq!(js().render(&expr), "1 + 2 + 3");
    }

    #[test]
    fn js_parens_lambda_callee() {
        let expr = Expr::call(
            Expr::lambda(vec!["x".into()], Expr::ident("x")),
            vec![Expr::int(1)],
        );
        assert_eq!(js().render(&expr), "((x) => x)(1)");
    }

    #[test]
    fn js_parens_equal_precedence_rhs() {
        let expr = Expr::bin_op(
            BinOp::Sub,
            Expr::int(1),
            Expr::bin_op(BinOp::Sub, Expr::int(2), Expr::int(3)),
        );
        assert_eq!(js().render(&expr), "1 - (2 - 3)");
    }

    #[test]
    fn js_parens_complex_method_receiver() {
        let expr = Expr::method_call(
            Expr::bin_op(BinOp::Add, Expr::int(1), Expr::int(2)),
            "run",
            vec![],
        );
        assert_eq!(js().render(&expr), "(1 + 2).run()");
    }

    #[test]
    fn js_wraps_record_lambda_body() {
        let expr = Expr::lambda(
            vec!["x".into()],
            Expr::record(vec![("value".into(), Expr::ident("x"))]),
        );
        assert_eq!(js().render(&expr), "(x) => ({value: x})");
    }

    // -- Lua backend tests --

    #[test]
    fn lua_nil() {
        assert_eq!(lua().render(&Expr::nil()), "nil");
    }

    #[test]
    fn lua_conditional_expr() {
        let expr = Expr::conditional(Expr::ident("flag"), Expr::int(1), Expr::int(0));
        assert_eq!(
            lua().render(&expr),
            "(function() if flag then return 1 else return 0 end end)()"
        );
    }

    #[test]
    fn lua_method_call() {
        let expr = Expr::method_call(Expr::ident("obj"), "run", vec![]);
        assert_eq!(lua().render(&expr), "obj:run()");
    }

    #[test]
    fn lua_lambda() {
        let expr = Expr::lambda(vec!["x".into()], Expr::ident("x"));
        assert_eq!(lua().render(&expr), "function(x) return x end");
    }

    #[test]
    fn lua_array() {
        let expr = Expr::array(vec![Expr::int(1), Expr::int(2)]);
        assert_eq!(lua().render(&expr), "{1, 2}");
    }

    #[test]
    fn lua_record() {
        let expr = Expr::record(vec![("x".into(), Expr::int(1))]);
        assert_eq!(lua().render(&expr), "{x = 1}");
    }

    #[test]
    fn lua_block() {
        let expr = Expr::block(vec![("_t0".into(), Expr::int(3))], Expr::ident("_t0"));
        assert_eq!(
            lua().render(&expr),
            "(function() local _t0 = 3; return _t0 end)()"
        );
    }

    #[test]
    fn lua_ne() {
        let expr = Expr::bin_op(BinOp::Ne, Expr::ident("a"), Expr::ident("b"));
        assert_eq!(lua().render(&expr), "a ~= b");
    }

    #[test]
    fn lua_not() {
        let expr = Expr::unary_op(UnaryOp::Not, Expr::bool_lit(true));
        assert_eq!(lua().render(&expr), "not true");
    }

    #[test]
    fn lua_keeps_whitespace_strings_literal() {
        assert_eq!(lua().render(&Expr::str_lit("  ")), "'  '");
    }

    #[test]
    fn lua_error() {
        assert_eq!(lua().render(&Expr::error("missing")), "--[[ missing ]]");
    }

    #[test]
    fn lua_concat() {
        let expr = Expr::bin_op(BinOp::Concat, Expr::str_lit("a"), Expr::str_lit("b"));
        assert_eq!(lua().render(&expr), "'a' .. 'b'");
    }

    #[test]
    fn lua_right_associative_concat_avoids_rhs_parens() {
        let expr = Expr::bin_op(
            BinOp::Concat,
            Expr::str_lit("a"),
            Expr::bin_op(BinOp::Concat, Expr::str_lit("b"), Expr::str_lit("c")),
        );
        assert_eq!(lua().render(&expr), "'a' .. 'b' .. 'c'");
    }

    #[test]
    fn lua_right_associative_concat_parens_lhs() {
        let expr = Expr::bin_op(
            BinOp::Concat,
            Expr::bin_op(BinOp::Concat, Expr::str_lit("a"), Expr::str_lit("b")),
            Expr::str_lit("c"),
        );
        assert_eq!(lua().render(&expr), "('a' .. 'b') .. 'c'");
    }

    // -- DelayRef rendering tests --

    #[test]
    fn js_delay_ref() {
        let expr = Expr::delay_ref("d_3_2", Expr::int(0));
        assert_eq!(js().render(&expr), "__delay('d_3_2', 0)");
    }

    #[test]
    fn js_delay_ref_with_complex_default() {
        let expr = Expr::delay_ref("d_0_0", Expr::str_lit("silence"));
        assert_eq!(js().render(&expr), "__delay('d_0_0', 'silence')");
    }

    #[test]
    fn js_domain_convert_uses_runtime_helper() {
        let expr = Expr::domain_convert(DomainBridgeKind::ControlToAudio, Expr::ident("signal"));
        assert_eq!(
            js().render(&expr),
            "__domain_convert('control_to_audio', signal)"
        );
    }

    #[test]
    fn lua_delay_ref() {
        let expr = Expr::delay_ref("d_3_2", Expr::int(0));
        assert_eq!(lua().render(&expr), "__delay('d_3_2', 0)");
    }

    #[test]
    fn lua_domain_convert_uses_runtime_helper() {
        let expr = Expr::domain_convert(DomainBridgeKind::AudioToControl, Expr::ident("signal"));
        assert_eq!(
            lua().render(&expr),
            "__domain_convert('audio_to_control', signal)"
        );
    }
}
