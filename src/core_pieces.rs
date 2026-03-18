//! Built-in core expression pieces.
//!
//! These pieces are target-agnostic symbols that lower into the shared `Expr`
//! AST. Hosts opt into them by registering the returned pieces in their own
//! registries.

use std::collections::BTreeMap;

use serde_json::Value;
use serde_json::json;

use crate::ast::{BinOp, Expr, UnaryOp};
use crate::piece::{
    ParamDef, ParamInlineMode, ParamSchema, ParamTextSemantics, ParamValueKind, Piece, PieceDef,
    PieceInputs,
};
use crate::types::{PieceCategory, PieceSemanticKind, PortType, TileSide};

/// Piece id for the built-in delay (feedback) piece.
///
/// The semantic pass and compiler use this to identify delay edges and
/// assign frame-buffer slots.
pub const DELAY_PIECE_ID: &str = "core.delay";

pub fn core_expression_pieces() -> Vec<Box<dyn Piece>> {
    vec![
        Box::new(IfExprPiece::new()),
        Box::new(DelayPiece::new()),
        Box::new(UnaryExprPiece::new(
            "core.not",
            "not",
            UnaryOp::Not,
            PortType::bool(),
            PortType::bool(),
            vec!["logic".into(), "boolean".into()],
        )),
        Box::new(BinaryExprPiece::new(
            "core.and",
            "and",
            BinOp::And,
            bool_param("lhs"),
            bool_param("rhs"),
            vec!["logic".into(), "boolean".into()],
        )),
        Box::new(BinaryExprPiece::new(
            "core.or",
            "or",
            BinOp::Or,
            bool_param("lhs"),
            bool_param("rhs"),
            vec!["logic".into(), "boolean".into()],
        )),
        Box::new(BinaryExprPiece::new(
            "core.eq",
            "eq",
            BinOp::Eq,
            any_param("lhs"),
            any_param("rhs"),
            vec!["comparison".into()],
        )),
        Box::new(BinaryExprPiece::new(
            "core.gt",
            "gt",
            BinOp::Gt,
            number_param("lhs"),
            number_param("rhs"),
            vec!["comparison".into(), "math".into()],
        )),
        Box::new(BinaryExprPiece::new(
            "core.lt",
            "lt",
            BinOp::Lt,
            number_param("lhs"),
            number_param("rhs"),
            vec!["comparison".into(), "math".into()],
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
    tags: Vec<String>,
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
        tags,
    }
}

fn bool_param(id: &str) -> ParamDef {
    ParamDef {
        id: id.into(),
        label: id.into(),
        side: TileSide::LEFT,
        schema: ParamSchema::Custom {
            port_type: PortType::bool().with_unspecified_domain(),
            value_kind: ParamValueKind::Bool,
            default: Some(json!(false)),
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

fn number_param(id: &str) -> ParamDef {
    ParamDef {
        id: id.into(),
        label: id.into(),
        side: TileSide::LEFT,
        schema: ParamSchema::Custom {
            port_type: PortType::number().with_unspecified_domain(),
            value_kind: ParamValueKind::Number,
            default: Some(json!(0.0)),
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

fn any_param(id: &str) -> ParamDef {
    ParamDef {
        id: id.into(),
        label: id.into(),
        side: TileSide::LEFT,
        schema: ParamSchema::Custom {
            port_type: PortType::any().with_unspecified_domain(),
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
                PortType::any().with_unspecified_domain(),
                vec!["control".into(), "conditional".into()],
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

    fn infer_output_type(
        &self,
        input_types: &BTreeMap<String, PortType>,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Option<PortType> {
        let fallback = self
            .def
            .output_type
            .clone()
            .unwrap_or_else(|| PortType::any().with_unspecified_domain());

        match (input_types.get("then"), input_types.get("else")) {
            (Some(then_type), Some(else_type)) => then_type
                .common_type(else_type)
                .or_else(|| Some(fallback.clone())),
            (Some(then_type), None) => Some(then_type.clone()),
            (None, Some(else_type)) => Some(else_type.clone()),
            (None, None) => Some(fallback),
        }
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
        tags: Vec<String>,
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
                            port_type: input_type.clone().with_unspecified_domain(),
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
                tags,
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

    fn infer_output_type(
        &self,
        input_types: &BTreeMap<String, PortType>,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Option<PortType> {
        let base = self
            .def
            .output_type
            .clone()
            .unwrap_or_else(|| PortType::any().with_unspecified_domain());
        match input_types.get("value").and_then(PortType::domain) {
            Some(domain) => Some(base.with_domain(domain)),
            None => Some(base.with_unspecified_domain()),
        }
    }
}

struct BinaryExprPiece {
    def: PieceDef,
    op: BinOp,
}

impl BinaryExprPiece {
    fn new(
        id: &str,
        label: &str,
        op: BinOp,
        lhs: ParamDef,
        rhs: ParamDef,
        tags: Vec<String>,
    ) -> Self {
        Self {
            def: base_def(
                id,
                label,
                PieceCategory::Transform,
                PieceSemanticKind::Operator,
                vec![lhs, rhs],
                PortType::bool(),
                tags,
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

    fn infer_output_type(
        &self,
        input_types: &BTreeMap<String, PortType>,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Option<PortType> {
        let base = self
            .def
            .output_type
            .clone()
            .unwrap_or_else(|| PortType::bool().with_unspecified_domain());
        let domain_source = match (input_types.get("lhs"), input_types.get("rhs")) {
            (Some(lhs), Some(rhs)) => lhs.common_type(rhs),
            (Some(lhs), None) => Some(lhs.clone()),
            (None, Some(rhs)) => Some(rhs.clone()),
            (None, None) => None,
        };

        match domain_source.and_then(|port_type| port_type.domain()) {
            Some(domain) => Some(base.with_domain(domain)),
            None => Some(base.with_unspecified_domain()),
        }
    }
}

// ---------------------------------------------------------------------------
// Delay (feedback) piece
// ---------------------------------------------------------------------------

struct DelayPiece {
    def: PieceDef,
}

impl DelayPiece {
    fn new() -> Self {
        Self {
            def: base_def(
                DELAY_PIECE_ID,
                "delay",
                PieceCategory::Control,
                PieceSemanticKind::Construct,
                vec![
                    // The signal to feed back — structural only (not required at
                    // compile time because the source may not be compiled yet).
                    ParamDef {
                        id: "value".into(),
                        label: "value".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Custom {
                            port_type: PortType::any().with_unspecified_domain(),
                            value_kind: ParamValueKind::Json,
                            default: None,
                            can_inline: false,
                            inline_mode: ParamInlineMode::Literal,
                            min: None,
                            max: None,
                        },
                        text_semantics: ParamTextSemantics::Plain,
                        variadic_group: None,
                        required: false,
                    },
                    // Initial value for frame 0 before any history exists.
                    ParamDef {
                        id: "default".into(),
                        label: "default".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Custom {
                            port_type: PortType::any().with_unspecified_domain(),
                            value_kind: ParamValueKind::Json,
                            default: None,
                            can_inline: true,
                            inline_mode: ParamInlineMode::Literal,
                            min: None,
                            max: None,
                        },
                        text_semantics: ParamTextSemantics::Plain,
                        variadic_group: None,
                        required: false,
                    },
                ],
                PortType::any().with_unspecified_domain(),
                vec!["feedback".into(), "delay".into(), "control".into()],
            ),
        }
    }
}

impl Piece for DelayPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
        let default = inputs.get("default").cloned().unwrap_or_else(Expr::nil);
        // Emit a DelayRef with an empty slot — the compiler fills in the
        // actual slot name based on the node's GridPos.
        Expr::delay_ref("", default)
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
    fn if_expr_inference_keeps_unspecified_domain_when_branch_domains_conflict() {
        let piece = find_piece("core.if_expr");
        let inferred = piece
            .infer_output_type(
                &BTreeMap::from([
                    (
                        "then".into(),
                        PortType::number().with_domain(crate::types::ExecutionDomain::Audio),
                    ),
                    (
                        "else".into(),
                        PortType::number().with_domain(crate::types::ExecutionDomain::Control),
                    ),
                ]),
                &BTreeMap::new(),
            )
            .expect("if expr output type");

        assert_eq!(inferred, PortType::any().with_unspecified_domain());
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

    #[test]
    fn delay_piece_metadata() {
        let piece = find_piece("core.delay");
        let def = piece.def();
        assert_eq!(def.id, "core.delay");
        assert_eq!(def.category, PieceCategory::Control);
        assert_eq!(def.semantic_kind, PieceSemanticKind::Construct);
        assert!(def.output_type.is_some());
        assert_eq!(def.params.len(), 2);
        assert_eq!(def.params[0].id, "value");
        assert!(!def.params[0].required);
        assert_eq!(def.params[1].id, "default");
        assert!(!def.params[1].required);
    }

    #[test]
    fn delay_piece_compiles_to_delay_ref_with_empty_slot() {
        let piece = find_piece("core.delay");
        let mut inputs = PieceInputs::default();
        inputs.scalar.insert("default".into(), Expr::int(0));

        let expr = piece.compile(&inputs, &BTreeMap::new());
        assert!(matches!(
            &expr.kind,
            ExprKind::DelayRef { slot, .. } if slot.is_empty()
        ));
        assert_eq!(JsBackend.render(&expr), "__delay('', 0)");
    }

    #[test]
    fn delay_piece_uses_nil_when_default_not_provided() {
        let piece = find_piece("core.delay");
        let inputs = PieceInputs::default();
        let expr = piece.compile(&inputs, &BTreeMap::new());
        assert!(matches!(
            &expr.kind,
            ExprKind::DelayRef { default, .. }
                if matches!(default.kind, ExprKind::Lit { ref value } if *value == crate::ast::Lit::Nil)
        ));
    }
}
