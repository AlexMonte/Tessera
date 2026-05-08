use std::collections::{BTreeMap, BTreeSet};

use crate::analysis::AnalysisCache;
use crate::diagnostics::{Diagnostic, DiagnosticKind};
use crate::graph::{Edge, Graph, GraphOp, Node};
use crate::internal::{duplicate_effective_input_sides, registry_sanity_diagnostics};
use crate::piece_registry::PieceRegistry;
use crate::types::{EdgeId, GridPos};

use super::auto_wire::{auto_wire_node, edge_connect_from};
use super::pruning::prune_invalid_edges_for_node;
use super::types::ApplyOpsOutcome;
use super::validation::validate_edge_connect;

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
        Some(*pos),
        format!(
            "{} out of bounds at ({}, {}), allowed cols=[0..{}), rows=[0..{})",
            label, pos.col, pos.row, graph.cols, graph.rows
        ),
    ));
    false
}

fn swap_rewrite_pos(pos: &mut GridPos, a: &GridPos, b: &GridPos) {
    if *pos == *a {
        *pos = *b;
    } else if *pos == *b {
        *pos = *a;
    }
}

/// Apply a batch of graph operations, collecting inverse ops for undo/redo.
pub fn apply_ops_to_graph(
    graph: &mut Graph,
    registry: &PieceRegistry,
    ops: &[GraphOp],
) -> Result<ApplyOpsOutcome, Vec<Diagnostic>> {
    let mut errors = registry_sanity_diagnostics(registry);
    let mut outcome = ApplyOpsOutcome::default();
    let mut inverse_chunks = Vec::<Vec<GraphOp>>::new();

    for op in ops {
        match op {
            GraphOp::NodePlace {
                position,
                piece_id,
                inline_params,
                pattern_source,
            } => {
                if !ensure_in_bounds(&mut errors, position, "node_place", graph) {
                    continue;
                }
                if graph.nodes.contains_key(position) {
                    errors.push(invalid_op(
                        Some(*position),
                        format!(
                            "node already exists at ({}, {})",
                            position.col, position.row
                        ),
                    ));
                    continue;
                }
                let Some(piece) = registry.get(piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(*position),
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
                            Some(*position),
                            format!("unknown inline param '{}'", param_id),
                        ));
                        inline_is_valid = false;
                        continue;
                    };
                    if !param_def.schema.can_inline() {
                        errors.push(invalid_op(
                            Some(*position),
                            format!("inline value is not allowed for '{}'", param_id),
                        ));
                        inline_is_valid = false;
                        continue;
                    }
                    if !param_def.schema.validate_inline_value(value) {
                        errors.push(invalid_op(
                            Some(*position),
                            format!("inline value has wrong type for '{}'", param_id),
                        ));
                        inline_is_valid = false;
                    }
                }
                if !inline_is_valid {
                    continue;
                }
                graph.nodes.insert(
                    *position,
                    Node {
                        piece_id: piece_id.clone(),
                        inline_params: inline_params.clone(),
                        pattern_source: pattern_source.clone(),
                        input_sides: Default::default(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                );
                outcome.applied_ops.push(GraphOp::NodePlace {
                    position: *position,
                    piece_id: piece_id.clone(),
                    inline_params: inline_params.clone(),
                    pattern_source: pattern_source.clone(),
                });
                inverse_chunks.push(vec![GraphOp::NodeRemove {
                    position: *position,
                }]);
            }
            GraphOp::NodeBatchPlace {
                nodes,
                edges,
                auto_wire,
            } => {
                let mut batch_errors = Vec::<Diagnostic>::new();
                let mut seen_positions = BTreeSet::<GridPos>::new();
                let mut shadow_graph = graph.clone();

                for entry in nodes {
                    if !ensure_in_bounds(
                        &mut batch_errors,
                        &entry.position,
                        "node_batch_place",
                        graph,
                    ) {
                        continue;
                    }
                    if !seen_positions.insert(entry.position) {
                        batch_errors.push(invalid_op(
                            Some(entry.position),
                            format!(
                                "duplicate batch position at ({}, {})",
                                entry.position.col, entry.position.row
                            ),
                        ));
                        continue;
                    }
                    if graph.nodes.contains_key(&entry.position) {
                        batch_errors.push(invalid_op(
                            Some(entry.position),
                            format!(
                                "target cell already occupied at ({}, {})",
                                entry.position.col, entry.position.row
                            ),
                        ));
                        continue;
                    }
                    let Some(piece) = registry.get(entry.piece_id.as_str()) else {
                        batch_errors.push(invalid_op(
                            Some(entry.position),
                            format!("unknown piece id '{}'", entry.piece_id),
                        ));
                        continue;
                    };

                    let mut entry_is_valid = true;
                    for (param_id, value) in &entry.inline_params {
                        let Some(param_def) = piece
                            .def()
                            .params
                            .iter()
                            .find(|param| param.id == *param_id)
                        else {
                            batch_errors.push(invalid_op(
                                Some(entry.position),
                                format!("unknown inline param '{}'", param_id),
                            ));
                            entry_is_valid = false;
                            continue;
                        };
                        if !param_def.schema.can_inline() {
                            batch_errors.push(invalid_op(
                                Some(entry.position),
                                format!("inline value is not allowed for '{}'", param_id),
                            ));
                            entry_is_valid = false;
                            continue;
                        }
                        if !param_def.schema.validate_inline_value(value) {
                            batch_errors.push(invalid_op(
                                Some(entry.position),
                                format!("inline value has wrong type for '{}'", param_id),
                            ));
                            entry_is_valid = false;
                        }
                    }

                    for param_id in entry.input_sides.keys() {
                        if piece.def().params.iter().any(|param| param.id == *param_id) {
                            continue;
                        }
                        batch_errors.push(invalid_op(
                            Some(entry.position),
                            format!("unknown param '{}'", param_id),
                        ));
                        entry_is_valid = false;
                    }

                    let staged_node = Node {
                        piece_id: entry.piece_id.clone(),
                        inline_params: entry.inline_params.clone(),
                        pattern_source: entry.pattern_source.clone(),
                        input_sides: entry.input_sides.clone(),
                        output_side: entry.output_side,
                        label: entry.label.clone(),
                        node_state: None,
                    };
                    for (side, params) in duplicate_effective_input_sides(&staged_node, piece.def())
                    {
                        batch_errors.push(invalid_op(
                            Some(entry.position),
                            format!(
                                "multiple params assigned to side '{side:?}': {}",
                                params.join(", ")
                            ),
                        ));
                        entry_is_valid = false;
                    }

                    if entry.output_side.is_some() && !piece.def().has_output() {
                        batch_errors.push(invalid_op(
                            Some(entry.position),
                            "cannot set output side on terminal piece",
                        ));
                        entry_is_valid = false;
                    }

                    if !entry_is_valid {
                        continue;
                    }

                    shadow_graph.nodes.insert(
                        entry.position,
                        Node {
                            piece_id: entry.piece_id.clone(),
                            inline_params: entry.inline_params.clone(),
                            pattern_source: entry.pattern_source.clone(),
                            input_sides: entry.input_sides.clone(),
                            output_side: entry.output_side,
                            label: entry.label.clone(),
                            node_state: None,
                        },
                    );
                }

                if !batch_errors.is_empty() {
                    errors.extend(batch_errors);
                    continue;
                }

                let mut staged_edges = Vec::<Edge>::new();
                for edge in edges {
                    let source_ok = ensure_in_bounds(
                        &mut batch_errors,
                        &edge.from,
                        "node_batch_place edge source",
                        &shadow_graph,
                    );
                    let target_ok = ensure_in_bounds(
                        &mut batch_errors,
                        &edge.to_node,
                        "node_batch_place edge target",
                        &shadow_graph,
                    );
                    if !(source_ok && target_ok) {
                        continue;
                    }
                    if !shadow_graph.nodes.contains_key(&edge.from) {
                        batch_errors.push(invalid_op(
                            Some(edge.from),
                            format!(
                                "missing source node at ({}, {})",
                                edge.from.col, edge.from.row
                            ),
                        ));
                        continue;
                    }
                    if !shadow_graph.nodes.contains_key(&edge.to_node) {
                        batch_errors.push(invalid_op(
                            Some(edge.to_node),
                            format!(
                                "missing target node at ({}, {})",
                                edge.to_node.col, edge.to_node.row
                            ),
                        ));
                        continue;
                    }
                    if edge.to_param.trim().is_empty() {
                        batch_errors.push(invalid_op(
                            Some(edge.to_node),
                            "edge target param cannot be empty",
                        ));
                        continue;
                    }
                    if let Err(reject) = validate_edge_connect(
                        &shadow_graph,
                        registry,
                        &edge.from,
                        &edge.to_node,
                        edge.to_param.as_str(),
                    ) {
                        batch_errors.push(invalid_op(
                            Some(edge.to_node),
                            reject
                                .detail
                                .unwrap_or_else(|| "edge connection rejected".to_string()),
                        ));
                        continue;
                    }

                    let staged_edge = Edge {
                        id: EdgeId::new(),
                        from: edge.from,
                        to_node: edge.to_node,
                        to_param: edge.to_param.clone(),
                    };
                    shadow_graph
                        .edges
                        .insert(staged_edge.id.clone(), staged_edge.clone());
                    staged_edges.push(staged_edge);
                }

                if !batch_errors.is_empty() {
                    errors.extend(batch_errors);
                    continue;
                }

                let mut auto_wire_outcome = ApplyOpsOutcome::default();
                let mut auto_wire_inverse_chunks = Vec::<Vec<GraphOp>>::new();
                if *auto_wire {
                    for entry in nodes {
                        auto_wire_node(
                            &mut shadow_graph,
                            registry,
                            &entry.position,
                            &mut auto_wire_outcome,
                            &mut auto_wire_inverse_chunks,
                        );
                    }
                }

                let mut batch_inverse = auto_wire_inverse_chunks
                    .into_iter()
                    .rev()
                    .flat_map(|chunk| chunk.into_iter())
                    .collect::<Vec<_>>();
                batch_inverse.extend(staged_edges.iter().rev().map(|edge| {
                    GraphOp::EdgeDisconnect {
                        edge_id: edge.id.clone(),
                    }
                }));
                batch_inverse.extend(nodes.iter().rev().map(|entry| GraphOp::NodeRemove {
                    position: entry.position,
                }));

                *graph = shadow_graph;
                outcome
                    .removed_edges
                    .extend(auto_wire_outcome.removed_edges.into_iter());
                outcome.applied_ops.push(GraphOp::NodeBatchPlace {
                    nodes: nodes.clone(),
                    edges: edges.clone(),
                    auto_wire: *auto_wire,
                });
                if !batch_inverse.is_empty() {
                    inverse_chunks.push(batch_inverse);
                }
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
                        Some(*to),
                        format!("target cell already occupied at ({}, {})", to.col, to.row),
                    ));
                    continue;
                }
                let Some(node) = graph.nodes.remove(from) else {
                    errors.push(invalid_op(
                        Some(*from),
                        format!("missing source node at ({}, {})", from.col, from.row),
                    ));
                    continue;
                };
                graph.nodes.insert(*to, node);

                for edge in graph.edges.values_mut() {
                    if edge.from == *from {
                        edge.from = *to;
                    }
                    if edge.to_node == *from {
                        edge.to_node = *to;
                    }
                }
                let mut removed_for_move = Vec::new();
                prune_invalid_edges_for_node(graph, registry, to, &mut removed_for_move);
                outcome
                    .removed_edges
                    .extend(removed_for_move.iter().cloned());

                let mut inverse = vec![GraphOp::NodeMove {
                    from: *to,
                    to: *from,
                }];
                for removed in &removed_for_move {
                    let mut restored = removed.clone();
                    if restored.from == *to {
                        restored.from = *from;
                    }
                    if restored.to_node == *to {
                        restored.to_node = *from;
                    }
                    inverse.push(edge_connect_from(&restored));
                }
                inverse_chunks.push(inverse);

                outcome.applied_ops.push(GraphOp::NodeMove {
                    from: *from,
                    to: *to,
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
                        Some(*a),
                        format!("missing node at ({}, {})", a.col, a.row),
                    ));
                    continue;
                }
                if !graph.nodes.contains_key(b) {
                    errors.push(invalid_op(
                        Some(*b),
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
                graph.nodes.insert(*a, node_b);
                graph.nodes.insert(*b, node_a);

                for edge in graph.edges.values_mut() {
                    swap_rewrite_pos(&mut edge.from, a, b);
                    swap_rewrite_pos(&mut edge.to_node, a, b);
                }

                let mut removed_for_swap: Vec<Edge> = Vec::new();
                prune_invalid_edges_for_node(graph, registry, a, &mut removed_for_swap);
                prune_invalid_edges_for_node(graph, registry, b, &mut removed_for_swap);
                outcome
                    .removed_edges
                    .extend(removed_for_swap.iter().cloned());

                let mut inverse = vec![GraphOp::NodeSwap { a: *a, b: *b }];
                for removed in &removed_for_swap {
                    let mut restored = removed.clone();
                    swap_rewrite_pos(&mut restored.from, a, b);
                    swap_rewrite_pos(&mut restored.to_node, a, b);
                    inverse.push(edge_connect_from(&restored));
                }
                inverse_chunks.push(inverse);

                outcome.applied_ops.push(GraphOp::NodeSwap { a: *a, b: *b });
            }
            GraphOp::NodeRemove { position } => {
                if !ensure_in_bounds(&mut errors, position, "node_remove", graph) {
                    continue;
                }
                let Some(removed_node) = graph.nodes.remove(position) else {
                    errors.push(invalid_op(
                        Some(*position),
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
                    position: *position,
                    piece_id: removed_node.piece_id.clone(),
                    inline_params: removed_node.inline_params.clone(),
                    pattern_source: removed_node.pattern_source.clone(),
                }];
                if removed_node.label.is_some() {
                    inverse.push(GraphOp::NodeSetLabel {
                        position: *position,
                        label: removed_node.label.clone(),
                    });
                }
                if removed_node.node_state.is_some() {
                    inverse.push(GraphOp::NodeSetState {
                        position: *position,
                        state: removed_node.node_state.clone(),
                    });
                }
                if removed_node.pattern_source.is_some() {
                    inverse.push(GraphOp::NodeSetPatternSurface {
                        position: *position,
                        pattern_source: removed_node.pattern_source.clone(),
                    });
                }
                for (param_id, side) in &removed_node.input_sides {
                    inverse.push(GraphOp::ParamSetSide {
                        position: *position,
                        param_id: param_id.clone(),
                        side: *side,
                    });
                }
                if let Some(side) = removed_node.output_side {
                    inverse.push(GraphOp::OutputSetSide {
                        position: *position,
                        side,
                    });
                }
                inverse.extend(removed_for_node.iter().map(edge_connect_from));
                inverse_chunks.push(inverse);

                outcome.applied_ops.push(GraphOp::NodeRemove {
                    position: *position,
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
                        Some(*from),
                        format!("missing source node at ({}, {})", from.col, from.row),
                    ));
                    continue;
                }
                if !graph.nodes.contains_key(to_node) {
                    errors.push(invalid_op(
                        Some(*to_node),
                        format!("missing target node at ({}, {})", to_node.col, to_node.row),
                    ));
                    continue;
                }
                if to_param.trim().is_empty() {
                    errors.push(invalid_op(
                        Some(*to_node),
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
                        Some(*to_node),
                        reject
                            .detail
                            .unwrap_or_else(|| "edge connection rejected".to_string()),
                    ));
                    continue;
                }

                let edge_id = edge_id.clone().unwrap_or_else(EdgeId::new);
                if graph.edges.contains_key(&edge_id) {
                    errors.push(invalid_op(
                        Some(*to_node),
                        format!("edge id '{}' already exists", edge_id.0),
                    ));
                    continue;
                }
                let edge = Edge {
                    id: edge_id.clone(),
                    from: *from,
                    to_node: *to_node,
                    to_param: to_param.clone(),
                };
                graph.edges.insert(edge.id.clone(), edge);
                outcome.applied_ops.push(GraphOp::EdgeConnect {
                    edge_id: Some(edge_id.clone()),
                    from: *from,
                    to_node: *to_node,
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
                        Some(*position),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                let Some(param_def) = piece.def().params.iter().find(|item| item.id == *param_id)
                else {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("unknown param '{}'", param_id),
                    ));
                    continue;
                };
                if !param_def.schema.can_inline() {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("inline value is not allowed for '{}'", param_id),
                    ));
                    continue;
                }
                if !param_def.schema.validate_inline_value(value) {
                    errors.push(invalid_op(
                        Some(*position),
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
                    position: *position,
                    param_id: param_id.clone(),
                    value: value.clone(),
                });
                if let Some(previous) = previous {
                    inverse_chunks.push(vec![GraphOp::ParamSetInline {
                        position: *position,
                        param_id: param_id.clone(),
                        value: previous,
                    }]);
                } else {
                    inverse_chunks.push(vec![GraphOp::ParamClearInline {
                        position: *position,
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
                        Some(*position),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                if !piece.def().params.iter().any(|item| item.id == *param_id) {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("unknown param '{}'", param_id),
                    ));
                    continue;
                }
                let previous = target_node.inline_params.remove(param_id);
                let Some(previous) = previous else {
                    continue;
                };
                outcome.applied_ops.push(GraphOp::ParamClearInline {
                    position: *position,
                    param_id: param_id.clone(),
                });
                inverse_chunks.push(vec![GraphOp::ParamSetInline {
                    position: *position,
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
                        Some(*position),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                if !piece.def().params.iter().any(|param| param.id == *param_id) {
                    errors.push(invalid_op(
                        Some(*position),
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
                let mut staged_node = target_node.clone();
                staged_node.input_sides.insert(param_id.clone(), *side);
                if let Some((_, params)) =
                    duplicate_effective_input_sides(&staged_node, piece.def())
                        .into_iter()
                        .find(|(_, params)| params.iter().any(|existing| existing == param_id))
                {
                    let occupied_param = params
                        .into_iter()
                        .find(|existing| existing != param_id)
                        .unwrap_or_else(|| param_id.clone());
                    errors.push(invalid_op(
                        Some(*position),
                        format!(
                            "side '{side:?}' is already assigned to param '{}'",
                            occupied_param
                        ),
                    ));
                    continue;
                }
                let previous = target_node.input_sides.insert(param_id.clone(), *side);
                outcome.applied_ops.push(GraphOp::ParamSetSide {
                    position: *position,
                    param_id: param_id.clone(),
                    side: *side,
                });
                if let Some(previous) = previous {
                    inverse_chunks.push(vec![GraphOp::ParamSetSide {
                        position: *position,
                        param_id: param_id.clone(),
                        side: previous,
                    }]);
                } else {
                    inverse_chunks.push(vec![GraphOp::ParamClearSide {
                        position: *position,
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
                        Some(*position),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                if !piece.def().params.iter().any(|param| param.id == *param_id) {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("unknown param '{}'", param_id),
                    ));
                    continue;
                }
                let mut staged_node = target_node.clone();
                let previous = staged_node.input_sides.remove(param_id);
                let Some(previous) = previous else {
                    continue;
                };
                if let Some((side, params)) =
                    duplicate_effective_input_sides(&staged_node, piece.def())
                        .into_iter()
                        .next()
                {
                    errors.push(invalid_op(
                        Some(*position),
                        format!(
                            "multiple params assigned to side '{side:?}': {}",
                            params.join(", ")
                        ),
                    ));
                    continue;
                }
                target_node.input_sides.remove(param_id);
                outcome.applied_ops.push(GraphOp::ParamClearSide {
                    position: *position,
                    param_id: param_id.clone(),
                });
                inverse_chunks.push(vec![GraphOp::ParamSetSide {
                    position: *position,
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
                        Some(*position),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                if !piece.def().has_output() {
                    errors.push(invalid_op(
                        Some(*position),
                        "cannot set output side on terminal piece",
                    ));
                    continue;
                }
                if target_node.output_side == Some(*side) {
                    continue;
                }
                let previous = target_node.output_side.replace(*side);
                outcome.applied_ops.push(GraphOp::OutputSetSide {
                    position: *position,
                    side: *side,
                });
                if let Some(previous) = previous {
                    inverse_chunks.push(vec![GraphOp::OutputSetSide {
                        position: *position,
                        side: previous,
                    }]);
                } else {
                    inverse_chunks.push(vec![GraphOp::OutputClearSide {
                        position: *position,
                    }]);
                }
            }
            GraphOp::OutputClearSide { position } => {
                if !ensure_in_bounds(&mut errors, position, "output_clear_side", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let Some(piece) = registry.get(target_node.piece_id.as_str()) else {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("unknown piece '{}'", target_node.piece_id),
                    ));
                    continue;
                };
                if !piece.def().has_output() {
                    continue;
                }
                let previous = target_node.output_side.take();
                let Some(previous) = previous else {
                    continue;
                };
                outcome.applied_ops.push(GraphOp::OutputClearSide {
                    position: *position,
                });
                inverse_chunks.push(vec![GraphOp::OutputSetSide {
                    position: *position,
                    side: previous,
                }]);
            }
            GraphOp::NodeAutoWire { position } => {
                if !ensure_in_bounds(&mut errors, position, "node_auto_wire", graph) {
                    continue;
                }
                if !graph.nodes.contains_key(position) {
                    errors.push(invalid_op(
                        Some(*position),
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
                        Some(*position),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let previous = target_node.label.clone();
                target_node.label = label.clone();
                outcome.applied_ops.push(GraphOp::NodeSetLabel {
                    position: *position,
                    label: label.clone(),
                });
                inverse_chunks.push(vec![GraphOp::NodeSetLabel {
                    position: *position,
                    label: previous,
                }]);
            }
            GraphOp::NodeSetState { position, state } => {
                if !ensure_in_bounds(&mut errors, position, "node_set_state", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let previous = target_node.node_state.clone();
                target_node.node_state = state.clone();
                outcome.applied_ops.push(GraphOp::NodeSetState {
                    position: *position,
                    state: state.clone(),
                });
                inverse_chunks.push(vec![GraphOp::NodeSetState {
                    position: *position,
                    state: previous,
                }]);
            }
            GraphOp::NodeSetPatternSurface {
                position,
                pattern_source,
            } => {
                if !ensure_in_bounds(&mut errors, position, "node_set_pattern_surface", graph) {
                    continue;
                }
                let Some(target_node) = graph.nodes.get_mut(position) else {
                    errors.push(invalid_op(
                        Some(*position),
                        format!("missing node at ({}, {})", position.col, position.row),
                    ));
                    continue;
                };
                let previous = target_node.pattern_source.clone();
                target_node.pattern_source = pattern_source.clone();
                outcome.applied_ops.push(GraphOp::NodeSetPatternSurface {
                    position: *position,
                    pattern_source: pattern_source.clone(),
                });
                inverse_chunks.push(vec![GraphOp::NodeSetPatternSurface {
                    position: *position,
                    pattern_source: previous,
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
                            Some((*pos, node.clone()))
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
                        position: *position,
                        piece_id: node.piece_id.clone(),
                        inline_params: node.inline_params.clone(),
                        pattern_source: node.pattern_source.clone(),
                    });
                    if node.label.is_some() {
                        inverse.push(GraphOp::NodeSetLabel {
                            position: *position,
                            label: node.label.clone(),
                        });
                    }
                    if node.node_state.is_some() {
                        inverse.push(GraphOp::NodeSetState {
                            position: *position,
                            state: node.node_state.clone(),
                        });
                    }
                    for (param_id, side) in &node.input_sides {
                        inverse.push(GraphOp::ParamSetSide {
                            position: *position,
                            param_id: param_id.clone(),
                            side: *side,
                        });
                    }
                    if let Some(side) = node.output_side {
                        inverse.push(GraphOp::OutputSetSide {
                            position: *position,
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

pub fn apply_ops_to_graph_cached(
    graph: &mut Graph,
    registry: &PieceRegistry,
    ops: &[GraphOp],
    cache: &mut AnalysisCache,
) -> Result<ApplyOpsOutcome, Vec<Diagnostic>> {
    let explicit_disconnect_targets = ops
        .iter()
        .filter_map(|op| match op {
            GraphOp::EdgeDisconnect { edge_id } => graph
                .edges
                .get(edge_id)
                .map(|edge| (edge_id.clone(), edge.to_node)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    match apply_ops_to_graph(graph, registry, ops) {
        Ok(outcome) => {
            cache.invalidate_from_apply_outcome(
                graph,
                &outcome.applied_ops,
                &outcome.removed_edges,
                &explicit_disconnect_targets,
            );
            Ok(outcome)
        }
        Err(errors) => {
            cache.clear();
            Err(errors)
        }
    }
}
