use std::collections::BTreeMap;

use serde_json::Value;

use crate::diagnostics::{Diagnostic, DiagnosticKind};
use crate::graph::{Graph, Node};
use crate::internal::{
    StructuralEdgeInfo, duplicate_effective_input_sides, effective_output_side,
    registry_sanity_diagnostics, validate_graph_edge_structure,
};
use crate::piece::PieceDef;
use crate::piece_registry::PieceRegistry;
use crate::types::{DomainBridge, EdgeId, GridPos, PortType, PortTypeConnectionError, TileSide};

use super::input_resolution::{
    TraversedInputOrigin, effective_source_output_type_for_edge, incoming_edge_for_param,
    traversed_input_origin_for_edge,
};

pub(super) fn validate_known_pieces(
    graph: &Graph,
    registry: &PieceRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.extend(registry_sanity_diagnostics(registry));
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
}

pub(super) fn validate_edge_structure(
    graph: &Graph,
    registry: &PieceRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<StructuralEdgeInfo> {
    validate_graph_edge_structure(graph, registry, diagnostics)
}

pub(super) fn validate_inline_and_required_params(
    graph: &Graph,
    registry: &PieceRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
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

        for (side, params) in duplicate_effective_input_sides(node, piece.def()) {
            diagnostics.push(Diagnostic::error(
                DiagnosticKind::DuplicateInputSide { side, params },
                Some(*pos),
            ));
        }

        for param in &piece.def().params {
            let has_edge = incoming_edge_for_param(graph, pos, param.id.as_str()).is_some();
            let has_inline = node.inline_params.contains_key(param.id.as_str());
            let has_default = param.schema.default_value().is_some();
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
}

pub(super) fn evaluate_edge_type_checks(
    graph: &Graph,
    registry: &PieceRegistry,
    pending_type_checks: Vec<StructuralEdgeInfo>,
    output_types: &BTreeMap<GridPos, PortType>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<EdgeId, DomainBridge> {
    let mut domain_bridges = BTreeMap::<EdgeId, DomainBridge>::new();

    for check in pending_type_checks {
        let Some(edge) = graph.edges.get(&check.edge_id) else {
            continue;
        };
        let Some(from_output_type) =
            effective_source_output_type_for_edge(graph, registry, edge, output_types)
        else {
            let source_pos = graph
                .edges
                .get(&check.edge_id)
                .map(
                    |edge| match traversed_input_origin_for_edge(graph, registry, edge) {
                        TraversedInputOrigin::Edge(origin) => origin.source_pos,
                        TraversedInputOrigin::Inline { .. }
                        | TraversedInputOrigin::Default { .. }
                        | TraversedInputOrigin::Missing => edge.from,
                    },
                )
                .unwrap_or(check.target_pos);
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::OutputFromTerminal {
                        position: source_pos,
                    },
                    Some(source_pos),
                )
                .with_edge(check.edge_id.clone()),
            );
            continue;
        };

        let source_pos = graph
            .edges
            .get(&check.edge_id)
            .map(
                |edge| match traversed_input_origin_for_edge(graph, registry, edge) {
                    TraversedInputOrigin::Edge(origin) => origin.source_pos,
                    TraversedInputOrigin::Inline { .. }
                    | TraversedInputOrigin::Default { .. }
                    | TraversedInputOrigin::Missing => edge.from,
                },
            )
            .unwrap_or(check.target_pos);

        match check.schema.resolve_connection(&from_output_type) {
            Ok(connection) => {
                if let Some(kind) = connection.bridge_kind {
                    domain_bridges.insert(
                        check.edge_id.clone(),
                        DomainBridge {
                            edge_id: check.edge_id.clone(),
                            source_pos,
                            target_pos: check.target_pos,
                            param: check.param.clone(),
                            kind,
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

    domain_bridges
}

pub(super) fn collect_outputs(
    graph: &Graph,
    registry: &PieceRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<GridPos> {
    let outputs = graph
        .nodes
        .iter()
        .filter_map(|(pos, node)| {
            registry
                .get(node.piece_id.as_str())
                .filter(|piece| piece.def().is_output())
                .map(|_| *pos)
        })
        .collect::<Vec<_>>();

    if outputs.is_empty() {
        diagnostics.push(Diagnostic::error(DiagnosticKind::NoOutputNode, None));
    }

    outputs
}

pub(super) fn validate_connector_nodes(
    graph: &Graph,
    registry: &PieceRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (pos, node) in &graph.nodes {
        let Some(piece) = registry.get(node.piece_id.as_str()) else {
            continue;
        };
        if piece.def().is_connector() && piece.def().connector_param().is_none() {
            diagnostics.push(Diagnostic::error(
                DiagnosticKind::InvalidOperation {
                    reason:
                        "invalid_registry: connector piece must declare exactly one input param"
                            .into(),
                },
                Some(*pos),
            ));
        }
    }
}

pub(super) fn warn_on_unreachable_nodes(
    graph: &Graph,
    outputs: &[GridPos],
    diagnostics: &mut Vec<Diagnostic>,
) {
    if outputs.is_empty() {
        return;
    }

    let reachable = graph.reachable_nodes(outputs);
    for pos in graph.nodes.keys() {
        if !reachable.contains(pos) {
            diagnostics.push(Diagnostic::warning(
                DiagnosticKind::UnreachableNode { position: *pos },
                Some(*pos),
            ));
        }
    }
}

pub(super) fn node_output_side(node: &Node, piece: &PieceDef) -> Option<TileSide> {
    effective_output_side(node, piece)
}
