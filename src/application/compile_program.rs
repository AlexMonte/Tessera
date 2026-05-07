use std::collections::{BTreeMap, BTreeSet};

use crate::application::{
    compile_container, compile_flow_control_node, compile_transform_node, normalize_program,
    validate_program_shape,
};
use crate::domain::{
    CycleDuration, CycleSpan, CycleTime, Diagnostic, InputEndpoint, NodeId, NormalizedProgram,
    OutputEndpoint, OutputPort, PatternIr, PatternOutput, PatternStream, PortGroupId,
    RootRelation, RootSurfaceNodeKind, StreamSource, StreamTarget, TesseraProgram,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct NodeOutputKey {
    node: NodeId,
    endpoint: OutputEndpoint,
}

type NodeOutputs = BTreeMap<OutputEndpoint, PatternStream>;

#[allow(dead_code)]
pub fn compile_program(program: &TesseraProgram) -> Result<PatternIr, Vec<Diagnostic>> {
    validate_program_shape(program)?;
    let normalized = normalize_program(program)?;
    compile_normalized_program(&normalized)
}

pub fn compile_normalized_program(program: &NormalizedProgram) -> Result<PatternIr, Vec<Diagnostic>> {
    let mut cache = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    let mut outputs = Vec::new();

    for (node_id, node) in &program.root_nodes {
        if let RootSurfaceNodeKind::Output(output) = node {
            let endpoint = output
                .signature
                .input_groups
                .first()
                .map(|group| group.group.clone())
                .unwrap_or_else(|| PortGroupId::new("inputs"));
            let streams = incoming_output_group_sources(program, node_id, &endpoint);
            let mut layered = Vec::new();
            for source in streams {
                layered.push(compile_source(program, &source, &mut cache, &mut visiting)?);
            }
            outputs.push(PatternOutput {
                id: node_id.clone(),
                events: PatternStream::layer(layered).events,
            });
        }
    }

    Ok(PatternIr { outputs })
}

#[allow(dead_code)]
pub fn compile_container_preview(
    program: &TesseraProgram,
    container_id: crate::domain::ContainerId,
    span: CycleSpan,
) -> Result<PatternStream, Vec<Diagnostic>> {
    let normalized_program = normalize_program(program)?;
    let normalized = normalized_program.containers.get(&container_id).cloned().ok_or_else(|| {
        vec![Diagnostic::new(
            crate::domain::DiagnosticCategory::Placement,
            crate::domain::DiagnosticKind::MissingContainer,
            "Container preview target is missing from normalized program.",
            Some(crate::domain::DiagnosticLocation::ContainerStack {
                container: container_id.clone(),
                index: 0,
            }),
        )]
    })?;
    compile_container(&normalized_program, &normalized, span, 0)
}

fn compile_source(
    program: &NormalizedProgram,
    source: &StreamSource,
    cache: &mut BTreeMap<NodeOutputKey, PatternStream>,
    visiting: &mut BTreeSet<NodeOutputKey>,
) -> Result<PatternStream, Vec<Diagnostic>> {
    compile_node_output(program, source.node.clone(), source.endpoint.clone(), cache, visiting)
}

fn compile_node_output(
    program: &NormalizedProgram,
    node: NodeId,
    endpoint: OutputEndpoint,
    cache: &mut BTreeMap<NodeOutputKey, PatternStream>,
    visiting: &mut BTreeSet<NodeOutputKey>,
) -> Result<PatternStream, Vec<Diagnostic>> {
    let key = NodeOutputKey {
        node: node.clone(),
        endpoint: endpoint.clone(),
    };
    if let Some(stream) = cache.get(&key) {
        return Ok(stream.clone());
    }
    if !visiting.insert(key.clone()) {
        return Err(vec![Diagnostic::new(
            crate::domain::DiagnosticCategory::Cycle,
            crate::domain::DiagnosticKind::RootCycle,
            "Compilation encountered a root cycle before validation resolved it.",
            Some(crate::domain::DiagnosticLocation::RootNode(node)),
        )]);
    }

    let outputs = compile_node_all_outputs(program, &key.node, cache, visiting)?;
    visiting.remove(&key);
    for (output_endpoint, stream) in outputs {
        cache.insert(
            NodeOutputKey {
                node: key.node.clone(),
                endpoint: output_endpoint,
            },
            stream,
        );
    }
    cache.get(&key).cloned().ok_or_else(|| {
        vec![Diagnostic::new(
            crate::domain::DiagnosticCategory::Compile,
            crate::domain::DiagnosticKind::CompileFailed,
            "Compiled node did not produce the requested output endpoint.",
            Some(crate::domain::DiagnosticLocation::RootNode(key.node.clone())),
        )]
    })
}

fn compile_node_all_outputs(
    program: &NormalizedProgram,
    node_id: &NodeId,
    cache: &mut BTreeMap<NodeOutputKey, PatternStream>,
    visiting: &mut BTreeSet<NodeOutputKey>,
) -> Result<NodeOutputs, Vec<Diagnostic>> {
    match program.root_nodes.get(node_id) {
        Some(RootSurfaceNodeKind::Container { container }) => {
            compile_container_outputs(program, node_id, container, cache, visiting)
        }
        Some(RootSurfaceNodeKind::Transform(transform)) => {
            compile_transform_outputs(program, node_id, transform, cache, visiting)
        }
        Some(RootSurfaceNodeKind::FlowControl(control)) => {
            compile_flow_control_outputs(program, node_id, control, cache, visiting)
        }
        Some(RootSurfaceNodeKind::Output(_)) => Err(vec![Diagnostic::new(
            crate::domain::DiagnosticCategory::RootRelation,
            crate::domain::DiagnosticKind::OutputCannotProduceStream,
            "Outputs consume streams and cannot be compiled as produced streams.",
            Some(crate::domain::DiagnosticLocation::RootNode(node_id.clone())),
        )]),
        None => Err(vec![Diagnostic::new(
            crate::domain::DiagnosticCategory::Placement,
            crate::domain::DiagnosticKind::MissingContainer,
            "Node is missing from the root surface.",
            Some(crate::domain::DiagnosticLocation::RootNode(node_id.clone())),
        )]),
    }
}

fn compile_container_outputs(
    program: &NormalizedProgram,
    node_id: &NodeId,
    container_id: &crate::domain::ContainerId,
    cache: &mut BTreeMap<NodeOutputKey, PatternStream>,
    visiting: &mut BTreeSet<NodeOutputKey>,
) -> Result<NodeOutputs, Vec<Diagnostic>> {
    let normalized = program.containers.get(container_id).cloned().ok_or_else(|| {
        vec![Diagnostic::new(
            crate::domain::DiagnosticCategory::Placement,
            crate::domain::DiagnosticKind::MissingContainer,
            "Container is missing from normalized program.",
            Some(crate::domain::DiagnosticLocation::RootNode(node_id.clone())),
        )]
    })?;
    let mut stream = compile_container(
        program,
        &normalized,
        CycleSpan {
            start: CycleTime(crate::domain::Rational::zero()),
            duration: CycleDuration(crate::domain::Rational::one()),
        },
        0,
    )?;

    let chain_sources = incoming_chain_sources(program, node_id);
    if chain_sources.len() > 1 {
        return Err(vec![Diagnostic::new(
            crate::domain::DiagnosticCategory::RootRelation,
            crate::domain::DiagnosticKind::InvalidChainTarget,
            "A container may only have one incoming ChainedTo source in this pass.",
            Some(crate::domain::DiagnosticLocation::RootNode(node_id.clone())),
        )]);
    }
    if let Some(source) = chain_sources.first() {
        stream = PatternStream::chain(compile_source(program, source, cache, visiting)?, stream);
    }

    Ok(BTreeMap::from_iter([(
        OutputEndpoint::Socket(OutputPort::new("out")),
        stream,
    )]))
}

fn compile_transform_outputs(
    program: &NormalizedProgram,
    node_id: &NodeId,
    transform: &crate::domain::TransformNode,
    cache: &mut BTreeMap<NodeOutputKey, PatternStream>,
    visiting: &mut BTreeSet<NodeOutputKey>,
) -> Result<NodeOutputs, Vec<Diagnostic>> {
    compile_transform_node(program, node_id, transform, |source| {
        compile_source(program, source, cache, visiting)
    })
}

fn compile_flow_control_outputs(
    program: &NormalizedProgram,
    node_id: &NodeId,
    control: &crate::domain::FlowControlNode,
    cache: &mut BTreeMap<NodeOutputKey, PatternStream>,
    visiting: &mut BTreeSet<NodeOutputKey>,
) -> Result<NodeOutputs, Vec<Diagnostic>> {
    compile_flow_control_node(program, node_id, control, |source| {
        compile_source(program, source, cache, visiting)
    })
}

fn incoming_output_group_sources(program: &NormalizedProgram, node_id: &NodeId, group: &PortGroupId) -> Vec<StreamSource> {
    let mut sources = Vec::new();
    for relation in &program.relations {
        if let RootRelation::FlowsTo { from, to } = relation
            && matches!(to, StreamTarget::OutputInput { node, endpoint: InputEndpoint::GroupMember { group: target_group, .. } } if node == node_id && target_group == group)
        {
            sources.push(from.clone());
        }
    }
    sources
}

fn incoming_chain_sources(program: &NormalizedProgram, node_id: &NodeId) -> Vec<StreamSource> {
    let mut sources = Vec::new();
    for relation in &program.relations {
        if let RootRelation::ChainedTo { from, to } = relation && to == node_id {
            sources.push(from.clone());
        }
    }
    sources
}
