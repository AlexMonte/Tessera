use crate::graph::{Edge, Graph};
use crate::internal::effective_input_side;
use crate::piece_registry::PieceRegistry;
use crate::types::{GridPos, adjacent_in_direction};

use super::validation::validate_edge_connect;

pub(super) fn edge_is_still_adjacent(edge: &Edge, graph: &Graph, registry: &PieceRegistry) -> bool {
    let Some(target_node) = graph.nodes.get(&edge.to_node) else {
        return false;
    };
    let Some(target_piece) = registry.get(target_node.piece_id.as_str()) else {
        return true;
    };
    let Some(_param_def) = target_piece
        .def()
        .params
        .iter()
        .find(|param| param.id == edge.to_param)
    else {
        return true;
    };
    let Some(target_side) =
        effective_input_side(target_node, target_piece.def(), edge.to_param.as_str())
    else {
        return false;
    };
    let expected = adjacent_in_direction(&edge.to_node, Some(target_side));
    expected == edge.from
}

pub(super) fn edge_is_still_valid(edge: &Edge, graph: &Graph, registry: &PieceRegistry) -> bool {
    let mut probe_graph = graph.clone();
    probe_graph.edges.remove(&edge.id);
    validate_edge_connect(
        &probe_graph,
        registry,
        &edge.from,
        &edge.to_node,
        edge.to_param.as_str(),
    )
    .is_ok()
}

pub(super) fn prune_invalid_edges_for_node(
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

pub(super) fn prune_invalid_touching_edges(
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
