mod input_resolution;
mod topo_sort;
mod type_inference;
mod validation;

pub(crate) use input_resolution::{
    effective_source_output_role_for_edge, effective_source_output_type_for_edge,
    incoming_edge_for_param, resolved_input_connection_for_param, resolved_input_types_for_piece,
    traversed_input_origin_for_edge,
};

use std::collections::{BTreeMap, BTreeSet};

use crate::analysis::{
    AnalysisCache, AnalyzedGraph, AnalyzedNode, ResolvedInput, ResolvedInputSource,
};
use crate::diagnostics::{Diagnostic, DiagnosticKind};
use crate::graph::{Graph, Node};
use crate::internal::{
    StructuralEdgeInfo, curate_diagnostics, role_mismatch_reason, roles_compatible,
};
use crate::piece::PieceDef;
use crate::piece_registry::PieceRegistry;
use crate::types::{DomainBridge, EdgeId, GridPos, PortRole, PortType};

use self::topo_sort::{TopoAnalysis, topo_sort_with_delay_edges};
use self::type_inference::{
    collect_delay_type_diagnostics, stabilize_output_types, stabilize_output_types_for_nodes,
};
use self::validation::{
    collect_outputs, evaluate_edge_type_checks, node_output_side, validate_connector_nodes,
    validate_edge_structure, validate_inline_and_required_params, validate_known_pieces,
    warn_on_unreachable_nodes,
};

struct AnalysisScaffold {
    diagnostics: Vec<Diagnostic>,
    pending_type_checks: Vec<StructuralEdgeInfo>,
    topo: TopoAnalysis,
    outputs: Vec<GridPos>,
}

struct FinalizedAnalysis {
    analyzed: AnalyzedGraph,
    component_members: BTreeMap<GridPos, BTreeSet<GridPos>>,
}

#[derive(Clone, Copy)]
struct ResolvedInputFacts<'a> {
    output_types: &'a BTreeMap<GridPos, PortType>,
    domain_bridges: &'a BTreeMap<EdgeId, DomainBridge>,
}

pub fn semantic_pass(graph: &Graph, registry: &PieceRegistry) -> AnalyzedGraph {
    analyze_graph_internal(graph, registry)
}

pub(crate) fn analyze_graph_internal(graph: &Graph, registry: &PieceRegistry) -> AnalyzedGraph {
    analyze_full(graph, registry).analyzed
}

pub(crate) fn infer_output_types_internal(
    graph: &Graph,
    registry: &PieceRegistry,
) -> BTreeMap<GridPos, PortType> {
    let scaffold = build_analysis_scaffold(graph, registry);
    stabilize_output_types(graph, registry, &scaffold.topo.eval_order)
}

pub fn analyze_cached(
    graph: &Graph,
    registry: &PieceRegistry,
    cache: &mut AnalysisCache,
) -> AnalyzedGraph {
    if cache.analyzed.is_none() {
        let finalized = analyze_full(graph, registry);
        return cache.store_analysis(graph, finalized.analyzed, finalized.component_members);
    }

    if cache.dirty.is_empty() {
        return cache
            .analyzed
            .clone()
            .expect("cache analyzed presence checked before returning cached result");
    }

    let cached = cache
        .analyzed
        .clone()
        .expect("dirty cached analysis must keep the previous analyzed graph");
    let scaffold = build_analysis_scaffold(graph, registry);
    let finalized = analyze_incremental(graph, registry, cache, &cached, scaffold);
    cache.store_analysis(graph, finalized.analyzed, finalized.component_members)
}

fn analyze_full(graph: &Graph, registry: &PieceRegistry) -> FinalizedAnalysis {
    let scaffold = build_analysis_scaffold(graph, registry);
    finalize_full_analysis(graph, registry, scaffold)
}

fn build_analysis_scaffold(graph: &Graph, registry: &PieceRegistry) -> AnalysisScaffold {
    let mut diagnostics = Vec::new();

    validate_known_pieces(graph, registry, &mut diagnostics);
    let pending_type_checks = validate_edge_structure(graph, registry, &mut diagnostics);
    validate_inline_and_required_params(graph, registry, &mut diagnostics);
    validate_connector_nodes(graph, registry, &mut diagnostics);

    let topo = topo_sort_with_delay_edges(graph);
    diagnostics.extend(topo.diagnostics.iter().cloned());

    let outputs = collect_outputs(graph, registry, &mut diagnostics);
    warn_on_unreachable_nodes(graph, &outputs, &mut diagnostics);

    AnalysisScaffold {
        diagnostics,
        pending_type_checks,
        topo,
        outputs,
    }
}

fn finalize_full_analysis(
    graph: &Graph,
    registry: &PieceRegistry,
    scaffold: AnalysisScaffold,
) -> FinalizedAnalysis {
    let mut diagnostics = scaffold.diagnostics;
    let output_types = stabilize_output_types(graph, registry, &scaffold.topo.eval_order);
    diagnostics.extend(collect_delay_type_diagnostics(
        graph,
        registry,
        &output_types,
        None,
    ));

    let domain_bridges = evaluate_all_pending_type_checks(
        graph,
        registry,
        scaffold.pending_type_checks,
        &output_types,
        &mut diagnostics,
    );
    let nodes = build_analyzed_nodes(graph, registry, &output_types, &domain_bridges, None);
    diagnostics.extend(collect_role_compatibility_diagnostics(&nodes, None));
    diagnostics.extend(collect_piece_semantic_diagnostics(&nodes, registry, None));
    diagnostics = curate_diagnostics(diagnostics);

    let component_members = scaffold.topo.component_members.clone();

    FinalizedAnalysis {
        analyzed: AnalyzedGraph {
            diagnostics,
            eval_order: scaffold.topo.eval_order,
            outputs: scaffold.outputs,
            nodes,
            output_types,
            domain_bridges,
            delay_edges: scaffold.topo.delay_edges,
        },
        component_members,
    }
}

fn analyze_incremental(
    graph: &Graph,
    registry: &PieceRegistry,
    cache: &AnalysisCache,
    cached: &AnalyzedGraph,
    scaffold: AnalysisScaffold,
) -> FinalizedAnalysis {
    let affected = expand_affected_nodes(graph, cache, &scaffold.topo.component_members);
    let seed_output_types = cached
        .output_types
        .iter()
        .filter(|(pos, _)| graph.nodes.contains_key(pos) && !affected.contains(pos))
        .map(|(pos, port_type)| (*pos, port_type.clone()))
        .collect::<BTreeMap<_, _>>();
    let output_types = stabilize_output_types_for_nodes(
        graph,
        registry,
        &scaffold.topo.eval_order,
        &seed_output_types,
        Some(&affected),
    );

    let affected_edge_ids = affected_edge_ids(graph, &affected, cache);
    let mut diagnostics = scaffold.diagnostics;
    diagnostics.extend(reuse_cached_incremental_diagnostics(
        cached,
        graph,
        &affected,
        &affected_edge_ids,
    ));
    diagnostics.extend(collect_delay_type_diagnostics(
        graph,
        registry,
        &output_types,
        Some(&affected),
    ));

    let mut domain_bridges = cached
        .domain_bridges
        .iter()
        .filter(|(edge_id, _)| {
            graph.edges.contains_key(*edge_id) && !affected_edge_ids.contains(*edge_id)
        })
        .map(|(edge_id, bridge)| (edge_id.clone(), bridge.clone()))
        .collect::<BTreeMap<_, _>>();
    let incremental_bridges = evaluate_all_pending_type_checks(
        graph,
        registry,
        scaffold
            .pending_type_checks
            .into_iter()
            .filter(|check| affected_edge_ids.contains(&check.edge_id))
            .collect(),
        &output_types,
        &mut diagnostics,
    );
    domain_bridges.extend(incremental_bridges);

    let mut nodes = cached
        .nodes
        .iter()
        .filter(|(pos, _)| graph.nodes.contains_key(pos) && !affected.contains(pos))
        .map(|(pos, node)| (*pos, node.clone()))
        .collect::<BTreeMap<_, _>>();
    nodes.extend(build_analyzed_nodes(
        graph,
        registry,
        &output_types,
        &domain_bridges,
        Some(&affected),
    ));

    diagnostics.extend(collect_role_compatibility_diagnostics(
        &nodes,
        Some(&affected),
    ));
    diagnostics.extend(collect_piece_semantic_diagnostics(
        &nodes,
        registry,
        Some(&affected),
    ));
    diagnostics = curate_diagnostics(diagnostics);

    let component_members = scaffold.topo.component_members.clone();

    FinalizedAnalysis {
        analyzed: AnalyzedGraph {
            diagnostics,
            eval_order: scaffold.topo.eval_order,
            outputs: scaffold.outputs,
            nodes,
            output_types,
            domain_bridges,
            delay_edges: scaffold.topo.delay_edges,
        },
        component_members,
    }
}

fn evaluate_all_pending_type_checks(
    graph: &Graph,
    registry: &PieceRegistry,
    pending_type_checks: Vec<StructuralEdgeInfo>,
    output_types: &BTreeMap<GridPos, PortType>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<EdgeId, DomainBridge> {
    evaluate_edge_type_checks(
        graph,
        registry,
        pending_type_checks,
        output_types,
        diagnostics,
    )
}

fn expand_affected_nodes(
    graph: &Graph,
    cache: &AnalysisCache,
    current_component_members: &BTreeMap<GridPos, BTreeSet<GridPos>>,
) -> BTreeSet<GridPos> {
    let current_downstream = current_downstream_dependents(graph);
    let mut affected = cache.dirty.clone();

    for dirty in &cache.dirty {
        add_current_touching_neighbors(graph, dirty, &mut affected);
        if let Some(previous_dependents) = cache.downstream_dependents.get(dirty) {
            affected.extend(previous_dependents.iter().copied());
        }
        if let Some(previous_targets) = cache.incoming_targets_by_source.get(dirty) {
            affected.extend(previous_targets.iter().copied());
        }
    }

    loop {
        let snapshot = affected.iter().copied().collect::<Vec<_>>();
        let before = affected.len();

        for pos in snapshot {
            if let Some(targets) = current_downstream.get(&pos) {
                affected.extend(targets.iter().copied());
            }
            if let Some(targets) = cache.downstream_dependents.get(&pos) {
                affected.extend(targets.iter().copied());
            }
            if let Some(targets) = cache.incoming_targets_by_source.get(&pos) {
                affected.extend(targets.iter().copied());
            }
            if let Some(members) = current_component_members.get(&pos) {
                affected.extend(members.iter().copied());
            }
            if let Some(members) = cache.component_members.get(&pos) {
                affected.extend(members.iter().copied());
            }
        }

        if affected.len() == before {
            break;
        }
    }

    affected.retain(|pos| graph.nodes.contains_key(pos));
    affected
}

fn current_downstream_dependents(graph: &Graph) -> BTreeMap<GridPos, BTreeSet<GridPos>> {
    let mut downstream = BTreeMap::<GridPos, BTreeSet<GridPos>>::new();

    for edge in graph.edges.values() {
        if graph.nodes.contains_key(&edge.from) && graph.nodes.contains_key(&edge.to_node) {
            downstream
                .entry(edge.from)
                .or_default()
                .insert(edge.to_node);
        }
    }

    downstream
}

fn add_current_touching_neighbors(
    graph: &Graph,
    dirty: &GridPos,
    affected: &mut BTreeSet<GridPos>,
) {
    for edge in graph.edges.values() {
        if edge.from == *dirty || edge.to_node == *dirty {
            affected.insert(edge.from);
            affected.insert(edge.to_node);
        }
    }
}

fn affected_edge_ids(
    graph: &Graph,
    affected: &BTreeSet<GridPos>,
    cache: &AnalysisCache,
) -> BTreeSet<EdgeId> {
    graph
        .edges
        .values()
        .filter(|edge| {
            affected.contains(&edge.from)
                || affected.contains(&edge.to_node)
                || !cache.edge_ids.contains(&edge.id)
        })
        .map(|edge| edge.id.clone())
        .collect()
}

fn reuse_cached_incremental_diagnostics(
    cached: &AnalyzedGraph,
    graph: &Graph,
    affected: &BTreeSet<GridPos>,
    affected_edge_ids: &BTreeSet<EdgeId>,
) -> Vec<Diagnostic> {
    cached
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            should_reuse_cached_diagnostic(diagnostic, graph, affected, affected_edge_ids)
        })
        .cloned()
        .collect()
}

fn should_reuse_cached_diagnostic(
    diagnostic: &Diagnostic,
    graph: &Graph,
    affected: &BTreeSet<GridPos>,
    affected_edge_ids: &BTreeSet<EdgeId>,
) -> bool {
    if is_incremental_edge_diagnostic(diagnostic) {
        return diagnostic.edge_id.as_ref().is_some_and(|edge_id| {
            graph.edges.contains_key(edge_id) && !affected_edge_ids.contains(edge_id)
        });
    }

    if is_incremental_node_diagnostic(diagnostic) {
        return diagnostic
            .site
            .is_some_and(|site| graph.nodes.contains_key(&site) && !affected.contains(&site));
    }

    false
}

fn is_incremental_edge_diagnostic(diagnostic: &Diagnostic) -> bool {
    matches!(
        diagnostic.kind,
        DiagnosticKind::OutputFromTerminal { .. }
            | DiagnosticKind::TypeMismatch { .. }
            | DiagnosticKind::UnsupportedDomainCrossing { .. }
    )
}

fn is_incremental_node_diagnostic(diagnostic: &Diagnostic) -> bool {
    matches!(diagnostic.kind, DiagnosticKind::DelayTypeMismatch { .. })
        || matches!(
            &diagnostic.kind,
            DiagnosticKind::InvalidOperation { reason } if reason.starts_with("role_mismatch:")
        )
        || matches!(diagnostic.kind, DiagnosticKind::PieceSemantic { .. })
}

fn collect_piece_semantic_diagnostics(
    nodes: &BTreeMap<GridPos, AnalyzedNode>,
    registry: &PieceRegistry,
    affected: Option<&BTreeSet<GridPos>>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (pos, node) in nodes {
        if affected.is_some_and(|affected| !affected.contains(pos)) {
            continue;
        }
        let Some(piece) = registry.get(node.piece_id.as_str()) else {
            continue;
        };
        diagnostics.extend(piece.validate_analysis(*pos, node));
    }

    diagnostics
}

fn collect_role_compatibility_diagnostics(
    nodes: &BTreeMap<GridPos, AnalyzedNode>,
    affected: Option<&BTreeSet<GridPos>>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (pos, node) in nodes {
        if affected.is_some_and(|affected| !affected.contains(pos)) {
            continue;
        }
        for (param_id, input) in &node.scalar_inputs {
            if input.is_missing() {
                continue;
            }

            let expected_role = node.input_roles.get(param_id).cloned().unwrap_or_default();
            let actual_role = match &input.source {
                ResolvedInputSource::Edge { from, .. } => nodes
                    .get(from)
                    .map(|source_node| source_node.output_role.clone())
                    .unwrap_or_default(),
                ResolvedInputSource::Inline { .. } | ResolvedInputSource::Default { .. } => {
                    PortRole::Value
                }
                ResolvedInputSource::Missing => continue,
            };

            if roles_compatible(&expected_role, &actual_role) {
                continue;
            }

            diagnostics.push(Diagnostic::error(
                DiagnosticKind::InvalidOperation {
                    reason: role_mismatch_reason(&expected_role, &actual_role, param_id),
                },
                Some(*pos),
            ));
        }
    }

    diagnostics
}

fn build_analyzed_nodes(
    graph: &Graph,
    registry: &PieceRegistry,
    output_types: &BTreeMap<GridPos, PortType>,
    domain_bridges: &BTreeMap<EdgeId, DomainBridge>,
    affected: Option<&BTreeSet<GridPos>>,
) -> BTreeMap<GridPos, AnalyzedNode> {
    graph
        .nodes
        .iter()
        .filter(|(pos, _)| affected.is_none_or(|affected| affected.contains(pos)))
        .map(|(pos, node)| {
            let analyzed = registry
                .get(node.piece_id.as_str())
                .map(|piece| {
                    analyzed_node_for_piece(
                        graph,
                        registry,
                        node,
                        pos,
                        piece.def(),
                        output_types,
                        domain_bridges,
                    )
                })
                .unwrap_or_else(|| AnalyzedNode {
                    piece_id: node.piece_id.clone(),
                    inline_params: node.inline_params.clone(),
                    scalar_inputs: BTreeMap::new(),
                    variadic_inputs: BTreeMap::new(),
                    input_types: BTreeMap::new(),
                    output_type: None,
                    input_roles: BTreeMap::new(),
                    output_role: Default::default(),
                    output_side: node.output_side,
                    node_state: node.node_state.clone(),
                });
            (*pos, analyzed)
        })
        .collect()
}

fn analyzed_node_for_piece(
    graph: &Graph,
    registry: &PieceRegistry,
    node: &Node,
    pos: &GridPos,
    piece: &PieceDef,
    output_types: &BTreeMap<GridPos, PortType>,
    domain_bridges: &BTreeMap<EdgeId, DomainBridge>,
) -> AnalyzedNode {
    let mut scalar_inputs = BTreeMap::<String, ResolvedInput>::new();
    let mut variadic_inputs = BTreeMap::<String, Vec<ResolvedInput>>::new();
    let facts = ResolvedInputFacts {
        output_types,
        domain_bridges,
    };

    for param in &piece.params {
        let resolved = resolve_input(graph, registry, node, pos, param, facts);

        record_resolved_input_in_declaration_order(
            &mut scalar_inputs,
            &mut variadic_inputs,
            param,
            resolved,
        );
    }

    let input_roles = piece
        .params
        .iter()
        .filter(|param| !param.role.is_value())
        .map(|param| (param.id.clone(), param.role.clone()))
        .collect();

    AnalyzedNode {
        piece_id: node.piece_id.clone(),
        inline_params: node.inline_params.clone(),
        scalar_inputs,
        variadic_inputs,
        input_types: resolved_input_types_for_piece(
            graph,
            registry,
            node,
            pos,
            piece,
            output_types,
        ),
        output_type: output_types
            .get(pos)
            .cloned()
            .or_else(|| piece.output_type.clone()),
        input_roles,
        output_role: piece.output_role.clone(),
        output_side: node_output_side(node, piece),
        node_state: node.node_state.clone(),
    }
}

fn record_resolved_input_in_declaration_order(
    scalar_inputs: &mut BTreeMap<String, ResolvedInput>,
    variadic_inputs: &mut BTreeMap<String, Vec<ResolvedInput>>,
    param: &crate::piece::ParamDef,
    resolved: ResolvedInput,
) {
    if let Some(group) = &param.variadic_group {
        // `analyzed_node_for_piece` walks `piece.params` in declaration order, so
        // appending here makes the host-facing variadic group order explicit.
        variadic_inputs
            .entry(group.clone())
            .or_default()
            .push(resolved.clone());
    }
    scalar_inputs.insert(param.id.clone(), resolved);
}

fn resolve_input(
    graph: &Graph,
    registry: &PieceRegistry,
    node: &Node,
    pos: &GridPos,
    param: &crate::piece::ParamDef,
    facts: ResolvedInputFacts<'_>,
) -> ResolvedInput {
    if let Some(edge) = incoming_edge_for_param(graph, pos, param.id.as_str()) {
        let traversed = traversed_input_origin_for_edge(graph, registry, edge);
        let connection = resolved_input_connection_for_param(
            graph,
            registry,
            node,
            pos,
            param.id.as_str(),
            &param.schema,
            facts.output_types,
        );
        let source = match traversed {
            input_resolution::TraversedInputOrigin::Edge(origin) => ResolvedInputSource::Edge {
                edge_id: edge.id.clone(),
                from: origin.source_pos,
                exit_side: origin.exit_side,
                via: origin.via,
            },
            input_resolution::TraversedInputOrigin::Inline { value, .. } => {
                ResolvedInputSource::Inline { value }
            }
            input_resolution::TraversedInputOrigin::Default { value, .. } => {
                ResolvedInputSource::Default { value }
            }
            input_resolution::TraversedInputOrigin::Missing => ResolvedInputSource::Missing,
        };

        return ResolvedInput {
            source,
            effective_type: connection
                .as_ref()
                .map(|connection| connection.effective_type.clone()),
            bridge_kind: facts
                .domain_bridges
                .get(&edge.id)
                .map(|bridge| bridge.kind)
                .or_else(|| connection.and_then(|connection| connection.bridge_kind)),
        };
    }

    if let Some(value) = node.inline_params.get(param.id.as_str()) {
        return ResolvedInput {
            source: ResolvedInputSource::Inline {
                value: value.clone(),
            },
            effective_type: param.schema.infer_inline_port_type(value),
            bridge_kind: None,
        };
    }

    if let Some(value) = param.schema.default_value() {
        return ResolvedInput {
            source: ResolvedInputSource::Default { value },
            effective_type: param.schema.resolved_port_type(None),
            bridge_kind: None,
        };
    }

    ResolvedInput {
        source: ResolvedInputSource::Missing,
        effective_type: None,
        bridge_kind: None,
    }
}

#[cfg(test)]
mod tests;
