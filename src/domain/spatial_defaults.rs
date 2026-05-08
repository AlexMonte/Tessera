use crate::domain::{
    FlowControlKind, FlowControlNode, InputEndpoint, InputPort, NodeSpatialBindings,
    OutputEndpoint, OutputPort, PortGroupId, PortMemberId, RootSurfaceNodeKind, SpatialSide,
    TransformKind,
};

pub fn default_spatial_bindings(node: &RootSurfaceNodeKind) -> NodeSpatialBindings {
    let mut bindings = match node {
        RootSurfaceNodeKind::Container { .. } => container_bindings(),
        RootSurfaceNodeKind::Transform(transform) => transform_bindings(transform.kind),
        RootSurfaceNodeKind::Output(_) => output_bindings(),
        RootSurfaceNodeKind::FlowControl(flow) => flow_control_bindings(flow.kind),
    };
    if let RootSurfaceNodeKind::FlowControl(flow) = node {
        apply_flow_member_default_bindings(flow, &mut bindings);
    }
    bindings
}

fn container_bindings() -> NodeSpatialBindings {
    let mut bindings = NodeSpatialBindings::default();
    bindings.outputs.insert(
        OutputEndpoint::Socket(OutputPort::new("out")),
        SpatialSide::East,
    );
    bindings
}

fn output_bindings() -> NodeSpatialBindings {
    let mut bindings = NodeSpatialBindings::default();
    bindings.inputs.insert(
        InputEndpoint::GroupMember {
            group: PortGroupId::new("inputs"),
            member: PortMemberId::new("main"),
        },
        SpatialSide::West,
    );
    bindings
}

fn transform_bindings(kind: TransformKind) -> NodeSpatialBindings {
    let mut bindings = NodeSpatialBindings::default();
    bindings.inputs.insert(
        InputEndpoint::Socket(InputPort::new("main")),
        SpatialSide::West,
    );
    bindings.outputs.insert(
        OutputEndpoint::Socket(OutputPort::new("out")),
        SpatialSide::East,
    );
    match kind {
        TransformKind::Slow | TransformKind::Fast => {
            bindings.inputs.insert(
                InputEndpoint::Socket(InputPort::new("factor")),
                SpatialSide::North,
            );
        }
        TransformKind::Gain
        | TransformKind::Attack
        | TransformKind::Transpose
        | TransformKind::Degrade => {
            bindings.inputs.insert(
                InputEndpoint::Socket(InputPort::new("amount")),
                SpatialSide::North,
            );
        }
        TransformKind::Rev => {}
    }
    bindings
}

fn flow_control_bindings(kind: FlowControlKind) -> NodeSpatialBindings {
    let mut bindings = NodeSpatialBindings::default();
    match kind {
        FlowControlKind::Layer | FlowControlKind::Merge => {
            bindings.outputs.insert(
                OutputEndpoint::Socket(OutputPort::new("out")),
                SpatialSide::East,
            );
        }
        FlowControlKind::Mix => {
            bindings.inputs.insert(
                InputEndpoint::Socket(InputPort::new("amount")),
                SpatialSide::North,
            );
            bindings.outputs.insert(
                OutputEndpoint::Socket(OutputPort::new("out")),
                SpatialSide::East,
            );
        }
        FlowControlKind::Split => {
            bindings.inputs.insert(
                InputEndpoint::Socket(InputPort::new("main")),
                SpatialSide::West,
            );
        }
        FlowControlKind::Mask => {
            bindings.inputs.insert(
                InputEndpoint::Socket(InputPort::new("main")),
                SpatialSide::West,
            );
            bindings.inputs.insert(
                InputEndpoint::Socket(InputPort::new("mask")),
                SpatialSide::North,
            );
            bindings.outputs.insert(
                OutputEndpoint::Socket(OutputPort::new("out")),
                SpatialSide::East,
            );
        }
        FlowControlKind::Switch => {
            bindings.inputs.insert(
                InputEndpoint::Socket(InputPort::new("control")),
                SpatialSide::North,
            );
            bindings.outputs.insert(
                OutputEndpoint::Socket(OutputPort::new("out")),
                SpatialSide::East,
            );
        }
        FlowControlKind::Route => {
            bindings.inputs.insert(
                InputEndpoint::Socket(InputPort::new("main")),
                SpatialSide::West,
            );
            bindings.inputs.insert(
                InputEndpoint::Socket(InputPort::new("control")),
                SpatialSide::North,
            );
        }
        FlowControlKind::Choice => {
            bindings.inputs.insert(
                InputEndpoint::Socket(InputPort::new("control")),
                SpatialSide::North,
            );
            bindings.outputs.insert(
                OutputEndpoint::Socket(OutputPort::new("out")),
                SpatialSide::East,
            );
        }
    }
    bindings
}

pub fn apply_flow_member_default_bindings(
    node: &FlowControlNode,
    bindings: &mut NodeSpatialBindings,
) {
    for (group, members) in &node.members.inputs {
        for member in members {
            bindings
                .inputs
                .entry(InputEndpoint::GroupMember {
                    group: group.clone(),
                    member: member.clone(),
                })
                .or_insert(SpatialSide::West);
        }
    }
    for (group, members) in &node.members.outputs {
        for member in members {
            bindings
                .outputs
                .entry(OutputEndpoint::GroupMember {
                    group: group.clone(),
                    member: member.clone(),
                })
                .or_insert(SpatialSide::East);
        }
    }
}
