use std::collections::BTreeMap;

use crate::domain::{
    ConnectionRule, Diagnostic, DiagnosticCategory, DiagnosticKind, DiagnosticLocation,
    FlowControlNode, InputEndpoint, NodeId, NormalizedProgram, PatternStream, PortCountRule,
    PortGroupId, RootRelation, StreamSource, StreamTarget,
};

use super::compile_transform::default_stream;
use super::flow_policy::{apply_flow_control_policy, NodeInputs, NodeOutputs};

pub fn compile_flow_control_node<F>(
    program: &NormalizedProgram,
    node_id: &NodeId,
    node: &FlowControlNode,
    mut resolve_source: F,
) -> Result<NodeOutputs, Vec<Diagnostic>>
where
    F: FnMut(&StreamSource) -> Result<PatternStream, Vec<Diagnostic>>,
{
    let mut inputs: NodeInputs = BTreeMap::new();

    for socket in &node.signature.input_sockets {
        let endpoint = InputEndpoint::Socket(socket.port.clone());
        let sources = incoming_socket_sources(program, node_id, &socket.port);
        if sources.len() > 1 {
            return Err(vec![Diagnostic::new(
                DiagnosticCategory::FlowControlTopology,
                DiagnosticKind::OptionalSocketMultiplyBound,
                "Input socket received more than one binding.",
                Some(DiagnosticLocation::InputEndpoint {
                    node: node_id.clone(),
                    endpoint,
                }),
            )]);
        }
        if let Some(source) = sources.first() {
            inputs.insert(endpoint, vec![resolve_source(source)?]);
        } else if let Some(default) = socket.default.as_ref() {
            inputs.insert(endpoint, vec![default_stream(default)]);
        } else if matches!(socket.connection, ConnectionRule::Required) {
            return Err(vec![Diagnostic::new(
                DiagnosticCategory::FlowControlTopology,
                DiagnosticKind::RequiredSocketMissing,
                "Required input socket is missing during flow-control compilation.",
                Some(DiagnosticLocation::InputEndpoint {
                    node: node_id.clone(),
                    endpoint,
                }),
            )]);
        }
    }

    for group in &node.signature.input_groups {
        let bindings = incoming_group_bindings(program, node_id, &group.group);
        if !port_count_satisfied(group.count, bindings.len() as u32) {
            return Err(vec![Diagnostic::new(
                DiagnosticCategory::FlowControlTopology,
                DiagnosticKind::PortCountViolation,
                "Input group binding count does not satisfy the group count rule.",
                Some(DiagnosticLocation::InputEndpoint {
                    node: node_id.clone(),
                    endpoint: InputEndpoint::GroupMember {
                        group: group.group.clone(),
                        member: crate::domain::PortMemberId::new("*"),
                    },
                }),
            )]);
        }
        for (endpoint, source) in bindings {
            inputs.insert(endpoint, vec![resolve_source(&source)?]);
        }
    }

    apply_flow_control_policy(node, inputs)
}

fn incoming_socket_sources(
    program: &NormalizedProgram,
    node_id: &NodeId,
    port: &crate::domain::InputPort,
) -> Vec<StreamSource> {
    let mut sources = Vec::new();
    for relation in &program.relations {
        let RootRelation::FlowsTo { from, to } = relation else {
            continue;
        };
        let matches_socket = match to {
            StreamTarget::TransformInput { node, endpoint }
            | StreamTarget::FlowControlInput { node, endpoint }
            | StreamTarget::OutputInput { node, endpoint } => {
                node == node_id
                    && matches!(endpoint, InputEndpoint::Socket(target_port) if target_port == port)
            }
        };
        if matches_socket {
            sources.push(from.clone());
        }
    }
    sources
}

fn incoming_group_bindings(
    program: &NormalizedProgram,
    node_id: &NodeId,
    group: &PortGroupId,
) -> Vec<(InputEndpoint, StreamSource)> {
    let mut bindings = Vec::new();
    for relation in &program.relations {
        let RootRelation::FlowsTo { from, to } = relation else {
            continue;
        };
        let endpoint = match to {
            StreamTarget::TransformInput { node, endpoint }
            | StreamTarget::FlowControlInput { node, endpoint }
            | StreamTarget::OutputInput { node, endpoint }
                if node == node_id =>
            {
                endpoint
            }
            _ => continue,
        };
        if let InputEndpoint::GroupMember {
            group: target_group,
            member,
        } = endpoint
            && target_group == group
        {
            bindings.push((
                InputEndpoint::GroupMember {
                    group: target_group.clone(),
                    member: member.clone(),
                },
                from.clone(),
            ));
        }
    }
    bindings
}

fn port_count_satisfied(rule: PortCountRule, count: u32) -> bool {
    match rule {
        PortCountRule::ZeroOrMore => true,
        PortCountRule::OneOrMore => count >= 1,
        PortCountRule::Exactly(expected) => count == expected,
        PortCountRule::Range { min, max } => count >= min && count <= max,
    }
}
