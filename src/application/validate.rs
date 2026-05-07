use std::collections::BTreeSet;

use crate::application::normalize_program;
use crate::domain::{
    Diagnostic, DiagnosticCategory, DiagnosticKind, DiagnosticLocation, InputEndpoint,
    NormalizedProgram, RootRelation, RootSurfaceNodeKind, StreamTarget, TesseraProgram,
};

use super::stream_shape::{normalized_node_output_shape, stream_shape_compatible};
use super::validate_root_graph::validate_root_graph;

pub fn validate_program_shape(program: &TesseraProgram) -> Result<(), Vec<Diagnostic>> {
    let diagnostics = validate_root_graph(program);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let normalized = normalize_program(program)?;
    let normalized_diagnostics = validate_normalized_program(&normalized);
    if normalized_diagnostics.is_empty() {
        Ok(())
    } else {
        Err(normalized_diagnostics)
    }
}

#[allow(dead_code)]
pub fn validate_program(program: &TesseraProgram) -> Result<(), Vec<Diagnostic>> {
    validate_program_shape(program)
}

fn validate_normalized_program(program: &NormalizedProgram) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for relation in &program.relations {
        let RootRelation::FlowsTo { from, to } = relation else {
            continue;
        };

        let source_shape = normalized_node_output_shape(
            program,
            &from.node,
            &from.endpoint,
            &mut BTreeSet::new(),
        );

        let (accepted_shape, location, message) = match to {
            StreamTarget::OutputInput { node, endpoint } => {
                let Some(RootSurfaceNodeKind::Output(output)) = program.root_nodes.get(node) else {
                    continue;
                };
                let shape = match endpoint {
                    InputEndpoint::Socket(port) => output.signature.input_socket(port).map(|spec| spec.shape),
                    InputEndpoint::GroupMember { group, .. } => output.signature.input_group(group).map(|spec| spec.shape),
                }
                .unwrap_or(crate::domain::StreamShape::EventPattern);
                (
                    shape,
                    DiagnosticLocation::RootNode(node.clone()),
                    "Output received an incompatible upstream stream shape after normalization.",
                )
            }
            StreamTarget::TransformInput { node, endpoint } => {
                let Some(RootSurfaceNodeKind::Transform(transform)) = program.root_nodes.get(node) else {
                    continue;
                };
                let shape = match endpoint {
                    InputEndpoint::Socket(port) => transform.signature.input_socket(port).map(|spec| spec.shape),
                    InputEndpoint::GroupMember { group, .. } => transform.signature.input_group(group).map(|spec| spec.shape),
                }
                .unwrap_or(crate::domain::StreamShape::Any);
                (
                    shape,
                    DiagnosticLocation::InputEndpoint { node: node.clone(), endpoint: endpoint.clone() },
                    "Transform input received an incompatible upstream stream shape after normalization.",
                )
            }
            StreamTarget::FlowControlInput { node, endpoint } => {
                let Some(RootSurfaceNodeKind::FlowControl(control)) = program.root_nodes.get(node) else {
                    continue;
                };
                let shape = match endpoint {
                    InputEndpoint::Socket(port) => control.signature.input_socket(port).map(|spec| spec.shape),
                    InputEndpoint::GroupMember { group, .. } => control.signature.input_group(group).map(|spec| spec.shape),
                }
                .unwrap_or(crate::domain::StreamShape::Any);
                (
                    shape,
                    DiagnosticLocation::InputEndpoint { node: node.clone(), endpoint: endpoint.clone() },
                    "Flow-control input received an incompatible upstream stream shape after normalization.",
                )
            }
        };

        if !stream_shape_compatible(source_shape, accepted_shape) {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::StreamShape,
                DiagnosticKind::InvalidStreamShape,
                message,
                Some(location),
            ));
        }
    }

    diagnostics
}
