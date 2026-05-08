use std::collections::BTreeMap;

use crate::domain::{
    AuthoredTesseraProgram, Diagnostic, DiagnosticCategory, DiagnosticKind, DiagnosticLocation,
    InputEndpoint, OutputEndpoint, RootSurfaceNodeKind,
};

pub fn validate_root_surface_shape(
    authored: &AuthoredTesseraProgram,
) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    validate_placements(authored, &mut diagnostics);
    validate_bindings(authored, &mut diagnostics);
    validate_explicit_relations_nodes(authored, &mut diagnostics);
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn validate_placements(authored: &AuthoredTesseraProgram, diagnostics: &mut Vec<Diagnostic>) {
    for node in authored.root_surface.nodes.keys() {
        if !authored.root_surface.placements.contains_key(node) {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::Placement,
                DiagnosticKind::MissingPlacement,
                "Root node is missing a spatial placement.",
                Some(DiagnosticLocation::RootNode(node.clone())),
            ));
        }
    }
    for node in authored.root_surface.placements.keys() {
        if !authored.root_surface.nodes.contains_key(node) {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::Placement,
                DiagnosticKind::UnknownPlacedNode,
                "Placement references an unknown root node.",
                Some(DiagnosticLocation::RootNode(node.clone())),
            ));
        }
    }
    let mut occupied = BTreeMap::new();
    for (node, placement) in &authored.root_surface.placements {
        if let Some(existing) = occupied.insert(placement.slot, node.clone()) {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::Placement,
                DiagnosticKind::OverlappingPlacement,
                "Two root nodes occupy the same board slot.",
                Some(DiagnosticLocation::RootNode(existing)),
            ));
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::Placement,
                DiagnosticKind::OverlappingPlacement,
                "Two root nodes occupy the same board slot.",
                Some(DiagnosticLocation::RootNode(node.clone())),
            ));
        }
    }
}

fn validate_bindings(authored: &AuthoredTesseraProgram, diagnostics: &mut Vec<Diagnostic>) {
    for (node_id, bindings) in &authored.root_surface.bindings {
        let Some(node) = authored.root_surface.nodes.get(node_id) else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::Placement,
                DiagnosticKind::UnknownBindingNode,
                "Spatial bindings reference an unknown root node.",
                Some(DiagnosticLocation::RootNode(node_id.clone())),
            ));
            continue;
        };
        for endpoint in bindings.inputs.keys() {
            if !input_endpoint_exists(node, endpoint) {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::RootRelation,
                    DiagnosticKind::UnknownInputSocket,
                    "Spatial input binding references an endpoint not present on this node.",
                    Some(DiagnosticLocation::InputEndpoint {
                        node: node_id.clone(),
                        endpoint: endpoint.clone(),
                    }),
                ));
            }
        }
        for endpoint in bindings.outputs.keys() {
            if !output_endpoint_exists(node, endpoint) {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::RootRelation,
                    DiagnosticKind::UnknownOutputSocket,
                    "Spatial output binding references an endpoint not present on this node.",
                    Some(DiagnosticLocation::OutputEndpoint {
                        node: node_id.clone(),
                        endpoint: endpoint.clone(),
                    }),
                ));
            }
        }
    }
}

fn input_endpoint_exists(node: &RootSurfaceNodeKind, endpoint: &InputEndpoint) -> bool {
    let Some(signature) = node_signature(node) else {
        return false;
    };
    match endpoint {
        InputEndpoint::Socket(port) => signature.input_socket(port).is_some(),
        InputEndpoint::GroupMember { group, .. } => signature.input_group(group).is_some(),
    }
}

fn output_endpoint_exists(node: &RootSurfaceNodeKind, endpoint: &OutputEndpoint) -> bool {
    if matches!(node, RootSurfaceNodeKind::Container { .. }) {
        return matches!(endpoint, OutputEndpoint::Socket(port) if port.0 == "out");
    }
    let Some(signature) = node_signature(node) else {
        return false;
    };
    match endpoint {
        OutputEndpoint::Socket(port) => signature.output_socket(port).is_some(),
        OutputEndpoint::GroupMember { group, .. } => signature.output_group(group).is_some(),
    }
}

fn node_signature(node: &RootSurfaceNodeKind) -> Option<&crate::domain::NodeSignature> {
    match node {
        RootSurfaceNodeKind::Container { .. } => None,
        RootSurfaceNodeKind::Transform(transform) => Some(&transform.signature),
        RootSurfaceNodeKind::FlowControl(flow) => Some(&flow.signature),
        RootSurfaceNodeKind::Output(output) => Some(&output.signature),
    }
}

fn validate_explicit_relations_nodes(
    authored: &AuthoredTesseraProgram,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for relation in &authored.root_surface.explicit_relations {
        match relation {
            crate::domain::RootRelation::FlowsTo { from, to } => {
                if !authored.root_surface.nodes.contains_key(&from.node) {
                    diagnostics.push(Diagnostic::new(
                        DiagnosticCategory::RootRelation,
                        DiagnosticKind::InvalidFlowSource,
                        "Explicit relation references an unknown source node.",
                        Some(DiagnosticLocation::RootNode(from.node.clone())),
                    ));
                }
                let target_node = match to {
                    crate::domain::StreamTarget::TransformInput { node, .. }
                    | crate::domain::StreamTarget::FlowControlInput { node, .. }
                    | crate::domain::StreamTarget::OutputInput { node, .. } => node,
                };
                if !authored.root_surface.nodes.contains_key(target_node) {
                    diagnostics.push(Diagnostic::new(
                        DiagnosticCategory::RootRelation,
                        DiagnosticKind::InvalidFlowTarget,
                        "Explicit relation references an unknown target node.",
                        Some(DiagnosticLocation::RootNode(target_node.clone())),
                    ));
                }
            }
            crate::domain::RootRelation::ChainedTo { from, to } => {
                if !authored.root_surface.nodes.contains_key(&from.node) {
                    diagnostics.push(Diagnostic::new(
                        DiagnosticCategory::RootRelation,
                        DiagnosticKind::InvalidFlowSource,
                        "Explicit chain references an unknown source node.",
                        Some(DiagnosticLocation::RootNode(from.node.clone())),
                    ));
                }
                if !authored.root_surface.nodes.contains_key(to) {
                    diagnostics.push(Diagnostic::new(
                        DiagnosticCategory::RootRelation,
                        DiagnosticKind::InvalidChainTarget,
                        "Explicit chain references an unknown target node.",
                        Some(DiagnosticLocation::RootNode(to.clone())),
                    ));
                }
            }
        }
    }
}
