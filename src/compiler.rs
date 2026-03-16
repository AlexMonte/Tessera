use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ast::{Expr, ExprKind, Origin};
use crate::diagnostics::{Diagnostic, DiagnosticKind, SemanticResult};
use crate::graph::{Graph, Node};
use crate::piece::{Piece, PieceInputs};
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
    pub terminals: Vec<Expr>,
    pub state_updates: Vec<NodeStateUpdate>,
}

#[derive(Debug, Clone)]
struct CompiledNodes {
    compiled: BTreeMap<GridPos, Expr>,
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

fn node_origin(pos: &GridPos) -> Origin {
    Origin {
        node: *pos,
        param: None,
    }
}

fn param_origin(pos: &GridPos, param: &str) -> Origin {
    Origin {
        node: *pos,
        param: Some(param.to_string()),
    }
}

fn unresolved_error_diagnostic(expr: &Expr, fallback: &GridPos) -> Diagnostic {
    let Some(error_expr) = expr.first_error() else {
        return error(
            DiagnosticKind::InvalidOperation {
                reason: "terminal expression contains unresolved error placeholder".into(),
            },
            Some(*fallback),
            None,
        );
    };

    let message = match &error_expr.kind {
        ExprKind::Error { message } => message.clone(),
        _ => "terminal expression contains unresolved error placeholder".into(),
    };
    let site = error_expr
        .origin
        .as_ref()
        .map(|origin| origin.node)
        .or(Some(*fallback));

    error(
        DiagnosticKind::InvalidOperation {
            reason: format!("terminal expression contains unresolved error placeholder: {message}"),
        },
        site,
        None,
    )
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
                    pos: *terminal,
                },
                Some(*terminal),
                None,
            )]);
        };

        // Runtime compilation rejects programs with surviving Error nodes.
        if mode == CompileMode::Runtime && expr.contains_error() {
            return Err(vec![unresolved_error_diagnostic(&expr, terminal)]);
        }

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
    overrides: &BTreeMap<GridPos, Expr>,
) -> Result<(Expr, Vec<NodeStateUpdate>), Vec<Diagnostic>> {
    let compiled_nodes = compile_nodes(graph, registry, sem, mode, overrides)?;
    let Some(expr) = compiled_nodes.compiled.get(root).cloned() else {
        return Err(vec![error(
            DiagnosticKind::UnknownNode { pos: *root },
            Some(*root),
            None,
        )]);
    };

    Ok((expr, compiled_nodes.state_updates))
}

/// Determine which side of `from` faces `to` based on grid adjacency.
fn direction_from_to(from: &GridPos, to: &GridPos) -> Option<TileSide> {
    match (to.col - from.col, to.row - from.row) {
        (1, 0) => Some(TileSide::RIGHT),
        (-1, 0) => Some(TileSide::LEFT),
        (0, -1) => Some(TileSide::TOP),
        (0, 1) => Some(TileSide::BOTTOM),
        _ => None,
    }
}

/// Resolve all parameter inputs for a single node by checking, in priority
/// order: connected upstream expression → inline parameter value → schema
/// default → required-param error.
fn resolve_param_inputs(
    piece: &dyn Piece,
    node: &Node,
    pos: &GridPos,
    graph: &Graph,
    compiled: &BTreeMap<GridPos, Expr>,
    multi_outputs: &BTreeMap<(GridPos, TileSide), Expr>,
) -> Result<PieceInputs, Vec<Diagnostic>> {
    let mut inputs = PieceInputs::default();
    let mut errors = Vec::<Diagnostic>::new();

    for param in &piece.def().params {
        let connected =
            incoming_edge_for_param(graph, pos, param.id.as_str()).and_then(|edge| {
                direction_from_to(&edge.from, &edge.to_node)
                    .and_then(|exit_side| multi_outputs.get(&(edge.from, exit_side)))
                    .or_else(|| compiled.get(&edge.from))
                    .cloned()
            });

        let resolved = if let Some(expr) = connected {
            Some(expr.with_origin_if_missing(param_origin(pos, param.id.as_str())))
        } else if let Some(value) = node.inline_params.get(param.id.as_str()) {
            if !param.schema.can_inline() {
                errors.push(error(
                    DiagnosticKind::InlineNotAllowed {
                        param: param.id.clone(),
                    },
                    Some(*pos),
                    None,
                ));
                None
            } else {
                let Some(expr) = param.schema.inline_expr(value) else {
                    errors.push(error(
                        DiagnosticKind::InlineTypeMismatch {
                            param: param.id.clone(),
                            expected: param.schema.expected_port_type(),
                            got_value: value.clone(),
                        },
                        Some(*pos),
                        None,
                    ));
                    continue;
                };
                Some(expr.with_origin_if_missing(param_origin(pos, param.id.as_str())))
            }
        } else if let Some(default_expr) = param.schema.default_expr() {
            Some(default_expr.with_origin_if_missing(param_origin(pos, param.id.as_str())))
        } else if param.required {
            errors.push(error(
                DiagnosticKind::MissingRequiredParam {
                    param: param.id.clone(),
                },
                Some(*pos),
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

    if errors.is_empty() {
        Ok(inputs)
    } else {
        Err(errors)
    }
}

/// Compile a single piece, handling the three-way branch for stateful vs
/// stateless pieces and recording any state transitions.
fn compile_piece_expr(
    piece: &dyn Piece,
    inputs: &PieceInputs,
    node: &Node,
    pos: &GridPos,
    mode: CompileMode,
) -> (Expr, Option<NodeStateUpdate>) {
    let (expr, state_update) = if let Some(state) = node.node_state.as_ref() {
        let (expr, next_state) = piece.compile_stateful(inputs, &node.inline_params, state);
        let update = if mode == CompileMode::Runtime && &next_state != state {
            Some(NodeStateUpdate {
                position: *pos,
                state: next_state,
            })
        } else {
            None
        };
        (expr, update)
    } else if let Some(initial) = piece.initial_state() {
        let (expr, next_state) = piece.compile_stateful(inputs, &node.inline_params, &initial);
        let update = if mode == CompileMode::Runtime {
            Some(NodeStateUpdate {
                position: *pos,
                state: next_state,
            })
        } else {
            None
        };
        (expr, update)
    } else {
        (piece.compile(inputs, &node.inline_params), None)
    };

    (expr.with_origin_if_missing(node_origin(pos)), state_update)
}

fn compile_nodes(
    graph: &Graph,
    registry: &PieceRegistry,
    sem: &SemanticResult,
    mode: CompileMode,
    overrides: &BTreeMap<GridPos, Expr>,
) -> Result<CompiledNodes, Vec<Diagnostic>> {
    if !sem.is_valid() {
        return Err(sem.diagnostics.clone());
    }

    let mut compiled = overrides
        .iter()
        .map(|(pos, expr)| {
            (*pos, expr.clone().with_origin_if_missing(node_origin(pos)))
        })
        .collect::<BTreeMap<_, _>>();
    let mut multi_outputs: BTreeMap<(GridPos, TileSide), Expr> = BTreeMap::new();
    let mut state_updates = Vec::<NodeStateUpdate>::new();
    let mut compile_errors = Vec::<Diagnostic>::new();

    for pos in &sem.eval_order {
        if compiled.contains_key(pos) {
            continue;
        }

        let Some(node) = graph.nodes.get(pos) else {
            compile_errors.push(error(
                DiagnosticKind::UnknownNode { pos: *pos },
                Some(*pos),
                None,
            ));
            continue;
        };

        let Some(piece) = registry.get(node.piece_id.as_str()) else {
            compile_errors.push(error(
                DiagnosticKind::UnknownPiece {
                    piece_id: node.piece_id.clone(),
                },
                Some(*pos),
                None,
            ));
            continue;
        };

        let inputs = match resolve_param_inputs(
            piece.as_ref(),
            node,
            pos,
            graph,
            &compiled,
            &multi_outputs,
        ) {
            Ok(inputs) => inputs,
            Err(mut errs) => {
                compile_errors.append(&mut errs);
                continue;
            }
        };

        let (expr, state_update) = compile_piece_expr(piece.as_ref(), &inputs, node, pos, mode);

        if let Some(update) = state_update {
            state_updates.push(update);
        }

        // Store per-side outputs for multi-output pieces (e.g. cross connectors).
        if let Some(side_exprs) = piece.compile_multi_output(&inputs, &node.inline_params) {
            for (side, side_expr) in side_exprs {
                multi_outputs.insert(
                    (*pos, side),
                    side_expr.with_origin_if_missing(node_origin(pos)),
                );
            }
        }

        compiled.insert(*pos, expr);
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
    use crate::ast::{Expr, ExprKind, Origin};
    use crate::backend::{Backend, JsBackend};
    use crate::graph::{Edge, Graph, Node};
    use crate::piece::{ParamDef, ParamSchema, Piece, PieceDef, PieceInputs};
    use crate::piece_registry::PieceRegistry;
    use crate::semantic::semantic_pass;
    use crate::types::{EdgeId, GridPos, PieceCategory, PieceSemanticKind, TileSide};

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
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
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

        fn compile(&self, inputs: &PieceInputs, inline_params: &BTreeMap<String, Value>) -> Expr {
            inputs
                .get("value")
                .cloned()
                .or_else(|| inline_params.get("value").map(Expr::from_json_value))
                .unwrap_or_else(|| Expr::str_lit("a"))
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
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
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

        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::method_call(
                inputs
                    .get("pattern")
                    .cloned()
                    .unwrap_or_else(|| Expr::error("missing")),
                "fast",
                Vec::new(),
            )
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
                    semantic_kind: PieceSemanticKind::Output,
                    namespace: "core".into(),
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

        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            inputs
                .get("pattern")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing"))
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
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
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
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
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
        assert_eq!(JsBackend.render(&expr), "'bd'.fast()");
    }

    #[test]
    fn compile_node_expr_uses_overrides_for_downstream_nodes() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);
        let overrides = BTreeMap::from([(GridPos { col: 0, row: 0 }, Expr::ident("arg1"))]);

        let (expr, _) = compile_node_expr(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            &GridPos { col: 2, row: 0 },
            &overrides,
        )
        .expect("compile terminal");

        assert_eq!(JsBackend.render(&expr), "arg1.fast()");
    }

    #[test]
    fn compile_graph_without_overrides_matches_existing_behavior() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);

        let program =
            compile_graph(&graph, &registry, &sem, CompileMode::Preview).expect("compile graph");

        assert_eq!(program.terminals.len(), 1);
        assert_eq!(JsBackend.render(&program.terminals[0]), "'bd'.fast()");
    }

    #[test]
    fn compile_inline_param_attaches_param_origin() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);

        let (expr, _) = compile_node_expr(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            &GridPos { col: 0, row: 0 },
            &BTreeMap::new(),
        )
        .expect("compile source");

        assert_eq!(
            expr.origin,
            Some(Origin {
                node: GridPos { col: 0, row: 0 },
                param: Some("value".into()),
            })
        );
    }

    struct ErrorSourcePiece {
        def: PieceDef,
    }

    impl ErrorSourcePiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.error_source".into(),
                    label: "broken".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                },
            }
        }
    }

    impl Piece for ErrorSourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::error("source missing")
        }
    }

    #[test]
    fn runtime_compile_reports_origin_of_nested_error() {
        let source_pos = GridPos { col: 0, row: 0 };
        let output_pos = GridPos { col: 1, row: 0 };
        let edge = Edge {
            id: EdgeId::new(),
            from: source_pos.clone(),
            to_node: output_pos.clone(),
            to_param: "pattern".into(),
        };

        let graph = Graph {
            nodes: BTreeMap::from([
                (
                    source_pos.clone(),
                    Node {
                        piece_id: "test.error_source".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos.clone(),
                    Node {
                        piece_id: "test.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge.id.clone(), edge)]),
            name: "runtime_error_origin".into(),
            cols: 2,
            rows: 1,
        };

        let mut registry = PieceRegistry::new();
        registry.register(ErrorSourcePiece::new());
        registry.register(TerminalPiece::new());

        let sem = semantic_pass(&graph, &registry);
        let err = compile_graph(&graph, &registry, &sem, CompileMode::Runtime)
            .expect_err("runtime error");

        assert_eq!(err.len(), 1);
        assert_eq!(err[0].site, Some(source_pos));
        match &err[0].kind {
            DiagnosticKind::InvalidOperation { reason } => {
                assert!(reason.contains("source missing"));
            }
            other => panic!("unexpected diagnostic: {other:?}"),
        }
    }

    #[test]
    fn compile_override_attaches_node_origin_when_missing() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);
        let overrides = BTreeMap::from([(GridPos { col: 0, row: 0 }, Expr::ident("arg1"))]);

        let (expr, _) = compile_node_expr(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            &GridPos { col: 2, row: 0 },
            &overrides,
        )
        .expect("compile terminal");

        match expr.kind {
            ExprKind::MethodCall { receiver, .. } => {
                assert_eq!(
                    receiver.origin,
                    Some(Origin {
                        node: GridPos { col: 0, row: 0 },
                        param: None,
                    })
                );
            }
            other => panic!("expected method call, got {other:?}"),
        }
    }
}
