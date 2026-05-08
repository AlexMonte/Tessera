use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::GridPos;
use crate::graph::{Edge, Graph, Node};
use crate::piece::{ParamSchema, PieceDef};
use crate::piece_registry::PieceRegistry;
use crate::types::{PortRole, PortType, PortTypeConnection, TileSide};

#[derive(Debug, Clone)]
pub(crate) struct TraversedEdgeSource {
    pub(crate) source_pos: GridPos,
    pub(crate) exit_side: TileSide,
    pub(crate) via: Vec<GridPos>,
}

#[derive(Debug, Clone)]
pub(crate) enum TraversedInputOrigin {
    Edge(TraversedEdgeSource),
    Inline {
        value: Value,
        source_type: Option<PortType>,
    },
    Default {
        value: Value,
        source_type: Option<PortType>,
    },
    Missing,
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

pub(crate) fn traversed_input_origin_for_edge(
    graph: &Graph,
    registry: &PieceRegistry,
    edge: &Edge,
) -> TraversedInputOrigin {
    let mut visited = BTreeSet::new();
    traversed_input_origin_for_edge_inner(graph, registry, edge, &mut visited)
}

fn traversed_input_origin_for_edge_inner(
    graph: &Graph,
    registry: &PieceRegistry,
    edge: &Edge,
    visited: &mut BTreeSet<GridPos>,
) -> TraversedInputOrigin {
    let Some(source_node) = graph.nodes.get(&edge.from) else {
        return direct_edge_origin(edge);
    };
    let Some(source_piece) = registry.get(source_node.piece_id.as_str()) else {
        return direct_edge_origin(edge);
    };
    let Some(param) = source_piece.def().connector_param() else {
        return direct_edge_origin(edge);
    };

    if !visited.insert(edge.from) {
        return direct_edge_origin(edge);
    }

    if let Some(upstream_edge) = incoming_edge_for_param(graph, &edge.from, param.id.as_str()) {
        return match traversed_input_origin_for_edge_inner(graph, registry, upstream_edge, visited)
        {
            TraversedInputOrigin::Edge(mut origin) => {
                origin.via.push(edge.from);
                TraversedInputOrigin::Edge(origin)
            }
            other => other,
        };
    }

    if let Some(value) = source_node.inline_params.get(param.id.as_str()) {
        return TraversedInputOrigin::Inline {
            value: value.clone(),
            source_type: param.schema.infer_inline_port_type(value),
        };
    }

    if let Some(value) = param.schema.default_value() {
        return TraversedInputOrigin::Default {
            value,
            source_type: param.schema.resolved_port_type(None),
        };
    }

    TraversedInputOrigin::Missing
}

pub(crate) fn effective_source_output_type_for_edge(
    graph: &Graph,
    registry: &PieceRegistry,
    edge: &Edge,
    inferred_outputs: &BTreeMap<GridPos, PortType>,
) -> Option<PortType> {
    match traversed_input_origin_for_edge(graph, registry, edge) {
        TraversedInputOrigin::Edge(origin) => inferred_outputs.get(&origin.source_pos).cloned(),
        TraversedInputOrigin::Inline { source_type, .. }
        | TraversedInputOrigin::Default { source_type, .. } => source_type,
        TraversedInputOrigin::Missing => None,
    }
}

pub(crate) fn effective_source_output_role_for_edge(
    graph: &Graph,
    registry: &PieceRegistry,
    edge: &Edge,
) -> Option<PortRole> {
    match traversed_input_origin_for_edge(graph, registry, edge) {
        TraversedInputOrigin::Edge(origin) => graph
            .nodes
            .get(&origin.source_pos)
            .and_then(|node| registry.get(node.piece_id.as_str()))
            .map(|piece| piece.def().output_role.clone()),
        TraversedInputOrigin::Inline { .. } | TraversedInputOrigin::Default { .. } => {
            Some(PortRole::Value)
        }
        TraversedInputOrigin::Missing => None,
    }
}

fn direct_edge_origin(edge: &Edge) -> TraversedInputOrigin {
    TraversedInputOrigin::Edge(TraversedEdgeSource {
        source_pos: edge.from,
        exit_side: direction_from_to(&edge.from, &edge.to_node).unwrap_or(TileSide::RIGHT),
        via: Vec::new(),
    })
}

fn direction_from_to(from: &GridPos, to: &GridPos) -> Option<TileSide> {
    match (to.col - from.col, to.row - from.row) {
        (1, 0) => Some(TileSide::RIGHT),
        (-1, 0) => Some(TileSide::LEFT),
        (0, -1) => Some(TileSide::TOP),
        (0, 1) => Some(TileSide::BOTTOM),
        _ => None,
    }
}

pub(crate) fn resolved_input_connection_for_param(
    graph: &Graph,
    registry: &PieceRegistry,
    node: &Node,
    pos: &GridPos,
    param_id: &str,
    schema: &ParamSchema,
    inferred_outputs: &BTreeMap<GridPos, PortType>,
) -> Option<PortTypeConnection> {
    if let Some(edge) = incoming_edge_for_param(graph, pos, param_id) {
        let source_type =
            effective_source_output_type_for_edge(graph, registry, edge, inferred_outputs)?;
        return schema.resolve_connection(&source_type).ok();
    }

    schema
        .resolved_port_type(node.inline_params.get(param_id))
        .map(|effective_type| PortTypeConnection {
            effective_type,
            bridge_kind: None,
        })
}

pub(super) fn resolved_input_type_for_param(
    graph: &Graph,
    registry: &PieceRegistry,
    node: &Node,
    pos: &GridPos,
    param_id: &str,
    schema: &ParamSchema,
    inferred_outputs: &BTreeMap<GridPos, PortType>,
) -> Option<PortType> {
    resolved_input_connection_for_param(
        graph,
        registry,
        node,
        pos,
        param_id,
        schema,
        inferred_outputs,
    )
    .map(|connection| connection.effective_type)
}

pub(crate) fn resolved_input_types_for_piece(
    graph: &Graph,
    registry: &PieceRegistry,
    node: &Node,
    pos: &GridPos,
    piece: &PieceDef,
    inferred_outputs: &BTreeMap<GridPos, PortType>,
) -> BTreeMap<String, PortType> {
    let mut input_types = BTreeMap::<String, PortType>::new();

    for param in &piece.params {
        if let Some(port_type) = resolved_input_type_for_param(
            graph,
            registry,
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
