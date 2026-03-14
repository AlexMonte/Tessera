use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::code_expr::CodeExpr;
use crate::diagnostics::{Diagnostic, DiagnosticKind, SemanticResult};
use crate::graph::Graph;
use crate::piece::PieceInputs;
use crate::piece_registry::PieceRegistry;
use crate::semantic::incoming_edge_for_param;
use crate::types::{EdgeId, GridPos, TileSide};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileMode {
    Preview,
    Runtime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStateUpdate {
    pub position: GridPos,
    pub state: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileProgram {
    pub terminals: Vec<CodeExpr>,
    pub state_updates: Vec<NodeStateUpdate>,
}

#[derive(Debug, Clone)]
struct CompiledNodes {
    compiled: BTreeMap<GridPos, CodeExpr>,
    state_updates: Vec<NodeStateUpdate>,
}

fn error(kind: DiagnosticKind, site: Option<GridPos>, edge_id: Option<EdgeId>) -> Diagnostic {
    let diagnostic = Diagnostic::error(kind, site);
    if let Some(edge_id) = edge_id {
        diagnostic.with_edge(edge_id)
    } else {
        diagnostic
    }
}

pub fn compile_graph(
    graph: &Graph,
    registry: &PieceRegistry,
    sem: &SemanticResult,
    mode: CompileMode,
) -> Result<CompileProgram, Vec<Diagnostic>> {
    let compiled_nodes = compile_nodes(graph, registry, sem, mode, &BTreeMap::new())?;

    let mut terminals = Vec::with_capacity(sem.terminals.len());
    for terminal in &sem.terminals {
        let Some(expr) = compiled_nodes.compiled.get(terminal).cloned() else {
            return Err(vec![error(
                DiagnosticKind::UnknownNode {
                    pos: terminal.clone(),
                },
                Some(terminal.clone()),
                None,
            )]);
        };
        terminals.push(expr);
    }

    Ok(CompileProgram {
        terminals,
        state_updates: compiled_nodes.state_updates,
    })
}

pub fn compile_node_expr(
    graph: &Graph,
    registry: &PieceRegistry,
    sem: &SemanticResult,
    mode: CompileMode,
    root: &GridPos,
    overrides: &BTreeMap<GridPos, CodeExpr>,
) -> Result<(CodeExpr, Vec<NodeStateUpdate>), Vec<Diagnostic>> {
    let compiled_nodes = compile_nodes(graph, registry, sem, mode, overrides)?;
    let Some(expr) = compiled_nodes.compiled.get(root).cloned() else {
        return Err(vec![error(
            DiagnosticKind::UnknownNode { pos: root.clone() },
            Some(root.clone()),
            None,
        )]);
    };

    Ok((expr, compiled_nodes.state_updates))
}

/// Determine which side of `from` faces `to` based on grid adjacency.
fn direction_from_to(from: &GridPos, to: &GridPos) -> TileSide {
    match (to.col - from.col, to.row - from.row) {
        (1, 0) => TileSide::RIGHT,
        (-1, 0) => TileSide::LEFT,
        (0, -1) => TileSide::TOP,
        (0, 1) => TileSide::BOTTOM,
        _ => TileSide::NONE,
    }
}

fn compile_nodes(
    graph: &Graph,
    registry: &PieceRegistry,
    sem: &SemanticResult,
    mode: CompileMode,
    overrides: &BTreeMap<GridPos, CodeExpr>,
) -> Result<CompiledNodes, Vec<Diagnostic>> {
    if !sem.is_valid() {
        return Err(sem.diagnostics.clone());
    }

    let mut compiled = overrides.clone();
    let mut multi_outputs: BTreeMap<(GridPos, TileSide), CodeExpr> = BTreeMap::new();
    let mut state_updates = Vec::<NodeStateUpdate>::new();
    let mut compile_errors = Vec::<Diagnostic>::new();

    for pos in &sem.eval_order {
        if compiled.contains_key(pos) {
            continue;
        }

        let Some(node) = graph.nodes.get(pos) else {
            compile_errors.push(error(
                DiagnosticKind::UnknownNode { pos: pos.clone() },
                Some(pos.clone()),
                None,
            ));
            continue;
        };

        let Some(piece) = registry.get(node.piece_id.as_str()) else {
            compile_errors.push(error(
                DiagnosticKind::UnknownPiece {
                    piece_id: node.piece_id.clone(),
                },
                Some(pos.clone()),
                None,
            ));
            continue;
        };

        let mut inputs = PieceInputs::default();

        for param in &piece.def().params {
            let connected =
                incoming_edge_for_param(graph, pos, param.id.as_str()).and_then(|edge| {
                    let exit_side = direction_from_to(&edge.from, &edge.to_node);
                    multi_outputs
                        .get(&(edge.from.clone(), exit_side))
                        .or_else(|| compiled.get(&edge.from))
                        .cloned()
                });

            let resolved = if let Some(expr) = connected {
                Some(expr)
            } else if let Some(value) = node.inline_params.get(param.id.as_str()) {
                if !param.schema.can_inline() {
                    compile_errors.push(error(
                        DiagnosticKind::InlineNotAllowed {
                            param: param.id.clone(),
                        },
                        Some(pos.clone()),
                        None,
                    ));
                    None
                } else {
                    let Some(expr) = param.schema.inline_expr(value) else {
                        compile_errors.push(error(
                            DiagnosticKind::InlineTypeMismatch {
                                param: param.id.clone(),
                                expected: param.schema.expected_port_type(),
                                got_value: value.clone(),
                            },
                            Some(pos.clone()),
                            None,
                        ));
                        continue;
                    };
                    Some(expr)
                }
            } else if let Some(default_expr) = param.schema.default_expr() {
                Some(default_expr)
            } else if param.required {
                compile_errors.push(error(
                    DiagnosticKind::MissingRequiredParam {
                        param: param.id.clone(),
                    },
                    Some(pos.clone()),
                    None,
                ));
                None
            } else {
                None
            };

            if let Some(expr) = resolved {
                if let Some(group) = param.variadic_group.as_ref() {
                    inputs
                        .variadic
                        .entry(group.clone())
                        .or_default()
                        .push(expr.clone());
                }
                inputs.scalar.insert(param.id.clone(), expr);
            }
        }

        if !compile_errors.is_empty() {
            continue;
        }

        let expr = if let Some(state) = node.node_state.as_ref() {
            let (expr, next_state) = piece.compile_stateful(&inputs, &node.inline_params, state);
            if mode == CompileMode::Runtime && &next_state != state {
                state_updates.push(NodeStateUpdate {
                    position: pos.clone(),
                    state: next_state,
                });
            }
            expr
        } else if let Some(initial) = piece.initial_state() {
            let (expr, next_state) = piece.compile_stateful(&inputs, &node.inline_params, &initial);
            if mode == CompileMode::Runtime {
                state_updates.push(NodeStateUpdate {
                    position: pos.clone(),
                    state: next_state,
                });
            }
            expr
        } else {
            piece.compile(&inputs, &node.inline_params)
        };

        // Store per-side outputs for multi-output pieces (e.g. cross connectors).
        if let Some(side_exprs) = piece.compile_multi_output(&inputs, &node.inline_params) {
            for (side, side_expr) in side_exprs {
                multi_outputs.insert((pos.clone(), side), side_expr);
            }
        }

        compiled.insert(pos.clone(), expr);
    }

    if !compile_errors.is_empty() {
        return Err(compile_errors);
    }

    Ok(CompiledNodes {
        compiled,
        state_updates,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::Value;

    use super::*;
    use crate::graph::{Edge, Graph, Node};
    use crate::piece::{ParamDef, ParamSchema, Piece, PieceDef, PieceInputs};
    use crate::piece_registry::PieceRegistry;
    use crate::semantic::semantic_pass;
    use crate::types::{EdgeId, GridPos, PieceCategory, TileSide};

    struct SourcePiece {
        def: PieceDef,
    }

    impl SourcePiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.source".into(),
                    label: "source".into(),
                    category: PieceCategory::Generator,
                    params: vec![ParamDef {
                        id: "value".into(),
                        label: "value".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Text {
                            default: "a".into(),
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                    }],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                },
            }
        }
    }

    impl Piece for SourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(
            &self,
            inputs: &PieceInputs,
            inline_params: &BTreeMap<String, Value>,
        ) -> CodeExpr {
            inputs
                .get("value")
                .cloned()
                .or_else(|| inline_params.get("value").cloned().map(CodeExpr::Literal))
                .unwrap_or_else(|| CodeExpr::Literal(Value::String("a".into())))
        }
    }

    struct MethodPiece {
        def: PieceDef,
    }

    impl MethodPiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.method".into(),
                    label: "method".into(),
                    category: PieceCategory::Transform,
                    params: vec![ParamDef {
                        id: "pattern".into(),
                        label: "pattern".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Text {
                            default: String::new(),
                            can_inline: false,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: true,
                    }],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                },
            }
        }
    }

    impl Piece for MethodPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(
            &self,
            inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
        ) -> CodeExpr {
            CodeExpr::Method {
                receiver: Box::new(
                    inputs
                        .get("pattern")
                        .cloned()
                        .unwrap_or_else(|| CodeExpr::Raw("missing".into())),
                ),
                method: "fast".into(),
                args: Vec::new(),
            }
        }
    }

    struct TerminalPiece {
        def: PieceDef,
    }

    impl TerminalPiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.output".into(),
                    label: "output".into(),
                    category: PieceCategory::Output,
                    params: vec![ParamDef {
                        id: "pattern".into(),
                        label: "pattern".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Text {
                            default: String::new(),
                            can_inline: false,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: true,
                    }],
                    output_type: None,
                    output_side: None,
                    description: None,
                },
            }
        }
    }

    impl Piece for TerminalPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(
            &self,
            inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
        ) -> CodeExpr {
            inputs
                .get("pattern")
                .cloned()
                .unwrap_or_else(|| CodeExpr::Raw("missing".into()))
        }
    }

    fn registry() -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(SourcePiece::new());
        registry.register(MethodPiece::new());
        registry.register(TerminalPiece::new());
        registry
    }

    fn graph() -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let method_pos = GridPos { col: 1, row: 0 };
        let output_pos = GridPos { col: 2, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos.clone(),
            to_node: method_pos.clone(),
            to_param: "pattern".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: method_pos.clone(),
            to_node: output_pos.clone(),
            to_param: "pattern".into(),
        };
        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.source".into(),
                        inline_params: BTreeMap::from([(
                            "value".into(),
                            Value::String("bd".into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    method_pos,
                    Node {
                        piece_id: "test.method".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "test.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a), (edge_b.id.clone(), edge_b)]),
            name: "compiler".into(),
            cols: 4,
            rows: 1,
        }
    }

    #[test]
    fn compile_node_expr_returns_selected_expression() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);

        let (expr, updates) = compile_node_expr(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            &GridPos { col: 1, row: 0 },
            &BTreeMap::new(),
        )
        .expect("compile method");

        assert!(updates.is_empty());
        assert_eq!(expr.render(), "'bd'.fast()");
    }

    #[test]
    fn compile_node_expr_uses_overrides_for_downstream_nodes() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);
        let overrides =
            BTreeMap::from([(GridPos { col: 0, row: 0 }, CodeExpr::Ident("arg1".into()))]);

        let (expr, _) = compile_node_expr(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            &GridPos { col: 2, row: 0 },
            &overrides,
        )
        .expect("compile terminal");

        assert_eq!(expr.render(), "arg1.fast()");
    }

    #[test]
    fn compile_graph_without_overrides_matches_existing_behavior() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);

        let program =
            compile_graph(&graph, &registry, &sem, CompileMode::Preview).expect("compile graph");

        assert_eq!(program.terminals.len(), 1);
        assert_eq!(program.terminals[0].render(), "'bd'.fast()");
    }
}
