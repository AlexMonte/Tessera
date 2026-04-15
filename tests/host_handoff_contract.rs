use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};
use tessera::{
    DELAY_PIECE_ID, DiagnosticKind, Edge, EdgeId, ExecutionDomain, Graph, GridPos, Node, ParamDef,
    ParamInlineMode, ParamSchema, ParamValueKind, Piece, PieceCategory, PieceDef, PieceRegistry,
    PieceSemanticKind, PortType, ResolvedInputSource, SUBGRAPH_INPUT_1_ID, SUBGRAPH_INPUT_2_ID,
    SUBGRAPH_OUTPUT_ID, TileSide, analyze_subgraph, semantic_pass, subgraph_editor_pieces,
};

struct StaticPiece {
    def: PieceDef,
}

impl StaticPiece {
    fn new(def: PieceDef) -> Self {
        Self { def }
    }
}

impl Piece for StaticPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn infer_output_type(
        &self,
        input_types: &BTreeMap<String, PortType>,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Option<PortType> {
        if self.def.id == "test.forward" {
            return input_types
                .get("value")
                .cloned()
                .or_else(|| self.def.output_type.clone());
        }

        self.def.output_type.clone()
    }
}

fn node(piece_id: &str) -> Node {
    Node {
        piece_id: piece_id.into(),
        inline_params: BTreeMap::new(),
        input_sides: BTreeMap::new(),
        output_side: None,
        label: None,
        node_state: None,
    }
}

fn contract_registry() -> PieceRegistry {
    let mut registry = PieceRegistry::new();

    registry.register(StaticPiece::new(PieceDef {
        id: "test.source".into(),
        label: "source".into(),
        category: PieceCategory::Generator,
        semantic_kind: PieceSemanticKind::Literal,
        namespace: "test".into(),
        params: vec![],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.audio_source".into(),
        label: "audio source".into(),
        category: PieceCategory::Generator,
        semantic_kind: PieceSemanticKind::Literal,
        namespace: "test".into(),
        params: vec![],
        output_type: Some(PortType::number().with_domain(ExecutionDomain::Audio)),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.forward".into(),
        label: "forward".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "test".into(),
        params: vec![ParamDef {
            id: "value".into(),
            label: "value".into(),
            side: TileSide::LEFT,
            schema: ParamSchema::Custom {
                port_type: PortType::any().with_unspecified_domain(),
                value_kind: ParamValueKind::Json,
                default: None,
                can_inline: false,
                inline_mode: ParamInlineMode::Literal,
                min: None,
                max: None,
            },
            text_semantics: Default::default(),
            variadic_group: None,
            required: true,
            role: Default::default(),
        }],
        output_type: Some(PortType::any().with_unspecified_domain()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.connector".into(),
        label: "connector".into(),
        category: PieceCategory::Connector,
        semantic_kind: PieceSemanticKind::Connector,
        namespace: "test".into(),
        params: vec![ParamDef {
            id: "value".into(),
            label: "value".into(),
            side: TileSide::LEFT,
            schema: ParamSchema::Custom {
                port_type: PortType::any().with_unspecified_domain(),
                value_kind: ParamValueKind::Json,
                default: None,
                can_inline: false,
                inline_mode: ParamInlineMode::Literal,
                min: None,
                max: None,
            },
            text_semantics: Default::default(),
            variadic_group: None,
            required: false,
            role: Default::default(),
        }],
        output_type: None,
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.output".into(),
        label: "output".into(),
        category: PieceCategory::Output,
        semantic_kind: PieceSemanticKind::Output,
        namespace: "test".into(),
        params: vec![ParamDef {
            id: "value".into(),
            label: "value".into(),
            side: TileSide::LEFT,
            schema: ParamSchema::Custom {
                port_type: PortType::text(),
                value_kind: ParamValueKind::Text,
                default: None,
                can_inline: false,
                inline_mode: ParamInlineMode::Literal,
                min: None,
                max: None,
            },
            text_semantics: Default::default(),
            variadic_group: None,
            required: true,
            role: Default::default(),
        }],
        output_type: None,
        output_side: None,
        output_role: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.number_output".into(),
        label: "number output".into(),
        category: PieceCategory::Output,
        semantic_kind: PieceSemanticKind::Output,
        namespace: "test".into(),
        params: vec![ParamDef {
            id: "value".into(),
            label: "value".into(),
            side: TileSide::LEFT,
            schema: ParamSchema::Custom {
                port_type: PortType::number(),
                value_kind: ParamValueKind::Number,
                default: None,
                can_inline: false,
                inline_mode: ParamInlineMode::Literal,
                min: None,
                max: None,
            },
            text_semantics: Default::default(),
            variadic_group: None,
            required: true,
            role: Default::default(),
        }],
        output_type: None,
        output_side: None,
        output_role: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: DELAY_PIECE_ID.into(),
        label: "delay".into(),
        category: PieceCategory::Control,
        semantic_kind: PieceSemanticKind::Intrinsic,
        namespace: "test".into(),
        params: vec![
            ParamDef {
                id: "default".into(),
                label: "default".into(),
                side: TileSide::LEFT,
                schema: ParamSchema::Custom {
                    port_type: PortType::any().with_unspecified_domain(),
                    value_kind: ParamValueKind::Json,
                    default: None,
                    can_inline: false,
                    inline_mode: ParamInlineMode::Literal,
                    min: None,
                    max: None,
                },
                text_semantics: Default::default(),
                variadic_group: None,
                required: false,
                role: Default::default(),
            },
            ParamDef {
                id: "value".into(),
                label: "value".into(),
                side: TileSide::BOTTOM,
                schema: ParamSchema::Custom {
                    port_type: PortType::any().with_unspecified_domain(),
                    value_kind: ParamValueKind::Json,
                    default: None,
                    can_inline: false,
                    inline_mode: ParamInlineMode::Literal,
                    min: None,
                    max: None,
                },
                text_semantics: Default::default(),
                variadic_group: None,
                required: false,
                role: Default::default(),
            },
        ],
        output_type: Some(PortType::any().with_unspecified_domain()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        description: None,
        tags: vec![],
    }));

    for piece in subgraph_editor_pieces() {
        let id = piece.def().id.clone();
        registry.register_arc(id, Arc::from(piece));
    }

    registry
}

#[test]
fn connector_bridge_fixture_exposes_host_handoff_facts() {
    let registry = contract_registry();
    let source_pos = GridPos { col: 0, row: 0 };
    let connector_pos = GridPos { col: 1, row: 0 };
    let output_pos = GridPos { col: 2, row: 0 };
    let source_edge_id = EdgeId::new();
    let output_edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (source_pos, node("test.audio_source")),
            (
                connector_pos,
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.connector")
                },
            ),
            (
                output_pos,
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.number_output")
                },
            ),
        ]),
        edges: BTreeMap::from([
            (
                source_edge_id.clone(),
                Edge {
                    id: source_edge_id,
                    from: source_pos,
                    to_node: connector_pos,
                    to_param: "value".into(),
                },
            ),
            (
                output_edge_id.clone(),
                Edge {
                    id: output_edge_id.clone(),
                    from: connector_pos,
                    to_node: output_pos,
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "connector bridge fixture".into(),
        cols: 3,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);

    assert!(analyzed.is_valid());
    assert_eq!(
        analyzed.eval_order,
        vec![source_pos, connector_pos, output_pos]
    );
    assert_eq!(analyzed.outputs, vec![output_pos]);
    assert_eq!(
        analyzed
            .output_nodes()
            .map(|(pos, node)| (pos, node.piece_id.clone()))
            .collect::<Vec<_>>(),
        vec![(output_pos, String::from("test.number_output"))]
    );
    assert!(analyzed.delay_edges.is_empty());

    let output = analyzed.node(&output_pos).expect("output analyzed node");
    let input = output.input("value").expect("resolved output input");
    assert!(matches!(
        &input.source,
        ResolvedInputSource::Edge {
            edge_id,
            from,
            exit_side,
            via,
        } if *edge_id == output_edge_id
            && *from == source_pos
            && *exit_side == TileSide::RIGHT
            && via == &vec![connector_pos]
    ));
    assert_eq!(input.effective_type, Some(PortType::number()));
    assert_eq!(
        input.bridge_kind,
        Some(tessera::DomainBridgeKind::AudioToControl)
    );
    assert_eq!(
        analyzed.output_types.get(&connector_pos),
        Some(&PortType::number().with_domain(ExecutionDomain::Audio))
    );

    let bridge = analyzed
        .domain_bridges
        .get(&output_edge_id)
        .expect("domain bridge");
    assert_eq!(bridge.source_pos, source_pos);
    assert_eq!(bridge.target_pos, output_pos);
    assert_eq!(bridge.param, "value");
    assert_eq!(bridge.kind, tessera::DomainBridgeKind::AudioToControl);
}

#[test]
fn state_feedback_fixture_classifies_delay_edges_without_cycle_errors() {
    let registry = contract_registry();
    let default_pos = GridPos { col: 1, row: 0 };
    let delay_pos = GridPos { col: 1, row: 1 };
    let feedback_pos = GridPos { col: 2, row: 1 };
    let default_edge_id = EdgeId::new();
    let feedforward_edge_id = EdgeId::new();
    let feedback_edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                default_pos,
                Node {
                    output_side: Some(TileSide::BOTTOM),
                    ..node("test.source")
                },
            ),
            (
                delay_pos,
                Node {
                    input_sides: BTreeMap::from([
                        ("default".into(), TileSide::TOP),
                        ("value".into(), TileSide::RIGHT),
                    ]),
                    output_side: Some(TileSide::RIGHT),
                    ..node(DELAY_PIECE_ID)
                },
            ),
            (
                feedback_pos,
                Node {
                    output_side: Some(TileSide::LEFT),
                    ..node("test.forward")
                },
            ),
        ]),
        edges: BTreeMap::from([
            (
                default_edge_id.clone(),
                Edge {
                    id: default_edge_id,
                    from: default_pos,
                    to_node: delay_pos,
                    to_param: "default".into(),
                },
            ),
            (
                feedforward_edge_id.clone(),
                Edge {
                    id: feedforward_edge_id.clone(),
                    from: delay_pos,
                    to_node: feedback_pos,
                    to_param: "value".into(),
                },
            ),
            (
                feedback_edge_id.clone(),
                Edge {
                    id: feedback_edge_id.clone(),
                    from: feedback_pos,
                    to_node: delay_pos,
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "state feedback fixture".into(),
        cols: 3,
        rows: 2,
    };

    let analyzed = semantic_pass(&graph, &registry);

    assert_eq!(
        analyzed.eval_order,
        vec![default_pos, delay_pos, feedback_pos]
    );
    assert!(analyzed.outputs.is_empty());
    assert!(analyzed.delay_edges.contains(&feedback_edge_id));
    assert!(!analyzed.delay_edges.contains(&feedforward_edge_id));
    assert!(
        analyzed
            .diagnostics
            .iter()
            .any(|diagnostic| { matches!(diagnostic.kind, DiagnosticKind::NoOutputNode) })
    );
    assert!(
        !analyzed
            .diagnostics
            .iter()
            .any(|diagnostic| { matches!(diagnostic.kind, DiagnosticKind::Cycle { .. }) })
    );

    let delay = analyzed.node(&delay_pos).expect("delay analyzed node");
    assert_eq!(delay.output_type, Some(PortType::text()));

    let default_input = delay.input("default").expect("default input");
    assert!(matches!(
        &default_input.source,
        ResolvedInputSource::Edge {
            from,
            exit_side,
            via,
            ..
        } if *from == default_pos && *exit_side == TileSide::BOTTOM && via.is_empty()
    ));
    assert_eq!(default_input.effective_type, Some(PortType::text()));

    let feedback_input = delay.input("value").expect("feedback input");
    assert!(matches!(
        &feedback_input.source,
        ResolvedInputSource::Edge {
            edge_id,
            from,
            exit_side,
            via,
        } if *edge_id == feedback_edge_id
            && *from == feedback_pos
            && *exit_side == TileSide::LEFT
            && via.is_empty()
    ));
}

#[test]
fn pure_cycle_fixture_reports_cycle_diagnostic() {
    let registry = contract_registry();
    let left_pos = GridPos { col: 0, row: 0 };
    let right_pos = GridPos { col: 1, row: 0 };
    let left_to_right = EdgeId::new();
    let right_to_left = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                left_pos,
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::RIGHT)]),
                    ..node("test.forward")
                },
            ),
            (
                right_pos,
                Node {
                    output_side: Some(TileSide::LEFT),
                    ..node("test.forward")
                },
            ),
        ]),
        edges: BTreeMap::from([
            (
                left_to_right.clone(),
                Edge {
                    id: left_to_right,
                    from: left_pos,
                    to_node: right_pos,
                    to_param: "value".into(),
                },
            ),
            (
                right_to_left.clone(),
                Edge {
                    id: right_to_left,
                    from: right_pos,
                    to_node: left_pos,
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "pure cycle fixture".into(),
        cols: 2,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);

    assert!(analyzed.diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::Cycle { involved }
                if involved.contains(&left_pos) && involved.contains(&right_pos)
        )
    }));
    assert!(analyzed.delay_edges.is_empty());
}

#[test]
fn subgraph_signature_fixture_is_ordered_and_complete() {
    let registry = contract_registry();
    let input_1_pos = GridPos { col: 0, row: 0 };
    let input_2_pos = GridPos { col: 0, row: 1 };
    let forward_pos = GridPos { col: 1, row: 0 };
    let output_pos = GridPos { col: 2, row: 0 };
    let input_edge_id = EdgeId::new();
    let output_edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                input_2_pos,
                Node {
                    piece_id: SUBGRAPH_INPUT_2_ID.into(),
                    inline_params: BTreeMap::from([
                        ("label".into(), json!("accent")),
                        ("port_type".into(), json!("number")),
                        ("required".into(), json!(false)),
                        ("is_receiver".into(), json!(true)),
                        ("default_value".into(), json!(0.5)),
                    ]),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (
                input_1_pos,
                Node {
                    piece_id: SUBGRAPH_INPUT_1_ID.into(),
                    inline_params: BTreeMap::from([
                        ("label".into(), json!("signal")),
                        ("port_type".into(), json!("text")),
                    ]),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (forward_pos, node("test.forward")),
            (
                output_pos,
                Node {
                    piece_id: SUBGRAPH_OUTPUT_ID.into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::from([("input".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
        ]),
        edges: BTreeMap::from([
            (
                input_edge_id.clone(),
                Edge {
                    id: input_edge_id,
                    from: input_1_pos,
                    to_node: forward_pos,
                    to_param: "value".into(),
                },
            ),
            (
                output_edge_id.clone(),
                Edge {
                    id: output_edge_id,
                    from: forward_pos,
                    to_node: output_pos,
                    to_param: "input".into(),
                },
            ),
        ]),
        name: "subgraph signature fixture".into(),
        cols: 3,
        rows: 2,
    };

    let signature = analyze_subgraph(&graph, &registry).expect("subgraph signature");

    assert_eq!(signature.output_pos, output_pos);
    assert_eq!(signature.output_type, Some(PortType::text()));
    assert_eq!(signature.inputs.len(), 2);

    assert_eq!(signature.inputs[0].slot, 1);
    assert_eq!(signature.inputs[0].pos, input_1_pos);
    assert_eq!(signature.inputs[0].label, "signal");
    assert_eq!(signature.inputs[0].port_type, PortType::text());
    assert!(signature.inputs[0].required);
    assert!(!signature.inputs[0].is_receiver);
    assert_eq!(signature.inputs[0].default_value, None);

    assert_eq!(signature.inputs[1].slot, 2);
    assert_eq!(signature.inputs[1].pos, input_2_pos);
    assert_eq!(signature.inputs[1].label, "accent");
    assert_eq!(signature.inputs[1].port_type, PortType::number());
    assert!(!signature.inputs[1].required);
    assert!(signature.inputs[1].is_receiver);
    assert_eq!(signature.inputs[1].default_value, Some(json!(0.5)));
}

#[test]
fn docs_describe_the_host_handoff_contract() {
    let foundation = include_str!("../docs/foundation.md");
    assert!(
        foundation.contains("Host crates must consume those facts directly."),
        "foundation.md should state that hosts lower from analyzed facts directly"
    );
}
