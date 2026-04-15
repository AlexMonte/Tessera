use std::collections::BTreeSet;

use serde_json::Value;

use crate::diagnostics::{Diagnostic, DiagnosticKind};
use crate::graph::Graph;
use crate::ops::validate_edge_connect;
use crate::piece_registry::PieceRegistry;
use crate::semantic::analyze_graph_internal;
use crate::types::GridPos;

use super::helpers::{slot_from_piece_id, subgraph_boundary_port_type};
use super::types::{
    SUBGRAPH_INPUT_1_ID, SUBGRAPH_INPUT_2_ID, SUBGRAPH_INPUT_3_ID, SUBGRAPH_OUTPUT_ID,
    SubgraphInput, SubgraphSignature,
};

const MAX_SUBGRAPH_INPUTS: usize = 3;

fn is_subgraph_input_id(piece_id: &str) -> bool {
    matches!(
        piece_id,
        SUBGRAPH_INPUT_1_ID | SUBGRAPH_INPUT_2_ID | SUBGRAPH_INPUT_3_ID
    )
}

/// Analyse a subgraph, returning its [`SubgraphSignature`] or diagnostics.
pub fn analyze_subgraph(
    graph: &Graph,
    registry: &PieceRegistry,
) -> Result<SubgraphSignature, Vec<Diagnostic>> {
    let sem = analyze_graph_internal(graph, registry);
    let mut inputs = Vec::<SubgraphInput>::new();
    let mut output_positions = Vec::<GridPos>::new();
    let mut diagnostics = sem
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == crate::diagnostics::DiagnosticSeverity::Error)
        .cloned()
        .collect::<Vec<_>>();

    for (pos, node) in &graph.nodes {
        if is_subgraph_input_id(node.piece_id.as_str()) {
            let slot = slot_from_piece_id(node.piece_id.as_str()).unwrap_or(1);
            let label = node
                .inline_params
                .get("label")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("input {slot}"));
            let port_type = subgraph_boundary_port_type(&node.inline_params);
            let required = node
                .inline_params
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let is_receiver = node
                .inline_params
                .get("is_receiver")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let default_value = node.inline_params.get("default_value").cloned();

            inputs.push(SubgraphInput {
                slot,
                pos: *pos,
                label,
                port_type,
                required,
                is_receiver,
                default_value,
            });
        } else if node.piece_id == SUBGRAPH_OUTPUT_ID {
            output_positions.push(*pos);
        }
    }

    if inputs.len() > MAX_SUBGRAPH_INPUTS {
        diagnostics.push(Diagnostic::error(
            DiagnosticKind::InvalidOperation {
                reason: format!("subgraph may declare at most {MAX_SUBGRAPH_INPUTS} inputs"),
            },
            inputs.get(MAX_SUBGRAPH_INPUTS).map(|i| i.pos),
        ));
    }

    let mut seen_slots = BTreeSet::new();
    for input in &inputs {
        if !seen_slots.insert(input.slot) {
            diagnostics.push(Diagnostic::error(
                DiagnosticKind::InvalidOperation {
                    reason: format!("duplicate subgraph input slot {}", input.slot),
                },
                Some(input.pos),
            ));
        }
    }

    if output_positions.is_empty() {
        diagnostics.push(Diagnostic::error(
            DiagnosticKind::InvalidOperation {
                reason: "subgraph requires exactly one output".into(),
            },
            None,
        ));
    } else if output_positions.len() > 1 {
        diagnostics.push(Diagnostic::error(
            DiagnosticKind::InvalidOperation {
                reason: "subgraph requires exactly one output".into(),
            },
            output_positions.get(1).cloned(),
        ));
    }

    let receiver_count = inputs.iter().filter(|i| i.is_receiver).count();
    if receiver_count > 1 {
        diagnostics.push(Diagnostic::error(
            DiagnosticKind::InvalidOperation {
                reason: "subgraph may declare at most one receiver input".into(),
            },
            inputs.iter().find(|i| i.is_receiver).map(|i| i.pos),
        ));
    }

    // Boundary edges should obey the same structural, type, and role rules as ordinary edges.
    for input in &inputs {
        for edge in graph.edges.values().filter(|e| e.from == input.pos) {
            let mut probe_graph = graph.clone();
            probe_graph.edges.remove(&edge.id);
            if let Err(reject) = validate_edge_connect(
                &probe_graph,
                registry,
                &input.pos,
                &edge.to_node,
                &edge.to_param,
            ) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticKind::InvalidOperation {
                            reason: reject
                                .detail
                                .unwrap_or_else(|| "invalid subgraph boundary edge".into()),
                        },
                        Some(edge.to_node),
                    )
                    .with_edge(edge.id.clone()),
                );
            }
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    inputs.sort_by_key(|i| i.slot);
    let output_pos = output_positions
        .into_iter()
        .next()
        .expect("checked output existence");
    let output_type = sem
        .nodes
        .get(&output_pos)
        .and_then(|output_node| output_node.input_types.get("input").cloned());
    Ok(SubgraphSignature {
        inputs,
        output_pos,
        output_type,
    })
}
