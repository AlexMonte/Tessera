use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{
    ConnectionRule, Container, ContainerId, ContainerSurfaceTile, Diagnostic, DiagnosticCategory,
    DiagnosticKind, DiagnosticLocation, FlowControlNode, InputEndpoint, NodeId, NodeInputRole,
    NodeSignature, OutputEndpoint, OutputNode, PortCountRule, RootRelation, RootSurfaceNodeKind,
    StreamSource, StreamTarget, TesseraProgram,
};

use super::stream_shape::stream_shape_compatible;

pub fn validate_root_graph(program: &TesseraProgram) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (id, node) in &program.root_nodes {
        if let RootSurfaceNodeKind::Container { container } = node {
            if let Some(container_def) = program.containers.get(container) {
                validate_container_surface(program, container, container_def, &mut diagnostics);
            } else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::Placement,
                    DiagnosticKind::MissingContainer,
                    "Root container node points to a missing container.",
                    Some(DiagnosticLocation::RootNode(id.clone())),
                ));
            }
        }
    }

    validate_relations(program, &mut diagnostics);
    diagnostics
}

#[allow(dead_code)]
pub fn incoming_chain_sources(program: &TesseraProgram, node_id: &NodeId) -> Vec<StreamSource> {
    let mut sources = Vec::new();
    for relation in &program.relations {
        if let RootRelation::ChainedTo { from, to } = relation
            && to == node_id
        {
            sources.push(from.clone());
        }
    }
    sources
}

pub fn incoming_flow_sources_to_socket(
    program: &TesseraProgram,
    node_id: &NodeId,
    port: &crate::domain::InputPort,
) -> Vec<StreamSource> {
    let mut sources = Vec::new();
    for relation in &program.relations {
        if let RootRelation::FlowsTo { from, to } = relation {
            match to {
                StreamTarget::TransformInput { node, endpoint }
                | StreamTarget::FlowControlInput { node, endpoint }
                | StreamTarget::OutputInput { node, endpoint }
                    if node == node_id
                        && matches!(endpoint, InputEndpoint::Socket(target_port) if target_port == port) =>
                {
                    sources.push(from.clone());
                }
                _ => {}
            }
        }
    }
    sources
}

pub fn incoming_flow_sources_to_group(
    program: &TesseraProgram,
    node_id: &NodeId,
    group: &crate::domain::PortGroupId,
) -> Vec<StreamSource> {
    let mut sources = Vec::new();
    for relation in &program.relations {
        if let RootRelation::FlowsTo { from, to } = relation {
            match to {
                StreamTarget::TransformInput { node, endpoint }
                | StreamTarget::FlowControlInput { node, endpoint }
                | StreamTarget::OutputInput { node, endpoint }
                    if node == node_id
                        && matches!(endpoint, InputEndpoint::GroupMember { group: target_group, .. } if target_group == group) =>
                {
                    sources.push(from.clone());
                }
                _ => {}
            }
        }
    }
    sources
}

pub fn node_resolves_to_stream(
    program: &TesseraProgram,
    node_id: &NodeId,
    visiting: &mut BTreeSet<NodeId>,
) -> bool {
    if !visiting.insert(node_id.clone()) {
        return false;
    }
    let result = match program.root_nodes.get(node_id) {
        Some(RootSurfaceNodeKind::Container { .. }) => true,
        Some(RootSurfaceNodeKind::Transform(transform)) => {
            signature_inputs_resolve(program, node_id, &transform.signature, visiting)
        }
        Some(RootSurfaceNodeKind::FlowControl(control)) => {
            signature_inputs_resolve(program, node_id, &control.signature, visiting)
        }
        Some(RootSurfaceNodeKind::Output(_)) | None => false,
    };
    visiting.remove(node_id);
    result
}

fn validate_container_surface(
    program: &TesseraProgram,
    container_id: &ContainerId,
    container: &Container,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (index, tile) in container.stack.iter().enumerate() {
        match tile {
            ContainerSurfaceTile::Transform => diagnostics.push(Diagnostic::new(
                DiagnosticCategory::Placement,
                DiagnosticKind::TransformInsideContainer,
                "Transform tiles cannot appear inside a container stack.",
                Some(DiagnosticLocation::ContainerStack {
                    container: container_id.clone(),
                    index,
                }),
            )),
            ContainerSurfaceTile::Output => diagnostics.push(Diagnostic::new(
                DiagnosticCategory::Placement,
                DiagnosticKind::OutputInsideContainer,
                "Output tiles cannot appear inside a container stack.",
                Some(DiagnosticLocation::ContainerStack {
                    container: container_id.clone(),
                    index,
                }),
            )),
            ContainerSurfaceTile::NestedContainer(nested) => {
                if let Some(nested_container) = program.containers.get(nested) {
                    validate_container_surface(program, nested, nested_container, diagnostics);
                } else {
                    diagnostics.push(Diagnostic::new(
                        DiagnosticCategory::Placement,
                        DiagnosticKind::MissingContainer,
                        "Nested container reference points to a missing container.",
                        Some(DiagnosticLocation::ContainerStack {
                            container: container_id.clone(),
                            index,
                        }),
                    ));
                }
            }
            ContainerSurfaceTile::Atom(_) => {}
        }
    }
}

fn validate_relations(program: &TesseraProgram, diagnostics: &mut Vec<Diagnostic>) {
    let adjacency = build_adjacency(&program.relations);

    for (index, relation) in program.relations.iter().enumerate() {
        match relation {
            RootRelation::ChainedTo { from, to } => {
                if !source_endpoint_exists(program, from)
                    || !node_resolves_to_stream(program, &from.node, &mut BTreeSet::new())
                {
                    diagnostics.push(Diagnostic::new(
                        DiagnosticCategory::RootRelation,
                        DiagnosticKind::InvalidChainSource,
                        "ChainedTo source must resolve to a produced stream endpoint.",
                        Some(DiagnosticLocation::OutputEndpoint {
                            node: from.node.clone(),
                            endpoint: from.endpoint.clone(),
                        }),
                    ));
                }
                if !matches!(
                    program.root_nodes.get(to),
                    Some(RootSurfaceNodeKind::Container { .. })
                ) {
                    diagnostics.push(Diagnostic::new(
                        DiagnosticCategory::RootRelation,
                        DiagnosticKind::InvalidChainTarget,
                        "ChainedTo target must be a container.",
                        Some(DiagnosticLocation::RootRelation { index }),
                    ));
                }
            }
            RootRelation::FlowsTo { from, to } => {
                validate_flow_relation(program, from, to, index, diagnostics);
            }
        }
    }

    for (node_id, node) in &program.root_nodes {
        match node {
            RootSurfaceNodeKind::Transform(transform) => {
                validate_node_bindings(
                    program,
                    node_id,
                    &transform.signature,
                    DiagnosticCategory::TransformTopology,
                    DiagnosticKind::TransformMissingMainInput,
                    diagnostics,
                );
            }
            RootSurfaceNodeKind::FlowControl(control) => {
                validate_node_bindings(
                    program,
                    node_id,
                    &control.signature,
                    DiagnosticCategory::FlowControlTopology,
                    DiagnosticKind::FlowControlCannotStartComposition,
                    diagnostics,
                );
                validate_declared_group_members(control, node_id, diagnostics);
            }
            RootSurfaceNodeKind::Output(output) => {
                validate_output_bindings(program, node_id, output, diagnostics);
            }
            RootSurfaceNodeKind::Container { .. } => {}
        }
    }

    for id in program.root_nodes.keys() {
        if has_cycle(id, &adjacency, &mut BTreeSet::new(), &mut BTreeSet::new()) {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::Cycle,
                DiagnosticKind::RootCycle,
                "The root surface contains a directed cycle.",
                Some(DiagnosticLocation::RootNode(id.clone())),
            ));
            break;
        }
    }
}

fn validate_flow_relation(
    program: &TesseraProgram,
    from: &StreamSource,
    to: &StreamTarget,
    index: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !source_endpoint_exists(program, from)
        || !node_resolves_to_stream(program, &from.node, &mut BTreeSet::new())
    {
        diagnostics.push(Diagnostic::new(
            DiagnosticCategory::RootRelation,
            DiagnosticKind::InvalidFlowSource,
            "FlowsTo source must resolve to a produced stream endpoint.",
            Some(DiagnosticLocation::OutputEndpoint {
                node: from.node.clone(),
                endpoint: from.endpoint.clone(),
            }),
        ));
        return;
    }

    let source_shape = source_endpoint_shape(program, from);
    let (target_node, target_endpoint, category) = match to {
        StreamTarget::TransformInput { node, endpoint } => {
            (node, endpoint, DiagnosticCategory::TransformTopology)
        }
        StreamTarget::FlowControlInput { node, endpoint } => {
            (node, endpoint, DiagnosticCategory::FlowControlTopology)
        }
        StreamTarget::OutputInput { node, endpoint } => {
            (node, endpoint, DiagnosticCategory::RootRelation)
        }
    };

    let Some(_signature) = target_signature(program, target_node) else {
        diagnostics.push(Diagnostic::new(
            category,
            DiagnosticKind::InvalidFlowTarget,
            "FlowsTo target node does not expose input endpoints.",
            Some(DiagnosticLocation::RootRelation { index }),
        ));
        return;
    };

    let kind = match input_endpoint_exists_on_node(program, target_node, target_endpoint) {
        Ok(shape) => {
            if !stream_shape_compatible(source_shape, shape) {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::StreamShape,
                    DiagnosticKind::EndpointShapeMismatch,
                    "Source stream shape is not compatible with the target input endpoint.",
                    Some(DiagnosticLocation::InputEndpoint {
                        node: target_node.clone(),
                        endpoint: target_endpoint.clone(),
                    }),
                ));
            }
            None
        }
        Err(kind) => Some(kind),
    };

    if let Some(kind) = kind {
        diagnostics.push(Diagnostic::new(
            category,
            kind,
            "Referenced input endpoint does not exist on the target node.",
            Some(DiagnosticLocation::InputEndpoint {
                node: target_node.clone(),
                endpoint: target_endpoint.clone(),
            }),
        ));
    }
}

fn validate_node_bindings(
    program: &TesseraProgram,
    node_id: &NodeId,
    signature: &NodeSignature,
    category: DiagnosticCategory,
    missing_main_kind: DiagnosticKind,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for socket in &signature.input_sockets {
        let bindings = incoming_flow_sources_to_socket(program, node_id, &socket.port);
        if bindings.len() > 1 {
            diagnostics.push(Diagnostic::new(
                category,
                DiagnosticKind::OptionalSocketMultiplyBound,
                "Input socket received more than one binding.",
                Some(DiagnosticLocation::InputEndpoint {
                    node: node_id.clone(),
                    endpoint: InputEndpoint::Socket(socket.port.clone()),
                }),
            ));
        }
        let resolves = bindings
            .iter()
            .any(|source| node_resolves_to_stream(program, &source.node, &mut BTreeSet::new()));
        if matches!(socket.role, NodeInputRole::Main) && !resolves && socket.default.is_none() {
            diagnostics.push(Diagnostic::new(
                category,
                missing_main_kind.clone(),
                "Node is missing its required main input.",
                Some(DiagnosticLocation::InputEndpoint {
                    node: node_id.clone(),
                    endpoint: InputEndpoint::Socket(socket.port.clone()),
                }),
            ));
        } else if matches!(socket.connection, ConnectionRule::Required)
            && !resolves
            && socket.default.is_none()
        {
            diagnostics.push(Diagnostic::new(
                category,
                DiagnosticKind::RequiredSocketMissing,
                "Required input socket is unbound.",
                Some(DiagnosticLocation::InputEndpoint {
                    node: node_id.clone(),
                    endpoint: InputEndpoint::Socket(socket.port.clone()),
                }),
            ));
        }
    }

    for group in &signature.input_groups {
        let count = incoming_flow_sources_to_group(program, node_id, &group.group).len() as u32;
        if !port_count_satisfied(group.count, count) {
            diagnostics.push(Diagnostic::new(
                category,
                DiagnosticKind::PortCountViolation,
                "Input group member count does not satisfy the count rule.",
                Some(DiagnosticLocation::InputEndpoint {
                    node: node_id.clone(),
                    endpoint: InputEndpoint::GroupMember {
                        group: group.group.clone(),
                        member: crate::domain::PortMemberId::new("*"),
                    },
                }),
            ));
        }
    }
}

fn validate_output_bindings(
    program: &TesseraProgram,
    node_id: &NodeId,
    output: &OutputNode,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for group in &output.signature.input_groups {
        let inputs = incoming_flow_sources_to_group(program, node_id, &group.group);
        if inputs.is_empty() {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::RootRelation,
                DiagnosticKind::OutputMissingInput,
                "Output must receive at least one input stream.",
                Some(DiagnosticLocation::RootNode(node_id.clone())),
            ));
        }
        let count = inputs.len() as u32;
        if !port_count_satisfied(group.count, count) {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::RootRelation,
                DiagnosticKind::PortCountViolation,
                "Output input group member count does not satisfy the count rule.",
                Some(DiagnosticLocation::InputEndpoint {
                    node: node_id.clone(),
                    endpoint: InputEndpoint::GroupMember {
                        group: group.group.clone(),
                        member: crate::domain::PortMemberId::new("*"),
                    },
                }),
            ));
        }
    }
}

fn validate_declared_group_members(
    control: &FlowControlNode,
    node_id: &NodeId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (group, members) in &control.members.inputs {
        if has_duplicates(members) {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::FlowControlTopology,
                DiagnosticKind::PortCountViolation,
                "Input group members must be unique.",
                Some(DiagnosticLocation::InputEndpoint {
                    node: node_id.clone(),
                    endpoint: InputEndpoint::GroupMember {
                        group: group.clone(),
                        member: crate::domain::PortMemberId::new("*"),
                    },
                }),
            ));
        }
    }
    for (group, members) in &control.members.outputs {
        if has_duplicates(members) {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::FlowControlTopology,
                DiagnosticKind::UnknownOutputGroupMember,
                "Output group members must be unique.",
                Some(DiagnosticLocation::OutputEndpoint {
                    node: node_id.clone(),
                    endpoint: OutputEndpoint::GroupMember {
                        group: group.clone(),
                        member: crate::domain::PortMemberId::new("*"),
                    },
                }),
            ));
        }
    }
}

fn target_signature<'a>(program: &'a TesseraProgram, node: &NodeId) -> Option<&'a NodeSignature> {
    match program.root_nodes.get(node) {
        Some(RootSurfaceNodeKind::Transform(transform)) => Some(&transform.signature),
        Some(RootSurfaceNodeKind::FlowControl(control)) => Some(&control.signature),
        Some(RootSurfaceNodeKind::Output(output)) => Some(&output.signature),
        _ => None,
    }
}

fn source_endpoint_exists(program: &TesseraProgram, source: &StreamSource) -> bool {
    match program.root_nodes.get(&source.node) {
        Some(RootSurfaceNodeKind::Container { .. }) => {
            matches!(&source.endpoint, OutputEndpoint::Socket(port) if port.0 == "out")
        }
        Some(RootSurfaceNodeKind::Transform(transform)) => {
            output_endpoint_exists(&transform.signature, &source.endpoint)
        }
        Some(RootSurfaceNodeKind::FlowControl(control)) => {
            output_endpoint_exists(&control.signature, &source.endpoint)
                && match &source.endpoint {
                    OutputEndpoint::Socket(_) => true,
                    OutputEndpoint::GroupMember { group, member } => control
                        .members
                        .outputs
                        .get(group)
                        .is_some_and(|members| members.contains(member)),
                }
        }
        Some(RootSurfaceNodeKind::Output(_)) | None => false,
    }
}

fn source_endpoint_shape(
    program: &TesseraProgram,
    source: &StreamSource,
) -> crate::domain::StreamShape {
    match program.root_nodes.get(&source.node) {
        Some(RootSurfaceNodeKind::Container { .. }) => crate::domain::StreamShape::Any,
        Some(RootSurfaceNodeKind::Transform(transform)) => {
            endpoint_shape_from_outputs(&transform.signature, &source.endpoint)
        }
        Some(RootSurfaceNodeKind::FlowControl(control)) => {
            endpoint_shape_from_outputs(&control.signature, &source.endpoint)
        }
        Some(RootSurfaceNodeKind::Output(_)) | None => crate::domain::StreamShape::Any,
    }
}

fn input_endpoint_exists_on_node(
    program: &TesseraProgram,
    node_id: &NodeId,
    endpoint: &InputEndpoint,
) -> Result<crate::domain::StreamShape, DiagnosticKind> {
    let Some(node) = program.root_nodes.get(node_id) else {
        return Err(DiagnosticKind::InvalidFlowTarget);
    };
    let Some(signature) = target_signature(program, node_id) else {
        return Err(DiagnosticKind::InvalidFlowTarget);
    };
    match endpoint {
        InputEndpoint::Socket(port) => signature
            .input_socket(port)
            .map(|spec| spec.shape)
            .ok_or(DiagnosticKind::UnknownInputSocket),
        InputEndpoint::GroupMember { group, member } => {
            let spec = signature
                .input_group(group)
                .ok_or(DiagnosticKind::UnknownInputGroup)?;
            if let RootSurfaceNodeKind::FlowControl(control) = node {
                let member_exists = control
                    .members
                    .inputs
                    .get(group)
                    .is_some_and(|members| members.contains(member));
                if !member_exists {
                    return Err(DiagnosticKind::UnknownInputGroupMember);
                }
            }
            Ok(spec.shape)
        }
    }
}

fn output_endpoint_exists(signature: &NodeSignature, endpoint: &OutputEndpoint) -> bool {
    match endpoint {
        OutputEndpoint::Socket(port) => signature.output_socket(port).is_some(),
        OutputEndpoint::GroupMember { group, .. } => signature.output_group(group).is_some(),
    }
}

fn endpoint_shape_from_outputs(
    signature: &NodeSignature,
    endpoint: &OutputEndpoint,
) -> crate::domain::StreamShape {
    match endpoint {
        OutputEndpoint::Socket(port) => signature
            .output_socket(port)
            .map(|spec| spec.shape)
            .unwrap_or(crate::domain::StreamShape::Any),
        OutputEndpoint::GroupMember { group, .. } => signature
            .output_group(group)
            .map(|spec| spec.shape)
            .unwrap_or(crate::domain::StreamShape::Any),
    }
}

fn signature_inputs_resolve(
    program: &TesseraProgram,
    node_id: &NodeId,
    signature: &NodeSignature,
    visiting: &mut BTreeSet<NodeId>,
) -> bool {
    signature.input_sockets.iter().all(|socket| {
        let resolves = incoming_flow_sources_to_socket(program, node_id, &socket.port)
            .iter()
            .any(|source| node_resolves_to_stream(program, &source.node, visiting));
        match socket.connection {
            ConnectionRule::Required => resolves || socket.default.is_some(),
            ConnectionRule::Optional => resolves || socket.default.is_some() || true,
        }
    }) && signature.input_groups.iter().all(|group| {
        let bindings = incoming_flow_sources_to_group(program, node_id, &group.group);
        bindings
            .iter()
            .all(|source| node_resolves_to_stream(program, &source.node, visiting))
            && port_count_satisfied(group.count, bindings.len() as u32)
    })
}

fn port_count_satisfied(rule: PortCountRule, count: u32) -> bool {
    match rule {
        PortCountRule::ZeroOrMore => true,
        PortCountRule::OneOrMore => count >= 1,
        PortCountRule::Exactly(expected) => count == expected,
        PortCountRule::Range { min, max } => count >= min && count <= max,
    }
}

fn build_adjacency(relations: &[RootRelation]) -> BTreeMap<NodeId, Vec<NodeId>> {
    let mut adjacency: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for relation in relations {
        match relation {
            RootRelation::ChainedTo { from, to } => {
                adjacency
                    .entry(from.node.clone())
                    .or_default()
                    .push(to.clone());
            }
            RootRelation::FlowsTo { from, to } => {
                let target = match to {
                    StreamTarget::OutputInput { node, .. } => node,
                    StreamTarget::TransformInput { node, .. } => node,
                    StreamTarget::FlowControlInput { node, .. } => node,
                };
                adjacency
                    .entry(from.node.clone())
                    .or_default()
                    .push(target.clone());
            }
        }
    }
    adjacency
}

fn has_cycle(
    node: &NodeId,
    adjacency: &BTreeMap<NodeId, Vec<NodeId>>,
    visiting: &mut BTreeSet<NodeId>,
    visited: &mut BTreeSet<NodeId>,
) -> bool {
    if visited.contains(node) {
        return false;
    }
    if !visiting.insert(node.clone()) {
        return true;
    }
    if let Some(next_nodes) = adjacency.get(node) {
        for next in next_nodes {
            if has_cycle(next, adjacency, visiting, visited) {
                return true;
            }
        }
    }
    visiting.remove(node);
    visited.insert(node.clone());
    false
}

fn has_duplicates<T: Ord + Clone>(values: &[T]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().any(|value| !seen.insert(value.clone()))
}
