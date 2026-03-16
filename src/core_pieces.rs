//! Built-in core expression pieces.
//!
//! These pieces are target-agnostic symbols that lower into the shared `Expr`
//! AST. Hosts opt into them by registering the returned pieces in their own
//! registries.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::ast::{BinOp, Expr, UnaryOp};
use crate::piece::{
    ParamDef, ParamInlineMode, ParamSchema, ParamTextSemantics, ParamValueKind, Piece, PieceDef,
    PieceInputs,
};
use crate::types::{PieceCategory, PieceSemanticKind, PortType, TileSide};

pub fn core_expression_pieces() -> Vec<Box<dyn Piece>> {
    vec![
        Box::new(IfExprPiece::new()),
        Box::new(UnaryExprPiece::new(
            "core.not",
            "not",
            UnaryOp::Not,
            PortType::bool(),
            PortType::bool(),
        )),
        Box::new(BinaryExprPiece::new(
            "core.and",
            "and",
            BinOp::And,
            bool_param("lhs"),
            bool_param("rhs"),
        )),
        Box::new(BinaryExprPiece::new(
            "core.or",
            "or",
            BinOp::Or,
            bool_param("lhs"),
            bool_param("rhs"),
        )),
        Box::new(BinaryExprPiece::new(
            "core.eq",
            "eq",
            BinOp::Eq,
            any_param("lhs"),
            any_param("rhs"),
        )),
        Box::new(BinaryExprPiece::new(
            "core.gt",
            "gt",
            BinOp::Gt,
            number_param("lhs"),
            number_param("rhs"),
        )),
        Box::new(BinaryExprPiece::new(
            "core.lt",
            "lt",
            BinOp::Lt,
            number_param("lhs"),
            number_param("rhs"),
        )),
    ]
}

fn base_def(
    id: &str,
    label: &str,
    category: PieceCategory,
    semantic_kind: PieceSemanticKind,
    params: Vec<ParamDef>,
    output_type: PortType,
) -> PieceDef {
    PieceDef {
        id: id.into(),
        label: label.into(),
        category,
        semantic_kind,
        namespace: "core".into(),
        params,
        output_type: Some(output_type),
        output_side: Some(TileSide::RIGHT),
        description: None,
    }
}

fn bool_param(id: &str) -> ParamDef {
    ParamDef {
        id: id.into(),
        label: id.into(),
        side: TileSide::LEFT,
        schema: ParamSchema::Bool {
            default: false,
            can_inline: true,
        },
        text_semantics: ParamTextSemantics::Plain,
        variadic_group: None,
        required: true,
    }
}

fn number_param(id: &str) -> ParamDef {
    ParamDef {
        id: id.into(),
        label: id.into(),
        side: TileSide::LEFT,
        schema: ParamSchema::Number {
            default: 0.0,
            min: None,
            max: None,
            can_inline: true,
        },
        text_semantics: ParamTextSemantics::Plain,
        variadic_group: None,
        required: true,
    }
}

fn any_param(id: &str) -> ParamDef {
    ParamDef {
        id: id.into(),
        label: id.into(),
        side: TileSide::LEFT,
        schema: ParamSchema::Custom {
            port_type: PortType::any(),
            value_kind: ParamValueKind::Json,
            default: None,
            can_inline: true,
            inline_mode: ParamInlineMode::Literal,
            min: None,
            max: None,
        },
        text_semantics: ParamTextSemantics::Plain,
        variadic_group: None,
        required: true,
    }
}

fn required_input(inputs: &PieceInputs, key: &str) -> Expr {
    inputs
        .get(key)
        .cloned()
        .unwrap_or_else(|| Expr::error(format!("missing {key}")))
}

struct IfExprPiece {
    def: PieceDef,
}

impl IfExprPiece {
    fn new() -> Self {
        Self {
            def: base_def(
                "core.if_expr",
                "if expr",
                PieceCategory::Control,
                PieceSemanticKind::Construct,
                vec![bool_param("cond"), any_param("then"), any_param("else")],
                PortType::any(),
            ),
        }
    }
}

impl Piece for IfExprPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
        Expr::conditional(
            required_input(inputs, "cond"),
            required_input(inputs, "then"),
            required_input(inputs, "else"),
        )
    }
}

struct UnaryExprPiece {
    def: PieceDef,
    op: UnaryOp,
}

impl UnaryExprPiece {
    fn new(
        id: &str,
        label: &str,
        op: UnaryOp,
        input_type: PortType,
        output_type: PortType,
    ) -> Self {
        Self {
            def: base_def(
                id,
                label,
                PieceCategory::Transform,
                PieceSemanticKind::Operator,
                vec![ParamDef {
                    id: "value".into(),
                    label: "value".into(),
                    side: TileSide::LEFT,
                    schema: match input_type.as_str() {
                        "bool" => ParamSchema::Bool {
                            default: false,
                            can_inline: true,
                        },
                        "number" => ParamSchema::Number {
                            default: 0.0,
                            min: None,
                            max: None,
                            can_inline: true,
                        },
                        _ => ParamSchema::Custom {
                            port_type: input_type.clone(),
                            value_kind: ParamValueKind::Json,
                            default: None,
                            can_inline: true,
                            inline_mode: ParamInlineMode::Literal,
                            min: None,
                            max: None,
                        },
                    },
                    text_semantics: ParamTextSemantics::Plain,
                    variadic_group: None,
                    required: true,
                }],
                output_type,
            ),
            op,
        }
    }
}

impl Piece for UnaryExprPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
        Expr::unary_op(self.op, required_input(inputs, "value"))
    }
}

struct BinaryExprPiece {
    def: PieceDef,
    op: BinOp,
}

impl BinaryExprPiece {
    fn new(id: &str, label: &str, op: BinOp, lhs: ParamDef, rhs: ParamDef) -> Self {
        Self {
            def: base_def(
                id,
                label,
                PieceCategory::Transform,
                PieceSemanticKind::Operator,
                vec![lhs, rhs],
                PortType::bool(),
            ),
            op,
        }
    }
}

impl Piece for BinaryExprPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
        Expr::bin_op(
            self.op,
            required_input(inputs, "lhs"),
            required_input(inputs, "rhs"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ExprKind;
    use crate::backend::{Backend, JsBackend};

    fn find_piece(id: &str) -> Box<dyn Piece> {
        core_expression_pieces()
            .into_iter()
            .find(|piece| piece.def().id == id)
            .expect("piece")
    }

    #[test]
    fn core_expression_piece_metadata_is_target_agnostic() {
        let pieces = core_expression_pieces();
        let defs = pieces.iter().map(|piece| piece.def()).collect::<Vec<_>>();

        assert!(defs.iter().any(|def| {
            def.id == "core.if_expr"
                && def.category == PieceCategory::Control
                && def.semantic_kind == PieceSemanticKind::Construct
                && def.namespace == "core"
        }));
        assert!(defs.iter().any(|def| {
            def.id == "core.not"
                && def.category == PieceCategory::Transform
                && def.semantic_kind == PieceSemanticKind::Operator
                && def.namespace == "core"
        }));
    }

    #[test]
    fn if_expr_compiles_to_conditional_and_renders_in_js() {
        let piece = find_piece("core.if_expr");
        let mut inputs = PieceInputs::default();
        inputs.scalar.insert("cond".into(), Expr::ident("flag"));
        inputs.scalar.insert("then".into(), Expr::int(1));
        inputs.scalar.insert("else".into(), Expr::int(0));

        let expr = piece.compile(&inputs, &BTreeMap::new());
        assert!(matches!(expr.kind, ExprKind::Conditional { .. }));
        assert_eq!(JsBackend.render(&expr), "flag ? 1 : 0");
    }

    #[test]
    fn logical_and_comparison_pieces_compile_to_bin_ops() {
        let cases = [
            ("core.and", BinOp::And, "true && false"),
            ("core.or", BinOp::Or, "true || false"),
            ("core.eq", BinOp::Eq, "1 == 1"),
            ("core.gt", BinOp::Gt, "3 > 2"),
            ("core.lt", BinOp::Lt, "2 < 3"),
        ];

        for (id, op, expected) in cases {
            let piece = find_piece(id);
            let mut inputs = PieceInputs::default();
            inputs.scalar.insert("lhs".into(), Expr::int(1));
            inputs.scalar.insert("rhs".into(), Expr::int(1));
            if matches!(op, BinOp::And | BinOp::Or) {
                inputs.scalar.insert("lhs".into(), Expr::bool_lit(true));
                inputs.scalar.insert("rhs".into(), Expr::bool_lit(false));
            } else if matches!(op, BinOp::Gt) {
                inputs.scalar.insert("lhs".into(), Expr::int(3));
                inputs.scalar.insert("rhs".into(), Expr::int(2));
            } else if matches!(op, BinOp::Lt) {
                inputs.scalar.insert("lhs".into(), Expr::int(2));
                inputs.scalar.insert("rhs".into(), Expr::int(3));
            }

            let expr = piece.compile(&inputs, &BTreeMap::new());
            assert!(matches!(expr.kind, ExprKind::BinOp { op: actual, .. } if actual == op));
            assert_eq!(JsBackend.render(&expr), expected);
        }
    }

    #[test]
    fn not_piece_compiles_to_unary_op() {
        let piece = find_piece("core.not");
        let mut inputs = PieceInputs::default();
        inputs.scalar.insert("value".into(), Expr::ident("flag"));

        let expr = piece.compile(&inputs, &BTreeMap::new());
        assert!(matches!(
            expr.kind,
            ExprKind::UnaryOp {
                op: UnaryOp::Not,
                ..
            }
        ));
        assert_eq!(JsBackend.render(&expr), "!flag");
    }
}
