use crate::graph::{Edge, Graph, GraphOp};
use crate::internal::effective_input_side;
use crate::piece_registry::PieceRegistry;
use crate::types::{EdgeId, GridPos, adjacent_in_direction};

use super::pruning::prune_invalid_touching_edges;
use super::types::ApplyOpsOutcome;
use super::validation::{node_output_side, probe_edge_connect, validate_edge_connect};

pub(super) fn auto_wire_node(
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

    let mut removed_for_node: Vec<Edge> = Vec::new();
    prune_invalid_touching_edges(graph, registry, position, &mut removed_for_node);

    let mut added_edges: Vec<Edge> = Vec::new();

    for param in &piece.def().params {
        if graph
            .edges
            .values()
            .any(|edge| edge.to_node == *position && edge.to_param == param.id)
        {
            continue;
        }
        if let Some(target_side) =
            effective_input_side(&node_snapshot, piece.def(), param.id.as_str())
        {
            let source_pos = adjacent_in_direction(position, Some(target_side));
            if source_pos == *position || !graph.nodes.contains_key(&source_pos) {
                continue;
            }
            if validate_edge_connect(graph, registry, &source_pos, position, param.id.as_str())
                .is_ok()
            {
                let edge = Edge {
                    id: EdgeId::new(),
                    from: source_pos,
                    to_node: *position,
                    to_param: param.id.clone(),
                };
                graph.edges.insert(edge.id.clone(), edge.clone());
                added_edges.push(edge);
            }
        } else {
            continue;
        }
    }

    if let Some(output_side) = node_output_side(&node_snapshot, piece.def()) {
        let target_pos = adjacent_in_direction(position, Some(output_side));
        if target_pos != *position && graph.nodes.contains_key(&target_pos) {
            let probe = probe_edge_connect(graph, registry, position, &target_pos, None);
            if let Some(to_param) = probe.to_param {
                let edge = Edge {
                    id: EdgeId::new(),
                    from: *position,
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
            from: edge.from,
            to_node: edge.to_node,
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

pub(super) fn edge_connect_from(edge: &Edge) -> GraphOp {
    GraphOp::EdgeConnect {
        edge_id: Some(edge.id.clone()),
        from: edge.from,
        to_node: edge.to_node,
        to_param: edge.to_param.clone(),
    }
}
