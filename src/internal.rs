use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{Diagnostic, DiagnosticKind, DiagnosticSeverity};
use crate::graph::{Graph, Node};
use crate::piece::{ParamDef, ParamSchema, PieceDef};
use crate::piece_registry::PieceRegistry;
use crate::subgraph::{SUBGRAPH_INPUT_1_ID, SUBGRAPH_INPUT_2_ID, SUBGRAPH_INPUT_3_ID};
use crate::types::{EdgeId, GridPos, PortRole, TileSide, adjacent_in_direction};

#[derive(Debug, Clone)]
pub(crate) struct StructuralEdgeInfo {
    pub(crate) edge_id: EdgeId,
    pub(crate) target_pos: GridPos,
    pub(crate) param: String,
    pub(crate) schema: ParamSchema,
}

pub(crate) fn registry_sanity_diagnostics(registry: &PieceRegistry) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for piece_id in registry.duplicate_piece_ids() {
        diagnostics.push(invalid_registry_diagnostic(format!(
            "invalid_registry: duplicate piece id '{piece_id}'"
        )));
    }

    for def in registry.all_defs() {
        let mut seen_params = BTreeSet::new();
        for param in &def.params {
            if !seen_params.insert(param.id.clone()) {
                diagnostics.push(invalid_registry_diagnostic(format!(
                    "invalid_registry: piece '{}' has duplicate param '{}'",
                    def.id, param.id
                )));
            }
        }

        if def.is_connector() && def.connector_param().is_none() {
            diagnostics.push(invalid_registry_diagnostic(format!(
                "invalid_registry: connector piece '{}' must declare exactly one input param",
                def.id
            )));
        }

        let mut params_by_side = BTreeMap::<TileSide, Vec<String>>::new();
        for param in &def.params {
            if !param_counts_toward_side_conflict(&def, param) {
                continue;
            }
            params_by_side
                .entry(param.side)
                .or_default()
                .push(param.id.clone());
        }
        for (side, mut params) in params_by_side {
            if params.len() < 2 {
                continue;
            }
            params.sort();
            diagnostics.push(invalid_registry_diagnostic(format!(
                "invalid_registry: piece '{}' has duplicate default input side {:?}: {}",
                def.id,
                side,
                params.join(", ")
            )));
        }

        if def.output_side.is_some() && !def.has_output() {
            diagnostics.push(invalid_registry_diagnostic(format!(
                "invalid_registry: piece '{}' cannot declare an output side without an output",
                def.id
            )));
        }

        diagnostics.extend(variadic_group_diagnostics(&def));
    }

    diagnostics
}

pub(crate) fn invalid_registry_diagnostic(reason: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        DiagnosticKind::InvalidOperation {
            reason: reason.into(),
        },
        None,
    )
}

pub(crate) fn roles_compatible(expected: &PortRole, actual: &PortRole) -> bool {
    match expected {
        PortRole::Value => matches!(actual, PortRole::Value),
        PortRole::Gate => matches!(actual, PortRole::Gate),
        PortRole::Signal => matches!(actual, PortRole::Signal),
        PortRole::Callback => matches!(actual, PortRole::Callback),
        PortRole::Sequence => matches!(actual, PortRole::Sequence),
        PortRole::Field { name } => {
            matches!(actual, PortRole::Field { name: actual_name } if actual_name == name)
        }
    }
}

pub(crate) fn role_label(role: &PortRole) -> String {
    match role {
        PortRole::Value => "value".into(),
        PortRole::Gate => "gate".into(),
        PortRole::Signal => "signal".into(),
        PortRole::Callback => "callback".into(),
        PortRole::Sequence => "sequence".into(),
        PortRole::Field { name } => format!("field:{name}"),
    }
}

pub(crate) fn role_mismatch_reason(
    expected: &PortRole,
    actual: &PortRole,
    param_id: &str,
) -> String {
    format!(
        "role_mismatch: expected {}, got {} for param '{}'",
        role_label(expected),
        role_label(actual),
        param_id
    )
}

fn variadic_group_diagnostics(def: &PieceDef) -> Vec<Diagnostic> {
    let mut groups = BTreeMap::<String, Vec<&ParamDef>>::new();
    for param in &def.params {
        let Some(group) = &param.variadic_group else {
            continue;
        };
        groups.entry(group.clone()).or_default().push(param);
    }

    let mut diagnostics = Vec::new();
    for (group, params) in groups {
        let param_ids = params
            .iter()
            .map(|param| param.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        if params.iter().any(|param| param.required) {
            diagnostics.push(invalid_registry_diagnostic(format!(
                "invalid_registry: piece '{}' variadic group '{}' cannot contain required params: {}",
                def.id, group, param_ids
            )));
        }

        let Some(first) = params.first() else {
            continue;
        };

        let first_port_type = first.schema.expected_port_type();
        if params
            .iter()
            .skip(1)
            .any(|param| param.schema.expected_port_type() != first_port_type)
        {
            diagnostics.push(invalid_registry_diagnostic(format!(
                "invalid_registry: piece '{}' variadic group '{}' mixes port types across params: {}",
                def.id, group, param_ids
            )));
        }

        if params.iter().skip(1).any(|param| param.role != first.role) {
            diagnostics.push(invalid_registry_diagnostic(format!(
                "invalid_registry: piece '{}' variadic group '{}' mixes roles across params: {}",
                def.id, group, param_ids
            )));
        }

        if params
            .iter()
            .skip(1)
            .any(|param| param.text_semantics != first.text_semantics)
        {
            diagnostics.push(invalid_registry_diagnostic(format!(
                "invalid_registry: piece '{}' variadic group '{}' mixes text semantics across params: {}",
                def.id, group, param_ids
            )));
        }

        let first_can_inline = first.schema.can_inline();
        if params
            .iter()
            .skip(1)
            .any(|param| param.schema.can_inline() != first_can_inline)
        {
            diagnostics.push(invalid_registry_diagnostic(format!(
                "invalid_registry: piece '{}' variadic group '{}' mixes inline capability across params: {}",
                def.id, group, param_ids
            )));
        }
    }

    diagnostics
}

pub(crate) fn find_param<'a>(piece: &'a PieceDef, param_id: &str) -> Option<&'a ParamDef> {
    piece.params.iter().find(|param| param.id == param_id)
}

pub(crate) fn effective_input_side(
    node: &Node,
    piece: &PieceDef,
    param_id: &str,
) -> Option<TileSide> {
    node.input_sides
        .get(param_id)
        .copied()
        .or_else(|| find_param(piece, param_id).map(|param| param.side))
}

pub(crate) fn effective_output_side(node: &Node, piece: &PieceDef) -> Option<TileSide> {
    if !piece.has_output() {
        None
    } else {
        node.output_side.or(piece.output_side)
    }
}

pub(crate) fn duplicate_effective_input_sides(
    node: &Node,
    piece: &PieceDef,
) -> Vec<(TileSide, Vec<String>)> {
    let mut params_by_side = BTreeMap::<TileSide, Vec<String>>::new();
    for param in &piece.params {
        if !param_counts_toward_side_conflict(piece, param) {
            continue;
        }
        if let Some(side) = effective_input_side(node, piece, param.id.as_str()) {
            params_by_side
                .entry(side)
                .or_default()
                .push(param.id.clone());
        }
    }

    params_by_side
        .into_iter()
        .filter_map(|(side, mut params)| {
            if params.len() < 2 {
                return None;
            }
            params.sort();
            Some((side, params))
        })
        .collect()
}

pub(crate) fn side_from_to_node(to_node: &GridPos, from: &GridPos) -> Option<TileSide> {
    match (from.col - to_node.col, from.row - to_node.row) {
        (1, 0) => Some(TileSide::RIGHT),
        (-1, 0) => Some(TileSide::LEFT),
        (0, -1) => Some(TileSide::TOP),
        (0, 1) => Some(TileSide::BOTTOM),
        _ => None,
    }
}

pub(crate) fn validate_graph_edge_structure(
    graph: &Graph,
    registry: &PieceRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<StructuralEdgeInfo> {
    let mut incoming_slots = BTreeSet::<(GridPos, String)>::new();
    let mut pending = Vec::<StructuralEdgeInfo>::new();

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

        let Some(param_def) = find_param(to_piece.def(), edge.to_param.as_str()) else {
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

        let Some(target_side) =
            effective_input_side(to_node, to_piece.def(), edge.to_param.as_str())
        else {
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

        if let Some(from_side) = effective_output_side(from_node, from_piece.def())
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

        pending.push(StructuralEdgeInfo {
            edge_id: edge.id.clone(),
            target_pos: edge.to_node,
            param: edge.to_param.clone(),
            schema: param_def.schema.clone(),
        });
    }

    pending
}

pub(crate) fn curate_diagnostics(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics.sort_by(|lhs, rhs| {
        diagnostic_severity_rank(&lhs.severity)
            .cmp(&diagnostic_severity_rank(&rhs.severity))
            .then(lhs.site.cmp(&rhs.site))
            .then(lhs.edge_id.cmp(&rhs.edge_id))
            .then(diagnostic_kind_key(&lhs.kind).cmp(&diagnostic_kind_key(&rhs.kind)))
    });

    let structural_edge_errors = diagnostics
        .iter()
        .filter_map(|diagnostic| {
            let edge_id = diagnostic.edge_id.clone()?;
            matches!(
                diagnostic.kind,
                DiagnosticKind::UnknownNode { .. }
                    | DiagnosticKind::UnknownParam { .. }
                    | DiagnosticKind::DuplicateConnection { .. }
                    | DiagnosticKind::NotAdjacent { .. }
                    | DiagnosticKind::SideMismatch { .. }
                    | DiagnosticKind::OutputFromTerminal { .. }
            )
            .then_some(edge_id)
        })
        .collect::<BTreeSet<_>>();

    let mut curated = Vec::new();
    let mut seen = BTreeSet::<(u8, Option<GridPos>, Option<EdgeId>, String)>::new();
    for diagnostic in diagnostics {
        if diagnostic
            .edge_id
            .as_ref()
            .is_some_and(|edge_id| structural_edge_errors.contains(edge_id))
            && matches!(
                diagnostic.kind,
                DiagnosticKind::TypeMismatch { .. }
                    | DiagnosticKind::UnsupportedDomainCrossing { .. }
            )
        {
            continue;
        }

        let key = (
            diagnostic_severity_rank(&diagnostic.severity),
            diagnostic.site,
            diagnostic.edge_id.clone(),
            diagnostic_kind_key(&diagnostic.kind),
        );
        if seen.insert(key) {
            curated.push(diagnostic);
        }
    }

    curated
}

fn diagnostic_severity_rank(severity: &DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 2,
    }
}

fn diagnostic_kind_key(kind: &DiagnosticKind) -> String {
    serde_json::to_string(kind).unwrap_or_else(|_| format!("{kind:?}"))
}

fn param_counts_toward_side_conflict(piece: &PieceDef, _param: &ParamDef) -> bool {
    !matches!(
        piece.id.as_str(),
        SUBGRAPH_INPUT_1_ID | SUBGRAPH_INPUT_2_ID | SUBGRAPH_INPUT_3_ID
    )
}
