use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::core_pieces::DELAY_PIECE_ID;
use crate::diagnostics::{Diagnostic, DiagnosticKind, SemanticResult};
use crate::graph::{Edge, Graph, Node};
use crate::piece::{ParamSchema, PieceDef};
use crate::piece_registry::PieceRegistry;
use crate::types::{
    DomainBridge, PortTypeConnection, PortTypeConnectionError, GridPos, PortType, TileSide,
    adjacent_in_direction,
};

#[derive(Debug, Clone)]
struct PendingEdgeTypeCheck {
    edge_id: crate::types::EdgeId,
    source_pos: GridPos,
    target_pos: GridPos,
    param: String,
    schema: ParamSchema,
}

pub(crate) fn incoming_edge_for_param<'a>(
    graph: &'a Graph,
    to_node: &GridPos,
    param: &str,
) -> Option<&'a Edge> {
    graph
        .edges
        .values()
        .find(|edge| &edge.to_node == to_node && edge.to_param == param)
}

fn node_param_side(node: &Node, param_id: &str) -> Option<TileSide> {
    node.input_sides.get(param_id).copied()
}

fn node_output_side(node: &Node, piece: &PieceDef) -> Option<TileSide> {
    if piece.output_type.is_none() {
        None
    } else {
        node.output_side.or(piece.output_side)
    }
}

pub(crate) fn resolved_input_connection_for_param(
    graph: &Graph,
    node: &Node,
    pos: &GridPos,
    param_id: &str,
    schema: &ParamSchema,
    inferred_outputs: &BTreeMap<GridPos, PortType>,
) -> Option<PortTypeConnection> {
    if let Some(edge) = incoming_edge_for_param(graph, pos, param_id) {
        let source_type = inferred_outputs.get(&edge.from)?;
        return schema.resolve_connection(source_type).ok();
    }

    schema
        .resolved_port_type(node.inline_params.get(param_id))
        .map(|effective_type| PortTypeConnection {
            effective_type,
            bridge_kind: None,
        })
}

fn resolved_input_type_for_param(
    graph: &Graph,
    node: &Node,
    pos: &GridPos,
    param_id: &str,
    schema: &ParamSchema,
    inferred_outputs: &BTreeMap<GridPos, PortType>,
) -> Option<PortType> {
    resolved_input_connection_for_param(graph, node, pos, param_id, schema, inferred_outputs)
        .map(|connection| connection.effective_type)
}

pub(crate) fn resolved_input_types_for_piece(
    graph: &Graph,
    node: &Node,
    pos: &GridPos,
    piece: &PieceDef,
    inferred_outputs: &BTreeMap<GridPos, PortType>,
) -> BTreeMap<String, PortType> {
    let mut input_types = BTreeMap::<String, PortType>::new();

    for param in &piece.params {
        if let Some(port_type) = resolved_input_type_for_param(
            graph,
            node,
            pos,
            param.id.as_str(),
            &param.schema,
            inferred_outputs,
        ) {
            input_types.insert(param.id.clone(), port_type);
        }
    }

    input_types
}

fn infer_output_types(
    graph: &Graph,
    registry: &PieceRegistry,
    eval_order: &[GridPos],
) -> BTreeMap<GridPos, PortType> {
    let mut output_types = BTreeMap::<GridPos, PortType>::new();

    for pos in eval_order {
        let Some(node) = graph.nodes.get(pos) else {
            continue;
        };
        let Some(piece) = registry.get(node.piece_id.as_str()) else {
            continue;
        };
        if piece.def().output_type.is_none() {
            continue;
        }

        let input_types =
            resolved_input_types_for_piece(graph, node, pos, piece.def(), &output_types);

        if let Some(port_type) = piece
            .infer_output_type(&input_types, &node.inline_params)
            .or_else(|| piece.def().output_type.clone())
        {
            output_types.insert(*pos, port_type);
        }
    }

    output_types
}

fn reconcile_delay_output_types(
    graph: &Graph,
    registry: &PieceRegistry,
    output_types: &mut BTreeMap<GridPos, PortType>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (pos, node) in &graph.nodes {
        if node.piece_id != DELAY_PIECE_ID {
            continue;
        }

        let Some(piece) = registry.get(node.piece_id.as_str()) else {
            continue;
        };
        let Some(default_param) = piece
            .def()
            .params
            .iter()
            .find(|param| param.id == "default")
        else {
            continue;
        };

        let default_type = resolved_input_type_for_param(
            graph,
            node,
            pos,
            "default",
            &default_param.schema,
            output_types,
        );
        let Some(value_param) = piece.def().params.iter().find(|param| param.id == "value") else {
            continue;
        };
        let feedback_type = resolved_input_type_for_param(
            graph,
            node,
            pos,
            "value",
            &value_param.schema,
            output_types,
        );

        match (default_type, feedback_type) {
            (Some(default), Some(feedback)) => {
                if let Some(common) = feedback
                    .resolve_connection(&default)
                    .ok()
                    .map(|connection| connection.effective_type)
                    .or_else(|| {
                        default
                            .resolve_connection(&feedback)
                            .ok()
                            .map(|connection| connection.effective_type)
                    })
                {
                    output_types.insert(*pos, common);
                } else {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticKind::DelayTypeMismatch {
                            default: default.clone(),
                            feedback: feedback.clone(),
                        },
                        Some(*pos),
                    ));
                    output_types.insert(*pos, PortType::any());
                }
            }
            (Some(default), None) => {
                output_types.insert(*pos, default);
            }
            (None, Some(feedback)) => {
                output_types.insert(*pos, feedback);
            }
            (None, None) => {}
        }
    }
}

pub fn semantic_pass(graph: &Graph, registry: &PieceRegistry) -> SemanticResult {
    let mut diagnostics = Vec::<Diagnostic>::new();
    let mut pending_type_checks = Vec::<PendingEdgeTypeCheck>::new();

    for (pos, node) in &graph.nodes {
        if registry.get(node.piece_id.as_str()).is_none() {
            diagnostics.push(Diagnostic::error(
                DiagnosticKind::UnknownPiece {
                    piece_id: node.piece_id.clone(),
                },
                Some(*pos),
            ));
        }
    }

    let mut incoming_slots = BTreeSet::<(GridPos, String)>::new();
    for edge in graph.edges.values() {
        let Some(from_node) = graph.nodes.get(&edge.from) else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::UnknownNode { pos: edge.from },
                    Some(edge.to_node),
                )
                .with_edge(edge.id.clone()),
            );
            continue;
        };
        let Some(to_node) = graph.nodes.get(&edge.to_node) else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::UnknownNode { pos: edge.to_node },
                    Some(edge.to_node),
                )
                .with_edge(edge.id.clone()),
            );
            continue;
        };
        let Some(from_piece) = registry.get(from_node.piece_id.as_str()) else {
            continue;
        };
        let Some(to_piece) = registry.get(to_node.piece_id.as_str()) else {
            continue;
        };

        if !incoming_slots.insert((edge.to_node, edge.to_param.clone())) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::DuplicateConnection {
                        to_node: edge.to_node,
                        to_param: edge.to_param.clone(),
                    },
                    Some(edge.to_node),
                )
                .with_edge(edge.id.clone()),
            );
        }

        let Some(param_def) = to_piece
            .def()
            .params
            .iter()
            .find(|param| param.id == edge.to_param)
        else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::UnknownParam {
                        piece_id: to_piece.def().id.clone(),
                        param: edge.to_param.clone(),
                    },
                    Some(edge.to_node),
                )
                .with_edge(edge.id.clone()),
            );
            continue;
        };

        let Some(target_side) = node_param_side(to_node, edge.to_param.as_str()) else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::InvalidOperation {
                        reason: format!(
                            "target param '{}' has no assigned side on placed node",
                            edge.to_param
                        ),
                    },
                    Some(edge.to_node),
                )
                .with_edge(edge.id.clone()),
            );
            continue;
        };
        let expected_neighbor = adjacent_in_direction(&edge.to_node, Some(target_side));
        if expected_neighbor != edge.from {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::NotAdjacent {
                        from_pos: edge.from,
                        to_pos: edge.to_node,
                    },
                    Some(edge.to_node),
                )
                .with_edge(edge.id.clone()),
            );
            continue;
        }

        if let Some(from_side) = node_output_side(from_node, from_piece.def())
            && !from_side.faces(target_side)
        {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::SideMismatch {
                        from_pos: edge.from,
                        to_pos: edge.to_node,
                        expected_side: target_side,
                    },
                    Some(edge.to_node),
                )
                .with_edge(edge.id.clone()),
            );
        }

        pending_type_checks.push(PendingEdgeTypeCheck {
            edge_id: edge.id.clone(),
            source_pos: edge.from,
            target_pos: edge.to_node,
            param: edge.to_param.clone(),
            schema: param_def.schema.clone(),
        });
    }

    for (pos, node) in &graph.nodes {
        let Some(piece) = registry.get(node.piece_id.as_str()) else {
            continue;
        };
        for inline_key in node.inline_params.keys() {
            let Some(param_def) = piece
                .def()
                .params
                .iter()
                .find(|param| &param.id == inline_key)
            else {
                diagnostics.push(Diagnostic::error(
                    DiagnosticKind::UnknownParam {
                        piece_id: piece.def().id.clone(),
                        param: inline_key.clone(),
                    },
                    Some(*pos),
                ));
                continue;
            };
            if !param_def.schema.can_inline() {
                diagnostics.push(Diagnostic::error(
                    DiagnosticKind::InlineNotAllowed {
                        param: inline_key.clone(),
                    },
                    Some(*pos),
                ));
                continue;
            }
            if !param_def
                .schema
                .validate_inline_value(node.inline_params.get(inline_key).unwrap_or(&Value::Null))
            {
                diagnostics.push(Diagnostic::error(
                    DiagnosticKind::InlineTypeMismatch {
                        param: inline_key.clone(),
                        expected: param_def.schema.expected_port_type(),
                        got_value: node
                            .inline_params
                            .get(inline_key)
                            .cloned()
                            .unwrap_or(Value::Null),
                    },
                    Some(*pos),
                ));
            }
        }

        for side_key in node.input_sides.keys() {
            if piece.def().params.iter().any(|param| &param.id == side_key) {
                continue;
            }
            diagnostics.push(Diagnostic::error(
                DiagnosticKind::UnknownParam {
                    piece_id: piece.def().id.clone(),
                    param: side_key.clone(),
                },
                Some(*pos),
            ));
        }

        for param in &piece.def().params {
            let has_edge = incoming_edge_for_param(graph, pos, param.id.as_str()).is_some();
            let has_inline = node.inline_params.contains_key(param.id.as_str());
            let has_default = param.schema.default_expr().is_some();
            if param.required && !has_edge && !has_inline && !has_default {
                diagnostics.push(Diagnostic::error(
                    DiagnosticKind::MissingRequiredParam {
                        param: param.id.clone(),
                    },
                    Some(*pos),
                ));
            }
        }
    }

    // Identify delay edges — edges feeding into a delay piece's "value"
    // param. These are excluded from the topological sort so that cycles
    // through delay nodes are legal.
    let delay_edges: BTreeSet<crate::types::EdgeId> = graph
        .edges
        .iter()
        .filter(|(_, edge)| {
            graph
                .nodes
                .get(&edge.to_node)
                .is_some_and(|node| node.piece_id == DELAY_PIECE_ID && edge.to_param == "value")
        })
        .map(|(id, _)| id.clone())
        .collect();

    let mut indegree = BTreeMap::<GridPos, usize>::new();
    let mut out_edges = BTreeMap::<GridPos, Vec<GridPos>>::new();

    for pos in graph.nodes.keys() {
        indegree.insert(*pos, 0);
    }

    for edge in graph.edges.values() {
        if !(graph.nodes.contains_key(&edge.from) && graph.nodes.contains_key(&edge.to_node)) {
            continue;
        }
        // Skip delay edges so delay nodes are scheduled before their
        // feedback source, breaking the cycle.
        if delay_edges.contains(&edge.id) {
            continue;
        }
        out_edges.entry(edge.from).or_default().push(edge.to_node);
        if let Some(target_indegree) = indegree.get_mut(&edge.to_node) {
            *target_indegree += 1;
        }
    }

    let mut frontier = indegree
        .iter()
        .filter_map(|(pos, degree)| if *degree == 0 { Some(*pos) } else { None })
        .collect::<BTreeSet<_>>();

    let mut eval_order = Vec::with_capacity(indegree.len());
    while let Some(next) = frontier.iter().next().cloned() {
        frontier.remove(&next);
        eval_order.push(next);

        if let Some(targets) = out_edges.get(&next) {
            for target in targets {
                if let Some(target_indegree) = indegree.get_mut(target) {
                    *target_indegree = target_indegree.saturating_sub(1);
                    if *target_indegree == 0 {
                        frontier.insert(*target);
                    }
                }
            }
        }
    }

    if eval_order.len() != graph.nodes.len() {
        let ordered = eval_order.iter().cloned().collect::<BTreeSet<_>>();
        let involved = graph
            .nodes
            .keys()
            .filter(|pos| !ordered.contains(*pos))
            .cloned()
            .collect::<Vec<_>>();
        diagnostics.push(Diagnostic::error(
            DiagnosticKind::Cycle {
                involved: involved.clone(),
            },
            involved.first().cloned(),
        ));
        for pos in involved {
            if !eval_order.iter().any(|item| item == &pos) {
                eval_order.push(pos);
            }
        }
    }

    let mut output_types = infer_output_types(graph, registry, &eval_order);
    let mut domain_bridges = BTreeMap::<crate::types::EdgeId, DomainBridge>::new();

    // Follow-up: delay nodes are scheduled before their feedback source
    // because `delay.value` edges are excluded from topo ordering. Now
    // that all sources are resolved, reconcile the frame-0 `default`
    // type with the feedback source type into a single output type.
    reconcile_delay_output_types(graph, registry, &mut output_types, &mut diagnostics);

    for check in pending_type_checks {
        let Some(from_output_type) = output_types.get(&check.source_pos) else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::OutputFromTerminal {
                        position: check.source_pos,
                    },
                    Some(check.target_pos),
                )
                .with_edge(check.edge_id),
            );
            continue;
        };

        match check.schema.resolve_connection(from_output_type) {
            Ok(connection) => {
                if let Some(bridge_kind) = connection.bridge_kind {
                    domain_bridges.insert(
                        check.edge_id.clone(),
                        DomainBridge {
                            edge_id: check.edge_id,
                            source_pos: check.source_pos,
                            target_pos: check.target_pos,
                            param: check.param,
                            kind: bridge_kind,
                        },
                    );
                }
            }
            Err(PortTypeConnectionError::ValueMismatch { expected, got }) => {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticKind::TypeMismatch {
                            expected,
                            got,
                            param: check.param,
                        },
                        Some(check.target_pos),
                    )
                    .with_edge(check.edge_id),
                );
            }
            Err(PortTypeConnectionError::UnsupportedDomain { expected, got }) => {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticKind::UnsupportedDomainCrossing {
                            expected,
                            got,
                            param: check.param,
                        },
                        Some(check.target_pos),
                    )
                    .with_edge(check.edge_id),
                );
            }
        }
    }

    let terminals = graph
        .nodes
        .iter()
        .filter_map(|(pos, node)| {
            registry
                .get(node.piece_id.as_str())
                .filter(|piece| piece.def().is_terminal())
                .map(|_| *pos)
        })
        .collect::<Vec<_>>();

    match terminals.len() {
        0 => {
            diagnostics.push(Diagnostic::error(DiagnosticKind::NoTerminalNode, None));
        }
        1 => {}
        _ => {
            // Multiple terminals: allowed, but surface as a warning so the UI can inform the user.
            diagnostics.push(Diagnostic::warning(
                DiagnosticKind::MultipleTerminalNodes {
                    positions: terminals.clone(),
                },
                None,
            ));
        }
    }

    // Backward reachability from all terminals simultaneously.
    if !terminals.is_empty() {
        let reachable = graph.reachable_nodes(&terminals);
        for pos in graph.nodes.keys() {
            if !reachable.contains(pos) {
                // UnreachableNode is a warning — isolated stub nodes should not block compilation.
                diagnostics.push(Diagnostic::warning(
                    DiagnosticKind::UnreachableNode { position: *pos },
                    Some(*pos),
                ));
            }
        }
    }

    SemanticResult {
        diagnostics,
        eval_order,
        terminals,
        output_types,
        domain_bridges,
        delay_edges,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use serde_json::{Value, json};

    use super::semantic_pass;
    use crate::core_pieces::core_expression_pieces;
    use crate::diagnostics::DiagnosticKind;
    use crate::graph::{Edge, Graph, Node};
    use crate::piece::{
        ParamDef, ParamInlineMode, ParamSchema, ParamTextSemantics, ParamValueKind, Piece,
        PieceDef, PieceInputs,
    };
    use crate::piece_registry::PieceRegistry;
    use crate::types::{
        DomainBridgeKind, EdgeId, ExecutionDomain, GridPos, PieceCategory, PieceSemanticKind,
        PortType, TileSide,
    };

    struct NumberSourcePiece {
        def: PieceDef,
    }

    impl NumberSourcePiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.number_source".into(),
                    label: "number source".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Literal,
                    namespace: "core".into(),
                    params: vec![],
                    output_type: Some(PortType::number()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for NumberSourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(
            &self,
            _inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
        ) -> crate::ast::Expr {
            crate::ast::Expr::int(1)
        }
    }

    struct PatternSourcePiece {
        def: PieceDef,
    }

    impl PatternSourcePiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.pattern_source".into(),
                    label: "pattern source".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Literal,
                    namespace: "core".into(),
                    params: vec![],
                    output_type: Some(PortType::new("pattern")),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for PatternSourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(
            &self,
            _inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
        ) -> crate::ast::Expr {
            crate::ast::Expr::pattern("bd")
        }
    }

    struct GenericForwardPiece {
        def: PieceDef,
    }

    impl GenericForwardPiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.forward".into(),
                    label: "forward".into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "value".into(),
                        label: "value".into(),
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
                    }],
                    output_type: Some(PortType::any()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for GenericForwardPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(
            &self,
            inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
        ) -> crate::ast::Expr {
            inputs
                .get("value")
                .cloned()
                .unwrap_or_else(|| crate::ast::Expr::error("missing value"))
        }

        fn infer_output_type(
            &self,
            input_types: &BTreeMap<String, PortType>,
            _inline_params: &BTreeMap<String, Value>,
        ) -> Option<PortType> {
            input_types
                .get("value")
                .cloned()
                .or_else(|| Some(PortType::any()))
        }
    }

    struct SinkPiece {
        def: PieceDef,
    }

    impl SinkPiece {
        fn new(id: &str, expected: PortType) -> Self {
            Self {
                def: PieceDef {
                    id: id.into(),
                    label: id.into(),
                    category: PieceCategory::Output,
                    semantic_kind: PieceSemanticKind::Output,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "value".into(),
                        label: "value".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Custom {
                            port_type: expected,
                            value_kind: ParamValueKind::Json,
                            default: None,
                            can_inline: false,
                            inline_mode: ParamInlineMode::Literal,
                            min: None,
                            max: None,
                        },
                        text_semantics: ParamTextSemantics::Plain,
                        variadic_group: None,
                        required: true,
                    }],
                    output_type: None,
                    output_side: None,
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for SinkPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(
            &self,
            inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
        ) -> crate::ast::Expr {
            inputs
                .get("value")
                .cloned()
                .unwrap_or_else(|| crate::ast::Expr::error("missing value"))
        }
    }

    struct DomainSourcePiece {
        def: PieceDef,
    }

    impl DomainSourcePiece {
        fn new(id: &str, output_type: PortType) -> Self {
            Self {
                def: PieceDef {
                    id: id.into(),
                    label: id.into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Literal,
                    namespace: "core".into(),
                    params: vec![],
                    output_type: Some(output_type),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for DomainSourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(
            &self,
            _inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
        ) -> crate::ast::Expr {
            crate::ast::Expr::ident(self.def.id.clone())
        }
    }

    struct DomainForwardPiece {
        def: PieceDef,
    }

    impl DomainForwardPiece {
        fn new(id: &str, expected_input: PortType) -> Self {
            Self {
                def: PieceDef {
                    id: id.into(),
                    label: id.into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "value".into(),
                        label: "value".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Custom {
                            port_type: expected_input,
                            value_kind: ParamValueKind::Json,
                            default: None,
                            can_inline: false,
                            inline_mode: ParamInlineMode::Literal,
                            min: None,
                            max: None,
                        },
                        text_semantics: ParamTextSemantics::Plain,
                        variadic_group: None,
                        required: true,
                    }],
                    output_type: Some(PortType::any().with_unspecified_domain()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for DomainForwardPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(
            &self,
            inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
        ) -> crate::ast::Expr {
            inputs
                .get("value")
                .cloned()
                .unwrap_or_else(|| crate::ast::Expr::error("missing value"))
        }

        fn infer_output_type(
            &self,
            input_types: &BTreeMap<String, PortType>,
            _inline_params: &BTreeMap<String, Value>,
        ) -> Option<PortType> {
            input_types.get("value").cloned()
        }
    }

    fn graph_for_inferred_passthrough_mismatch() -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let forward_pos = GridPos { col: 1, row: 0 };
        let sink_pos = GridPos { col: 2, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: forward_pos,
            to_param: "value".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: forward_pos,
            to_node: sink_pos,
            to_param: "value".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.number_source".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    forward_pos,
                    Node {
                        piece_id: "test.forward".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    sink_pos,
                    Node {
                        piece_id: "test.text_sink".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a), (edge_b.id.clone(), edge_b)]),
            name: "inferred_passthrough".into(),
            cols: 3,
            rows: 1,
        }
    }

    #[test]
    fn semantic_pass_uses_inferred_output_types_for_generic_pieces() {
        let mut registry = PieceRegistry::new();
        registry.register(NumberSourcePiece::new());
        registry.register(GenericForwardPiece::new());
        registry.register(SinkPiece::new("test.text_sink", PortType::text()));

        let graph = graph_for_inferred_passthrough_mismatch();
        let sem = semantic_pass(&graph, &registry);

        assert_eq!(
            sem.output_types.get(&GridPos { col: 1, row: 0 }),
            Some(&PortType::number())
        );
        assert!(sem.diagnostics.iter().any(|diagnostic| {
            matches!(
                &diagnostic.kind,
                DiagnosticKind::TypeMismatch { got, param, .. }
                    if *got == PortType::number() && param == "value"
            )
        }));
    }

    #[test]
    fn semantic_pass_infers_if_expr_output_type_from_inline_branches() {
        let mut registry = PieceRegistry::new();
        for piece in core_expression_pieces() {
            let id = piece.def().id.clone();
            registry.register_arc(id, Arc::from(piece));
        }
        registry.register(SinkPiece::new("test.number_sink", PortType::number()));

        let if_pos = GridPos { col: 0, row: 0 };
        let sink_pos = GridPos { col: 1, row: 0 };
        let edge = Edge {
            id: EdgeId::new(),
            from: if_pos,
            to_node: sink_pos,
            to_param: "value".into(),
        };

        let graph = Graph {
            nodes: BTreeMap::from([
                (
                    if_pos,
                    Node {
                        piece_id: "core.if_expr".into(),
                        inline_params: BTreeMap::from([
                            ("then".into(), json!(1)),
                            ("else".into(), json!(2)),
                        ]),
                        input_sides: BTreeMap::from([
                            ("cond".into(), TileSide::LEFT),
                            ("then".into(), TileSide::LEFT),
                            ("else".into(), TileSide::LEFT),
                        ]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    sink_pos,
                    Node {
                        piece_id: "test.number_sink".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge.id.clone(), edge)]),
            name: "if_expr_number".into(),
            cols: 2,
            rows: 1,
        };

        let sem = semantic_pass(&graph, &registry);

        assert_eq!(sem.output_types.get(&if_pos), Some(&PortType::number()));
        assert!(
            !sem.diagnostics
                .iter()
                .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::TypeMismatch { .. }))
        );
    }

    fn bridge_registry() -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(DomainSourcePiece::new(
            "test.control_source",
            PortType::number().with_domain(ExecutionDomain::Control),
        ));
        registry.register(DomainSourcePiece::new(
            "test.audio_source",
            PortType::number().with_domain(ExecutionDomain::Audio),
        ));
        registry.register(DomainSourcePiece::new(
            "test.event_source",
            PortType::number().with_domain(ExecutionDomain::Event),
        ));
        registry.register(DomainForwardPiece::new(
            "test.audio_forward",
            PortType::number().with_domain(ExecutionDomain::Audio),
        ));
        registry.register(SinkPiece::new(
            "test.audio_sink",
            PortType::number().with_domain(ExecutionDomain::Audio),
        ));
        registry.register(SinkPiece::new(
            "test.control_sink",
            PortType::number().with_domain(ExecutionDomain::Control),
        ));
        registry.register(SinkPiece::new(
            "test.event_sink",
            PortType::number().with_domain(ExecutionDomain::Event),
        ));
        registry.register(SinkPiece::new("test.any_sink", PortType::any()));
        registry
    }

    fn simple_edge_graph(source_piece_id: &str, sink_piece_id: &str) -> (Graph, EdgeId) {
        let source_pos = GridPos { col: 0, row: 0 };
        let sink_pos = GridPos { col: 1, row: 0 };
        let edge = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: sink_pos,
            to_param: "value".into(),
        };
        let edge_id = edge.id.clone();

        (
            Graph {
                nodes: BTreeMap::from([
                    (
                        source_pos,
                        Node {
                            piece_id: source_piece_id.into(),
                            inline_params: BTreeMap::new(),
                            input_sides: BTreeMap::new(),
                            output_side: None,
                            label: None,
                            node_state: None,
                        },
                    ),
                    (
                        sink_pos,
                        Node {
                            piece_id: sink_piece_id.into(),
                            inline_params: BTreeMap::new(),
                            input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                            output_side: None,
                            label: None,
                            node_state: None,
                        },
                    ),
                ]),
                edges: BTreeMap::from([(edge.id.clone(), edge)]),
                name: "bridge_graph".into(),
                cols: 2,
                rows: 1,
            },
            edge_id,
        )
    }

    #[test]
    fn semantic_pass_records_supported_domain_bridges() {
        let registry = bridge_registry();

        let cases = [
            (
                "test.control_source",
                "test.audio_sink",
                DomainBridgeKind::ControlToAudio,
            ),
            (
                "test.audio_source",
                "test.control_sink",
                DomainBridgeKind::AudioToControl,
            ),
            (
                "test.event_source",
                "test.control_sink",
                DomainBridgeKind::EventToControl,
            ),
        ];

        for (source_piece, sink_piece, expected_bridge) in cases {
            let (graph, edge_id) = simple_edge_graph(source_piece, sink_piece);
            let sem = semantic_pass(&graph, &registry);

            assert!(
                sem.diagnostics.is_empty(),
                "expected no diagnostics for {source_piece} -> {sink_piece}, got {:?}",
                sem.diagnostics
            );
            assert_eq!(
                sem.domain_bridges.get(&edge_id).map(|bridge| bridge.kind),
                Some(expected_bridge)
            );
        }
    }

    #[test]
    fn semantic_pass_reports_unsupported_domain_crossing() {
        let registry = bridge_registry();
        let (graph, edge_id) = simple_edge_graph("test.audio_source", "test.event_sink");
        let sem = semantic_pass(&graph, &registry);

        assert!(sem.domain_bridges.get(&edge_id).is_none());
        assert!(sem.diagnostics.iter().any(|diagnostic| {
            matches!(
                &diagnostic.kind,
                DiagnosticKind::UnsupportedDomainCrossing { expected, got, param }
                    if *expected == PortType::number().with_domain(ExecutionDomain::Event)
                        && *got == PortType::number().with_domain(ExecutionDomain::Audio)
                        && param == "value"
            )
        }));
    }

    #[test]
    fn semantic_pass_uses_effective_bridged_input_types_for_inference() {
        let registry = bridge_registry();
        let source_pos = GridPos { col: 0, row: 0 };
        let forward_pos = GridPos { col: 1, row: 0 };
        let sink_pos = GridPos { col: 2, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: forward_pos,
            to_param: "value".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: forward_pos,
            to_node: sink_pos,
            to_param: "value".into(),
        };

        let graph = Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.control_source".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    forward_pos,
                    Node {
                        piece_id: "test.audio_forward".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    sink_pos,
                    Node {
                        piece_id: "test.any_sink".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a.clone()), (edge_b.id.clone(), edge_b)]),
            name: "bridge_inference".into(),
            cols: 3,
            rows: 1,
        };

        let sem = semantic_pass(&graph, &registry);

        assert_eq!(
            sem.output_types.get(&forward_pos),
            Some(&PortType::number().with_domain(ExecutionDomain::Audio))
        );
        assert_eq!(
            sem.domain_bridges.get(&edge_a.id).map(|bridge| bridge.kind),
            Some(DomainBridgeKind::ControlToAudio)
        );
    }

    // -- delay / feedback loop tests --

    fn delay_feedback_registry() -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(NumberSourcePiece::new());
        registry.register(PatternSourcePiece::new());
        registry.register(GenericForwardPiece::new());
        registry.register(SinkPiece::new("test.any_sink", PortType::any()));
        for piece in crate::core_pieces::core_expression_pieces() {
            let id = piece.def().id.clone();
            registry.register_arc(id, Arc::from(piece));
        }
        registry
    }

    /// Build a graph with a feedback cycle through a delay node:
    ///
    /// ```text
    /// source(0,0) → forward(1,0) → sink(2,0)
    ///                    ↑               |
    ///                    └── delay(0,1) ←─┘  (delay.value ← sink is NOT adjacent,
    ///                                          but we skip adjacency for this test
    ///                                          by making a simpler graph)
    /// ```
    ///
    /// Simplified: source(0,0) → delay(1,0) → sink(2,0)
    ///             delay.value ← source  (creates cycle: source→delay→sink, delay.value←source)
    ///
    /// Even simpler cycle: A(0,0) → delay(1,0), delay.value ← A
    /// This forms a cycle: A→delay, delay.value→A. But delay.value is a delay edge,
    /// so the cycle is broken.
    fn delay_cycle_graph() -> (Graph, EdgeId) {
        let source_pos = GridPos { col: 0, row: 0 };
        let delay_pos = GridPos { col: 1, row: 0 };
        let sink_pos = GridPos { col: 2, row: 0 };

        // source → delay (via delay's "default" param — not a delay edge)
        let edge_src_to_delay = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: delay_pos,
            to_param: "default".into(),
        };
        // delay → sink
        let edge_delay_to_sink = Edge {
            id: EdgeId::new(),
            from: delay_pos,
            to_node: sink_pos,
            to_param: "value".into(),
        };
        // sink → delay.value (this is the feedback / delay edge)
        // Note: In a real graph this would need adjacency. For the topo sort
        // test we just need the edges to exist — adjacency errors are separate.
        let feedback_edge = Edge {
            id: EdgeId::new(),
            from: sink_pos,
            to_node: delay_pos,
            to_param: "value".into(),
        };
        let feedback_id = feedback_edge.id.clone();

        let graph = Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.number_source".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    delay_pos,
                    Node {
                        piece_id: "core.delay".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([
                            ("default".into(), TileSide::LEFT),
                            ("value".into(), TileSide::RIGHT),
                        ]),
                        output_side: Some(TileSide::RIGHT),
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    sink_pos,
                    Node {
                        piece_id: "test.any_sink".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([
                (edge_src_to_delay.id.clone(), edge_src_to_delay),
                (edge_delay_to_sink.id.clone(), edge_delay_to_sink),
                (feedback_edge.id.clone(), feedback_edge),
            ]),
            name: "delay_cycle".into(),
            cols: 3,
            rows: 1,
        };

        (graph, feedback_id)
    }

    fn delay_node(inline_params: BTreeMap<String, Value>) -> Node {
        Node {
            piece_id: "core.delay".into(),
            inline_params,
            input_sides: BTreeMap::from([
                ("default".into(), TileSide::LEFT),
                ("value".into(), TileSide::RIGHT),
            ]),
            output_side: Some(TileSide::RIGHT),
            label: None,
            node_state: None,
        }
    }

    fn source_node(piece_id: &str) -> Node {
        Node {
            piece_id: piece_id.into(),
            inline_params: BTreeMap::new(),
            input_sides: BTreeMap::new(),
            output_side: None,
            label: None,
            node_state: None,
        }
    }

    fn delay_type_graph(
        default_inline: Option<Value>,
        default_source_piece: Option<&str>,
        feedback_source_piece: Option<&str>,
    ) -> (Graph, GridPos) {
        let left_pos = GridPos { col: 0, row: 0 };
        let delay_pos = GridPos { col: 1, row: 0 };
        let right_pos = GridPos { col: 2, row: 0 };

        let mut inline_params = BTreeMap::new();
        if let Some(value) = default_inline {
            inline_params.insert("default".into(), value);
        }

        let mut nodes = BTreeMap::from([(delay_pos, delay_node(inline_params))]);
        let mut edges = BTreeMap::new();

        if let Some(piece_id) = default_source_piece {
            nodes.insert(left_pos, source_node(piece_id));
            let edge = Edge {
                id: EdgeId::new(),
                from: left_pos,
                to_node: delay_pos,
                to_param: "default".into(),
            };
            edges.insert(edge.id.clone(), edge);
        }

        if let Some(piece_id) = feedback_source_piece {
            nodes.insert(right_pos, source_node(piece_id));
            let edge = Edge {
                id: EdgeId::new(),
                from: right_pos,
                to_node: delay_pos,
                to_param: "value".into(),
            };
            edges.insert(edge.id.clone(), edge);
        }

        (
            Graph {
                nodes,
                edges,
                name: "delay_type_graph".into(),
                cols: 3,
                rows: 1,
            },
            delay_pos,
        )
    }

    #[test]
    fn cycle_through_delay_node_is_legal() {
        let registry = delay_feedback_registry();
        let (graph, feedback_id) = delay_cycle_graph();
        let sem = semantic_pass(&graph, &registry);

        // The cycle goes through a delay edge, so there should be no Cycle error.
        assert!(
            !sem.diagnostics
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::Cycle { .. })),
            "cycle through delay should not produce a Cycle diagnostic"
        );

        // The feedback edge should be in delay_edges.
        assert!(sem.delay_edges.contains(&feedback_id));
    }

    #[test]
    fn delay_node_appears_before_feedback_source_in_eval_order() {
        let registry = delay_feedback_registry();
        let (graph, _) = delay_cycle_graph();
        let sem = semantic_pass(&graph, &registry);

        let delay_pos = GridPos { col: 1, row: 0 };
        let sink_pos = GridPos { col: 2, row: 0 };

        let delay_idx = sem.eval_order.iter().position(|p| *p == delay_pos);
        let sink_idx = sem.eval_order.iter().position(|p| *p == sink_pos);

        // The delay node should be scheduled before the sink (its feedback source).
        assert!(
            delay_idx < sink_idx,
            "delay node should appear before sink in eval order"
        );
    }

    #[test]
    fn direct_cycle_without_delay_still_errors() {
        let registry = delay_feedback_registry();

        let a_pos = GridPos { col: 0, row: 0 };
        let b_pos = GridPos { col: 1, row: 0 };

        let edge_a_to_b = Edge {
            id: EdgeId::new(),
            from: a_pos,
            to_node: b_pos,
            to_param: "value".into(),
        };
        let edge_b_to_a = Edge {
            id: EdgeId::new(),
            from: b_pos,
            to_node: a_pos,
            to_param: "value".into(),
        };

        let graph = Graph {
            nodes: BTreeMap::from([
                (
                    a_pos,
                    Node {
                        piece_id: "test.forward".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    b_pos,
                    Node {
                        piece_id: "test.forward".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([
                (edge_a_to_b.id.clone(), edge_a_to_b),
                (edge_b_to_a.id.clone(), edge_b_to_a),
            ]),
            name: "direct_cycle".into(),
            cols: 2,
            rows: 1,
        };

        let sem = semantic_pass(&graph, &registry);

        assert!(
            sem.diagnostics
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::Cycle { .. })),
            "direct cycle without delay should produce a Cycle diagnostic"
        );
    }

    #[test]
    fn delay_type_inference_uses_default_when_feedback_is_unresolved() {
        let registry = delay_feedback_registry();
        let (graph, _) = delay_cycle_graph();
        let sem = semantic_pass(&graph, &registry);

        let delay_pos = GridPos { col: 1, row: 0 };
        assert_eq!(sem.output_types.get(&delay_pos), Some(&PortType::number()));
    }

    #[test]
    fn delay_type_inference_uses_matching_default_and_feedback_type() {
        let registry = delay_feedback_registry();
        let (graph, delay_pos) = delay_type_graph(Some(json!(0)), None, Some("test.number_source"));
        let sem = semantic_pass(&graph, &registry);

        assert_eq!(sem.output_types.get(&delay_pos), Some(&PortType::number()));
        assert!(
            !sem.diagnostics.iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::DelayTypeMismatch { .. }
            ))
        );
    }

    #[test]
    fn delay_type_inference_reports_mismatched_default_and_feedback_types() {
        let registry = delay_feedback_registry();
        let (graph, delay_pos) =
            delay_type_graph(Some(json!("silence")), None, Some("test.number_source"));
        let sem = semantic_pass(&graph, &registry);

        assert_eq!(sem.output_types.get(&delay_pos), Some(&PortType::any()));
        assert!(sem.diagnostics.iter().any(|diagnostic| {
            matches!(
                &diagnostic.kind,
                DiagnosticKind::DelayTypeMismatch { default, feedback }
                    if *default == PortType::text() && *feedback == PortType::number()
            )
        }));
    }

    #[test]
    fn delay_type_inference_uses_default_type_without_feedback() {
        let registry = delay_feedback_registry();
        let (graph, delay_pos) = delay_type_graph(Some(json!(0)), None, None);
        let sem = semantic_pass(&graph, &registry);

        assert_eq!(sem.output_types.get(&delay_pos), Some(&PortType::number()));
    }

    #[test]
    fn delay_type_inference_uses_feedback_type_without_default() {
        let registry = delay_feedback_registry();
        let (graph, delay_pos) = delay_type_graph(None, None, Some("test.number_source"));
        let sem = semantic_pass(&graph, &registry);

        assert_eq!(sem.output_types.get(&delay_pos), Some(&PortType::number()));
    }

    #[test]
    fn delay_type_inference_reconciles_compatible_subtypes() {
        let registry = delay_feedback_registry();
        let (graph, delay_pos) =
            delay_type_graph(Some(json!("bd")), None, Some("test.pattern_source"));
        let sem = semantic_pass(&graph, &registry);

        assert_eq!(
            sem.output_types.get(&delay_pos),
            Some(&PortType::new("pattern"))
        );
    }
}
