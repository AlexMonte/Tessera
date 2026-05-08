use std::collections::BTreeMap;

use crate::graph::{Graph, Node};
use crate::internal::{
    effective_input_side, effective_output_side, registry_sanity_diagnostics, role_mismatch_reason,
    roles_compatible, side_from_to_node,
};
use crate::piece::PieceDef;
use crate::piece_registry::PieceRegistry;
use crate::semantic::{
    effective_source_output_role_for_edge, effective_source_output_type_for_edge,
    infer_output_types_internal,
};
use crate::types::{
    DomainBridgeKind, GridPos, PortRole, PortType, PortTypeConnectionError, TileSide,
    adjacent_in_direction,
};

use super::types::{EdgeConnectProbeReason, EdgeTargetParamProbe, RepairSuggestion};

pub(super) struct EdgeConnectBase<'a> {
    pub(super) from_node: &'a Node,
    pub(super) to_node_ref: &'a Node,
    pub(super) from_piece_def: PieceDef,
    pub(super) to_piece_def: PieceDef,
}

pub(super) fn resolve_edge_connect_base<'a>(
    graph: &'a Graph,
    registry: &PieceRegistry,
    from: &GridPos,
    to_node: &GridPos,
) -> Result<EdgeConnectBase<'a>, EdgeTargetParamProbe> {
    if let Some(reject) = registry_probe_rejection(registry) {
        return Err(reject);
    }
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

pub(super) fn source_output_type_for_target_side(
    base: &EdgeConnectBase<'_>,
    graph: &Graph,
    registry: &PieceRegistry,
    inferred_output_types: &BTreeMap<GridPos, PortType>,
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
                position: *from,
                side: target_side.opposite(),
            }],
        ));
    }
    if let Some(output_type) = inferred_output_types.get(from) {
        return Ok(output_type.clone());
    }

    let Some(param) = base.from_piece_def.connector_param() else {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::OutputFromTerminal,
            "cannot connect output from terminal piece",
        ));
    };
    let Some(edge) = graph
        .edges
        .values()
        .find(|edge| edge.to_node == *from && edge.to_param == param.id)
    else {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::OutputFromTerminal,
            "cannot connect output from terminal piece",
        ));
    };

    effective_source_output_type_for_edge(graph, registry, edge, inferred_output_types).ok_or_else(
        || {
            EdgeTargetParamProbe::reject(
                EdgeConnectProbeReason::OutputFromTerminal,
                "cannot connect output from terminal piece",
            )
        },
    )
}

fn source_output_role(
    base: &EdgeConnectBase<'_>,
    graph: &Graph,
    registry: &PieceRegistry,
    from: &GridPos,
) -> Result<PortRole, EdgeTargetParamProbe> {
    if !base.from_piece_def.is_connector() {
        if !base.from_piece_def.has_output() {
            return Err(EdgeTargetParamProbe::reject(
                EdgeConnectProbeReason::OutputFromTerminal,
                "cannot connect output from terminal piece",
            ));
        }
        return Ok(base.from_piece_def.output_role.clone());
    }

    let Some(param) = base.from_piece_def.connector_param() else {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::OutputFromTerminal,
            "cannot connect output from terminal piece",
        ));
    };
    let Some(edge) = graph
        .edges
        .values()
        .find(|edge| edge.to_node == *from && edge.to_param == param.id)
    else {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::OutputFromTerminal,
            "cannot connect output from terminal piece",
        ));
    };

    effective_source_output_role_for_edge(graph, registry, edge).ok_or_else(|| {
        EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::OutputFromTerminal,
            "cannot connect output from terminal piece",
        )
    })
}

pub(super) fn node_output_side(node: &Node, piece: &PieceDef) -> Option<TileSide> {
    effective_output_side(node, piece)
}

/// Pick the best matching target param for an edge based on adjacency, side, and type.
pub fn pick_target_param_for_edge(
    graph: &Graph,
    registry: &PieceRegistry,
    from: &GridPos,
    to_node: &GridPos,
) -> EdgeTargetParamProbe {
    let inferred_output_types = infer_output_types_internal(graph, registry);
    pick_target_param_for_edge_with_output_types(
        graph,
        registry,
        from,
        to_node,
        &inferred_output_types,
    )
}

pub(super) fn pick_target_param_for_edge_with_output_types(
    graph: &Graph,
    registry: &PieceRegistry,
    from: &GridPos,
    to_node: &GridPos,
    inferred_output_types: &BTreeMap<GridPos, PortType>,
) -> EdgeTargetParamProbe {
    let base = match resolve_edge_connect_base(graph, registry, from, to_node) {
        Ok(base) => base,
        Err(err) => return err,
    };

    let Some(target_side) = side_from_to_node(to_node, from) else {
        // Suggest moving source to the first open adjacent position with a compatible param.
        let mut suggestions = Vec::new();
        for param in &base.to_piece_def.params {
            if let Some(param_side) =
                effective_input_side(base.to_node_ref, &base.to_piece_def, param.id.as_str())
            {
                let adj = adjacent_in_direction(to_node, Some(param_side));
                if adj != *to_node && !graph.nodes.contains_key(&adj) {
                    suggestions.push(RepairSuggestion::MoveNode {
                        node: *from,
                        to: adj,
                    });
                    break;
                }
            } else {
                continue;
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

    let source_output_type = match source_output_type_for_target_side(
        &base,
        graph,
        registry,
        inferred_output_types,
        from,
        target_side,
    ) {
        Ok(output_type) => output_type,
        Err(err) => return err,
    };
    let source_output_role = match source_output_role(&base, graph, registry, from) {
        Ok(role) => role,
        Err(err) => return err,
    };

    let mut saw_side_candidate = false;
    let mut saw_open_side_candidate = false;
    let mut bridgeable_candidate = None::<(String, DomainBridgeKind)>;
    let mut unsupported_domain_detail = None::<String>;
    let mut role_mismatch_detail = None::<String>;
    for param in &base.to_piece_def.params {
        let Some(param_side) =
            effective_input_side(base.to_node_ref, &base.to_piece_def, param.id.as_str())
        else {
            continue;
        };
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
        if !roles_compatible(&param.role, &source_output_role) {
            role_mismatch_detail.get_or_insert_with(|| {
                role_mismatch_reason(&param.role, &source_output_role, param.id.as_str())
                    .to_string()
            });
            continue;
        }
        match param.schema.resolve_connection(&source_output_type) {
            Ok(connection) => {
                if let Some(bridge_kind) = connection.bridge_kind {
                    bridgeable_candidate.get_or_insert_with(|| (param.id.clone(), bridge_kind));
                } else {
                    return EdgeTargetParamProbe::accept(param.id.clone());
                }
            }
            Err(PortTypeConnectionError::UnsupportedDomain { expected, got }) => {
                unsupported_domain_detail.get_or_insert_with(|| {
                    format!(
                        "unsupported domain crossing: expected {:?}, got {:?}",
                        expected, got
                    )
                });
            }
            Err(PortTypeConnectionError::ValueMismatch { .. }) => {}
        }
    }

    if !saw_side_candidate {
        // Suggest moving a type-compatible param to the connecting side.
        let suggestions = base
            .to_piece_def
            .params
            .iter()
            .filter(|p| {
                roles_compatible(&p.role, &source_output_role)
                    && p.schema.resolve_connection(&source_output_type).is_ok()
            })
            .take(1)
            .map(|p| RepairSuggestion::SetParamSide {
                position: *to_node,
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
            .filter(|p| {
                effective_input_side(base.to_node_ref, &base.to_piece_def, p.id.as_str())
                    == Some(target_side)
            })
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
    if let Some((param_id, bridge_kind)) = bridgeable_candidate {
        return EdgeTargetParamProbe::accept_with_bridge(param_id, Some(bridge_kind));
    }
    if let Some(detail) = role_mismatch_detail {
        return EdgeTargetParamProbe::reject(EdgeConnectProbeReason::TypeMismatch, detail);
    }
    if let Some(detail) = unsupported_domain_detail {
        return EdgeTargetParamProbe::reject(EdgeConnectProbeReason::UnsupportedDomain, detail);
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
) -> Result<Option<DomainBridgeKind>, EdgeTargetParamProbe> {
    let inferred_output_types = infer_output_types_internal(graph, registry);
    validate_edge_connect_with_output_types(
        graph,
        registry,
        from,
        to_node,
        to_param,
        &inferred_output_types,
    )
}

pub(super) fn validate_edge_connect_with_output_types(
    graph: &Graph,
    registry: &PieceRegistry,
    from: &GridPos,
    to_node: &GridPos,
    to_param: &str,
    inferred_output_types: &BTreeMap<GridPos, PortType>,
) -> Result<Option<DomainBridgeKind>, EdgeTargetParamProbe> {
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

    let Some(target_side) = effective_input_side(base.to_node_ref, &base.to_piece_def, to_param)
    else {
        let suggestions = side_from_to_node(to_node, from)
            .into_iter()
            .map(|side| RepairSuggestion::SetParamSide {
                position: *to_node,
                param_id: to_param.to_string(),
                side,
            })
            .collect();
        return Err(EdgeTargetParamProbe::reject_with(
            EdgeConnectProbeReason::NoParamOnTargetSide,
            format!("target param '{}' has no assigned side", to_param),
            suggestions,
        ));
    };
    let expected = adjacent_in_direction(to_node, Some(target_side));
    if expected != *from {
        return Err(EdgeTargetParamProbe::reject_with(
            EdgeConnectProbeReason::NotAdjacent,
            format!(
                "edge must come from adjacent {:?} cell ({}, {})",
                target_side, expected.col, expected.row
            ),
            vec![RepairSuggestion::MoveNode {
                node: *from,
                to: expected,
            }],
        ));
    }

    let source_output_type = source_output_type_for_target_side(
        &base,
        graph,
        registry,
        inferred_output_types,
        from,
        target_side,
    )?;
    let source_output_role = source_output_role(&base, graph, registry, from)?;
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
    if !roles_compatible(&param_def.role, &source_output_role) {
        return Err(EdgeTargetParamProbe::reject(
            EdgeConnectProbeReason::TypeMismatch,
            role_mismatch_reason(&param_def.role, &source_output_role, to_param),
        ));
    }
    match param_def.schema.resolve_connection(&source_output_type) {
        Ok(connection) => Ok(connection.bridge_kind),
        Err(PortTypeConnectionError::ValueMismatch { expected, got }) => {
            Err(EdgeTargetParamProbe::reject(
                EdgeConnectProbeReason::TypeMismatch,
                format!("type mismatch: expected {:?}, got {:?}", expected, got),
            ))
        }
        Err(PortTypeConnectionError::UnsupportedDomain { expected, got }) => {
            Err(EdgeTargetParamProbe::reject(
                EdgeConnectProbeReason::UnsupportedDomain,
                format!(
                    "unsupported domain crossing: expected {:?}, got {:?}",
                    expected, got
                ),
            ))
        }
    }
}

fn registry_probe_rejection(registry: &PieceRegistry) -> Option<EdgeTargetParamProbe> {
    registry_sanity_diagnostics(registry)
        .into_iter()
        .find_map(|diagnostic| match diagnostic.kind {
            crate::diagnostics::DiagnosticKind::InvalidOperation { reason } => Some(
                EdgeTargetParamProbe::reject(EdgeConnectProbeReason::NoCompatibleParam, reason),
            ),
            _ => None,
        })
}

/// Probe an edge using either an explicit param or automatic target-param selection.
pub fn probe_edge_connect(
    graph: &Graph,
    registry: &PieceRegistry,
    from: &GridPos,
    to_node: &GridPos,
    to_param: Option<&str>,
) -> EdgeTargetParamProbe {
    let inferred_output_types = infer_output_types_internal(graph, registry);
    if let Some(to_param) = to_param {
        return match validate_edge_connect_with_output_types(
            graph,
            registry,
            from,
            to_node,
            to_param,
            &inferred_output_types,
        ) {
            Ok(bridge_kind) => {
                EdgeTargetParamProbe::accept_with_bridge(to_param.to_string(), bridge_kind)
            }
            Err(reject) => reject,
        };
    }
    pick_target_param_for_edge_with_output_types(
        graph,
        registry,
        from,
        to_node,
        &inferred_output_types,
    )
}
