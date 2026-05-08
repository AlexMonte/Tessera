use std::collections::BTreeMap;

use crate::domain::{
    CycleDuration, CycleSpan, CycleTime, DefaultStreamBehavior, Diagnostic, DiagnosticCategory,
    DiagnosticKind, DiagnosticLocation, EventField, FieldValue, InputEndpoint, InputPort, NodeId,
    NormalizedProgram, OutputEndpoint, OutputPort, PatternEvent, PatternStream, RootRelation,
    StreamSource, StreamTarget, TransformKind, TransformNode,
};

pub type TransformAuxInputs = Vec<(InputPort, PatternStream)>;
pub type NodeOutputs = BTreeMap<OutputEndpoint, PatternStream>;

pub fn compile_transform_node<F>(
    program: &NormalizedProgram,
    node_id: &NodeId,
    transform: &TransformNode,
    mut resolve_source: F,
) -> Result<NodeOutputs, Vec<Diagnostic>>
where
    F: FnMut(&StreamSource) -> Result<PatternStream, Vec<Diagnostic>>,
{
    let mut main_stream = None;
    let mut aux_streams = Vec::new();
    for socket in &transform.signature.input_sockets {
        let sources = incoming_socket_sources(program, node_id, &socket.port);
        if sources.len() > 1 {
            return Err(vec![Diagnostic::new(
                DiagnosticCategory::TransformTopology,
                DiagnosticKind::OptionalSocketMultiplyBound,
                "Input socket received more than one binding.",
                Some(DiagnosticLocation::InputEndpoint {
                    node: node_id.clone(),
                    endpoint: InputEndpoint::Socket(socket.port.clone()),
                }),
            )]);
        }
        let stream = if let Some(source) = sources.first() {
            Some(resolve_source(source)?)
        } else {
            socket.default.as_ref().map(default_stream)
        };
        match socket.role {
            crate::domain::NodeInputRole::Main => main_stream = stream,
            _ => {
                if let Some(stream) = stream {
                    aux_streams.push((socket.port.clone(), stream));
                }
            }
        }
    }
    let Some(main_stream) = main_stream else {
        return Err(vec![Diagnostic::new(
            DiagnosticCategory::RootRelation,
            DiagnosticKind::TransformMissingMainInput,
            "Transform main input could not be resolved during compilation.",
            Some(DiagnosticLocation::RootNode(node_id.clone())),
        )]);
    };
    let stream = apply_transform(node_id, main_stream, aux_streams, transform)?;
    Ok(BTreeMap::from_iter([(
        OutputEndpoint::Socket(OutputPort::new("out")),
        stream,
    )]))
}

pub(crate) fn apply_transform(
    node_id: &NodeId,
    main_stream: PatternStream,
    aux_streams: TransformAuxInputs,
    transform: &TransformNode,
) -> Result<PatternStream, Vec<Diagnostic>> {
    let stream = match transform.kind {
        TransformKind::Slow => scale_by_aux(node_id, main_stream, aux_streams, true)?,
        TransformKind::Fast => scale_by_aux(node_id, main_stream, aux_streams, false)?,
        TransformKind::Rev => main_stream.reverse(),
        TransformKind::Gain => annotate_with_aux(main_stream, aux_streams, EventField::Gain),
        TransformKind::Attack => annotate_with_aux(main_stream, aux_streams, EventField::Attack),
        TransformKind::Transpose => {
            annotate_with_aux(main_stream, aux_streams, EventField::Transpose)
        }
        TransformKind::Degrade => annotate_with_aux(main_stream, aux_streams, EventField::Degrade),
    };
    Ok(stream)
}

pub(crate) fn default_stream(default: &DefaultStreamBehavior) -> PatternStream {
    match default {
        DefaultStreamBehavior::ConstantScalar { value } => PatternStream {
            events: vec![PatternEvent {
                span: CycleSpan {
                    start: CycleTime(crate::domain::Rational::zero()),
                    duration: CycleDuration(crate::domain::Rational::one()),
                },
                value: crate::domain::EventValue::Scalar { value: *value },
                fields: Vec::new(),
            }],
        },
    }
}

fn incoming_socket_sources(
    program: &NormalizedProgram,
    node_id: &NodeId,
    port: &InputPort,
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

fn scale_by_aux(
    node_id: &NodeId,
    stream: PatternStream,
    aux_streams: TransformAuxInputs,
    slow: bool,
) -> Result<PatternStream, Vec<Diagnostic>> {
    let factor = aux_streams
        .first()
        .and_then(|(_, stream)| stream.events.first())
        .and_then(|event| match event.value {
            crate::domain::EventValue::Scalar { value } => Some(value),
            _ => None,
        })
        .unwrap_or_else(crate::domain::Rational::one);
    if factor <= crate::domain::Rational::zero() {
        return Err(vec![Diagnostic::new(
            DiagnosticCategory::TransformArgument,
            DiagnosticKind::InvalidTransformArgument,
            "Fast/slow factor must be greater than zero.",
            Some(DiagnosticLocation::RootNode(node_id.clone())),
        )]);
    }
    Ok(if slow {
        stream.slow(factor)
    } else {
        stream.fast(factor)
    })
}

fn annotate_with_aux(
    mut stream: PatternStream,
    aux_streams: TransformAuxInputs,
    constructor: fn(FieldValue) -> EventField,
) -> PatternStream {
    let field = aux_streams
        .first()
        .and_then(|(_, stream)| stream.events.first())
        .and_then(|event| match event.value {
            crate::domain::EventValue::Scalar { value } => {
                Some(constructor(FieldValue::Rational { value }))
            }
            _ => None,
        });
    if let Some(field) = field {
        for event in &mut stream.events {
            event.fields.push(field.clone());
        }
    }
    stream
}
