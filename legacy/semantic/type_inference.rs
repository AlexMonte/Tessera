use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{Diagnostic, DiagnosticKind};
use crate::graph::Graph;
use crate::piece::PieceDef;
use crate::piece_registry::PieceRegistry;
use crate::types::{DELAY_PIECE_ID, GridPos, PortType};

use super::input_resolution::{resolved_input_type_for_param, resolved_input_types_for_piece};

pub(super) fn stabilize_output_types(
    graph: &Graph,
    registry: &PieceRegistry,
    eval_order: &[GridPos],
) -> BTreeMap<GridPos, PortType> {
    stabilize_output_types_for_nodes(graph, registry, eval_order, &BTreeMap::new(), None)
}

pub(super) fn stabilize_output_types_for_nodes(
    graph: &Graph,
    registry: &PieceRegistry,
    eval_order: &[GridPos],
    seed: &BTreeMap<GridPos, PortType>,
    affected: Option<&BTreeSet<GridPos>>,
) -> BTreeMap<GridPos, PortType> {
    let mut output_types =
        infer_output_types_for_nodes(graph, registry, eval_order, seed, affected);
    apply_delay_output_types(graph, registry, &mut output_types, affected);

    for _ in 0..graph.nodes.len() {
        let mut next =
            infer_output_types_for_nodes(graph, registry, eval_order, &output_types, affected);
        apply_delay_output_types(graph, registry, &mut next, affected);
        if next == output_types {
            break;
        }
        output_types = next;
    }

    output_types
}

pub(super) fn collect_delay_type_diagnostics(
    graph: &Graph,
    registry: &PieceRegistry,
    output_types: &BTreeMap<GridPos, PortType>,
    affected: Option<&BTreeSet<GridPos>>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (pos, node) in &graph.nodes {
        if affected.is_some_and(|affected| !affected.contains(pos)) {
            continue;
        }
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
            registry,
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
            registry,
            node,
            pos,
            "value",
            &value_param.schema,
            output_types,
        );

        match (default_type, feedback_type) {
            (Some(default), Some(feedback)) => {
                if feedback
                    .resolve_connection(&default)
                    .ok()
                    .map(|connection| connection.effective_type)
                    .or_else(|| {
                        default
                            .resolve_connection(&feedback)
                            .ok()
                            .map(|connection| connection.effective_type)
                    })
                    .is_some()
                {
                } else {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticKind::DelayTypeMismatch {
                            default: default.clone(),
                            feedback: feedback.clone(),
                        },
                        Some(*pos),
                    ));
                }
            }
            (None, None) => {}
            (Some(_), None) | (None, Some(_)) => {}
        }
    }

    diagnostics
}

fn infer_output_types_for_nodes(
    graph: &Graph,
    registry: &PieceRegistry,
    eval_order: &[GridPos],
    seed: &BTreeMap<GridPos, PortType>,
    affected: Option<&BTreeSet<GridPos>>,
) -> BTreeMap<GridPos, PortType> {
    let mut output_types = seed.clone();

    for pos in eval_order {
        if affected.is_some_and(|affected| !affected.contains(pos)) {
            continue;
        }
        let Some(node) = graph.nodes.get(pos) else {
            output_types.remove(pos);
            continue;
        };
        let Some(piece) = registry.get(node.piece_id.as_str()) else {
            output_types.remove(pos);
            continue;
        };
        if !piece.def().has_output() {
            output_types.remove(pos);
            continue;
        }

        let input_types =
            resolved_input_types_for_piece(graph, registry, node, pos, piece.def(), &output_types);

        if let Some(port_type) = piece
            .infer_output_type(&input_types, &node.inline_params)
            .or_else(|| infer_connector_output_type(piece.def(), &input_types))
            .or_else(|| piece.def().output_type.clone())
        {
            output_types.insert(*pos, port_type);
        } else {
            output_types.remove(pos);
        }
    }

    output_types
}

fn apply_delay_output_types(
    graph: &Graph,
    registry: &PieceRegistry,
    output_types: &mut BTreeMap<GridPos, PortType>,
    affected: Option<&BTreeSet<GridPos>>,
) {
    for (pos, node) in &graph.nodes {
        if affected.is_some_and(|affected| !affected.contains(pos)) {
            continue;
        }
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
            registry,
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
            registry,
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
                    output_types.insert(*pos, PortType::any());
                }
            }
            (Some(default), None) => {
                output_types.insert(*pos, default);
            }
            (None, Some(feedback)) => {
                output_types.insert(*pos, feedback);
            }
            (None, None) => {
                output_types.remove(pos);
            }
        }
    }
}

fn infer_connector_output_type(
    piece: &PieceDef,
    input_types: &BTreeMap<String, PortType>,
) -> Option<PortType> {
    piece
        .connector_param()
        .and_then(|param| input_types.get(param.id.as_str()).cloned())
}
