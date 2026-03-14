use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::diagnostics::{Diagnostic, DiagnosticKind, SemanticResult};
use crate::graph::{Edge, Graph, Node};
use crate::piece::PieceDef;
use crate::piece_registry::PieceRegistry;
use crate::types::{GridPos, TileSide, adjacent_in_direction};

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

pub fn semantic_pass(graph: &Graph, registry: &PieceRegistry) -> SemanticResult {
    let mut diagnostics = Vec::<Diagnostic>::new();

    for (pos, node) in &graph.nodes {
        if registry.get(node.piece_id.as_str()).is_none() {
            diagnostics.push(Diagnostic::error(
                DiagnosticKind::UnknownPiece {
                    piece_id: node.piece_id.clone(),
                },
                Some(pos.clone()),
            ));
        }
    }

    let mut incoming_slots = BTreeSet::<(GridPos, String)>::new();
    for edge in graph.edges.values() {
        let Some(from_node) = graph.nodes.get(&edge.from) else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::UnknownNode {
                        pos: edge.from.clone(),
                    },
                    Some(edge.to_node.clone()),
                )
                .with_edge(edge.id.clone()),
            );
            continue;
        };
        let Some(to_node) = graph.nodes.get(&edge.to_node) else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::UnknownNode {
                        pos: edge.to_node.clone(),
                    },
                    Some(edge.to_node.clone()),
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

        if !incoming_slots.insert((edge.to_node.clone(), edge.to_param.clone())) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::DuplicateConnection {
                        to_node: edge.to_node.clone(),
                        to_param: edge.to_param.clone(),
                    },
                    Some(edge.to_node.clone()),
                )
                .with_edge(edge.id.clone()),
            );
        }

        let Some(param_def) = to_piece
            .def()
            .params
            .iter()
            .find(|param| param.id == edge.to_param)
        else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::UnknownParam {
                        piece_id: to_piece.def().id.clone(),
                        param: edge.to_param.clone(),
                    },
                    Some(edge.to_node.clone()),
                )
                .with_edge(edge.id.clone()),
            );
            continue;
        };

        let target_side = node_param_side(to_node, edge.to_param.as_str(), param_def.side);
        let expected_neighbor = adjacent_in_direction(&edge.to_node, &target_side);
        if expected_neighbor != edge.from {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::NotAdjacent {
                        from_pos: edge.from.clone(),
                        to_pos: edge.to_node.clone(),
                    },
                    Some(edge.to_node.clone()),
                )
                .with_edge(edge.id.clone()),
            );
            continue;
        }

        if let Some(from_side) = node_output_side(from_node, from_piece.def()) {
            if !from_side.faces(target_side) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticKind::SideMismatch {
                            from_pos: edge.from.clone(),
                            to_pos: edge.to_node.clone(),
                            expected_side: target_side,
                        },
                        Some(edge.to_node.clone()),
                    )
                    .with_edge(edge.id.clone()),
                );
            }
        }

        let Some(from_output_type) = from_piece.def().output_type.as_ref() else {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::OutputFromTerminal {
                        position: edge.from.clone(),
                    },
                    Some(edge.to_node.clone()),
                )
                .with_edge(edge.id.clone()),
            );
            continue;
        };

        if !param_def.schema.accepts(from_output_type) {
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticKind::TypeMismatch {
                        expected: param_def.schema.expected_port_type(),
                        got: from_output_type.clone(),
                        param: edge.to_param.clone(),
                    },
                    Some(edge.to_node.clone()),
                )
                .with_edge(edge.id.clone()),
            );
        }
    }

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
                    Some(pos.clone()),
                ));
                continue;
            };
            if !param_def.schema.can_inline() {
                diagnostics.push(Diagnostic::error(
                    DiagnosticKind::InlineNotAllowed {
                        param: inline_key.clone(),
                    },
                    Some(pos.clone()),
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
                    Some(pos.clone()),
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
                Some(pos.clone()),
            ));
        }

        for param in &piece.def().params {
            let has_edge = incoming_edge_for_param(graph, pos, param.id.as_str()).is_some();
            let has_inline = node.inline_params.contains_key(param.id.as_str());
            let has_default = param.schema.default_expr().is_some();
            if param.required && !has_edge && !has_inline && !has_default {
                diagnostics.push(Diagnostic::error(
                    DiagnosticKind::MissingRequiredParam {
                        param: param.id.clone(),
                    },
                    Some(pos.clone()),
                ));
            }
        }
    }

    let mut indegree = BTreeMap::<GridPos, usize>::new();
    let mut out_edges = BTreeMap::<GridPos, Vec<GridPos>>::new();

    for pos in graph.nodes.keys() {
        indegree.insert(pos.clone(), 0);
    }

    for edge in graph.edges.values() {
        if !(graph.nodes.contains_key(&edge.from) && graph.nodes.contains_key(&edge.to_node)) {
            continue;
        }
        out_edges
            .entry(edge.from.clone())
            .or_default()
            .push(edge.to_node.clone());
        if let Some(target_indegree) = indegree.get_mut(&edge.to_node) {
            *target_indegree += 1;
        }
    }

    let mut frontier = indegree
        .iter()
        .filter_map(|(pos, degree)| {
            if *degree == 0 {
                Some(pos.clone())
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();

    let mut eval_order = Vec::with_capacity(indegree.len());
    while let Some(next) = frontier.iter().next().cloned() {
        frontier.remove(&next);
        eval_order.push(next.clone());

        if let Some(targets) = out_edges.get(&next) {
            for target in targets {
                if let Some(target_indegree) = indegree.get_mut(target) {
                    *target_indegree = target_indegree.saturating_sub(1);
                    if *target_indegree == 0 {
                        frontier.insert(target.clone());
                    }
                }
            }
        }
    }

    if eval_order.len() != graph.nodes.len() {
        let ordered = eval_order.iter().cloned().collect::<BTreeSet<_>>();
        let involved = graph
            .nodes
            .keys()
            .filter(|pos| !ordered.contains(*pos))
            .cloned()
            .collect::<Vec<_>>();
        diagnostics.push(Diagnostic::error(
            DiagnosticKind::Cycle {
                involved: involved.clone(),
            },
            involved.first().cloned(),
        ));
        for pos in involved {
            if !eval_order.iter().any(|item| item == &pos) {
                eval_order.push(pos);
            }
        }
    }

    let terminals = graph
        .nodes
        .iter()
        .filter_map(|(pos, node)| {
            registry
                .get(node.piece_id.as_str())
                .filter(|piece| piece.def().is_terminal())
                .map(|_| pos.clone())
        })
        .collect::<Vec<_>>();

    match terminals.len() {
        0 => {
            diagnostics.push(Diagnostic::error(DiagnosticKind::NoTerminalNode, None));
        }
        1 => {}
        _ => {
            // Multiple terminals: allowed, but surface as a warning so the UI can inform the user.
            diagnostics.push(Diagnostic::warning(
                DiagnosticKind::MultipleTerminalNodes {
                    positions: terminals.clone(),
                },
                None,
            ));
        }
    }

    // Backward reachability from all terminals simultaneously.
    if !terminals.is_empty() {
        let mut reachable = BTreeSet::<GridPos>::new();
        let mut frontier: Vec<GridPos> = terminals.clone();
        while let Some(next) = frontier.pop() {
            if !reachable.insert(next.clone()) {
                continue;
            }
            for edge in graph.edges.values() {
                if edge.to_node == next {
                    frontier.push(edge.from.clone());
                }
            }
        }

        for pos in graph.nodes.keys() {
            if !reachable.contains(pos) {
                // UnreachableNode is a warning — isolated stub nodes should not block compilation.
                diagnostics.push(Diagnostic::warning(
                    DiagnosticKind::UnreachableNode {
                        position: pos.clone(),
                    },
                    Some(pos.clone()),
                ));
            }
        }
    }

    SemanticResult {
        diagnostics,
        eval_order,
        terminals,
    }
}
