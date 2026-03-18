//! Optimization pass internals for [`Expr`].
//!
//! This module provides the implementation behind [`Expr::fold_constants`],
//! [`Expr::eliminate_dead_code`], [`Expr::simplify_conditionals`], and
//! [`Expr::hoist_common_subexprs`]. All types are private — the public API
//! is the method surface on `Expr`.

use std::collections::HashMap;

use super::*;

#[derive(Debug, Clone, Copy)]
enum NumberLit {
    Int(i64),
    Float(f64),
}

impl NumberLit {
    fn as_f64(self) -> f64 {
        match self {
            NumberLit::Int(v) => v as f64,
            NumberLit::Float(v) => v,
        }
    }
}

// -- shared helpers ---------------------------------------------------------

fn lit_value(expr: &Expr) -> Option<&Lit> {
    match &expr.kind {
        ExprKind::Lit { value } => Some(value),
        _ => None,
    }
}

fn bool_lit_value(expr: &Expr) -> Option<bool> {
    match lit_value(expr) {
        Some(Lit::Bool(v)) => Some(*v),
        _ => None,
    }
}

fn numeric_lit(expr: &Expr) -> Option<NumberLit> {
    match lit_value(expr) {
        Some(Lit::Int(v)) => Some(NumberLit::Int(*v)),
        Some(Lit::Float(v)) => Some(NumberLit::Float(*v)),
        _ => None,
    }
}

fn finite_float_expr(value: f64) -> Option<Expr> {
    value.is_finite().then(|| Expr::float(value))
}

fn inherit_origin(expr: Expr, origin: &Option<Origin>) -> Expr {
    match origin {
        Some(origin) => expr.with_origin(origin.clone()),
        None => expr,
    }
}

fn backfill_origin(expr: Expr, origin: &Option<Origin>) -> Expr {
    match origin {
        Some(origin) => expr.with_origin_if_missing(origin.clone()),
        None => expr,
    }
}

// -- constant folding -------------------------------------------------------

pub(super) fn fold_expr(expr: &Expr) -> Expr {
    let origin = expr.origin.clone();
    match &expr.kind {
        ExprKind::Lit { .. } | ExprKind::Ident { .. } | ExprKind::Error { .. } => expr.clone(),
        ExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => {
            let cond = fold_expr(cond);
            let then_expr = fold_expr(then_expr);
            let else_expr = fold_expr(else_expr);
            if let Some(flag) = bool_lit_value(&cond) {
                let branch = if flag { then_expr } else { else_expr };
                return backfill_origin(branch, &origin);
            }
            inherit_origin(Expr::conditional(cond, then_expr, else_expr), &origin)
        }
        ExprKind::Call { func, args } => inherit_origin(
            Expr::call(fold_expr(func), args.iter().map(fold_expr).collect()),
            &origin,
        ),
        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => inherit_origin(
            Expr::method_call(
                fold_expr(receiver),
                method.clone(),
                args.iter().map(fold_expr).collect(),
            ),
            &origin,
        ),
        ExprKind::Field { object, field } => {
            inherit_origin(Expr::field(fold_expr(object), field.clone()), &origin)
        }
        ExprKind::Index { object, index } => {
            inherit_origin(Expr::index(fold_expr(object), fold_expr(index)), &origin)
        }
        ExprKind::BinOp { op, lhs, rhs } => {
            let lhs = fold_expr(lhs);
            let rhs = fold_expr(rhs);
            if let Some(folded) = fold_bin_op(*op, &lhs, &rhs, &origin) {
                return folded;
            }
            inherit_origin(Expr::bin_op(*op, lhs, rhs), &origin)
        }
        ExprKind::UnaryOp { op, operand } => {
            let operand = fold_expr(operand);
            if let Some(folded) = fold_unary_op(*op, &operand, &origin) {
                return folded;
            }
            inherit_origin(Expr::unary_op(*op, operand), &origin)
        }
        ExprKind::Lambda { params, body } => {
            inherit_origin(Expr::lambda(params.clone(), fold_expr(body)), &origin)
        }
        ExprKind::Array { elements } => inherit_origin(
            Expr::array(elements.iter().map(fold_expr).collect()),
            &origin,
        ),
        ExprKind::Record { entries } => inherit_origin(
            Expr::record(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), fold_expr(value)))
                    .collect(),
            ),
            &origin,
        ),
        ExprKind::Block { bindings, result } => inherit_origin(
            Expr::block(
                bindings
                    .iter()
                    .map(|(name, value)| (name.clone(), fold_expr(value)))
                    .collect(),
                fold_expr(result),
            ),
            &origin,
        ),
        ExprKind::DelayRef { slot, default } => {
            inherit_origin(Expr::delay_ref(slot.clone(), fold_expr(default)), &origin)
        }
        ExprKind::DomainConvert { kind, input } => inherit_origin(
            Expr::domain_convert(*kind, fold_expr(input)),
            &origin,
        ),
    }
}

fn fold_bin_op(op: BinOp, lhs: &Expr, rhs: &Expr, origin: &Option<Origin>) -> Option<Expr> {
    match op {
        BinOp::And => fold_and(lhs, rhs, origin),
        BinOp::Or => fold_or(lhs, rhs, origin),
        BinOp::Concat => fold_concat(lhs, rhs, origin),
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
            fold_arithmetic(op, lhs, rhs, origin)
        }
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            fold_comparison(op, lhs, rhs, origin)
        }
    }
}

fn fold_unary_op(op: UnaryOp, operand: &Expr, origin: &Option<Origin>) -> Option<Expr> {
    match (op, lit_value(operand)) {
        (UnaryOp::Neg, Some(Lit::Int(v))) => v
            .checked_neg()
            .map(Expr::int)
            .map(|e| inherit_origin(e, origin)),
        (UnaryOp::Neg, Some(Lit::Float(v))) => {
            finite_float_expr(-v).map(|e| inherit_origin(e, origin))
        }
        (UnaryOp::Not, Some(Lit::Bool(v))) => Some(inherit_origin(Expr::bool_lit(!v), origin)),
        _ => None,
    }
}

fn fold_and(lhs: &Expr, rhs: &Expr, origin: &Option<Origin>) -> Option<Expr> {
    if let (Some(l), Some(r)) = (bool_lit_value(lhs), bool_lit_value(rhs)) {
        return Some(inherit_origin(Expr::bool_lit(l && r), origin));
    }
    match bool_lit_value(lhs) {
        Some(false) => Some(inherit_origin(Expr::bool_lit(false), origin)),
        Some(true) => Some(backfill_origin(rhs.clone(), origin)),
        None => None,
    }
}

fn fold_or(lhs: &Expr, rhs: &Expr, origin: &Option<Origin>) -> Option<Expr> {
    if let (Some(l), Some(r)) = (bool_lit_value(lhs), bool_lit_value(rhs)) {
        return Some(inherit_origin(Expr::bool_lit(l || r), origin));
    }
    match bool_lit_value(lhs) {
        Some(true) => Some(inherit_origin(Expr::bool_lit(true), origin)),
        Some(false) => Some(backfill_origin(rhs.clone(), origin)),
        None => None,
    }
}

fn fold_concat(lhs: &Expr, rhs: &Expr, origin: &Option<Origin>) -> Option<Expr> {
    match (lit_value(lhs), lit_value(rhs)) {
        (Some(Lit::Str { value: l, .. }), Some(Lit::Str { value: r, .. })) => Some(inherit_origin(
            Expr::new(ExprKind::Lit {
                value: Lit::Str {
                    value: format!("{l}{r}"),
                    syntax: StringSyntax::Default,
                },
            }),
            origin,
        )),
        _ => None,
    }
}

fn fold_arithmetic(op: BinOp, lhs: &Expr, rhs: &Expr, origin: &Option<Origin>) -> Option<Expr> {
    let l = numeric_lit(lhs)?;
    let r = numeric_lit(rhs)?;
    let expr = match (l, r) {
        (NumberLit::Int(l), NumberLit::Int(r)) => fold_int_arithmetic(op, l, r)?,
        (l, r) => fold_float_arithmetic(op, l.as_f64(), r.as_f64())?,
    };
    Some(inherit_origin(expr, origin))
}

fn fold_int_arithmetic(op: BinOp, lhs: i64, rhs: i64) -> Option<Expr> {
    match op {
        BinOp::Add => lhs.checked_add(rhs).map(Expr::int),
        BinOp::Sub => lhs.checked_sub(rhs).map(Expr::int),
        BinOp::Mul => lhs.checked_mul(rhs).map(Expr::int),
        BinOp::Div => {
            if rhs == 0 {
                return None;
            }
            if lhs % rhs == 0 {
                lhs.checked_div(rhs).map(Expr::int)
            } else {
                finite_float_expr(lhs as f64 / rhs as f64)
            }
        }
        BinOp::Mod => {
            if rhs == 0 {
                return None;
            }
            Some(Expr::int(lhs % rhs))
        }
        _ => None,
    }
}

fn fold_float_arithmetic(op: BinOp, lhs: f64, rhs: f64) -> Option<Expr> {
    match op {
        BinOp::Add => finite_float_expr(lhs + rhs),
        BinOp::Sub => finite_float_expr(lhs - rhs),
        BinOp::Mul => finite_float_expr(lhs * rhs),
        BinOp::Div => {
            if rhs == 0.0 {
                return None;
            }
            finite_float_expr(lhs / rhs)
        }
        BinOp::Mod => {
            if rhs == 0.0 {
                return None;
            }
            finite_float_expr(lhs % rhs)
        }
        _ => None,
    }
}

fn fold_comparison(op: BinOp, lhs: &Expr, rhs: &Expr, origin: &Option<Origin>) -> Option<Expr> {
    let l = numeric_lit(lhs)?;
    let r = numeric_lit(rhs)?;
    let value = match (l, r) {
        (NumberLit::Int(l), NumberLit::Int(r)) => match op {
            BinOp::Eq => l == r,
            BinOp::Ne => l != r,
            BinOp::Lt => l < r,
            BinOp::Le => l <= r,
            BinOp::Gt => l > r,
            BinOp::Ge => l >= r,
            _ => return None,
        },
        (l, r) => {
            let l = l.as_f64();
            let r = r.as_f64();
            match op {
                BinOp::Eq => l == r,
                BinOp::Ne => l != r,
                BinOp::Lt => l < r,
                BinOp::Le => l <= r,
                BinOp::Gt => l > r,
                BinOp::Ge => l >= r,
                _ => return None,
            }
        }
    };
    Some(inherit_origin(Expr::bool_lit(value), origin))
}

// -- dead code elimination --------------------------------------------------

fn is_numeric_zero(expr: &Expr) -> bool {
    match lit_value(expr) {
        Some(Lit::Int(0)) => true,
        Some(Lit::Float(v)) if *v == 0.0 => true,
        _ => false,
    }
}

fn is_numeric_one(expr: &Expr) -> bool {
    match lit_value(expr) {
        Some(Lit::Int(1)) => true,
        Some(Lit::Float(v)) if *v == 1.0 => true,
        _ => false,
    }
}

fn is_empty_string(expr: &Expr) -> bool {
    matches!(lit_value(expr), Some(Lit::Str { value, .. }) if value.is_empty())
}

pub(super) fn dce_expr(expr: &Expr) -> Expr {
    let origin = expr.origin.clone();
    match &expr.kind {
        ExprKind::Lit { .. } | ExprKind::Ident { .. } | ExprKind::Error { .. } => expr.clone(),
        ExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => inherit_origin(
            Expr::conditional(dce_expr(cond), dce_expr(then_expr), dce_expr(else_expr)),
            &origin,
        ),
        ExprKind::Call { func, args } => inherit_origin(
            Expr::call(dce_expr(func), args.iter().map(dce_expr).collect()),
            &origin,
        ),
        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => inherit_origin(
            Expr::method_call(
                dce_expr(receiver),
                method.clone(),
                args.iter().map(dce_expr).collect(),
            ),
            &origin,
        ),
        ExprKind::Field { object, field } => {
            inherit_origin(Expr::field(dce_expr(object), field.clone()), &origin)
        }
        ExprKind::Index { object, index } => {
            inherit_origin(Expr::index(dce_expr(object), dce_expr(index)), &origin)
        }
        ExprKind::BinOp { op, lhs, rhs } => {
            let lhs = dce_expr(lhs);
            let rhs = dce_expr(rhs);
            if let Some(simplified) = dce_bin_op(*op, &lhs, &rhs, &origin) {
                return simplified;
            }
            inherit_origin(Expr::bin_op(*op, lhs, rhs), &origin)
        }
        ExprKind::UnaryOp { op, operand } => {
            let operand = dce_expr(operand);
            if let Some(simplified) = dce_unary_op(*op, &operand, &origin) {
                return simplified;
            }
            inherit_origin(Expr::unary_op(*op, operand), &origin)
        }
        ExprKind::Lambda { params, body } => {
            inherit_origin(Expr::lambda(params.clone(), dce_expr(body)), &origin)
        }
        ExprKind::Array { elements } => inherit_origin(
            Expr::array(elements.iter().map(dce_expr).collect()),
            &origin,
        ),
        ExprKind::Record { entries } => inherit_origin(
            Expr::record(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), dce_expr(value)))
                    .collect(),
            ),
            &origin,
        ),
        ExprKind::Block { bindings, result } => inherit_origin(
            Expr::block(
                bindings
                    .iter()
                    .map(|(name, value)| (name.clone(), dce_expr(value)))
                    .collect(),
                dce_expr(result),
            ),
            &origin,
        ),
        ExprKind::DelayRef { slot, default } => {
            inherit_origin(Expr::delay_ref(slot.clone(), dce_expr(default)), &origin)
        }
        ExprKind::DomainConvert { kind, input } => inherit_origin(
            Expr::domain_convert(*kind, dce_expr(input)),
            &origin,
        ),
    }
}

fn dce_bin_op(op: BinOp, lhs: &Expr, rhs: &Expr, origin: &Option<Origin>) -> Option<Expr> {
    // NOTE: Absorbing-element rewrites assume expressions are pure. If Tessera
    // ever grows impure pieces, gate these rules on an explicit purity check.
    //
    // `fold_constants` already handles many literal-on-the-left cases, but DCE
    // intentionally repeats the identity/absorbing rules so this pass remains
    // correct and useful when run standalone.
    match op {
        BinOp::Add => {
            if is_numeric_zero(rhs) {
                return Some(backfill_origin(lhs.clone(), origin));
            }
            if is_numeric_zero(lhs) {
                return Some(backfill_origin(rhs.clone(), origin));
            }
            None
        }
        BinOp::Sub => {
            if is_numeric_zero(rhs) {
                return Some(backfill_origin(lhs.clone(), origin));
            }
            None
        }
        BinOp::Mul => {
            if is_numeric_zero(lhs) || is_numeric_zero(rhs) {
                return Some(inherit_origin(Expr::int(0), origin));
            }
            if is_numeric_one(rhs) {
                return Some(backfill_origin(lhs.clone(), origin));
            }
            if is_numeric_one(lhs) {
                return Some(backfill_origin(rhs.clone(), origin));
            }
            None
        }
        BinOp::Div => {
            if is_numeric_one(rhs) {
                return Some(backfill_origin(lhs.clone(), origin));
            }
            None
        }
        BinOp::Concat => {
            if is_empty_string(rhs) {
                return Some(backfill_origin(lhs.clone(), origin));
            }
            if is_empty_string(lhs) {
                return Some(backfill_origin(rhs.clone(), origin));
            }
            None
        }
        BinOp::And => {
            if bool_lit_value(rhs) == Some(false) {
                return Some(inherit_origin(Expr::bool_lit(false), origin));
            }
            if bool_lit_value(rhs) == Some(true) {
                return Some(backfill_origin(lhs.clone(), origin));
            }
            if bool_lit_value(lhs) == Some(true) {
                return Some(backfill_origin(rhs.clone(), origin));
            }
            None
        }
        BinOp::Or => {
            if bool_lit_value(rhs) == Some(true) {
                return Some(inherit_origin(Expr::bool_lit(true), origin));
            }
            if bool_lit_value(rhs) == Some(false) {
                return Some(backfill_origin(lhs.clone(), origin));
            }
            if bool_lit_value(lhs) == Some(false) {
                return Some(backfill_origin(rhs.clone(), origin));
            }
            None
        }
        _ => None,
    }
}

fn dce_unary_op(op: UnaryOp, operand: &Expr, origin: &Option<Origin>) -> Option<Expr> {
    if let ExprKind::UnaryOp {
        op: inner_op,
        operand: inner,
    } = &operand.kind
        && *inner_op == op
    {
        return Some(backfill_origin(inner.as_ref().clone(), origin));
    }
    None
}

// -- conditional simplification ---------------------------------------------

pub(super) fn simplify_conditionals_expr(expr: &Expr) -> Expr {
    let origin = expr.origin.clone();
    match &expr.kind {
        ExprKind::Lit { .. } | ExprKind::Ident { .. } | ExprKind::Error { .. } => expr.clone(),
        ExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => {
            let cond = simplify_conditionals_expr(cond);
            let then_expr = simplify_conditionals_expr(then_expr);
            let else_expr = simplify_conditionals_expr(else_expr);
            // cond ? x : x  →  x
            if exprs_structurally_equal(&then_expr, &else_expr) {
                return backfill_origin(then_expr, &origin);
            }
            inherit_origin(Expr::conditional(cond, then_expr, else_expr), &origin)
        }
        ExprKind::Call { func, args } => inherit_origin(
            Expr::call(
                simplify_conditionals_expr(func),
                args.iter().map(simplify_conditionals_expr).collect(),
            ),
            &origin,
        ),
        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => inherit_origin(
            Expr::method_call(
                simplify_conditionals_expr(receiver),
                method.clone(),
                args.iter().map(simplify_conditionals_expr).collect(),
            ),
            &origin,
        ),
        ExprKind::Field { object, field } => inherit_origin(
            Expr::field(simplify_conditionals_expr(object), field.clone()),
            &origin,
        ),
        ExprKind::Index { object, index } => inherit_origin(
            Expr::index(
                simplify_conditionals_expr(object),
                simplify_conditionals_expr(index),
            ),
            &origin,
        ),
        ExprKind::BinOp { op, lhs, rhs } => inherit_origin(
            Expr::bin_op(
                *op,
                simplify_conditionals_expr(lhs),
                simplify_conditionals_expr(rhs),
            ),
            &origin,
        ),
        ExprKind::UnaryOp { op, operand } => inherit_origin(
            Expr::unary_op(*op, simplify_conditionals_expr(operand)),
            &origin,
        ),
        ExprKind::Lambda { params, body } => inherit_origin(
            Expr::lambda(params.clone(), simplify_conditionals_expr(body)),
            &origin,
        ),
        ExprKind::Array { elements } => inherit_origin(
            Expr::array(elements.iter().map(simplify_conditionals_expr).collect()),
            &origin,
        ),
        ExprKind::Record { entries } => inherit_origin(
            Expr::record(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), simplify_conditionals_expr(value)))
                    .collect(),
            ),
            &origin,
        ),
        ExprKind::Block { bindings, result } => inherit_origin(
            Expr::block(
                bindings
                    .iter()
                    .map(|(name, value)| (name.clone(), simplify_conditionals_expr(value)))
                    .collect(),
                simplify_conditionals_expr(result),
            ),
            &origin,
        ),
        ExprKind::DelayRef { slot, default } => inherit_origin(
            Expr::delay_ref(slot.clone(), simplify_conditionals_expr(default)),
            &origin,
        ),
        ExprKind::DomainConvert { kind, input } => inherit_origin(
            Expr::domain_convert(*kind, simplify_conditionals_expr(input)),
            &origin,
        ),
    }
}

/// Structural equality ignoring origins — two expressions produce the same
/// runtime value if their `ExprKind` trees are identical.
fn exprs_structurally_equal(a: &Expr, b: &Expr) -> bool {
    match (&a.kind, &b.kind) {
        (ExprKind::Lit { value: va }, ExprKind::Lit { value: vb }) => va == vb,
        (ExprKind::Ident { name: na }, ExprKind::Ident { name: nb }) => na == nb,
        (
            ExprKind::Conditional {
                cond: ca,
                then_expr: ta,
                else_expr: ea,
            },
            ExprKind::Conditional {
                cond: cb,
                then_expr: tb,
                else_expr: eb,
            },
        ) => {
            exprs_structurally_equal(ca, cb)
                && exprs_structurally_equal(ta, tb)
                && exprs_structurally_equal(ea, eb)
        }
        (ExprKind::Call { func: fa, args: aa }, ExprKind::Call { func: fb, args: ab }) => {
            exprs_structurally_equal(fa, fb)
                && aa.len() == ab.len()
                && aa
                    .iter()
                    .zip(ab)
                    .all(|(x, y)| exprs_structurally_equal(x, y))
        }
        (
            ExprKind::MethodCall {
                receiver: ra,
                method: ma,
                args: aa,
            },
            ExprKind::MethodCall {
                receiver: rb,
                method: mb,
                args: ab,
            },
        ) => {
            ma == mb
                && exprs_structurally_equal(ra, rb)
                && aa.len() == ab.len()
                && aa
                    .iter()
                    .zip(ab)
                    .all(|(x, y)| exprs_structurally_equal(x, y))
        }
        (
            ExprKind::Field {
                object: oa,
                field: fa,
            },
            ExprKind::Field {
                object: ob,
                field: fb,
            },
        ) => fa == fb && exprs_structurally_equal(oa, ob),
        (
            ExprKind::Index {
                object: oa,
                index: ia,
            },
            ExprKind::Index {
                object: ob,
                index: ib,
            },
        ) => exprs_structurally_equal(oa, ob) && exprs_structurally_equal(ia, ib),
        (
            ExprKind::BinOp {
                op: opa,
                lhs: la,
                rhs: ra,
            },
            ExprKind::BinOp {
                op: opb,
                lhs: lb,
                rhs: rb,
            },
        ) => opa == opb && exprs_structurally_equal(la, lb) && exprs_structurally_equal(ra, rb),
        (
            ExprKind::UnaryOp {
                op: opa,
                operand: oa,
            },
            ExprKind::UnaryOp {
                op: opb,
                operand: ob,
            },
        ) => opa == opb && exprs_structurally_equal(oa, ob),
        (
            ExprKind::Lambda {
                params: pa,
                body: ba,
            },
            ExprKind::Lambda {
                params: pb,
                body: bb,
            },
        ) => pa == pb && exprs_structurally_equal(ba, bb),
        (ExprKind::Array { elements: ea }, ExprKind::Array { elements: eb }) => {
            ea.len() == eb.len()
                && ea
                    .iter()
                    .zip(eb)
                    .all(|(x, y)| exprs_structurally_equal(x, y))
        }
        (ExprKind::Record { entries: ea }, ExprKind::Record { entries: eb }) => {
            ea.len() == eb.len()
                && ea
                    .iter()
                    .zip(eb)
                    .all(|((ka, va), (kb, vb))| ka == kb && exprs_structurally_equal(va, vb))
        }
        (
            ExprKind::Block {
                bindings: ba,
                result: ra,
            },
            ExprKind::Block {
                bindings: bb,
                result: rb,
            },
        ) => {
            ba.len() == bb.len()
                && ba
                    .iter()
                    .zip(bb)
                    .all(|((na, va), (nb, vb))| na == nb && exprs_structurally_equal(va, vb))
                && exprs_structurally_equal(ra, rb)
        }
        (
            ExprKind::DelayRef {
                slot: sa,
                default: da,
            },
            ExprKind::DelayRef {
                slot: sb,
                default: db,
            },
        ) => sa == sb && exprs_structurally_equal(da, db),
        (
            ExprKind::DomainConvert { kind: ka, input: ea },
            ExprKind::DomainConvert { kind: kb, input: eb },
        ) => ka == kb && exprs_structurally_equal(ea, eb),
        (ExprKind::Error { message: ma }, ExprKind::Error { message: mb }) => ma == mb,
        _ => false,
    }
}

// -- common sub-expression elimination (CSE) --------------------------------

/// Compute a structural fingerprint for an expression, ignoring origins.
/// Two expressions with the same fingerprint are structurally equal.
fn expr_fingerprint(expr: &Expr) -> String {
    let mut buf = String::new();
    write_fingerprint(expr, &mut buf);
    buf
}

fn write_fingerprint(expr: &Expr, buf: &mut String) {
    use std::fmt::Write;
    match &expr.kind {
        ExprKind::Lit { value } => match value {
            Lit::Nil => buf.push_str("nil"),
            Lit::Bool(v) => write!(buf, "b:{v}").unwrap(),
            Lit::Int(v) => write!(buf, "i:{v}").unwrap(),
            Lit::Float(v) => write!(buf, "f:{v}").unwrap(),
            Lit::Str { value, syntax } => write!(buf, "s:{syntax:?}:{value}").unwrap(),
        },
        ExprKind::Ident { name } => write!(buf, "id:{name}").unwrap(),
        ExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => {
            buf.push_str("if(");
            write_fingerprint(cond, buf);
            buf.push(',');
            write_fingerprint(then_expr, buf);
            buf.push(',');
            write_fingerprint(else_expr, buf);
            buf.push(')');
        }
        ExprKind::Call { func, args } => {
            buf.push_str("call(");
            write_fingerprint(func, buf);
            for a in args {
                buf.push(',');
                write_fingerprint(a, buf);
            }
            buf.push(')');
        }
        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => {
            buf.push_str("mcall(");
            write_fingerprint(receiver, buf);
            write!(buf, ".{method}").unwrap();
            for a in args {
                buf.push(',');
                write_fingerprint(a, buf);
            }
            buf.push(')');
        }
        ExprKind::Field { object, field } => {
            buf.push_str("fld(");
            write_fingerprint(object, buf);
            write!(buf, ".{field})").unwrap();
        }
        ExprKind::Index { object, index } => {
            buf.push_str("idx(");
            write_fingerprint(object, buf);
            buf.push(',');
            write_fingerprint(index, buf);
            buf.push(')');
        }
        ExprKind::BinOp { op, lhs, rhs } => {
            write!(buf, "bin:{op:?}(").unwrap();
            write_fingerprint(lhs, buf);
            buf.push(',');
            write_fingerprint(rhs, buf);
            buf.push(')');
        }
        ExprKind::UnaryOp { op, operand } => {
            write!(buf, "un:{op:?}(").unwrap();
            write_fingerprint(operand, buf);
            buf.push(')');
        }
        ExprKind::Lambda { params, body } => {
            buf.push_str("lam(");
            buf.push_str(&params.join(","));
            buf.push(',');
            write_fingerprint(body, buf);
            buf.push(')');
        }
        ExprKind::Array { elements } => {
            buf.push_str("arr(");
            for (i, e) in elements.iter().enumerate() {
                if i > 0 {
                    buf.push(',');
                }
                write_fingerprint(e, buf);
            }
            buf.push(')');
        }
        ExprKind::Record { entries } => {
            buf.push_str("rec(");
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    buf.push(',');
                }
                write!(buf, "{k}:").unwrap();
                write_fingerprint(v, buf);
            }
            buf.push(')');
        }
        ExprKind::Block { bindings, result } => {
            buf.push_str("blk(");
            for (i, (n, v)) in bindings.iter().enumerate() {
                if i > 0 {
                    buf.push(',');
                }
                write!(buf, "{n}=").unwrap();
                write_fingerprint(v, buf);
            }
            buf.push(',');
            write_fingerprint(result, buf);
            buf.push(')');
        }
        ExprKind::DelayRef { slot, default } => {
            write!(buf, "delay:{slot}(").unwrap();
            write_fingerprint(default, buf);
            buf.push(')');
        }
        ExprKind::DomainConvert { kind, input } => {
            write!(buf, "bridge:{}(", kind.as_str()).unwrap();
            write_fingerprint(input, buf);
            buf.push(')');
        }
        ExprKind::Error { message } => write!(buf, "err:{message}").unwrap(),
    }
}

/// Returns true if `expr` is "trivial" — an ident or literal that should
/// never be hoisted into a let-binding.
fn is_trivial(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::Lit { .. } | ExprKind::Ident { .. } | ExprKind::Error { .. }
    )
}

/// Walk `expr` and count how many times each non-trivial sub-expression
/// fingerprint appears.
fn count_subexprs(expr: &Expr, counts: &mut HashMap<String, (usize, Expr)>) {
    if is_trivial(expr) {
        return;
    }
    let fp = expr_fingerprint(expr);
    let entry = counts.entry(fp).or_insert_with(|| (0, expr.clone()));
    entry.0 += 1;
    // recurse into children
    match &expr.kind {
        ExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => {
            count_subexprs(cond, counts);
            count_subexprs(then_expr, counts);
            count_subexprs(else_expr, counts);
        }
        ExprKind::Call { func, args } => {
            count_subexprs(func, counts);
            for a in args {
                count_subexprs(a, counts);
            }
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            count_subexprs(receiver, counts);
            for a in args {
                count_subexprs(a, counts);
            }
        }
        ExprKind::Field { object, .. } => count_subexprs(object, counts),
        ExprKind::Index { object, index } => {
            count_subexprs(object, counts);
            count_subexprs(index, counts);
        }
        ExprKind::BinOp { lhs, rhs, .. } => {
            count_subexprs(lhs, counts);
            count_subexprs(rhs, counts);
        }
        ExprKind::UnaryOp { operand, .. } => count_subexprs(operand, counts),
        ExprKind::Lambda { body, .. } => count_subexprs(body, counts),
        ExprKind::Array { elements } => {
            for e in elements {
                count_subexprs(e, counts);
            }
        }
        ExprKind::Record { entries } => {
            for (_, v) in entries {
                count_subexprs(v, counts);
            }
        }
        ExprKind::Block { bindings, result } => {
            for (_, v) in bindings {
                count_subexprs(v, counts);
            }
            count_subexprs(result, counts);
        }
        ExprKind::DelayRef { default, .. } => count_subexprs(default, counts),
        ExprKind::DomainConvert { input, .. } => count_subexprs(input, counts),
        ExprKind::Lit { .. } | ExprKind::Ident { .. } | ExprKind::Error { .. } => {}
    }
}

fn replace_subexpr_descendants(expr: &Expr, replacements: &HashMap<String, String>) -> Expr {
    let origin = expr.origin.clone();
    let replaced = match &expr.kind {
        ExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => Expr::conditional(
            replace_subexprs(cond, replacements),
            replace_subexprs(then_expr, replacements),
            replace_subexprs(else_expr, replacements),
        ),
        ExprKind::Call { func, args } => Expr::call(
            replace_subexprs(func, replacements),
            args.iter()
                .map(|a| replace_subexprs(a, replacements))
                .collect(),
        ),
        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => Expr::method_call(
            replace_subexprs(receiver, replacements),
            method.clone(),
            args.iter()
                .map(|a| replace_subexprs(a, replacements))
                .collect(),
        ),
        ExprKind::Field { object, field } => {
            Expr::field(replace_subexprs(object, replacements), field.clone())
        }
        ExprKind::Index { object, index } => Expr::index(
            replace_subexprs(object, replacements),
            replace_subexprs(index, replacements),
        ),
        ExprKind::BinOp { op, lhs, rhs } => Expr::bin_op(
            *op,
            replace_subexprs(lhs, replacements),
            replace_subexprs(rhs, replacements),
        ),
        ExprKind::UnaryOp { op, operand } => {
            Expr::unary_op(*op, replace_subexprs(operand, replacements))
        }
        ExprKind::Lambda { params, body } => {
            Expr::lambda(params.clone(), replace_subexprs(body, replacements))
        }
        ExprKind::Array { elements } => Expr::array(
            elements
                .iter()
                .map(|e| replace_subexprs(e, replacements))
                .collect(),
        ),
        ExprKind::Record { entries } => Expr::record(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), replace_subexprs(v, replacements)))
                .collect(),
        ),
        ExprKind::Block { bindings, result } => Expr::block(
            bindings
                .iter()
                .map(|(n, v)| (n.clone(), replace_subexprs(v, replacements)))
                .collect(),
            replace_subexprs(result, replacements),
        ),
        ExprKind::DelayRef { slot, default } => {
            Expr::delay_ref(slot.clone(), replace_subexprs(default, replacements))
        }
        ExprKind::DomainConvert { kind, input } => {
            Expr::domain_convert(*kind, replace_subexprs(input, replacements))
        }
        ExprKind::Lit { .. } | ExprKind::Ident { .. } | ExprKind::Error { .. } => expr.clone(),
    };
    inherit_origin(replaced, &origin)
}

/// Replace all occurrences of hoisted sub-expressions with their bound
/// identifier.
fn replace_subexprs(expr: &Expr, replacements: &HashMap<String, String>) -> Expr {
    if is_trivial(expr) {
        return expr.clone();
    }
    let fp = expr_fingerprint(expr);
    if let Some(ident_name) = replacements.get(&fp) {
        return backfill_origin(Expr::ident(ident_name), &expr.origin);
    }
    replace_subexpr_descendants(expr, replacements)
}

/// Hoist common sub-expressions shared across terminal expressions into
/// `Block` let-bindings.
///
/// For each terminal, sub-expressions that appear 2+ times across all
/// terminals (combined) are extracted into `_t0`, `_t1`, … bindings.
/// Each terminal is wrapped in a `Block` if it has any hoistable references.
pub(super) fn hoist_common_subexprs(terminals: &[Expr]) -> Vec<Expr> {
    if terminals.is_empty() {
        return vec![];
    }

    // 1. Count all sub-expression occurrences across all terminals.
    let mut counts: HashMap<String, (usize, Expr)> = HashMap::new();
    for t in terminals {
        count_subexprs(t, &mut counts);
    }

    // 2. Collect fingerprints that appear ≥2 times, sorted by fingerprint
    //    length descending (largest/most complex first) so that we hoist
    //    the outermost duplicate first.
    let mut hoistable: Vec<(String, Expr)> = counts
        .into_iter()
        .filter(|(_, (count, _))| *count >= 2)
        .map(|(fp, (_, expr))| (fp, expr))
        .collect();

    if hoistable.is_empty() {
        return terminals.to_vec();
    }

    // Sort by fingerprint length descending — longer fingerprints represent
    // larger sub-trees, which should be hoisted first.
    hoistable.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    // 3. Assign names and build replacement map.
    let mut replacements: HashMap<String, String> = HashMap::new();
    let mut bindings: Vec<(String, Expr)> = Vec::new();

    for (i, (fp, expr)) in hoistable.into_iter().enumerate() {
        let name = format!("_t{i}");
        replacements.insert(fp, name.clone());
        bindings.push((name, expr));
    }

    // 4. Replace sub-expressions in binding values themselves (inner CSE).
    let bindings: Vec<(String, Expr)> = bindings
        .into_iter()
        .map(|(name, expr)| {
            let expr_fingerprint = expr_fingerprint(&expr);
            let replaced = if replacements
                .get(&expr_fingerprint)
                .is_some_and(|bound_name| bound_name == &name)
            {
                replace_subexpr_descendants(&expr, &replacements)
            } else {
                replace_subexprs(&expr, &replacements)
            };
            (name, replaced)
        })
        .collect();

    // 5. Replace sub-expressions in each terminal and wrap in Block.
    terminals
        .iter()
        .map(|terminal| {
            let replaced = replace_subexprs(terminal, &replacements);
            let origin = terminal.origin.clone();
            inherit_origin(Expr::block(bindings.clone(), replaced), &origin)
        })
        .collect()
}
