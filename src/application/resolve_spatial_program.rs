use std::collections::BTreeSet;

use crate::domain::{
    AuthoredTesseraProgram, Diagnostic, DiagnosticCategory, DiagnosticKind, DiagnosticLocation,
    InputEndpoint, NodeId, OutputEndpoint, RootRelation, RootSurfaceNodeKind, SpatialSide,
    StreamSource, StreamTarget, TesseraProgram,
};

use super::validate_surface::validate_root_surface_shape;

pub fn resolve_spatial_program(
    authored: &AuthoredTesseraProgram,
) -> Result<TesseraProgram, Vec<Diagnostic>> {
    validate_root_surface_shape(authored)?;
    let mut relations = authored.root_surface.explicit_relations.clone();
    let inferred = infer_spatial_relations(authored)?;
    relations.extend(inferred);
    relations = dedupe_relations(relations);
    Ok(TesseraProgram {
        root_nodes: authored.root_surface.nodes.clone(),
        containers: authored.containers.clone(),
        relations,
    })
}

fn infer_spatial_relations(
    authored: &AuthoredTesseraProgram,
) -> Result<Vec<RootRelation>, Vec<Diagnostic>> {
    let mut relations = Vec::new();
    let mut diagnostics = Vec::new();
    for (target_node, bindings) in &authored.root_surface.bindings {
        for (input_endpoint, side) in &bindings.inputs {
            if !side.is_enabled() {
                continue;
            }
            match infer_input_relation(authored, target_node, input_endpoint, *side) {
                Ok(Some(relation)) => relations.push(relation),
                Ok(None) => {}
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(relations)
    } else {
        Err(diagnostics)
    }
}

fn infer_input_relation(
    authored: &AuthoredTesseraProgram,
    target_node: &NodeId,
    input_endpoint: &InputEndpoint,
    side: SpatialSide,
) -> Result<Option<RootRelation>, Diagnostic> {
    let Some(source_node) = neighbor_at_side(authored, target_node, side) else {
        return Ok(None);
    };
    let source_endpoint = choose_source_endpoint(authored, &source_node, side.opposite())
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCategory::RootRelation,
                DiagnosticKind::InvalidFlowSource,
                "No unique source endpoint found on neighboring tile for the requested side.",
                Some(DiagnosticLocation::RootNode(source_node.clone())),
            )
        })?;
    let target = stream_target_for_input(authored, target_node.clone(), input_endpoint.clone())?;
    Ok(Some(RootRelation::FlowsTo {
        from: StreamSource {
            node: source_node,
            endpoint: source_endpoint,
        },
        to: target,
    }))
}

fn neighbor_at_side(
    authored: &AuthoredTesseraProgram,
    node: &NodeId,
    side: SpatialSide,
) -> Option<NodeId> {
    let placement = authored.root_surface.placements.get(node)?;
    let (dx, dy) = side.offset()?;
    let wanted = placement.slot.offset(dx, dy);
    authored
        .root_surface
        .placements
        .iter()
        .find_map(|(candidate, candidate_place)| {
            if candidate == node {
                return None;
            }
            if candidate_place.slot == wanted {
                Some(candidate.clone())
            } else {
                None
            }
        })
}

fn choose_source_endpoint(
    authored: &AuthoredTesseraProgram,
    source_node: &NodeId,
    side_facing_target: SpatialSide,
) -> Option<OutputEndpoint> {
    let bindings = authored.root_surface.bindings.get(source_node)?;
    let mut candidates = bindings
        .outputs
        .iter()
        .filter_map(|(endpoint, side)| {
            if *side == side_facing_target {
                Some(endpoint.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}

fn stream_target_for_input(
    authored: &AuthoredTesseraProgram,
    target_node: NodeId,
    endpoint: InputEndpoint,
) -> Result<StreamTarget, Diagnostic> {
    let node = authored
        .root_surface
        .nodes
        .get(&target_node)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCategory::RootRelation,
                DiagnosticKind::InvalidFlowTarget,
                "Spatial target node does not exist.",
                Some(DiagnosticLocation::RootNode(target_node.clone())),
            )
        })?;
    match node {
        RootSurfaceNodeKind::Transform(_) => Ok(StreamTarget::TransformInput {
            node: target_node,
            endpoint,
        }),
        RootSurfaceNodeKind::FlowControl(_) => Ok(StreamTarget::FlowControlInput {
            node: target_node,
            endpoint,
        }),
        RootSurfaceNodeKind::Output(_) => Ok(StreamTarget::OutputInput {
            node: target_node,
            endpoint,
        }),
        RootSurfaceNodeKind::Container { .. } => Err(Diagnostic::new(
            DiagnosticCategory::RootRelation,
            DiagnosticKind::InvalidFlowTarget,
            "Containers do not accept FlowsTo inputs by default.",
            Some(DiagnosticLocation::RootNode(target_node)),
        )),
    }
}

fn dedupe_relations(relations: Vec<RootRelation>) -> Vec<RootRelation> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for relation in relations {
        let key = format!("{relation:?}");
        if seen.insert(key) {
            out.push(relation);
        }
    }
    out
}
