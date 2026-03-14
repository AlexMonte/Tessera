//! Graph mutation, wiring validation, and auto-repair helpers.

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, DiagnosticKind};
use crate::graph::{Edge, Graph, GraphOp, Node};
use crate::piece::PieceDef;
use crate::piece_registry::PieceRegistry;
use crate::types::{EdgeId, GridPos, PortType, TileSide, adjacent_in_direction};

/// A machine-readable repair suggestion that a UI can present as a one-click fix.
///
/// Each variant maps directly to one or more `GraphOp`s via [`RepairSuggestion::to_ops`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RepairSuggestion {
    /// Move a node to a new position to satisfy adjacency.
    MoveNode { node: GridPos, to: GridPos },
    /// Rotate the source node's output side to face the target.
    SetOutputSide { position: GridPos, side: TileSide },
    /// Move a param's input side so it faces the source.
    SetParamSide {
        position: GridPos,
        param_id: String,
        side: TileSide,
    },
    /// Disconnect an existing edge to free up a param slot.
    DisconnectEdge { edge_id: EdgeId },
}

impl RepairSuggestion {
    /// Convert this suggestion into the graph operations needed to apply it.
    pub fn to_ops(&self) -> Vec<GraphOp> {
        match self {
            RepairSuggestion::MoveNode { node, to } => vec![GraphOp::NodeMove {
                from: node.clone(),
                to: to.clone(),
            }],
            RepairSuggestion::SetOutputSide { position, side } => {
                vec![GraphOp::OutputSetSide {
                    position: position.clone(),
                    side: *side,
                }]
            }
            RepairSuggestion::SetParamSide {
                position,
                param_id,
                side,
            } => vec![GraphOp::ParamSetSide {
                position: position.clone(),
                param_id: param_id.clone(),
                side: *side,
            }],
            RepairSuggestion::DisconnectEdge { edge_id } => vec![GraphOp::EdgeDisconnect {
                edge_id: edge_id.clone(),
            }],
        }
    }
}

fn invalid_op(site: Option<GridPos>, reason: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        DiagnosticKind::InvalidOperation {
            reason: reason.into(),
        },
        site,
    )
}

fn in_bounds(pos: &GridPos, graph: &Graph) -> bool {
    let cols = graph.cols as i32;
    let rows = graph.rows as i32;
    (0..cols).contains(&pos.col) && (0..rows).contains(&pos.row)
}

fn ensure_in_bounds(
    errors: &mut Vec<Diagnostic>,
    pos: &GridPos,
    label: &str,
    graph: &Graph,
) -> bool {
    if in_bounds(pos, graph) {
        return true;
    }
    errors.push(invalid_op(
        Some(pos.clone()),
        format!(
            "{} out of bounds at ({}, {}), allowed cols=[0..{}), rows=[0..{})",
            label, pos.col, pos.row, graph.cols, graph.rows
        ),
    ));
    false
}

#[derive(Debug, Default)]
/// Result of applying a batch of graph operations.
pub struct ApplyOpsOutcome {
    /// Edges removed as a side effect of the mutation batch.
    pub removed_edges: Vec<Edge>,
    /// Canonicalized ops that were actually applied.
    pub applied_ops: Vec<GraphOp>,
    /// Inverse ops that can restore the previous graph state.
    pub undo_ops: Vec<GraphOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Structured reason why an attempted edge connection was rejected.
pub enum EdgeConnectProbeReason {
    UnknownSourceNode,
    UnknownTargetNode,
    UnknownSourcePiece,
    UnknownTargetPiece,
    UnknownTargetParam,
    NotAdjacent,
    SideMismatch,
    OutputFromTerminal,
    NoParamOnTargetSide,
    TargetParamOccupied,
    TypeMismatch,
    NoCompatibleParam,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Result of probing an edge connection, with optional repair suggestions.
pub struct EdgeTargetParamProbe {
    pub to_param: Option<String>,
    pub reason: Option<EdgeConnectProbeReason>,
    pub detail: Option<String>,
    /// Machine-readable repair suggestions. Each entry maps to graph ops via `to_ops()`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<RepairSuggestion>,
}

impl EdgeTargetParamProbe {
    fn accept(param: String) -> Self {
        Self {
            to_param: Some(param),
            reason: None,
            detail: None,
            suggestions: Vec::new(),
        }
    }

    fn reject(reason: EdgeConnectProbeReason, detail: impl Into<String>) -> Self {
        Self {
            to_param: None,
            reason: Some(reason),
            detail: Some(detail.into()),
            suggestions: Vec::new(),
        }
    }

    fn reject_with(
        reason: EdgeConnectProbeReason,
        detail: impl Into<String>,
        suggestions: Vec<RepairSuggestion>,
    ) -> Self {
        Self {
            to_param: None,
            reason: Some(reason),
            detail: Some(detail.into()),
            suggestions,
        }
    }
}

struct EdgeConnectBase<'a> {
    from_node: &'a Node,
    to_node_ref: &'a Node,
    from_piece_def: PieceDef,
    to_piece_def: PieceDef,
}

fn resolve_edge_connect_base<'a>(
    graph: &'a Graph,
    registry: &PieceRegistry,
    from: &GridPos,
    to_node: &GridPos,
) -> Result<EdgeConnectBase<'a>, EdgeTargetParamProbe> {
    let Some(from_node) = graph.nodes.get(from) else {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::UnknownSourceNode,
            format!("missing source node at ({}, {})", from.col, from.row),
        ));
    };
    let Some(to_node_ref) = graph.nodes.get(to_node) else {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::UnknownTargetNode,
            format!("missing target node at ({}, {})", to_node.col, to_node.row),
        ));
    };
    let Some(from_piece) = registry.get(from_node.piece_id.as_str()) else {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::UnknownSourcePiece,
            format!("unknown source piece '{}'", from_node.piece_id),
        ));
    };
    let Some(to_piece) = registry.get(to_node_ref.piece_id.as_str()) else {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::UnknownTargetPiece,
            format!("unknown target piece '{}'", to_node_ref.piece_id),
        ));
    };
    Ok(EdgeConnectBase {
        from_node,
        to_node_ref,
        from_piece_def: from_piece.def().clone(),
        to_piece_def: to_piece.def().clone(),
    })
}

fn source_output_type_for_target_side(
    base: &EdgeConnectBase<'_>,
    from: &GridPos,
    target_side: TileSide,
) -> Result<PortType, EdgeTargetParamProbe> {
    if let Some(output_side) = node_output_side(base.from_node, &base.from_piece_def)
        && !output_side.faces(target_side)
    {
        return Err(EdgeTargetParamProbe::reject_with(
            EdgeConnectProbeReason::SideMismatch,
            format!(
                "side mismatch: source {:?} does not face target {:?}",
                output_side, target_side
            ),
            vec![RepairSuggestion::SetOutputSide {
                position: from.clone(),
                side: target_side.opposite(),
            }],
        ));
    }
    let Some(output_type) = base.from_piece_def.output_type.as_ref() else {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::OutputFromTerminal,
            "cannot connect output from terminal piece",
        ));
    };
    Ok(output_type.clone())
}

fn side_from_to_node(to_node: &GridPos, from: &GridPos) -> Option<TileSide> {
    if from.col == to_node.col + 1 && from.row == to_node.row {
        return Some(TileSide::RIGHT);
    }
    if from.col == to_node.col - 1 && from.row == to_node.row {
        return Some(TileSide::LEFT);
    }
    if from.col == to_node.col && from.row == to_node.row - 1 {
        return Some(TileSide::TOP);
    }
    if from.col == to_node.col && from.row == to_node.row + 1 {
        return Some(TileSide::BOTTOM);
    }
    None
}

/// Pick the best matching target param for an edge based on adjacency, side, and type.
pub fn pick_target_param_for_edge(
    graph: &Graph,
    registry: &PieceRegistry,
    from: &GridPos,
    to_node: &GridPos,
) -> EdgeTargetParamProbe {
    let base = match resolve_edge_connect_base(graph, registry, from, to_node) {
        Ok(base) => base,
        Err(err) => return err,
    };

    let Some(target_side) = side_from_to_node(to_node, from) else {
        // Suggest moving source to the first open adjacent position with a compatible param.
        let mut suggestions = Vec::new();
        for param in &base.to_piece_def.params {
            let param_side = node_param_side(base.to_node_ref, param.id.as_str(), param.side);
            if param_side == TileSide::NONE {
                continue;
            }
            let adj = adjacent_in_direction(to_node, &param_side);
            if adj != *to_node && !graph.nodes.contains_key(&adj) {
                suggestions.push(RepairSuggestion::MoveNode {
                    node: from.clone(),
                    to: adj,
                });
                break;
            }
        }
        return EdgeTargetParamProbe::reject_with(
            EdgeConnectProbeReason::NotAdjacent,
            format!(
                "edge must come from an adjacent cell to ({}, {})",
                to_node.col, to_node.row
            ),
            suggestions,
        );
    };

    let source_output_type = match source_output_type_for_target_side(&base, from, target_side) {
        Ok(output_type) => output_type,
        Err(err) => return err,
    };

    let mut saw_side_candidate = false;
    let mut saw_open_side_candidate = false;
    for param in &base.to_piece_def.params {
        let param_side = node_param_side(base.to_node_ref, param.id.as_str(), param.side);
        if param_side != target_side {
            continue;
        }
        saw_side_candidate = true;
        if graph
            .edges
            .values()
            .any(|edge| edge.to_node == *to_node && edge.to_param == param.id)
        {
            continue;
        }
        saw_open_side_candidate = true;
        if param.schema.accepts(&source_output_type) {
            return EdgeTargetParamProbe::accept(param.id.clone());
        }
    }

    if !saw_side_candidate {
        // Suggest moving a type-compatible param to the connecting side.
        let suggestions = base
            .to_piece_def
            .params
            .iter()
            .filter(|p| p.schema.accepts(&source_output_type))
            .take(1)
            .map(|p| RepairSuggestion::SetParamSide {
                position: to_node.clone(),
                param_id: p.id.clone(),
                side: target_side,
            })
            .collect();
        return EdgeTargetParamProbe::reject_with(
            EdgeConnectProbeReason::NoParamOnTargetSide,
            format!(
                "target has no input on side {:?} for source ({}, {})",
                target_side, from.col, from.row
            ),
            suggestions,
        );
    }
    if !saw_open_side_candidate {
        // Suggest disconnecting one of the occupied edges on this side.
        let suggestions = base
            .to_piece_def
            .params
            .iter()
            .filter(|p| node_param_side(base.to_node_ref, p.id.as_str(), p.side) == target_side)
            .filter_map(|p| {
                graph
                    .edges
                    .values()
                    .find(|e| e.to_node == *to_node && e.to_param == p.id)
                    .map(|e| RepairSuggestion::DisconnectEdge {
                        edge_id: e.id.clone(),
                    })
            })
            .collect();
        return EdgeTargetParamProbe::reject_with(
            EdgeConnectProbeReason::TargetParamOccupied,
            format!(
                "all target params on side {:?} are already connected",
                target_side
            ),
            suggestions,
        );
    }
    EdgeTargetParamProbe::reject(
        EdgeConnectProbeReason::TypeMismatch,
        "type mismatch: no open target param on the connecting side accepts the source output type",
    )
}

/// Validate an explicit edge connection to a concrete target param.
pub fn validate_edge_connect(
    graph: &Graph,
    registry: &PieceRegistry,
    from: &GridPos,
    to_node: &GridPos,
    to_param: &str,
) -> Result<(), EdgeTargetParamProbe> {
    let base = resolve_edge_connect_base(graph, registry, from, to_node)?;

    let Some(param_def) = base
        .to_piece_def
        .params
        .iter()
        .find(|param| param.id == to_param)
    else {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::UnknownTargetParam,
            format!("unknown target param '{}'", to_param),
        ));
    };

    let target_side = node_param_side(base.to_node_ref, to_param, param_def.side);
    let expected = adjacent_in_direction(to_node, &target_side);
    if expected != *from {
        return Err(EdgeTargetParamProbe::reject_with(
            EdgeConnectProbeReason::NotAdjacent,
            format!(
                "edge must come from adjacent {:?} cell ({}, {})",
                target_side, expected.col, expected.row
            ),
            vec![RepairSuggestion::MoveNode {
                node: from.clone(),
                to: expected,
            }],
        ));
    }

    let source_output_type = source_output_type_for_target_side(&base, from, target_side)?;
    if let Some(existing) = graph
        .edges
        .values()
        .find(|edge| edge.to_node == *to_node && edge.to_param == *to_param)
    {
        return Err(EdgeTargetParamProbe::reject_with(
            EdgeConnectProbeReason::TargetParamOccupied,
            format!("target param '{}' already connected", to_param),
            vec![RepairSuggestion::DisconnectEdge {
                edge_id: existing.id.clone(),
            }],
        ));
    }
    if !param_def.schema.accepts(&source_output_type) {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::TypeMismatch,
            format!(
                "type mismatch: expected {:?}, got {:?}",
                param_def.schema.expected_port_type(),
                source_output_type
            ),
        ));
    }
    Ok(())
}

/// Probe an edge using either an explicit param or automatic target-param selection.
pub fn probe_edge_connect(
    graph: &Graph,
    registry: &PieceRegistry,
    from: &GridPos,
    to_node: &GridPos,
    to_param: Option<&str>,
) -> EdgeTargetParamProbe {
    if let Some(to_param) = to_param {
        return match validate_edge_connect(graph, registry, from, to_node, to_param) {
            Ok(()) => EdgeTargetParamProbe::accept(to_param.to_string()),
            Err(reject) => reject,
        };
    }
    pick_target_param_for_edge(graph, registry, from, to_node)
}

fn edge_is_still_adjacent(edge: &Edge, graph: &Graph, registry: &PieceRegistry) -> bool {
    let Some(target_node) = graph.nodes.get(&edge.to_node) else {
        return false;
    };
    let Some(target_piece) = registry.get(target_node.piece_id.as_str()) else {
        return true;
    };
    let Some(param_def) = target_piece
        .def()
        .params
        .iter()
        .find(|param| param.id == edge.to_param)
    else {
        return true;
    };
    let expected = adjacent_in_direction(
        &edge.to_node,
        &node_param_side(target_node, edge.to_param.as_str(), param_def.side),
    );
    expected == edge.from
}

fn node_param_side(node: &Node, param_id: &str, fallback: TileSide) -> TileSide {
    node.input_sides.get(param_id).copied().unwrap_or(fallback)
}

fn node_output_side(node: &Node, piece: &PieceDef) -> Option<TileSide> {
    if piece.output_type.is_none() {
        None
    } else {
        node.output_side.or(piece.output_side)
    }
}

fn edge_is_still_valid(edge: &Edge, graph: &Graph, registry: &PieceRegistry) -> bool {
    let Ok(base) = resolve_edge_connect_base(graph, registry, &edge.from, &edge.to_node) else {
        return false;
    };
    let Some(param_def) = base
        .to_piece_def
        .params
        .iter()
        .find(|param| param.id == edge.to_param)
    else {
        return false;
    };
    let target_side = node_param_side(base.to_node_ref, edge.to_param.as_str(), param_def.side);
    if target_side == TileSide::NONE {
        return false;
    }
    let expected = adjacent_in_direction(&edge.to_node, &target_side);
    if expected != edge.from {
        return false;
    }
    let Ok(source_output_type) = source_output_type_for_target_side(&base, &edge.from, target_side)
    else {
        return false;
    };
    param_def.schema.accepts(&source_output_type)
}

fn prune_invalid_edges_for_node(
    graph: &mut Graph,
    registry: &PieceRegistry,
    node_pos: &GridPos,
    removed_edges: &mut Vec<Edge>,
) {
    let mut remove_ids = Vec::new();
    for (edge_id, edge) in &graph.edges {
        if (edge.from == *node_pos || edge.to_node == *node_pos)
            && !edge_is_still_adjacent(edge, graph, registry)
        {
            remove_ids.push(edge_id.clone());
        }
    }
    for edge_id in remove_ids {
        if let Some(edge) = graph.edges.remove(&edge_id) {
            removed_edges.push(edge);
        }
    }
}

fn prune_invalid_touching_edges(
    graph: &mut Graph,
    registry: &PieceRegistry,
    node_pos: &GridPos,
    removed_edges: &mut Vec<Edge>,
) {
    let mut remove_ids = Vec::new();
    for (edge_id, edge) in &graph.edges {
        if (edge.from == *node_pos || edge.to_node == *node_pos)
            && !edge_is_still_valid(edge, graph, registry)
        {
            remove_ids.push(edge_id.clone());
        }
    }
    for edge_id in remove_ids {
        if let Some(edge) = graph.edges.remove(&edge_id) {
            removed_edges.push(edge);
        }
    }
}

fn auto_wire_node(
    graph: &mut Graph,
    registry: &PieceRegistry,
    position: &GridPos,
    outcome: &mut ApplyOpsOutcome,
    inverse_chunks: &mut Vec<Vec<GraphOp>>,
) {
    let Some(node_snapshot) = graph.nodes.get(position).cloned() else {
        return;
    };
    let Some(piece) = registry.get(node_snapshot.piece_id.as_str()) else {
        return;
    };

    let mut removed_for_node = Vec::new();
    prune_invalid_touching_edges(graph, registry, position, &mut removed_for_node);

    let mut added_edges = Vec::new();

    for param in &piece.def().params {
        if graph
            .edges
            .values()
            .any(|edge| edge.to_node == *position && edge.to_param == param.id)
        {
            continue;
        }
        let target_side = node_param_side(&node_snapshot, param.id.as_str(), param.side);
        if target_side == TileSide::NONE {
            continue;
        }
        let source_pos = adjacent_in_direction(position, &target_side);
        if source_pos == *position || !graph.nodes.contains_key(&source_pos) {
            continue;
        }
        if validate_edge_connect(graph, registry, &source_pos, position, param.id.as_str()).is_ok()
        {
            let edge = Edge {
                id: EdgeId::new(),
                from: source_pos,
                to_node: position.clone(),
                to_param: param.id.clone(),
            };
            graph.edges.insert(edge.id.clone(), edge.clone());
            added_edges.push(edge);
        }
    }

    if let Some(output_side) = node_output_side(&node_snapshot, piece.def())
        && output_side != TileSide::NONE
    {
        let target_pos = adjacent_in_direction(position, &output_side);
        if target_pos != *position && graph.nodes.contains_key(&target_pos) {
            let probe = probe_edge_connect(graph, registry, position, &target_pos, None);
            if let Some(to_param) = probe.to_param {
                let edge = Edge {
                    id: EdgeId::new(),
                    from: position.clone(),
                    to_node: target_pos,
                    to_param,
                };
                graph.edges.insert(edge.id.clone(), edge.clone());
                added_edges.push(edge);
            }
        }
    }

    if removed_for_node.is_empty() && added_edges.is_empty() {
        return;
    }

    let mut inverse = Vec::new();
    for edge in &removed_for_node {
        outcome.removed_edges.push(edge.clone());
        outcome.applied_ops.push(GraphOp::EdgeDisconnect {
            edge_id: edge.id.clone(),
        });
        inverse.push(edge_connect_from(edge));
    }
    for edge in &added_edges {
        outcome.applied_ops.push(GraphOp::EdgeConnect {
            edge_id: Some(edge.id.clone()),
            from: edge.from.clone(),
            to_node: edge.to_node.clone(),
            to_param: edge.to_param.clone(),
        });
        inverse.insert(
            0,
            GraphOp::EdgeDisconnect {
                edge_id: edge.id.clone(),
            },
        );
    }
    inverse_chunks.push(inverse);
}

fn edge_connect_from(edge: &Edge) -> GraphOp {
    GraphOp::EdgeConnect {
        edge_id: Some(edge.id.clone()),
        from: edge.from.clone(),
        to_node: edge.to_node.clone(),
        to_param: edge.to_param.clone(),
    }
}

fn swap_rewrite_pos(pos: &mut GridPos, a: &GridPos, b: &GridPos) {
    if *pos == *a {
        *pos = b.clone();
    } else if *pos == *b {
        *pos = a.clone();
    }
}

/// Apply a batch of graph operations, collecting inverse ops for undo/redo.
pub fn apply_ops_to_graph(
    graph: &mut Graph,
    registry: &PieceRegistry,
    ops: &[GraphOp],
) -> Result<ApplyOpsOutcome, Vec<Diagnostic>> {
    let mut errors = Vec::<Diagnostic>::new();
    let mut outcome = ApplyOpsOutcome::default();
    let mut inverse_chunks = Vec::<Vec<GraphOp>>::new();

    for op in ops {
        match op {
            GraphOp::NodePlace {
                position,
                piece_id,
                inline_params,
            } => {
                if !ensure_in_bounds(&mut errors, position, "node_place", graph) {
                    continue;
                }
                if graph.nodes.contains_key(position) {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!(
                            "node already exists at ({}, {})",
                            position.col, position.row
                        ),
                    ));
                    continue;
                }
                let Some(piece) = registry.get(piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown piece id '{}'", piece_id),
                    ));
                    continue;
                };
                let mut inline_is_valid = true;
                for (param_id, value) in inline_params {
                    let Some(param_def) = piece
                        .def()
                        .params
                        .iter()
                        .find(|param| param.id == *param_id)
                    else {
                        errors.push(invalid_op(
                            Some(position.clone()),
                            format!("unknown inline param '{}'", param_id),
                        ));
                        inline_is_valid = false;
                        continue;
                    };
                    if !param_def.schema.can_inline() {
                        errors.push(invalid_op(
                            Some(position.clone()),
                            format!("inline value is not allowed for '{}'", param_id),
                        ));
                        inline_is_valid = false;
                        continue;
                    }
                    if !param_def.schema.validate_inline_value(value) {
                        errors.push(invalid_op(
                            Some(position.clone()),
                            format!("inline value has wrong type for '{}'", param_id),
                        ));
                        inline_is_valid = false;
                    }
                }
                if !inline_is_valid {
                    continue;
                }
                graph.nodes.insert(
                    position.clone(),
                    Node {
                        piece_id: piece_id.clone(),
                        inline_params: inline_params.clone(),
                        input_sides: Default::default(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                );
                outcome.applied_ops.push(GraphOp::NodePlace {
                    position: position.clone(),
                    piece_id: piece_id.clone(),
                    inline_params: inline_params.clone(),
                });
                inverse_chunks.push(vec![GraphOp::NodeRemove {
                    position: position.clone(),
                }]);
            }
            GraphOp::NodeMove { from, to } => {
                if from == to {
                    continue;
                }
                if !ensure_in_bounds(&mut errors, from, "node_move source", graph) {
                    continue;
                }
                if !ensure_in_bounds(&mut errors, to, "node_move target", graph) {
                    continue;
                }
                if graph.nodes.contains_key(to) {
                    errors.push(invalid_op(
                        Some(to.clone()),
                        format!("target cell already occupied at ({}, {})", to.col, to.row),
                    ));
                    continue;
                }
                let Some(node) = graph.nodes.remove(from) else {
                    errors.push(invalid_op(
                        Some(from.clone()),
                        format!("missing source node at ({}, {})", from.col, from.row),
                    ));
                    continue;
                };
                graph.nodes.insert(to.clone(), node);

                for edge in graph.edges.values_mut() {
                    if edge.from == *from {
                        edge.from = to.clone();
                    }
                    if edge.to_node == *from {
                        edge.to_node = to.clone();
                    }
                }
                let mut removed_for_move = Vec::new();
                prune_invalid_edges_for_node(graph, registry, to, &mut removed_for_move);
                outcome
                    .removed_edges
                    .extend(removed_for_move.iter().cloned());

                let mut inverse = vec![GraphOp::NodeMove {
                    from: to.clone(),
                    to: from.clone(),
                }];
                for removed in &removed_for_move {
                    let mut restored = removed.clone();
                    if restored.from == *to {
                        restored.from = from.clone();
                    }
                    if restored.to_node == *to {
                        restored.to_node = from.clone();
                    }
                    inverse.push(edge_connect_from(&restored));
                }
                inverse_chunks.push(inverse);

                outcome.applied_ops.push(GraphOp::NodeMove {
                    from: from.clone(),
                    to: to.clone(),
                });
            }
            GraphOp::NodeSwap { a, b } => {
                if a == b {
                    continue;
                }
                if !ensure_in_bounds(&mut errors, a, "node_swap position a", graph) {
                    continue;
                }
                if !ensure_in_bounds(&mut errors, b, "node_swap position b", graph) {
                    continue;
                }
                if !graph.nodes.contains_key(a) {
                    errors.push(invalid_op(
                        Some(a.clone()),
                        format!("missing node at ({}, {})", a.col, a.row),
                    ));
                    continue;
                }
                if !graph.nodes.contains_key(b) {
                    errors.push(invalid_op(
                        Some(b.clone()),
                        format!("missing node at ({}, {})", b.col, b.row),
                    ));
                    continue;
                }

                let node_a = graph
                    .nodes
                    .remove(a)
                    .expect("checked node existence before swap");
                let node_b = graph
                    .nodes
                    .remove(b)
                    .expect("checked node existence before swap");
                graph.nodes.insert(a.clone(), node_b);
                graph.nodes.insert(b.clone(), node_a);

                for edge in graph.edges.values_mut() {
                    swap_rewrite_pos(&mut edge.from, a, b);
                    swap_rewrite_pos(&mut edge.to_node, a, b);
                }

                let mut removed_for_swap = Vec::new();
                prune_invalid_edges_for_node(graph, registry, a, &mut removed_for_swap);
                prune_invalid_edges_for_node(graph, registry, b, &mut removed_for_swap);
                outcome
                    .removed_edges
                    .extend(removed_for_swap.iter().cloned());

                let mut inverse = vec![GraphOp::NodeSwap {
                    a: a.clone(),
                    b: b.clone(),
                }];
                for removed in &removed_for_swap {
                    let mut restored = removed.clone();
                    swap_rewrite_pos(&mut restored.from, a, b);
                    swap_rewrite_pos(&mut restored.to_node, a, b);
                    inverse.push(edge_connect_from(&restored));
                }
                inverse_chunks.push(inverse);

                outcome.applied_ops.push(GraphOp::NodeSwap {
                    a: a.clone(),
                    b: b.clone(),
                });
            }
            GraphOp::NodeRemove { position } => {
                if !ensure_in_bounds(&mut errors, position, "node_remove", graph) {
                    continue;
                }
                let Some(removed_node) = graph.nodes.remove(position) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let mut remove_ids = Vec::new();
                for (edge_id, edge) in &graph.edges {
                    if edge.from == *position || edge.to_node == *position {
                        remove_ids.push(edge_id.clone());
                    }
                }
                let mut removed_for_node = Vec::new();
                for edge_id in remove_ids {
                    if let Some(edge) = graph.edges.remove(&edge_id) {
                        removed_for_node.push(edge);
                    }
                }
                outcome
                    .removed_edges
                    .extend(removed_for_node.iter().cloned());

                let mut inverse = vec![GraphOp::NodePlace {
                    position: position.clone(),
                    piece_id: removed_node.piece_id.clone(),
                    inline_params: removed_node.inline_params.clone(),
                }];
                if removed_node.label.is_some() {
                    inverse.push(GraphOp::NodeSetLabel {
                        position: position.clone(),
                        label: removed_node.label.clone(),
                    });
                }
                if removed_node.node_state.is_some() {
                    inverse.push(GraphOp::NodeSetState {
                        position: position.clone(),
                        state: removed_node.node_state.clone(),
                    });
                }
                for (param_id, side) in &removed_node.input_sides {
                    inverse.push(GraphOp::ParamSetSide {
                        position: position.clone(),
                        param_id: param_id.clone(),
                        side: *side,
                    });
                }
                if let Some(side) = removed_node.output_side {
                    inverse.push(GraphOp::OutputSetSide {
                        position: position.clone(),
                        side,
                    });
                }
                inverse.extend(removed_for_node.iter().map(edge_connect_from));
                inverse_chunks.push(inverse);

                outcome.applied_ops.push(GraphOp::NodeRemove {
                    position: position.clone(),
                });
            }
            GraphOp::EdgeConnect {
                edge_id,
                from,
                to_node,
                to_param,
            } => {
                let source_ok = ensure_in_bounds(&mut errors, from, "edge_connect source", graph);
                let target_ok =
                    ensure_in_bounds(&mut errors, to_node, "edge_connect target", graph);
                if !(source_ok && target_ok) {
                    continue;
                }
                if !graph.nodes.contains_key(from) {
                    errors.push(invalid_op(
                        Some(from.clone()),
                        format!("missing source node at ({}, {})", from.col, from.row),
                    ));
                    continue;
                }
                if !graph.nodes.contains_key(to_node) {
                    errors.push(invalid_op(
                        Some(to_node.clone()),
                        format!("missing target node at ({}, {})", to_node.col, to_node.row),
                    ));
                    continue;
                }
                if to_param.trim().is_empty() {
                    errors.push(invalid_op(
                        Some(to_node.clone()),
                        "edge target param cannot be empty",
                    ));
                    continue;
                }
                let restoring_exact_edge = edge_id.is_some();
                if let Err(reject) =
                    validate_edge_connect(graph, registry, from, to_node, to_param.as_str())
                    && !restoring_exact_edge
                {
                    errors.push(invalid_op(
                        Some(to_node.clone()),
                        reject
                            .detail
                            .unwrap_or_else(|| "edge connection rejected".to_string()),
                    ));
                    continue;
                }

                let edge_id = edge_id.clone().unwrap_or_else(EdgeId::new);
                if graph.edges.contains_key(&edge_id) {
                    errors.push(invalid_op(
                        Some(to_node.clone()),
                        format!("edge id '{}' already exists", edge_id.0),
                    ));
                    continue;
                }
                let edge = Edge {
                    id: edge_id.clone(),
                    from: from.clone(),
                    to_node: to_node.clone(),
                    to_param: to_param.clone(),
                };
                graph.edges.insert(edge.id.clone(), edge);
                outcome.applied_ops.push(GraphOp::EdgeConnect {
                    edge_id: Some(edge_id.clone()),
                    from: from.clone(),
                    to_node: to_node.clone(),
                    to_param: to_param.clone(),
                });
                inverse_chunks.push(vec![GraphOp::EdgeDisconnect { edge_id }]);
            }
            GraphOp::EdgeDisconnect { edge_id } => {
                let Some(disconnected) = graph.edges.remove(edge_id) else {
                    errors.push(invalid_op(None, format!("missing edge '{}'", edge_id.0)));
                    continue;
                };
                outcome.applied_ops.push(GraphOp::EdgeDisconnect {
                    edge_id: edge_id.clone(),
                });
                inverse_chunks.push(vec![edge_connect_from(&disconnected)]);
            }
            GraphOp::ParamSetInline {
                position,
                param_id,
                value,
            } => {
                if !ensure_in_bounds(&mut errors, position, "param_set_inline", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                let Some(param_def) = piece.def().params.iter().find(|item| item.id == *param_id)
                else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown param '{}'", param_id),
                    ));
                    continue;
                };
                if !param_def.schema.can_inline() {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("inline value is not allowed for '{}'", param_id),
                    ));
                    continue;
                }
                if !param_def.schema.validate_inline_value(value) {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("inline value has wrong type for '{}'", param_id),
                    ));
                    continue;
                }
                if target_node
                    .inline_params
                    .get(param_id)
                    .is_some_and(|existing| existing == value)
                {
                    continue;
                }
                let previous = target_node
                    .inline_params
                    .insert(param_id.clone(), value.clone());
                outcome.applied_ops.push(GraphOp::ParamSetInline {
                    position: position.clone(),
                    param_id: param_id.clone(),
                    value: value.clone(),
                });
                if let Some(previous) = previous {
                    inverse_chunks.push(vec![GraphOp::ParamSetInline {
                        position: position.clone(),
                        param_id: param_id.clone(),
                        value: previous,
                    }]);
                } else {
                    inverse_chunks.push(vec![GraphOp::ParamClearInline {
                        position: position.clone(),
                        param_id: param_id.clone(),
                    }]);
                }
            }
            GraphOp::ParamClearInline { position, param_id } => {
                if !ensure_in_bounds(&mut errors, position, "param_clear_inline", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                if !piece.def().params.iter().any(|item| item.id == *param_id) {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown param '{}'", param_id),
                    ));
                    continue;
                }
                let previous = target_node.inline_params.remove(param_id);
                let Some(previous) = previous else {
                    continue;
                };
                outcome.applied_ops.push(GraphOp::ParamClearInline {
                    position: position.clone(),
                    param_id: param_id.clone(),
                });
                inverse_chunks.push(vec![GraphOp::ParamSetInline {
                    position: position.clone(),
                    param_id: param_id.clone(),
                    value: previous,
                }]);
            }
            GraphOp::ParamSetSide {
                position,
                param_id,
                side,
            } => {
                if !ensure_in_bounds(&mut errors, position, "param_set_side", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                if !piece.def().params.iter().any(|param| param.id == *param_id) {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown param '{}'", param_id),
                    ));
                    continue;
                }
                if target_node
                    .input_sides
                    .get(param_id)
                    .is_some_and(|existing| existing == side)
                {
                    continue;
                }
                let previous = target_node.input_sides.insert(param_id.clone(), *side);
                outcome.applied_ops.push(GraphOp::ParamSetSide {
                    position: position.clone(),
                    param_id: param_id.clone(),
                    side: *side,
                });
                if let Some(previous) = previous {
                    inverse_chunks.push(vec![GraphOp::ParamSetSide {
                        position: position.clone(),
                        param_id: param_id.clone(),
                        side: previous,
                    }]);
                } else {
                    inverse_chunks.push(vec![GraphOp::ParamClearSide {
                        position: position.clone(),
                        param_id: param_id.clone(),
                    }]);
                }
            }
            GraphOp::ParamClearSide { position, param_id } => {
                if !ensure_in_bounds(&mut errors, position, "param_clear_side", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                if !piece.def().params.iter().any(|param| param.id == *param_id) {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown param '{}'", param_id),
                    ));
                    continue;
                }
                let previous = target_node.input_sides.remove(param_id);
                let Some(previous) = previous else {
                    continue;
                };
                outcome.applied_ops.push(GraphOp::ParamClearSide {
                    position: position.clone(),
                    param_id: param_id.clone(),
                });
                inverse_chunks.push(vec![GraphOp::ParamSetSide {
                    position: position.clone(),
                    param_id: param_id.clone(),
                    side: previous,
                }]);
            }
            GraphOp::OutputSetSide { position, side } => {
                if !ensure_in_bounds(&mut errors, position, "output_set_side", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                if piece.def().output_type.is_none() {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        "cannot set output side on terminal piece",
                    ));
                    continue;
                }
                if target_node.output_side == Some(*side) {
                    continue;
                }
                let previous = target_node.output_side.replace(*side);
                outcome.applied_ops.push(GraphOp::OutputSetSide {
                    position: position.clone(),
                    side: *side,
                });
                if let Some(previous) = previous {
                    inverse_chunks.push(vec![GraphOp::OutputSetSide {
                        position: position.clone(),
                        side: previous,
                    }]);
                } else {
                    inverse_chunks.push(vec![GraphOp::OutputClearSide {
                        position: position.clone(),
                    }]);
                }
            }
            GraphOp::OutputClearSide { position } => {
                if !ensure_in_bounds(&mut errors, position, "output_clear_side", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                if piece.def().output_type.is_none() {
                    continue;
                }
                let previous = target_node.output_side.take();
                let Some(previous) = previous else {
                    continue;
                };
                outcome.applied_ops.push(GraphOp::OutputClearSide {
                    position: position.clone(),
                });
                inverse_chunks.push(vec![GraphOp::OutputSetSide {
                    position: position.clone(),
                    side: previous,
                }]);
            }
            GraphOp::NodeAutoWire { position } => {
                if !ensure_in_bounds(&mut errors, position, "node_auto_wire", graph) {
                    continue;
                }
                if !graph.nodes.contains_key(position) {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                }
                auto_wire_node(graph, registry, position, &mut outcome, &mut inverse_chunks);
            }
            GraphOp::NodeSetLabel { position, label } => {
                if !ensure_in_bounds(&mut errors, position, "node_set_label", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let previous = target_node.label.clone();
                target_node.label = label.clone();
                outcome.applied_ops.push(GraphOp::NodeSetLabel {
                    position: position.clone(),
                    label: label.clone(),
                });
                inverse_chunks.push(vec![GraphOp::NodeSetLabel {
                    position: position.clone(),
                    label: previous,
                }]);
            }
            GraphOp::NodeSetState { position, state } => {
                if !ensure_in_bounds(&mut errors, position, "node_set_state", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(position.clone()),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let previous = target_node.node_state.clone();
                target_node.node_state = state.clone();
                outcome.applied_ops.push(GraphOp::NodeSetState {
                    position: position.clone(),
                    state: state.clone(),
                });
                inverse_chunks.push(vec![GraphOp::NodeSetState {
                    position: position.clone(),
                    state: previous,
                }]);
            }
            GraphOp::ResizeGrid { cols, rows } => {
                if *cols == 0 || *rows == 0 {
                    errors.push(invalid_op(None, "grid dimensions must be at least 1x1"));
                    continue;
                }
                let previous_cols = graph.cols;
                let previous_rows = graph.rows;
                if *cols == previous_cols && *rows == previous_rows {
                    continue;
                }
                graph.cols = *cols;
                graph.rows = *rows;
                let removed_nodes = graph
                    .nodes
                    .iter()
                    .filter_map(|(pos, node)| {
                        if in_bounds(pos, graph) {
                            None
                        } else {
                            Some((pos.clone(), node.clone()))
                        }
                    })
                    .collect::<Vec<_>>();
                for (pos, _) in &removed_nodes {
                    graph.nodes.remove(pos);
                }

                let mut remove_ids = Vec::new();
                for (edge_id, edge) in &graph.edges {
                    let endpoints_exist = graph.nodes.contains_key(&edge.from)
                        && graph.nodes.contains_key(&edge.to_node);
                    if !endpoints_exist
                        || !in_bounds(&edge.from, graph)
                        || !in_bounds(&edge.to_node, graph)
                    {
                        remove_ids.push(edge_id.clone());
                    }
                }
                let mut removed_for_resize = Vec::new();
                for edge_id in remove_ids {
                    if let Some(edge) = graph.edges.remove(&edge_id) {
                        removed_for_resize.push(edge);
                    }
                }
                outcome
                    .removed_edges
                    .extend(removed_for_resize.iter().cloned());

                outcome.applied_ops.push(GraphOp::ResizeGrid {
                    cols: *cols,
                    rows: *rows,
                });
                let mut inverse = vec![GraphOp::ResizeGrid {
                    cols: previous_cols,
                    rows: previous_rows,
                }];
                for (position, node) in &removed_nodes {
                    inverse.push(GraphOp::NodePlace {
                        position: position.clone(),
                        piece_id: node.piece_id.clone(),
                        inline_params: node.inline_params.clone(),
                    });
                    if node.label.is_some() {
                        inverse.push(GraphOp::NodeSetLabel {
                            position: position.clone(),
                            label: node.label.clone(),
                        });
                    }
                    if node.node_state.is_some() {
                        inverse.push(GraphOp::NodeSetState {
                            position: position.clone(),
                            state: node.node_state.clone(),
                        });
                    }
                    for (param_id, side) in &node.input_sides {
                        inverse.push(GraphOp::ParamSetSide {
                            position: position.clone(),
                            param_id: param_id.clone(),
                            side: *side,
                        });
                    }
                    if let Some(side) = node.output_side {
                        inverse.push(GraphOp::OutputSetSide {
                            position: position.clone(),
                            side,
                        });
                    }
                }
                inverse.extend(removed_for_resize.iter().map(edge_connect_from));
                inverse_chunks.push(inverse);
            }
        }
    }

    if errors.is_empty() {
        outcome.undo_ops = inverse_chunks
            .into_iter()
            .rev()
            .flat_map(|chunk| chunk.into_iter())
            .collect();
        Ok(outcome)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::Value;

    use super::*;
    use crate::code_expr::CodeExpr;
    use crate::diagnostics::DiagnosticKind;
    use crate::piece::{
        ParamDef, ParamInlineMode, ParamSchema, ParamValueKind, Piece, PieceDef, PieceInputs,
    };
    use crate::piece_registry::PieceRegistry;
    use crate::types::{EdgeId, PieceCategory, PortType, TileSide};

    struct TestPiece {
        def: PieceDef,
    }

    fn pattern_port() -> PortType {
        PortType::new("pattern")
    }

    fn pattern_schema() -> ParamSchema {
        ParamSchema::Custom {
            port_type: pattern_port(),
            value_kind: ParamValueKind::Text,
            default: None,
            can_inline: false,
            inline_mode: ParamInlineMode::Raw,
            min: None,
            max: None,
        }
    }

    impl TestPiece {
        fn source(id: &str) -> Self {
            Self {
                def: PieceDef {
                    id: id.to_string(),
                    label: id.to_string(),
                    category: PieceCategory::Generator,
                    params: vec![ParamDef {
                        id: "value".to_string(),
                        label: "value".to_string(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Text {
                            default: "bd".to_string(),
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                    }],
                    output_type: Some(pattern_port()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                },
            }
        }

        fn transform(id: &str) -> Self {
            Self {
                def: PieceDef {
                    id: id.to_string(),
                    label: id.to_string(),
                    category: PieceCategory::Transform,
                    params: vec![ParamDef {
                        id: "pattern".to_string(),
                        label: "pattern".to_string(),
                        side: TileSide::LEFT,
                        schema: pattern_schema(),
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: true,
                    }],
                    output_type: Some(pattern_port()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                },
            }
        }

        fn terminal(id: &str) -> Self {
            Self {
                def: PieceDef {
                    id: id.to_string(),
                    label: id.to_string(),
                    category: PieceCategory::Output,
                    params: vec![ParamDef {
                        id: "pattern".to_string(),
                        label: "pattern".to_string(),
                        side: TileSide::LEFT,
                        schema: pattern_schema(),
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

    impl Piece for TestPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(
            &self,
            _inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
        ) -> CodeExpr {
            CodeExpr::Raw(self.def.id.clone())
        }
    }

    fn sample_registry() -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(TestPiece::source("strudel.sound"));
        registry.register(TestPiece::transform("strudel.fast"));
        registry.register(TestPiece::terminal("strudel.output"));
        registry
    }

    fn sample_swap_graph() -> Graph {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            GridPos { col: 0, row: 0 },
            Node {
                piece_id: "strudel.sound".to_string(),
                inline_params: BTreeMap::from([(
                    "value".to_string(),
                    Value::String("bd".to_string()),
                )]),
                input_sides: BTreeMap::new(),
                output_side: None,
                label: None,
                node_state: None,
            },
        );
        nodes.insert(
            GridPos { col: 1, row: 0 },
            Node {
                piece_id: "strudel.fast".to_string(),
                inline_params: BTreeMap::new(),
                input_sides: BTreeMap::new(),
                output_side: None,
                label: None,
                node_state: None,
            },
        );
        nodes.insert(
            GridPos { col: 2, row: 0 },
            Node {
                piece_id: "strudel.output".to_string(),
                inline_params: BTreeMap::new(),
                input_sides: BTreeMap::new(),
                output_side: None,
                label: None,
                node_state: None,
            },
        );

        let edge_a = Edge {
            id: EdgeId::new(),
            from: GridPos { col: 0, row: 0 },
            to_node: GridPos { col: 1, row: 0 },
            to_param: "pattern".to_string(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: GridPos { col: 1, row: 0 },
            to_node: GridPos { col: 2, row: 0 },
            to_param: "pattern".to_string(),
        };

        Graph {
            nodes,
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a), (edge_b.id.clone(), edge_b)]),
            name: "swap-test".to_string(),
            cols: 9,
            rows: 9,
        }
    }

    fn canonical_json(graph: &Graph) -> String {
        serde_json::to_string(graph).expect("serialize")
    }

    #[test]
    fn node_swap_rejects_out_of_bounds_and_missing_nodes() {
        let mut graph = sample_swap_graph();
        let registry = sample_registry();

        let out_of_bounds = apply_ops_to_graph(
            &mut graph,
            &registry,
            &[GraphOp::NodeSwap {
                a: GridPos { col: -1, row: 0 },
                b: GridPos { col: 1, row: 0 },
            }],
        )
        .expect_err("out-of-bounds swap must fail");
        assert!(
            out_of_bounds
                .iter()
                .any(|diag| matches!(diag.kind, DiagnosticKind::InvalidOperation { .. }))
        );

        let missing = apply_ops_to_graph(
            &mut graph,
            &registry,
            &[GraphOp::NodeSwap {
                a: GridPos { col: 0, row: 0 },
                b: GridPos { col: 8, row: 8 },
            }],
        )
        .expect_err("swap with missing node must fail");
        assert!(
            missing
                .iter()
                .any(|diag| matches!(diag.kind, DiagnosticKind::InvalidOperation { .. }))
        );
    }

    #[test]
    fn node_swap_is_invertible_and_restores_pruned_edges() {
        let mut graph = sample_swap_graph();
        let before = canonical_json(&graph);
        let registry = sample_registry();

        let outcome = apply_ops_to_graph(
            &mut graph,
            &registry,
            &[GraphOp::NodeSwap {
                a: GridPos { col: 0, row: 0 },
                b: GridPos { col: 1, row: 0 },
            }],
        )
        .expect("swap should apply");

        assert!(
            outcome
                .applied_ops
                .iter()
                .any(|op| matches!(op, GraphOp::NodeSwap { .. }))
        );
        assert_eq!(
            graph
                .nodes
                .get(&GridPos { col: 0, row: 0 })
                .map(|node| node.piece_id.as_str()),
            Some("strudel.fast")
        );
        assert_eq!(
            graph
                .nodes
                .get(&GridPos { col: 1, row: 0 })
                .map(|node| node.piece_id.as_str()),
            Some("strudel.sound")
        );
        assert!(!outcome.removed_edges.is_empty());

        apply_ops_to_graph(&mut graph, &registry, outcome.undo_ops.as_slice())
            .expect("undo must restore swapped graph");
        assert_eq!(before, canonical_json(&graph));
    }

    #[test]
    fn resize_grid_prunes_out_of_bounds_nodes_with_invertible_undo() {
        let mut graph = sample_swap_graph();
        graph.nodes.insert(
            GridPos { col: 7, row: 0 },
            Node {
                piece_id: "strudel.sound".to_string(),
                inline_params: BTreeMap::from([(
                    "value".to_string(),
                    Value::String("sn".to_string()),
                )]),
                input_sides: BTreeMap::new(),
                output_side: None,
                label: Some("kept-source".to_string()),
                node_state: None,
            },
        );
        graph.nodes.insert(
            GridPos { col: 8, row: 0 },
            Node {
                piece_id: "strudel.output".to_string(),
                inline_params: BTreeMap::new(),
                input_sides: BTreeMap::new(),
                output_side: None,
                label: Some("trimmed-terminal".to_string()),
                node_state: Some(Value::String("opaque".to_string())),
            },
        );
        let trimmed_edge = Edge {
            id: EdgeId::new(),
            from: GridPos { col: 7, row: 0 },
            to_node: GridPos { col: 8, row: 0 },
            to_param: "pattern".to_string(),
        };
        graph
            .edges
            .insert(trimmed_edge.id.clone(), trimmed_edge.clone());

        let before = canonical_json(&graph);
        let registry = sample_registry();
        let outcome = apply_ops_to_graph(
            &mut graph,
            &registry,
            &[GraphOp::ResizeGrid { cols: 8, rows: 9 }],
        )
        .expect("resize should succeed");

        assert_eq!(graph.cols, 8);
        assert_eq!(graph.rows, 9);
        assert!(!graph.nodes.contains_key(&GridPos { col: 8, row: 0 }));
        assert!(!graph.edges.contains_key(&trimmed_edge.id));
        assert!(
            outcome
                .removed_edges
                .iter()
                .any(|edge| edge.id == trimmed_edge.id)
        );

        apply_ops_to_graph(&mut graph, &registry, outcome.undo_ops.as_slice())
            .expect("undo should restore trimmed nodes and edges");
        assert_eq!(before, canonical_json(&graph));
    }

    #[test]
    fn node_auto_wire_connects_adjacent_input_and_output() {
        let registry = sample_registry();
        let mut nodes = BTreeMap::new();
        nodes.insert(
            GridPos { col: 0, row: 0 },
            Node {
                piece_id: "strudel.sound".to_string(),
                inline_params: BTreeMap::from([(
                    "value".to_string(),
                    Value::String("bd".to_string()),
                )]),
                input_sides: BTreeMap::new(),
                output_side: None,
                label: None,
                node_state: None,
            },
        );
        nodes.insert(
            GridPos { col: 1, row: 0 },
            Node {
                piece_id: "strudel.fast".to_string(),
                inline_params: BTreeMap::new(),
                input_sides: BTreeMap::new(),
                output_side: None,
                label: None,
                node_state: None,
            },
        );
        nodes.insert(
            GridPos { col: 2, row: 0 },
            Node {
                piece_id: "strudel.output".to_string(),
                inline_params: BTreeMap::new(),
                input_sides: BTreeMap::new(),
                output_side: None,
                label: None,
                node_state: None,
            },
        );
        let mut graph = Graph {
            nodes,
            edges: BTreeMap::new(),
            name: "auto-wire".to_string(),
            cols: 9,
            rows: 9,
        };

        let outcome = apply_ops_to_graph(
            &mut graph,
            &registry,
            &[GraphOp::NodeAutoWire {
                position: GridPos { col: 1, row: 0 },
            }],
        )
        .expect("auto wire should succeed");

        assert_eq!(graph.edges.len(), 2);
        assert!(
            graph.edges.values().any(|edge| {
                edge.from == GridPos { col: 0, row: 0 }
                    && edge.to_node == GridPos { col: 1, row: 0 }
                    && edge.to_param == "pattern"
            }),
            "expected sound -> fast input edge"
        );
        assert!(
            graph.edges.values().any(|edge| {
                edge.from == GridPos { col: 1, row: 0 }
                    && edge.to_node == GridPos { col: 2, row: 0 }
                    && edge.to_param == "pattern"
            }),
            "expected fast -> output edge"
        );
        assert_eq!(outcome.removed_edges.len(), 0);
        assert_eq!(outcome.undo_ops.len(), 2);
    }

    #[test]
    fn node_auto_wire_removes_invalid_edge_and_respects_none_side() {
        let registry = sample_registry();
        let edge = Edge {
            id: EdgeId::new(),
            from: GridPos { col: 0, row: 0 },
            to_node: GridPos { col: 1, row: 0 },
            to_param: "pattern".to_string(),
        };
        let mut graph = Graph {
            nodes: BTreeMap::from([
                (
                    GridPos { col: 0, row: 0 },
                    Node {
                        piece_id: "strudel.sound".to_string(),
                        inline_params: BTreeMap::from([(
                            "value".to_string(),
                            Value::String("bd".to_string()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    GridPos { col: 1, row: 0 },
                    Node {
                        piece_id: "strudel.fast".to_string(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".to_string(), TileSide::NONE)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge.id.clone(), edge.clone())]),
            name: "auto-wire-none".to_string(),
            cols: 9,
            rows: 9,
        };

        let outcome = apply_ops_to_graph(
            &mut graph,
            &registry,
            &[GraphOp::NodeAutoWire {
                position: GridPos { col: 1, row: 0 },
            }],
        )
        .expect("auto wire should prune invalid edge");

        assert!(graph.edges.is_empty());
        assert_eq!(outcome.removed_edges.len(), 1);
        assert_eq!(outcome.removed_edges[0].id, edge.id);
        apply_ops_to_graph(&mut graph, &registry, outcome.undo_ops.as_slice())
            .expect("undo should restore removed edge");
        assert_eq!(graph.edges.len(), 1);
    }
}
