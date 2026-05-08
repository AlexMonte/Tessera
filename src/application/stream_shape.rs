use crate::domain::{
    AtomExpr, AtomExprKind, MusicalValue, NodeId, NormalizedContainer, NormalizedProgram,
    OutputEndpoint, RootSurfaceNodeKind, StreamShape,
};
use std::collections::BTreeSet;

pub fn stream_shape_compatible(source: StreamShape, target: StreamShape) -> bool {
    matches!(target, StreamShape::Any)
        || matches!(source, StreamShape::Any)
        || source == target
        || matches!(
            (source, target),
            (StreamShape::NotePattern, StreamShape::EventPattern)
        )
        || matches!(
            (source, target),
            (StreamShape::ScalarPattern, StreamShape::ControlPattern)
        )
}

pub fn infer_normalized_container_shape(
    program: &NormalizedProgram,
    container: &NormalizedContainer,
) -> StreamShape {
    let expr_shapes = container
        .exprs
        .iter()
        .map(|expr| infer_expr_shape(program, expr, &mut BTreeSet::new()))
        .collect::<Vec<_>>();
    merge_expr_shapes(expr_shapes)
}

pub fn normalized_node_output_shape(
    program: &NormalizedProgram,
    node_id: &NodeId,
    endpoint: &OutputEndpoint,
    visiting: &mut BTreeSet<NodeId>,
) -> StreamShape {
    if !visiting.insert(node_id.clone()) {
        return StreamShape::Any;
    }
    let shape = match program.root_nodes.get(node_id) {
        Some(RootSurfaceNodeKind::Container { container }) => program
            .containers
            .get(container)
            .map(|container| infer_normalized_container_shape(program, container))
            .unwrap_or(StreamShape::Any),
        Some(RootSurfaceNodeKind::Transform(transform)) => match endpoint {
            OutputEndpoint::Socket(port) => transform
                .signature
                .output_socket(port)
                .map(|spec| spec.shape)
                .unwrap_or(StreamShape::Any),
            OutputEndpoint::GroupMember { group, .. } => transform
                .signature
                .output_group(group)
                .map(|spec| spec.shape)
                .unwrap_or(StreamShape::Any),
        },
        Some(RootSurfaceNodeKind::FlowControl(control)) => match endpoint {
            OutputEndpoint::Socket(port) => control
                .signature
                .output_socket(port)
                .map(|spec| spec.shape)
                .unwrap_or(StreamShape::Any),
            OutputEndpoint::GroupMember { group, .. } => control
                .signature
                .output_group(group)
                .map(|spec| spec.shape)
                .unwrap_or(StreamShape::Any),
        },
        Some(RootSurfaceNodeKind::Output(_)) | None => StreamShape::Any,
    };
    visiting.remove(node_id);
    shape
}

fn infer_expr_shape(
    program: &NormalizedProgram,
    expr: &AtomExpr,
    visiting: &mut BTreeSet<NodeId>,
) -> StreamShape {
    match &expr.kind {
        AtomExprKind::Value(value) => infer_value_shape(program, value, visiting),
        AtomExprKind::Choice(options) | AtomExprKind::Parallel(options) => merge_expr_shapes(
            options
                .iter()
                .map(|expr| infer_expr_shape(program, expr, visiting))
                .collect(),
        ),
    }
}

fn infer_value_shape(
    program: &NormalizedProgram,
    value: &MusicalValue,
    visiting: &mut BTreeSet<NodeId>,
) -> StreamShape {
    match value {
        MusicalValue::Note(_) => StreamShape::NotePattern,
        MusicalValue::Rest => StreamShape::EventPattern,
        MusicalValue::Scalar(_) => StreamShape::ScalarPattern,
        MusicalValue::NestedContainer(container_id) => program
            .containers
            .get(container_id)
            .map(|container| infer_normalized_container_shape(program, container))
            .unwrap_or_else(|| {
                let _ = visiting;
                StreamShape::Any
            }),
    }
}

fn merge_expr_shapes(shapes: Vec<StreamShape>) -> StreamShape {
    if shapes.is_empty() {
        return StreamShape::Any;
    }
    if shapes
        .iter()
        .all(|shape| matches!(shape, StreamShape::ScalarPattern))
    {
        return StreamShape::ScalarPattern;
    }
    if shapes
        .iter()
        .all(|shape| matches!(shape, StreamShape::NotePattern))
    {
        return StreamShape::NotePattern;
    }
    StreamShape::EventPattern
}
