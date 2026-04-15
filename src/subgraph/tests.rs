use std::collections::BTreeMap;

use serde_json::Value;

use super::*;
use crate::graph::{Edge, Graph, Node};
use crate::piece::{ParamDef, ParamInlineMode, ParamSchema, ParamValueKind, Piece, PieceDef};
use crate::piece_registry::PieceRegistry;
use crate::types::{
    EdgeId, ExecutionDomain, GridPos, PieceCategory, PieceSemanticKind, PortType, TileSide,
};

fn make_input(slot: u8, port_type: &str, required: bool, is_receiver: bool) -> SubgraphInput {
    SubgraphInput {
        slot,
        pos: GridPos {
            col: i32::from(slot),
            row: 0,
        },
        label: format!("arg{slot}"),
        port_type: PortType::from(port_type),
        required,
        is_receiver,
        default_value: None,
    }
}

#[test]
fn generated_piece_preserves_audio_number_schema_and_output_type() {
    let audio_number = PortType::number().with_domain(ExecutionDomain::Audio);
    let piece = GeneratedSubgraphPiece::new(
        "fx",
        "fx",
        &[SubgraphInput {
            slot: 1,
            pos: GridPos { col: 0, row: 0 },
            label: "signal".into(),
            port_type: audio_number.clone(),
            required: true,
            is_receiver: false,
            default_value: None,
        }],
        Some(audio_number.clone()),
    );

    assert_eq!(piece.def().output_type, Some(audio_number.clone()));
    match &piece.def().params[0].schema {
        ParamSchema::Custom { port_type, .. } => assert_eq!(port_type, &audio_number),
        other => panic!("expected custom schema, got {other:?}"),
    }
}

#[test]
fn subgraph_editor_pieces_include_input_and_output_boundaries() {
    let pieces = subgraph_editor_pieces();
    let ids = pieces
        .iter()
        .map(|piece| piece.def().id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            SUBGRAPH_INPUT_1_ID,
            SUBGRAPH_INPUT_2_ID,
            SUBGRAPH_INPUT_3_ID,
            SUBGRAPH_OUTPUT_ID
        ]
    );
}

struct SimpleTransform {
    def: PieceDef,
}

impl SimpleTransform {
    fn new() -> Self {
        Self {
            def: PieceDef {
                id: "test.transform".into(),
                label: "xform".into(),
                category: PieceCategory::Transform,
                semantic_kind: PieceSemanticKind::Operator,
                namespace: "core".into(),
                params: vec![ParamDef {
                    id: "input".into(),
                    label: "input".into(),
                    side: TileSide::LEFT,
                    schema: ParamSchema::Custom {
                        port_type: PortType::any(),
                        value_kind: ParamValueKind::None,
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
                output_type: Some(PortType::any()),
                output_side: Some(TileSide::RIGHT),
                output_role: Default::default(),
                description: None,
                tags: vec![],
            },
        }
    }
}

impl Piece for SimpleTransform {
    fn def(&self) -> &PieceDef {
        &self.def
    }
}

fn editor_registry() -> PieceRegistry {
    let mut registry = PieceRegistry::new();
    for piece in subgraph_editor_pieces() {
        let id = piece.def().id.clone();
        registry.register_arc(id, std::sync::Arc::from(piece));
    }
    registry.register(SimpleTransform::new());
    registry
}

fn simple_subgraph() -> (Graph, PieceRegistry) {
    let registry = editor_registry();
    let input_pos = GridPos { col: 0, row: 0 };
    let xform_pos = GridPos { col: 1, row: 0 };
    let output_pos = GridPos { col: 2, row: 0 };
    let edge_a_id = EdgeId::new();
    let edge_b_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                input_pos,
                Node {
                    piece_id: SUBGRAPH_INPUT_1_ID.into(),
                    inline_params: BTreeMap::from([("label".into(), Value::String("src".into()))]),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (
                xform_pos,
                Node {
                    piece_id: "test.transform".into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::from([("input".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
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
                edge_a_id.clone(),
                Edge {
                    id: edge_a_id,
                    from: input_pos,
                    to_node: xform_pos,
                    to_param: "input".into(),
                },
            ),
            (
                edge_b_id.clone(),
                Edge {
                    id: edge_b_id,
                    from: xform_pos,
                    to_node: output_pos,
                    to_param: "input".into(),
                },
            ),
        ]),
        name: "sub".into(),
        cols: 3,
        rows: 1,
    };
    (graph, registry)
}

#[test]
fn analyze_subgraph_extracts_signature() {
    let (graph, registry) = simple_subgraph();
    let signature = analyze_subgraph(&graph, &registry).expect("signature");

    assert_eq!(signature.inputs.len(), 1);
    assert_eq!(signature.inputs[0].label, "src");
    assert_eq!(signature.output_pos, GridPos { col: 2, row: 0 });
    assert_eq!(signature.output_type, Some(PortType::any()));
}

#[test]
fn analyze_subgraph_preserves_input_metadata() {
    let registry = editor_registry();
    let input_pos = GridPos { col: 0, row: 0 };
    let xform_pos = GridPos { col: 1, row: 0 };
    let output_pos = GridPos { col: 2, row: 0 };
    let edge_a_id = EdgeId::new();
    let edge_b_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                input_pos,
                Node {
                    piece_id: SUBGRAPH_INPUT_1_ID.into(),
                    inline_params: BTreeMap::from([
                        ("label".into(), Value::String("signal".into())),
                        ("port_type".into(), Value::String("number".into())),
                        ("domain".into(), Value::String("audio".into())),
                        ("required".into(), Value::Bool(false)),
                        ("is_receiver".into(), Value::Bool(true)),
                        ("default_value".into(), Value::from(0.5)),
                    ]),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (
                xform_pos,
                Node {
                    piece_id: "test.transform".into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::from([("input".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
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
                edge_a_id.clone(),
                Edge {
                    id: edge_a_id,
                    from: input_pos,
                    to_node: xform_pos,
                    to_param: "input".into(),
                },
            ),
            (
                edge_b_id.clone(),
                Edge {
                    id: edge_b_id,
                    from: xform_pos,
                    to_node: output_pos,
                    to_param: "input".into(),
                },
            ),
        ]),
        name: "metadata".into(),
        cols: 3,
        rows: 1,
    };

    let signature = analyze_subgraph(&graph, &registry).expect("signature");
    assert_eq!(signature.inputs.len(), 1);
    assert_eq!(
        signature.inputs[0],
        SubgraphInput {
            slot: 1,
            pos: input_pos,
            label: String::from("signal"),
            port_type: PortType::number().with_domain(ExecutionDomain::Audio),
            required: false,
            is_receiver: true,
            default_value: Some(Value::from(0.5)),
        }
    );
}

#[test]
fn subgraph_pieces_build_generated_piece_defs() {
    let signature = SubgraphSignature {
        inputs: vec![make_input(1, "text", true, false)],
        output_pos: GridPos { col: 1, row: 0 },
        output_type: Some(PortType::text()),
    };

    let pieces = subgraph_pieces(&[("echo", "Echo", signature)]);
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].def().id, "tessera.subgraph.echo");
    assert_eq!(pieces[0].def().output_type, Some(PortType::text()));
}

#[test]
fn analyze_subgraph_requires_single_output() {
    let registry = editor_registry();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: SUBGRAPH_INPUT_1_ID.into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    piece_id: SUBGRAPH_OUTPUT_ID.into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::from([("input".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (
                GridPos { col: 2, row: 0 },
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
        edges: BTreeMap::new(),
        name: "invalid".into(),
        cols: 3,
        rows: 1,
    };

    let diagnostics = analyze_subgraph(&graph, &registry).expect_err("multiple outputs");
    assert!(diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.kind,
            crate::diagnostics::DiagnosticKind::InvalidOperation { .. }
        )
    }));
}
