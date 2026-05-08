use std::collections::BTreeMap;

use tessera::prelude::{
    AtomExprKind, AtomModifier, AtomOperatorToken, AtomTile, ConnectionRule, Container,
    ContainerId, ContainerKind, ContainerSurfaceTile, DefaultStreamBehavior, DiagnosticKind,
    DiagnosticLocation, EventField, EventValue, FieldValue, FlowControlKind, FlowControlNode,
    FlowControlPolicy, GroupMembers, InputEndpoint, InputGroupSpec, InputPort, InputSocketSpec,
    MusicalValue, NodeId, NodeInputRole, NodeSignature, NoteAtom, OutputEndpoint, OutputNode,
    PortCountRule, PortGroupId, PortMemberId, Rational, RootRelation, RootSurfaceNodeKind,
    ScalarAtom, StreamShape, StreamSource, StreamTarget, TesseraCompiler, TesseraProgram,
    TransformKind, TransformNode,
};

fn output_input(member: &str) -> InputEndpoint {
    InputEndpoint::GroupMember {
        group: PortGroupId::new("inputs"),
        member: PortMemberId::new(member),
    }
}

fn transform_input(port: &str) -> InputEndpoint {
    InputEndpoint::Socket(InputPort::new(port))
}

#[test]
fn transform_without_main_input_is_rejected() {
    let mut root_nodes = BTreeMap::new();
    root_nodes.insert(
        NodeId::new("slow"),
        RootSurfaceNodeKind::Transform(TransformNode::new(TransformKind::Slow)),
    );
    let diagnostics = TesseraCompiler::new()
        .validate(&TesseraProgram {
            root_nodes,
            containers: BTreeMap::new(),
            relations: vec![],
        })
        .diagnostics;
    assert!(
        diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::TransformMissingMainInput)
    );
}

#[test]
fn invalid_port_count_range_is_rejected() {
    assert!(PortCountRule::range(3, 2).is_err());
}

#[test]
fn node_signature_rejects_duplicate_socket_ids() {
    let signature = NodeSignature::new(
        vec![
            InputSocketSpec {
                port: InputPort::new("main"),
                role: NodeInputRole::Main,
                shape: StreamShape::EventPattern,
                connection: ConnectionRule::Required,
                side: None,
                default: None,
            },
            InputSocketSpec {
                port: InputPort::new("main"),
                role: NodeInputRole::Aux,
                shape: StreamShape::ScalarPattern,
                connection: ConnectionRule::Optional,
                side: None,
                default: None,
            },
        ],
        vec![],
        vec![],
        vec![],
    );

    assert!(signature.is_err());
}

#[test]
fn node_signature_rejects_invalid_default_stream_shape() {
    let signature = NodeSignature::new(
        vec![InputSocketSpec {
            port: InputPort::new("main"),
            role: NodeInputRole::Main,
            shape: StreamShape::EventPattern,
            connection: ConnectionRule::Optional,
            side: None,
            default: Some(DefaultStreamBehavior::ConstantScalar {
                value: Rational::from_integer(2),
            }),
        }],
        vec![],
        vec![],
        vec![],
    );

    assert!(signature.is_err());
}

#[test]
fn group_members_reject_duplicate_members() {
    let mut members = GroupMembers::default();
    members.outputs.insert(
        PortGroupId::new("branches"),
        vec![PortMemberId::new("high"), PortMemberId::new("high")],
    );

    assert!(members.validate().is_err());
}

#[test]
fn node_signature_rejects_socket_group_name_collision() {
    let result = NodeSignature::new(
        vec![InputSocketSpec {
            port: InputPort::new("streams"),
            role: NodeInputRole::Main,
            shape: StreamShape::EventPattern,
            connection: ConnectionRule::Required,
            side: None,
            default: None,
        }],
        vec![InputGroupSpec {
            group: PortGroupId::new("streams"),
            role: NodeInputRole::Main,
            shape: StreamShape::EventPattern,
            count: PortCountRule::OneOrMore,
        }],
        vec![],
        vec![],
    );
    assert!(result.is_err());
}

#[test]
fn note_adjacent_scalar_binds_absolute_octave() {
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("phrase"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![
                ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("e"))),
                ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(3))),
            ],
        },
    );

    let normalized = TesseraCompiler::new()
        .normalize(&TesseraProgram {
            root_nodes: BTreeMap::new(),
            containers,
            relations: vec![],
        })
        .expect("program should normalize");
    let normalized = normalized
        .containers
        .get(&ContainerId::new("phrase"))
        .expect("container should exist");

    match &normalized.exprs[0].kind {
        AtomExprKind::Value(MusicalValue::Note(note)) => {
            assert_eq!(note.value, "e".into());
            assert_eq!(note.octave, Some(3));
        }
        other => panic!("expected octave-bound note, got {other:?}"),
    }
}

#[test]
fn multiple_adjacent_scalars_after_note_are_ambiguous() {
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("phrase"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![
                ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("e"))),
                ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(2))),
                ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(1))),
            ],
        },
    );

    let diagnostics = TesseraCompiler::new()
        .normalize(&TesseraProgram {
            root_nodes: BTreeMap::new(),
            containers,
            relations: vec![],
        })
        .expect_err("ambiguous octave binding should fail");

    assert!(
        diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::AmbiguousOctaveBinding)
    );
}

#[test]
fn normalization_preserves_nested_ids_and_broad_modifiers() {
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("child"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                "x",
            )))],
        },
    );
    containers.insert(
        ContainerId::new("parent"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![
                ContainerSurfaceTile::NestedContainer(ContainerId::new("child")),
                ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Fast)),
                ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(2))),
                ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Replicate)),
                ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(3))),
                ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Choice)),
                ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("y"))),
            ],
        },
    );

    let normalized = TesseraCompiler::new()
        .normalize(&TesseraProgram {
            root_nodes: BTreeMap::new(),
            containers,
            relations: vec![],
        })
        .expect("program should normalize");
    let normalized = normalized
        .containers
        .get(&ContainerId::new("parent"))
        .expect("container should exist");

    match &normalized.exprs[0].kind {
        AtomExprKind::Choice(branches) => {
            assert_eq!(
                branches[0].modifiers,
                vec![
                    AtomModifier::Fast(Rational::from_integer(2)),
                    AtomModifier::Replicate(3)
                ]
            );
        }
        value => panic!("unexpected normalized kind: {value:?}"),
    }
}

#[test]
fn scalar_only_stream_is_rejected_by_default_output() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("control"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Scalar(
                ScalarAtom::integer(2),
            ))],
        },
    );
    root_nodes.insert(
        NodeId::new("control"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("control"),
        },
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let diagnostics = TesseraCompiler::new()
        .validate(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("control")),
                to: StreamTarget::OutputInput {
                    node: NodeId::new("out"),
                    endpoint: output_input("main"),
                },
            }],
        })
        .diagnostics;

    assert!(
        diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::InvalidStreamShape)
    );
}

#[test]
fn normalized_shape_inference_tracks_scalar_nested_choice_and_parallel_containers() {
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("scalar_child"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Scalar(
                ScalarAtom::integer(2),
            ))],
        },
    );
    containers.insert(
        ContainerId::new("nested_scalar"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::NestedContainer(ContainerId::new(
                "scalar_child",
            ))],
        },
    );
    containers.insert(
        ContainerId::new("choice_scalar"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![
                ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(1))),
                ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Choice)),
                ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(2))),
            ],
        },
    );
    containers.insert(
        ContainerId::new("parallel_scalar"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![
                ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(1))),
                ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Parallel)),
                ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(2))),
            ],
        },
    );

    let normalized = TesseraCompiler::new()
        .normalize(&TesseraProgram {
            root_nodes: BTreeMap::new(),
            containers,
            relations: vec![],
        })
        .expect("program should normalize");

    for id in [
        "scalar_child",
        "nested_scalar",
        "choice_scalar",
        "parallel_scalar",
    ] {
        let container = normalized
            .containers
            .get(&ContainerId::new(id))
            .expect("container should exist");
        match id {
            "scalar_child" => assert!(matches!(
                container.exprs[0].kind,
                AtomExprKind::Value(MusicalValue::Scalar(_))
            )),
            "nested_scalar" => assert!(matches!(
                container.exprs[0].kind,
                AtomExprKind::Value(MusicalValue::NestedContainer(_))
            )),
            "choice_scalar" => assert!(matches!(container.exprs[0].kind, AtomExprKind::Choice(_))),
            "parallel_scalar" => {
                assert!(matches!(container.exprs[0].kind, AtomExprKind::Parallel(_)))
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn compile_program_supports_aux_stream_modulation_and_output_collection() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    for (id, tile) in [
        ("main", AtomTile::Note(NoteAtom::new("a"))),
        ("after", AtomTile::Note(NoteAtom::new("b"))),
    ] {
        containers.insert(
            ContainerId::new(id),
            Container {
                kind: ContainerKind::Sequence,
                stack: vec![ContainerSurfaceTile::Atom(tile)],
            },
        );
        root_nodes.insert(
            NodeId::new(id),
            RootSurfaceNodeKind::Container {
                container: ContainerId::new(id),
            },
        );
    }
    containers.insert(
        ContainerId::new("mod"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Scalar(
                ScalarAtom::integer(4),
            ))],
        },
    );
    root_nodes.insert(
        NodeId::new("mod"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("mod"),
        },
    );
    root_nodes.insert(
        NodeId::new("slow"),
        RootSurfaceNodeKind::Transform(TransformNode::new(TransformKind::Slow)),
    );
    root_nodes.insert(
        NodeId::new("gain"),
        RootSurfaceNodeKind::Transform(TransformNode::new(TransformKind::Gain)),
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let program = TesseraProgram {
        root_nodes,
        containers,
        relations: vec![
            RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("main")),
                to: StreamTarget::TransformInput {
                    node: NodeId::new("slow"),
                    endpoint: transform_input("main"),
                },
            },
            RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("mod")),
                to: StreamTarget::TransformInput {
                    node: NodeId::new("slow"),
                    endpoint: transform_input("factor"),
                },
            },
            RootRelation::ChainedTo {
                from: StreamSource::node(NodeId::new("slow")),
                to: NodeId::new("after"),
            },
            RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("after")),
                to: StreamTarget::TransformInput {
                    node: NodeId::new("gain"),
                    endpoint: transform_input("main"),
                },
            },
            RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("mod")),
                to: StreamTarget::TransformInput {
                    node: NodeId::new("gain"),
                    endpoint: transform_input("amount"),
                },
            },
            RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("gain")),
                to: StreamTarget::OutputInput {
                    node: NodeId::new("out"),
                    endpoint: output_input("gain"),
                },
            },
            RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("main")),
                to: StreamTarget::OutputInput {
                    node: NodeId::new("out"),
                    endpoint: output_input("dry"),
                },
            },
        ],
    };

    let ir = TesseraCompiler::new()
        .compile_ir(&program)
        .expect("program should compile");
    let events = ir.outputs[0].events();
    assert_eq!(events.len(), 3);
    assert!(
        events
            .iter()
            .any(|event| event.span.duration.0 == Rational::from_integer(4))
    );
    assert!(
        events.iter().any(|event| event
            .fields
            .contains(&EventField::Gain(FieldValue::Rational {
                value: Rational::from_integer(4)
            })))
    );
}

#[test]
fn diagnostics_carry_locations() {
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("bad"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Transform],
        },
    );
    let mut root_nodes = BTreeMap::new();
    root_nodes.insert(
        NodeId::new("bad"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("bad"),
        },
    );

    let diagnostics = TesseraCompiler::new()
        .validate(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![],
        })
        .diagnostics;
    assert!(diagnostics.iter().any(|diagnostic| matches!(
        diagnostic.location,
        Some(DiagnosticLocation::ContainerStack { .. })
    )));
}

#[test]
fn endpoint_diagnostics_name_unknown_input_socket() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("phrase"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                "a",
            )))],
        },
    );
    root_nodes.insert(
        NodeId::new("phrase"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("phrase"),
        },
    );
    root_nodes.insert(
        NodeId::new("slow"),
        RootSurfaceNodeKind::Transform(TransformNode::new(TransformKind::Slow)),
    );

    let diagnostics = TesseraCompiler::new()
        .validate(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("phrase")),
                to: StreamTarget::TransformInput {
                    node: NodeId::new("slow"),
                    endpoint: InputEndpoint::Socket(InputPort::new("not-real")),
                },
            }],
        })
        .diagnostics;

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::UnknownInputSocket
            && matches!(
                diagnostic.location,
                Some(DiagnosticLocation::InputEndpoint { .. })
            )
    }));
}

#[test]
fn unknown_input_group_member_is_rejected() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("phrase"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                "a",
            )))],
        },
    );
    root_nodes.insert(
        NodeId::new("phrase"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("phrase"),
        },
    );
    root_nodes.insert(
        NodeId::new("layer"),
        RootSurfaceNodeKind::FlowControl(FlowControlNode::new(FlowControlKind::Layer)),
    );

    let diagnostics = TesseraCompiler::new()
        .validate(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("phrase")),
                to: StreamTarget::FlowControlInput {
                    node: NodeId::new("layer"),
                    endpoint: InputEndpoint::GroupMember {
                        group: PortGroupId::new("streams"),
                        member: PortMemberId::new("missing"),
                    },
                },
            }],
        })
        .diagnostics;

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::UnknownInputGroupMember)
    );
}

#[test]
fn fast_zero_factor_is_diagnostic() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("main"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                "a",
            )))],
        },
    );
    containers.insert(
        ContainerId::new("zero"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Scalar(
                ScalarAtom::integer(0),
            ))],
        },
    );
    root_nodes.insert(
        NodeId::new("main"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("main"),
        },
    );
    root_nodes.insert(
        NodeId::new("zero"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("zero"),
        },
    );
    root_nodes.insert(
        NodeId::new("fast"),
        RootSurfaceNodeKind::Transform(TransformNode::new(TransformKind::Fast)),
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let diagnostics = TesseraCompiler::new()
        .compile_ir(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("main")),
                    to: StreamTarget::TransformInput {
                        node: NodeId::new("fast"),
                        endpoint: transform_input("main"),
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("zero")),
                    to: StreamTarget::TransformInput {
                        node: NodeId::new("fast"),
                        endpoint: transform_input("factor"),
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("fast")),
                    to: StreamTarget::OutputInput {
                        node: NodeId::new("out"),
                        endpoint: output_input("main"),
                    },
                },
            ],
        })
        .expect_err("zero factor should produce a diagnostic");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::InvalidTransformArgument)
    );
}

#[test]
fn flow_control_group_bindings_preserve_target_members() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("a"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                "a",
            )))],
        },
    );
    containers.insert(
        ContainerId::new("b"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                "b",
            )))],
        },
    );
    root_nodes.insert(
        NodeId::new("a"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("a"),
        },
    );
    root_nodes.insert(
        NodeId::new("b"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("b"),
        },
    );
    root_nodes.insert(
        NodeId::new("layer"),
        RootSurfaceNodeKind::FlowControl(FlowControlNode::new(FlowControlKind::Layer)),
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let ir = TesseraCompiler::new()
        .compile_ir(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("a")),
                    to: StreamTarget::FlowControlInput {
                        node: NodeId::new("layer"),
                        endpoint: InputEndpoint::GroupMember {
                            group: PortGroupId::new("streams"),
                            member: PortMemberId::new("b"),
                        },
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("b")),
                    to: StreamTarget::FlowControlInput {
                        node: NodeId::new("layer"),
                        endpoint: InputEndpoint::GroupMember {
                            group: PortGroupId::new("streams"),
                            member: PortMemberId::new("a"),
                        },
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("layer")),
                    to: StreamTarget::OutputInput {
                        node: NodeId::new("out"),
                        endpoint: output_input("main"),
                    },
                },
            ],
        })
        .expect("layer graph should compile");

    let values = ir.outputs[0]
        .events()
        .into_iter()
        .filter_map(|event| match &event.value {
            EventValue::Note { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 2);
    assert!(values.contains(&"a".to_string()));
    assert!(values.contains(&"b".to_string()));
}

#[test]
fn public_api_compiles_simple_program() {
    use tessera::prelude::*;

    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("phrase"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                "a",
            )))],
        },
    );
    root_nodes.insert(
        NodeId::new("a"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("phrase"),
        },
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );
    let program = TesseraProgram {
        root_nodes,
        containers,
        relations: vec![RootRelation::FlowsTo {
            from: StreamSource::node(NodeId::new("a")),
            to: StreamTarget::OutputInput {
                node: NodeId::new("out"),
                endpoint: InputEndpoint::GroupMember {
                    group: PortGroupId::new("inputs"),
                    member: PortMemberId::new("main"),
                },
            },
        }],
    };
    let compiler = TesseraCompiler::new();
    let report = compiler.compile(&program).expect("program should compile");
    assert_eq!(report.ir.outputs.len(), 1);
}

#[test]
fn public_api_validation_report_returns_diagnostics() {
    use tessera::prelude::*;

    let mut root_nodes = BTreeMap::new();
    root_nodes.insert(
        NodeId::new("slow"),
        RootSurfaceNodeKind::Transform(TransformNode::new(TransformKind::Slow)),
    );
    let program = TesseraProgram {
        root_nodes,
        containers: BTreeMap::new(),
        relations: vec![],
    };
    let compiler = TesseraCompiler::new();
    let report = compiler.validate(&program);
    assert!(report.is_invalid());
    assert!(!report.diagnostics.is_empty());
}

#[test]
fn public_api_compile_ir_returns_ir_without_exposing_pipeline() {
    use tessera::prelude::*;

    let mut root_nodes = BTreeMap::new();
    root_nodes.insert(
        NodeId::new("slow"),
        RootSurfaceNodeKind::Transform(TransformNode::new(TransformKind::Slow)),
    );
    let program = TesseraProgram {
        root_nodes,
        containers: BTreeMap::new(),
        relations: vec![],
    };
    let compiler = TesseraCompiler::new();
    let result = compiler.compile_ir(&program);
    assert!(result.is_err());
}

#[test]
fn public_api_preview_container_uses_program_context() {
    use tessera::prelude::*;

    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("phrase"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                "a",
            )))],
        },
    );
    let program = TesseraProgram {
        root_nodes: BTreeMap::new(),
        containers,
        relations: vec![],
    };
    let compiler = TesseraCompiler::new();
    let report = compiler
        .preview_container(
            &program,
            ContainerId::new("phrase"),
            CycleSpan {
                start: CycleTime(Rational::zero()),
                duration: CycleDuration(Rational::one()),
            },
        )
        .expect("preview should compile");
    assert_eq!(report.stream.events.len(), 1);

    let diagnostics = compiler
        .preview_container(
            &program,
            ContainerId::new("missing"),
            CycleSpan {
                start: CycleTime(Rational::zero()),
                duration: CycleDuration(Rational::one()),
            },
        )
        .expect_err("missing preview target should produce diagnostics");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::MissingContainer)
    );
}

#[test]
fn chained_to_after_transform_uses_transformed_duration() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    for (id, tile) in [
        ("a", AtomTile::Note(NoteAtom::new("a"))),
        ("b", AtomTile::Note(NoteAtom::new("b"))),
    ] {
        containers.insert(
            ContainerId::new(id),
            Container {
                kind: ContainerKind::Sequence,
                stack: vec![ContainerSurfaceTile::Atom(tile)],
            },
        );
        root_nodes.insert(
            NodeId::new(id),
            RootSurfaceNodeKind::Container {
                container: ContainerId::new(id),
            },
        );
    }
    containers.insert(
        ContainerId::new("factor"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Scalar(
                ScalarAtom::integer(3),
            ))],
        },
    );
    root_nodes.insert(
        NodeId::new("factor"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("factor"),
        },
    );
    root_nodes.insert(
        NodeId::new("slow"),
        RootSurfaceNodeKind::Transform(TransformNode::new(TransformKind::Slow)),
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let ir = TesseraCompiler::new()
        .compile_ir(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("a")),
                    to: StreamTarget::TransformInput {
                        node: NodeId::new("slow"),
                        endpoint: transform_input("main"),
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("factor")),
                    to: StreamTarget::TransformInput {
                        node: NodeId::new("slow"),
                        endpoint: transform_input("factor"),
                    },
                },
                RootRelation::ChainedTo {
                    from: StreamSource::node(NodeId::new("slow")),
                    to: NodeId::new("b"),
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("b")),
                    to: StreamTarget::OutputInput {
                        node: NodeId::new("out"),
                        endpoint: output_input("main"),
                    },
                },
            ],
        })
        .expect("program should compile");

    let events = ir.outputs[0].events();
    assert_eq!(events[0].span.duration.0, Rational::from_integer(3));
    assert_eq!(events[1].span.start.0, Rational::from_integer(3));
}

#[test]
fn split_outputs_are_endpoint_addressed() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("phrase"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![
                ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("a"))),
                ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("b"))),
            ],
        },
    );
    root_nodes.insert(
        NodeId::new("phrase"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("phrase"),
        },
    );
    root_nodes.insert(
        NodeId::new("split"),
        RootSurfaceNodeKind::FlowControl(FlowControlNode::new(FlowControlKind::Split)),
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let program = TesseraProgram {
        root_nodes,
        containers,
        relations: vec![
            RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("phrase")),
                to: StreamTarget::FlowControlInput {
                    node: NodeId::new("split"),
                    endpoint: transform_input("main"),
                },
            },
            RootRelation::FlowsTo {
                from: StreamSource {
                    node: NodeId::new("split"),
                    endpoint: OutputEndpoint::GroupMember {
                        group: PortGroupId::new("branches"),
                        member: PortMemberId::new("even"),
                    },
                },
                to: StreamTarget::OutputInput {
                    node: NodeId::new("out"),
                    endpoint: output_input("even"),
                },
            },
            RootRelation::FlowsTo {
                from: StreamSource {
                    node: NodeId::new("split"),
                    endpoint: OutputEndpoint::GroupMember {
                        group: PortGroupId::new("branches"),
                        member: PortMemberId::new("odd"),
                    },
                },
                to: StreamTarget::OutputInput {
                    node: NodeId::new("out"),
                    endpoint: output_input("odd"),
                },
            },
        ],
    };

    let ir = TesseraCompiler::new()
        .compile_ir(&program)
        .expect("split program should compile");
    assert_eq!(ir.outputs[0].events().len(), 2);
}

#[test]
fn flow_control_layer_compiles_and_normalized_entrypoint_matches() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    for id in ["a", "b"] {
        containers.insert(
            ContainerId::new(id),
            Container {
                kind: ContainerKind::Sequence,
                stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                    id,
                )))],
            },
        );
        root_nodes.insert(
            NodeId::new(id),
            RootSurfaceNodeKind::Container {
                container: ContainerId::new(id),
            },
        );
    }
    root_nodes.insert(
        NodeId::new("layer"),
        RootSurfaceNodeKind::FlowControl(FlowControlNode::new(FlowControlKind::Layer)),
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let program = TesseraProgram {
        root_nodes,
        containers,
        relations: vec![
            RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("a")),
                to: StreamTarget::FlowControlInput {
                    node: NodeId::new("layer"),
                    endpoint: InputEndpoint::GroupMember {
                        group: PortGroupId::new("streams"),
                        member: PortMemberId::new("a"),
                    },
                },
            },
            RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("b")),
                to: StreamTarget::FlowControlInput {
                    node: NodeId::new("layer"),
                    endpoint: InputEndpoint::GroupMember {
                        group: PortGroupId::new("streams"),
                        member: PortMemberId::new("b"),
                    },
                },
            },
            RootRelation::FlowsTo {
                from: StreamSource::node(NodeId::new("layer")),
                to: StreamTarget::OutputInput {
                    node: NodeId::new("out"),
                    endpoint: output_input("main"),
                },
            },
        ],
    };

    let report = TesseraCompiler::new()
        .compile(&program)
        .expect("program should compile");
    assert_eq!(report.normalized.containers.len(), 2);
    assert_eq!(report.ir.outputs[0].events().len(), 2);
}

#[test]
fn merge_append_chains_streams_sequentially() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    for id in ["a", "b"] {
        containers.insert(
            ContainerId::new(id),
            Container {
                kind: ContainerKind::Sequence,
                stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                    id,
                )))],
            },
        );
        root_nodes.insert(
            NodeId::new(id),
            RootSurfaceNodeKind::Container {
                container: ContainerId::new(id),
            },
        );
    }
    root_nodes.insert(
        NodeId::new("merge"),
        RootSurfaceNodeKind::FlowControl(
            FlowControlNode::new(FlowControlKind::Merge)
                .with_policy(FlowControlPolicy::MergeAppend),
        ),
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let ir = TesseraCompiler::new()
        .compile_ir(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("a")),
                    to: StreamTarget::FlowControlInput {
                        node: NodeId::new("merge"),
                        endpoint: InputEndpoint::GroupMember {
                            group: PortGroupId::new("streams"),
                            member: PortMemberId::new("a"),
                        },
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("b")),
                    to: StreamTarget::FlowControlInput {
                        node: NodeId::new("merge"),
                        endpoint: InputEndpoint::GroupMember {
                            group: PortGroupId::new("streams"),
                            member: PortMemberId::new("b"),
                        },
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("merge")),
                    to: StreamTarget::OutputInput {
                        node: NodeId::new("out"),
                        endpoint: output_input("main"),
                    },
                },
            ],
        })
        .expect("merge append should compile");

    let events = ir.outputs[0].events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].span.start.0, Rational::from_integer(1));
}

#[test]
fn mask_scale_adds_gain_from_mask_stream() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("main"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                "a",
            )))],
        },
    );
    containers.insert(
        ContainerId::new("mask"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Scalar(
                ScalarAtom::integer(3),
            ))],
        },
    );
    root_nodes.insert(
        NodeId::new("main"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("main"),
        },
    );
    root_nodes.insert(
        NodeId::new("mask_src"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("mask"),
        },
    );
    root_nodes.insert(
        NodeId::new("mask"),
        RootSurfaceNodeKind::FlowControl(
            FlowControlNode::new(FlowControlKind::Mask).with_policy(FlowControlPolicy::MaskScale),
        ),
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let ir = TesseraCompiler::new()
        .compile_ir(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("main")),
                    to: StreamTarget::FlowControlInput {
                        node: NodeId::new("mask"),
                        endpoint: transform_input("main"),
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("mask_src")),
                    to: StreamTarget::FlowControlInput {
                        node: NodeId::new("mask"),
                        endpoint: transform_input("mask"),
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("mask")),
                    to: StreamTarget::OutputInput {
                        node: NodeId::new("out"),
                        endpoint: output_input("main"),
                    },
                },
            ],
        })
        .expect("mask scale should compile");

    let events = ir.outputs[0].events();
    assert!(
        events[0]
            .fields
            .contains(&EventField::Gain(FieldValue::Rational {
                value: Rational::from_integer(3)
            }))
    );
}

#[test]
fn route_by_label_partitions_named_note_streams() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    containers.insert(
        ContainerId::new("phrase"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![
                ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("a"))),
                ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("b"))),
            ],
        },
    );
    root_nodes.insert(
        NodeId::new("phrase"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("phrase"),
        },
    );
    let mut route =
        FlowControlNode::new(FlowControlKind::Route).with_policy(FlowControlPolicy::RouteByLabel);
    route.members.outputs.insert(
        PortGroupId::new("routes"),
        vec![PortMemberId::new("a"), PortMemberId::new("b")],
    );
    root_nodes.insert(
        NodeId::new("route"),
        RootSurfaceNodeKind::FlowControl(route),
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let ir = TesseraCompiler::new()
        .compile_ir(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("phrase")),
                    to: StreamTarget::FlowControlInput {
                        node: NodeId::new("route"),
                        endpoint: transform_input("main"),
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource {
                        node: NodeId::new("route"),
                        endpoint: OutputEndpoint::GroupMember {
                            group: PortGroupId::new("routes"),
                            member: PortMemberId::new("a"),
                        },
                    },
                    to: StreamTarget::OutputInput {
                        node: NodeId::new("out"),
                        endpoint: output_input("a"),
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource {
                        node: NodeId::new("route"),
                        endpoint: OutputEndpoint::GroupMember {
                            group: PortGroupId::new("routes"),
                            member: PortMemberId::new("b"),
                        },
                    },
                    to: StreamTarget::OutputInput {
                        node: NodeId::new("out"),
                        endpoint: output_input("b"),
                    },
                },
            ],
        })
        .expect("route by label should compile");

    assert_eq!(ir.outputs[0].events().len(), 2);
}

#[test]
fn switch_control_value_selects_candidate_index() {
    let mut root_nodes = BTreeMap::new();
    let mut containers = BTreeMap::new();
    for id in ["a", "b"] {
        containers.insert(
            ContainerId::new(id),
            Container {
                kind: ContainerKind::Sequence,
                stack: vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(
                    id,
                )))],
            },
        );
        root_nodes.insert(
            NodeId::new(id),
            RootSurfaceNodeKind::Container {
                container: ContainerId::new(id),
            },
        );
    }
    containers.insert(
        ContainerId::new("ctrl"),
        Container {
            kind: ContainerKind::Sequence,
            stack: vec![ContainerSurfaceTile::Atom(AtomTile::Scalar(
                ScalarAtom::integer(1),
            ))],
        },
    );
    root_nodes.insert(
        NodeId::new("ctrl"),
        RootSurfaceNodeKind::Container {
            container: ContainerId::new("ctrl"),
        },
    );
    root_nodes.insert(
        NodeId::new("switch"),
        RootSurfaceNodeKind::FlowControl(
            FlowControlNode::new(FlowControlKind::Switch)
                .with_policy(FlowControlPolicy::SwitchControlValue),
        ),
    );
    root_nodes.insert(
        NodeId::new("out"),
        RootSurfaceNodeKind::Output(OutputNode::default()),
    );

    let ir = TesseraCompiler::new()
        .compile_ir(&TesseraProgram {
            root_nodes,
            containers,
            relations: vec![
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("a")),
                    to: StreamTarget::FlowControlInput {
                        node: NodeId::new("switch"),
                        endpoint: InputEndpoint::GroupMember {
                            group: PortGroupId::new("candidates"),
                            member: PortMemberId::new("a"),
                        },
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("b")),
                    to: StreamTarget::FlowControlInput {
                        node: NodeId::new("switch"),
                        endpoint: InputEndpoint::GroupMember {
                            group: PortGroupId::new("candidates"),
                            member: PortMemberId::new("b"),
                        },
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("ctrl")),
                    to: StreamTarget::FlowControlInput {
                        node: NodeId::new("switch"),
                        endpoint: transform_input("control"),
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("switch")),
                    to: StreamTarget::OutputInput {
                        node: NodeId::new("out"),
                        endpoint: output_input("main"),
                    },
                },
            ],
        })
        .expect("switch control value should compile");

    let events = ir.outputs[0].events();
    match &events[0].value {
        EventValue::Note { value, .. } => assert_eq!(value, "b"),
        other => panic!("expected note event, got {other:?}"),
    }
}
