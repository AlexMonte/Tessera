use std::collections::BTreeMap;

use serde_json::json;

use super::{analyze_cached, analyze_graph_internal, infer_output_types_internal, semantic_pass};
use crate::analysis::{AnalysisCache, AnalyzedNode, ResolvedInputSource};
use crate::diagnostics::{Diagnostic, DiagnosticKind, DiagnosticSeverity};
use crate::graph::{Edge, Graph, GraphOp, Node};
use crate::ops::apply_ops_to_graph_cached;
use crate::pattern::{
    PatternAtom, PatternContainer, PatternContainerKind, PatternItem, PatternItemKind, PatternPos,
    PatternRoot, PatternSurface,
};
use crate::piece::{
    ParamDef, ParamInlineMode, ParamSchema, ParamTextSemantics, ParamValueKind, Piece, PieceDef,
};
use crate::piece_registry::PieceRegistry;
use crate::types::{
    DELAY_PIECE_ID, EdgeId, ExecutionDomain, GridPos, PieceCategory, PieceSemanticKind, PortRole,
    PortType, Rational, TemporalEdgeKind, TemporalNodeKind, TileSide,
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
        _inline_params: &BTreeMap<String, serde_json::Value>,
    ) -> Option<PortType> {
        if self.def.id == "test.forward" {
            return input_types
                .get("value")
                .cloned()
                .or_else(|| self.def.output_type.clone());
        }
        self.def.output_type.clone()
    }

    fn validate_analysis(&self, position: GridPos, node: &AnalyzedNode) -> Vec<Diagnostic> {
        if self.def.id != "test.analysis_hook" {
            return Vec::new();
        }

        let mut diagnostics = Vec::new();
        for (param_id, expected_code) in [
            ("edge", "edge_source"),
            ("inline", "inline_source"),
            ("defaulted", "default_source"),
            ("missing", "missing_source"),
        ] {
            let Some(input) = node.input(param_id) else {
                continue;
            };
            let matches_expected = matches!(
                (param_id, &input.source),
                ("edge", ResolvedInputSource::Edge { .. })
                    | ("inline", ResolvedInputSource::Inline { .. })
                    | ("defaulted", ResolvedInputSource::Default { .. })
                    | ("missing", ResolvedInputSource::Missing)
            );

            if matches_expected {
                diagnostics.push(Diagnostic::piece_semantic_info(
                    self.def.id.clone(),
                    expected_code,
                    format!("resolved expected source for '{param_id}'"),
                    Some(position),
                ));
            }
        }

        diagnostics
    }
}

fn registry() -> PieceRegistry {
    let mut registry = PieceRegistry::new();

    registry.register(StaticPiece::new(PieceDef {
        id: "test.source".into(),
        label: "source".into(),
        category: PieceCategory::Generator,
        semantic_kind: PieceSemanticKind::Literal,
        namespace: "core".into(),
        params: vec![],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.audio_source".into(),
        label: "audio source".into(),
        category: PieceCategory::Generator,
        semantic_kind: PieceSemanticKind::Literal,
        namespace: "core".into(),
        params: vec![],
        output_type: Some(PortType::number().with_domain(ExecutionDomain::Audio)),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.number_source".into(),
        label: "number source".into(),
        category: PieceCategory::Generator,
        semantic_kind: PieceSemanticKind::Literal,
        namespace: "core".into(),
        params: vec![],
        output_type: Some(PortType::number()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.forward".into(),
        label: "forward".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
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
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.inline_default".into(),
        label: "inline default".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
        params: vec![
            ParamDef {
                id: "text".into(),
                label: "text".into(),
                side: TileSide::LEFT,
                schema: ParamSchema::Text {
                    default: String::new(),
                    can_inline: true,
                },
                text_semantics: Default::default(),
                variadic_group: None,
                required: false,
                role: Default::default(),
            },
            ParamDef {
                id: "time".into(),
                label: "time".into(),
                side: TileSide::BOTTOM,
                schema: ParamSchema::Rational {
                    default: Rational::new(1, 4).unwrap(),
                    can_inline: true,
                },
                text_semantics: Default::default(),
                variadic_group: None,
                required: false,
                role: Default::default(),
            },
        ],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.output".into(),
        label: "output".into(),
        category: PieceCategory::Output,
        semantic_kind: PieceSemanticKind::Output,
        namespace: "core".into(),
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
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.number_output".into(),
        label: "number output".into(),
        category: PieceCategory::Output,
        semantic_kind: PieceSemanticKind::Output,
        namespace: "core".into(),
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
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.bool_output".into(),
        label: "bool output".into(),
        category: PieceCategory::Output,
        semantic_kind: PieceSemanticKind::Output,
        namespace: "core".into(),
        params: vec![ParamDef {
            id: "value".into(),
            label: "value".into(),
            side: TileSide::LEFT,
            schema: ParamSchema::Bool {
                default: false,
                can_inline: false,
            },
            text_semantics: Default::default(),
            variadic_group: None,
            required: true,
            role: Default::default(),
        }],
        output_type: None,
        output_side: None,
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: DELAY_PIECE_ID.into(),
        label: "delay".into(),
        category: PieceCategory::Control,
        semantic_kind: PieceSemanticKind::Intrinsic,
        namespace: "core".into(),
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
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.multi_input".into(),
        label: "multi_input".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
        params: vec![
            ParamDef {
                id: "left_a".into(),
                label: "left_a".into(),
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
                required: false,
                role: Default::default(),
            },
            ParamDef {
                id: "left_b".into(),
                label: "left_b".into(),
                side: TileSide::BOTTOM,
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
                required: false,
                role: Default::default(),
            },
        ],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.connector".into(),
        label: "connector".into(),
        category: PieceCategory::Connector,
        semantic_kind: PieceSemanticKind::Connector,
        namespace: "core".into(),
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
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.analysis_hook".into(),
        label: "analysis_hook".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
        params: vec![
            ParamDef {
                id: "edge".into(),
                label: "edge".into(),
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
            },
            ParamDef {
                id: "inline".into(),
                label: "inline".into(),
                side: TileSide::BOTTOM,
                schema: ParamSchema::Text {
                    default: String::new(),
                    can_inline: true,
                },
                text_semantics: Default::default(),
                variadic_group: None,
                required: false,
                role: Default::default(),
            },
            ParamDef {
                id: "defaulted".into(),
                label: "defaulted".into(),
                side: TileSide::TOP,
                schema: ParamSchema::Text {
                    default: "fallback".into(),
                    can_inline: true,
                },
                text_semantics: Default::default(),
                variadic_group: None,
                required: false,
                role: Default::default(),
            },
            ParamDef {
                id: "missing".into(),
                label: "missing".into(),
                side: TileSide::RIGHT,
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
                required: false,
                role: Default::default(),
            },
        ],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.signal_source".into(),
        label: "signal source".into(),
        category: PieceCategory::Generator,
        semantic_kind: PieceSemanticKind::Literal,
        namespace: "core".into(),
        params: vec![],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: PortRole::Signal,
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry.register(StaticPiece::new(PieceDef {
        id: "test.gate_output".into(),
        label: "gate output".into(),
        category: PieceCategory::Output,
        semantic_kind: PieceSemanticKind::Output,
        namespace: "core".into(),
        params: vec![ParamDef {
            id: "value".into(),
            label: "value".into(),
            side: TileSide::LEFT,
            schema: ParamSchema::Text {
                default: String::new(),
                can_inline: false,
            },
            text_semantics: Default::default(),
            variadic_group: None,
            required: true,
            role: PortRole::Gate,
        }],
        output_type: None,
        output_side: None,
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    registry
}

fn node(piece_id: &str) -> Node {
    Node {
        piece_id: piece_id.into(),
        inline_params: BTreeMap::new(),
        pattern_source: None,
        input_sides: BTreeMap::new(),
        output_side: None,
        label: None,
        node_state: None,
    }
}

fn grouped_custom_param(id: &str, side: TileSide, port_type: PortType) -> ParamDef {
    ParamDef {
        id: id.into(),
        label: id.into(),
        side,
        schema: ParamSchema::Custom {
            port_type,
            value_kind: ParamValueKind::Text,
            default: None,
            can_inline: true,
            inline_mode: ParamInlineMode::Literal,
            min: None,
            max: None,
        },
        text_semantics: Default::default(),
        variadic_group: Some("args".into()),
        required: false,
        role: Default::default(),
    }
}

fn variadic_order_piece() -> PieceDef {
    PieceDef {
        id: "test.variadic_group".into(),
        label: "variadic group".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
        params: vec![
            grouped_custom_param("z_edge", TileSide::LEFT, PortType::text()),
            grouped_custom_param("a_inline", TileSide::TOP, PortType::text()),
            ParamDef {
                schema: ParamSchema::Custom {
                    port_type: PortType::text(),
                    value_kind: ParamValueKind::Text,
                    default: Some(json!("fallback")),
                    can_inline: true,
                    inline_mode: ParamInlineMode::Literal,
                    min: None,
                    max: None,
                },
                ..grouped_custom_param("m_default", TileSide::BOTTOM, PortType::text())
            },
            grouped_custom_param("b_missing", TileSide::RIGHT, PortType::text()),
        ],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }
}

fn has_invalid_registry_reason(analyzed: &crate::analysis::AnalyzedGraph, expected: &str) -> bool {
    analyzed.diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::InvalidOperation { reason } if reason == expected
        )
    })
}

fn assert_cached_matches_fresh(
    graph: &Graph,
    registry: &PieceRegistry,
    cache: &mut AnalysisCache,
) -> crate::analysis::AnalyzedGraph {
    let cached = analyze_cached(graph, registry, cache);
    let fresh = semantic_pass(graph, registry);
    assert_eq!(format!("{cached:#?}"), format!("{fresh:#?}"));
    cached
}

#[test]
fn analyze_graph_internal_matches_public_semantic_pass() {
    let registry = registry();
    let edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.audio_source")),
            (GridPos { col: 1, row: 0 }, node("test.number_output")),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id,
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "internal analysis equivalence".into(),
        cols: 2,
        rows: 1,
    };

    let internal = analyze_graph_internal(&graph, &registry);
    let public = semantic_pass(&graph, &registry);
    assert_eq!(format!("{internal:#?}"), format!("{public:#?}"));
}

#[test]
fn infer_output_types_internal_matches_public_semantic_pass_output_types() {
    let registry = registry();
    let source_to_connector = EdgeId::new();
    let connector_to_output = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.audio_source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.connector")
                },
            ),
            (GridPos { col: 2, row: 0 }, node("test.number_output")),
        ]),
        edges: BTreeMap::from([
            (
                source_to_connector.clone(),
                Edge {
                    id: source_to_connector,
                    from: GridPos { col: 0, row: 0 },
                    to_node: GridPos { col: 1, row: 0 },
                    to_param: "value".into(),
                },
            ),
            (
                connector_to_output.clone(),
                Edge {
                    id: connector_to_output,
                    from: GridPos { col: 1, row: 0 },
                    to_node: GridPos { col: 2, row: 0 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "internal type inference equivalence".into(),
        cols: 3,
        rows: 1,
    };

    let internal = infer_output_types_internal(&graph, &registry);
    let public = semantic_pass(&graph, &registry).output_types;
    assert_eq!(internal, public);
}

#[test]
fn semantic_pass_collects_multiple_explicit_outputs_without_warning() {
    let registry = registry();
    let top_edge_id = EdgeId::new();
    let bottom_edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.output")
                },
            ),
            (GridPos { col: 0, row: 1 }, node("test.source")),
            (
                GridPos { col: 1, row: 1 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.output")
                },
            ),
        ]),
        edges: BTreeMap::from([
            (
                top_edge_id.clone(),
                Edge {
                    id: top_edge_id,
                    from: GridPos { col: 0, row: 0 },
                    to_node: GridPos { col: 1, row: 0 },
                    to_param: "value".into(),
                },
            ),
            (
                bottom_edge_id.clone(),
                Edge {
                    id: bottom_edge_id,
                    from: GridPos { col: 0, row: 1 },
                    to_node: GridPos { col: 1, row: 1 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "outputs".into(),
        cols: 3,
        rows: 2,
    };

    let analyzed = semantic_pass(&graph, &registry);
    assert_eq!(analyzed.outputs.len(), 2);
    assert!(
        !analyzed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
    );
}

#[test]
fn semantic_pass_reports_outputs_in_deterministic_grid_order() {
    let registry = registry();
    let upper_edge_id = EdgeId::new();
    let lower_edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 1 }, node("test.source")),
            (
                GridPos { col: 1, row: 1 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.output")
                },
            ),
            (GridPos { col: 0, row: 0 }, node("test.source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.output")
                },
            ),
        ]),
        edges: BTreeMap::from([
            (
                lower_edge_id.clone(),
                Edge {
                    id: lower_edge_id,
                    from: GridPos { col: 0, row: 1 },
                    to_node: GridPos { col: 1, row: 1 },
                    to_param: "value".into(),
                },
            ),
            (
                upper_edge_id.clone(),
                Edge {
                    id: upper_edge_id,
                    from: GridPos { col: 0, row: 0 },
                    to_node: GridPos { col: 1, row: 0 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "ordered outputs".into(),
        cols: 2,
        rows: 2,
    };

    let analyzed = semantic_pass(&graph, &registry);
    assert_eq!(
        analyzed.outputs,
        vec![GridPos { col: 1, row: 0 }, GridPos { col: 1, row: 1 }]
    );

    let walked = analyzed
        .output_nodes()
        .map(|(pos, node)| (pos, node.piece_id.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        walked,
        vec![
            (GridPos { col: 1, row: 0 }, String::from("test.output")),
            (GridPos { col: 1, row: 1 }, String::from("test.output")),
        ]
    );
}

#[test]
fn semantic_pass_marks_missing_required_inputs_as_missing_sources() {
    let registry = registry();
    let graph = Graph {
        nodes: BTreeMap::from([(
            GridPos { col: 0, row: 0 },
            Node {
                input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                ..node("test.output")
            },
        )]),
        edges: BTreeMap::new(),
        name: "missing".into(),
        cols: 1,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    let output = analyzed
        .node(&GridPos { col: 0, row: 0 })
        .expect("analyzed output node");
    let input = output.input("value").expect("resolved value input");
    assert!(matches!(input.source, ResolvedInputSource::Missing));
    assert!(input.is_missing());
    assert!(analyzed.diagnostics.iter().any(|diagnostic| {
        matches!(diagnostic.kind, DiagnosticKind::MissingRequiredParam { ref param } if param == "value")
    }));
}

#[test]
fn semantic_pass_exposes_inline_and_default_sources() {
    let registry = registry();
    let edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.inline_default".into(),
                    inline_params: BTreeMap::from([("text".into(), json!("bd"))]),
                    pattern_source: None,
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.output")
                },
            ),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id,
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "inline".into(),
        cols: 2,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    let node = &analyzed.nodes[&GridPos { col: 0, row: 0 }];
    assert!(matches!(
        node.scalar_inputs["text"].source,
        ResolvedInputSource::Inline { .. }
    ));
    assert!(matches!(
        node.scalar_inputs["time"].source,
        ResolvedInputSource::Default { .. }
    ));
    assert_eq!(
        node.scalar_inputs["time"].effective_type,
        Some(PortType::rational())
    );
}

#[test]
fn semantic_pass_analyzes_pattern_sources_on_nodes() {
    let registry = registry();
    let edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.source".into(),
                    inline_params: BTreeMap::new(),
                    pattern_source: Some(PatternSurface {
                        roots: vec![
                            PatternRoot {
                                position: PatternPos { col: 0, row: 0 },
                                container: PatternContainer {
                                    kind: PatternContainerKind::Subdivide,
                                    items: vec![
                                        PatternItem {
                                            position: PatternPos { col: 0, row: 0 },
                                            kind: PatternItemKind::Atom(PatternAtom::Note {
                                                value: "b4".into(),
                                            }),
                                        },
                                        PatternItem {
                                            position: PatternPos { col: 1, row: 0 },
                                            kind: PatternItemKind::Atom(PatternAtom::Rest),
                                        },
                                    ],
                                },
                            },
                            PatternRoot {
                                position: PatternPos { col: 1, row: 0 },
                                container: PatternContainer {
                                    kind: PatternContainerKind::Alternate,
                                    items: vec![
                                        PatternItem {
                                            position: PatternPos { col: 0, row: 0 },
                                            kind: PatternItemKind::Atom(PatternAtom::Note {
                                                value: "d5".into(),
                                            }),
                                        },
                                        PatternItem {
                                            position: PatternPos { col: 1, row: 0 },
                                            kind: PatternItemKind::Atom(PatternAtom::Note {
                                                value: "e5".into(),
                                            }),
                                        },
                                    ],
                                },
                            },
                        ],
                    }),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.output")
                },
            ),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id,
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "pattern source".into(),
        cols: 2,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    let pattern = analyzed
        .node(&GridPos { col: 0, row: 0 })
        .and_then(|node| node.pattern_source.as_ref())
        .expect("pattern source analysis");

    assert_eq!(pattern.root_cycles, 2);
    assert_eq!(pattern.roots.len(), 2);
    assert_eq!(pattern.roots[0].spans.len(), 2);
    assert!(pattern.diagnostics.is_empty());
}

#[test]
fn semantic_pass_exposes_connected_edges_and_domain_bridges() {
    let registry = registry();
    let edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.audio_source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.number_output")
                },
            ),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id.clone(),
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "bridge".into(),
        cols: 2,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    let output = &analyzed.nodes[&GridPos { col: 1, row: 0 }];
    let expected_edge_id = edge_id.clone();
    assert!(matches!(
        output.scalar_inputs["value"].source,
        ResolvedInputSource::Edge {
            ref edge_id,
            from,
            exit_side,
            ..
        }
            if *edge_id == expected_edge_id
                && from == GridPos { col: 0, row: 0 }
                && exit_side == TileSide::RIGHT
    ));
    assert_eq!(
        output.scalar_inputs["value"].effective_type,
        Some(PortType::number())
    );
    assert_eq!(
        output.scalar_inputs["value"].bridge_kind,
        Some(crate::types::DomainBridgeKind::AudioToControl)
    );
}

#[test]
fn semantic_pass_rejects_number_to_bool_without_coercion() {
    let registry = registry();
    let edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.number_source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.bool_output")
                },
            ),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id.clone(),
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "bool mismatch".into(),
        cols: 2,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    assert!(analyzed.diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::TypeMismatch { expected, got, param }
                if expected == &PortType::bool()
                    && got == &PortType::number()
                    && param == "value"
        )
    }));
}

#[test]
fn semantic_pass_reports_delay_type_mismatch_for_incompatible_feedback() {
    let registry = registry();
    let default_edge_id = EdgeId::new();
    let feedback_edge_id = EdgeId::new();
    let output_edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.number_source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([
                        ("default".into(), TileSide::LEFT),
                        ("value".into(), TileSide::BOTTOM),
                    ]),
                    ..node(DELAY_PIECE_ID)
                },
            ),
            (GridPos { col: 1, row: 1 }, node("test.source")),
            (
                GridPos { col: 2, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.output")
                },
            ),
        ]),
        edges: BTreeMap::from([
            (
                default_edge_id.clone(),
                Edge {
                    id: default_edge_id.clone(),
                    from: GridPos { col: 0, row: 0 },
                    to_node: GridPos { col: 1, row: 0 },
                    to_param: "default".into(),
                },
            ),
            (
                feedback_edge_id.clone(),
                Edge {
                    id: feedback_edge_id,
                    from: GridPos { col: 1, row: 1 },
                    to_node: GridPos { col: 1, row: 0 },
                    to_param: "value".into(),
                },
            ),
            (
                output_edge_id.clone(),
                Edge {
                    id: output_edge_id,
                    from: GridPos { col: 1, row: 0 },
                    to_node: GridPos { col: 2, row: 0 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "delay mismatch".into(),
        cols: 3,
        rows: 2,
    };

    let analyzed = semantic_pass(&graph, &registry);
    assert!(analyzed.diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::DelayTypeMismatch { default, feedback }
                if default == &PortType::number() && feedback == &PortType::text()
        )
    }));
}

#[test]
fn semantic_pass_traverses_connectors_for_sources_types_and_bridges() {
    let registry = registry();
    let connector_edge_id = EdgeId::new();
    let output_edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.audio_source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.connector")
                },
            ),
            (
                GridPos { col: 2, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.number_output")
                },
            ),
        ]),
        edges: BTreeMap::from([
            (
                connector_edge_id.clone(),
                Edge {
                    id: connector_edge_id,
                    from: GridPos { col: 0, row: 0 },
                    to_node: GridPos { col: 1, row: 0 },
                    to_param: "value".into(),
                },
            ),
            (
                output_edge_id.clone(),
                Edge {
                    id: output_edge_id.clone(),
                    from: GridPos { col: 1, row: 0 },
                    to_node: GridPos { col: 2, row: 0 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "connector traversal".into(),
        cols: 3,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    let output = &analyzed.nodes[&GridPos { col: 2, row: 0 }];
    assert!(matches!(
        output.scalar_inputs["value"].source,
        ResolvedInputSource::Edge {
            ref edge_id,
            from,
            exit_side,
            ref via,
        } if *edge_id == output_edge_id
            && from == GridPos { col: 0, row: 0 }
            && exit_side == TileSide::RIGHT
            && via == &vec![GridPos { col: 1, row: 0 }]
    ));
    assert_eq!(
        output.scalar_inputs["value"].effective_type,
        Some(PortType::number())
    );
    assert_eq!(
        output.scalar_inputs["value"].bridge_kind,
        Some(crate::types::DomainBridgeKind::AudioToControl)
    );
    assert_eq!(
        analyzed.output_types.get(&GridPos { col: 1, row: 0 }),
        Some(&PortType::number().with_domain(ExecutionDomain::Audio))
    );
    assert_eq!(
        analyzed
            .domain_bridges
            .get(&output_edge_id)
            .map(|bridge| bridge.source_pos),
        Some(GridPos { col: 0, row: 0 })
    );
}

#[test]
fn semantic_pass_warns_on_unreachable_nodes_from_explicit_outputs() {
    let registry = registry();
    let edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.output")
                },
            ),
            (GridPos { col: 0, row: 1 }, node("test.source")),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id,
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "reachable".into(),
        cols: 2,
        rows: 2,
    };

    let analyzed = semantic_pass(&graph, &registry);
    assert!(analyzed.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.kind,
            DiagnosticKind::UnreachableNode {
                position: GridPos { col: 0, row: 1 }
            }
        )
    }));
}

#[test]
fn semantic_pass_classifies_delay_edges() {
    let registry = registry();
    let default_edge_id = EdgeId::new();
    let delay_edge_id = EdgeId::new();
    let output_edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    input_sides: BTreeMap::from([
                        ("default".into(), TileSide::LEFT),
                        ("value".into(), TileSide::BOTTOM),
                    ]),
                    ..node(DELAY_PIECE_ID)
                },
            ),
            (GridPos { col: 1, row: 1 }, node("test.source")),
            (
                GridPos { col: 2, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.output")
                },
            ),
        ]),
        edges: BTreeMap::from([
            (
                default_edge_id.clone(),
                Edge {
                    id: default_edge_id.clone(),
                    from: GridPos { col: 0, row: 0 },
                    to_node: GridPos { col: 1, row: 0 },
                    to_param: "default".into(),
                },
            ),
            (
                delay_edge_id.clone(),
                Edge {
                    id: delay_edge_id.clone(),
                    from: GridPos { col: 1, row: 1 },
                    to_node: GridPos { col: 1, row: 0 },
                    to_param: "value".into(),
                },
            ),
            (
                output_edge_id.clone(),
                Edge {
                    id: output_edge_id,
                    from: GridPos { col: 1, row: 0 },
                    to_node: GridPos { col: 2, row: 0 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "delay".into(),
        cols: 3,
        rows: 2,
    };

    let analyzed = semantic_pass(&graph, &registry);
    assert!(analyzed.delay_edges.contains(&delay_edge_id));
    assert_eq!(analyzed.outputs, vec![GridPos { col: 2, row: 0 }]);
    assert_eq!(
        analyzed.temporal_nodes.get(&GridPos { col: 1, row: 0 }),
        Some(&TemporalNodeKind::Delay)
    );
    assert_eq!(
        analyzed.temporal_edges.get(&delay_edge_id),
        Some(&TemporalEdgeKind::DelayFeedback)
    );
    assert_eq!(
        analyzed.temporal_edges.get(&default_edge_id),
        Some(&TemporalEdgeKind::State)
    );
}

#[test]
fn semantic_pass_reports_duplicate_input_sides() {
    let registry = registry();
    let graph = Graph {
        nodes: BTreeMap::from([(
            GridPos { col: 0, row: 0 },
            Node {
                piece_id: "test.multi_input".into(),
                inline_params: BTreeMap::new(),
                pattern_source: None,
                input_sides: BTreeMap::from([
                    ("left_a".into(), TileSide::LEFT),
                    ("left_b".into(), TileSide::LEFT),
                ]),
                output_side: None,
                label: None,
                node_state: None,
            },
        )]),
        edges: BTreeMap::new(),
        name: "duplicate sides".into(),
        cols: 1,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    assert!(analyzed.diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::DuplicateInputSide { side, params }
                if *side == TileSide::LEFT
                    && params == &vec![String::from("left_a"), String::from("left_b")]
        )
    }));
}

#[test]
fn semantic_pass_runs_piece_local_analysis_hooks_on_normalized_inputs() {
    let registry = registry();
    let input_edge_id = EdgeId::new();
    let output_edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    piece_id: "test.analysis_hook".into(),
                    inline_params: BTreeMap::from([("inline".into(), json!("manual"))]),
                    pattern_source: None,
                    input_sides: BTreeMap::from([
                        ("edge".into(), TileSide::LEFT),
                        ("inline".into(), TileSide::BOTTOM),
                        ("defaulted".into(), TileSide::TOP),
                        ("missing".into(), TileSide::RIGHT),
                    ]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (
                GridPos { col: 2, row: 0 },
                Node {
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    ..node("test.output")
                },
            ),
        ]),
        edges: BTreeMap::from([
            (
                input_edge_id.clone(),
                Edge {
                    id: input_edge_id,
                    from: GridPos { col: 0, row: 0 },
                    to_node: GridPos { col: 1, row: 0 },
                    to_param: "edge".into(),
                },
            ),
            (
                output_edge_id.clone(),
                Edge {
                    id: output_edge_id,
                    from: GridPos { col: 1, row: 0 },
                    to_node: GridPos { col: 2, row: 0 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "analysis hook".into(),
        cols: 3,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    let mut found = analyzed
        .diagnostics
        .iter()
        .filter_map(|diagnostic| match &diagnostic.kind {
            DiagnosticKind::PieceSemantic { code, .. } => Some(code.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    found.sort();

    assert_eq!(
        found,
        vec![
            "default_source",
            "edge_source",
            "inline_source",
            "missing_source",
        ]
    );
}

#[test]
fn semantic_pass_uses_piece_default_input_sides_without_overrides() {
    let registry = registry();
    let edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.source")),
            (GridPos { col: 1, row: 0 }, node("test.output")),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id,
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "default sides".into(),
        cols: 2,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    assert!(matches!(
        analyzed.nodes[&GridPos { col: 1, row: 0 }].scalar_inputs["value"].source,
        ResolvedInputSource::Edge { .. }
    ));
    assert!(
        !analyzed
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::NotAdjacent { .. }))
    );
}

#[test]
fn semantic_pass_reports_role_mismatch_for_incompatible_edge() {
    let registry = registry();
    let edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.signal_source")),
            (GridPos { col: 1, row: 0 }, node("test.gate_output")),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id,
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "role mismatch".into(),
        cols: 2,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    assert!(
        analyzed
            .diagnostics
            .iter()
            .any(|diagnostic| { matches!(&diagnostic.kind, DiagnosticKind::RoleMismatch { .. }) })
    );
}

#[test]
fn semantic_pass_reports_duplicate_piece_registration() {
    let mut registry = PieceRegistry::new();
    let def = PieceDef {
        id: "test.duplicate".into(),
        label: "duplicate".into(),
        category: PieceCategory::Generator,
        semantic_kind: PieceSemanticKind::Literal,
        namespace: "core".into(),
        params: vec![],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    };
    registry.register(StaticPiece::new(def.clone()));
    registry.register(StaticPiece::new(def));

    let graph = Graph {
        nodes: BTreeMap::from([(GridPos { col: 0, row: 0 }, node("test.duplicate"))]),
        edges: BTreeMap::new(),
        name: "duplicate registry".into(),
        cols: 1,
        rows: 1,
    };

    let analyzed = semantic_pass(&graph, &registry);
    assert!(analyzed.diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::InvalidOperation { reason }
                if reason == "invalid_registry: duplicate piece id 'test.duplicate'"
        )
    }));
}

#[test]
fn semantic_pass_reports_variadic_group_mixed_port_types() {
    let mut registry = PieceRegistry::new();
    registry.register(StaticPiece::new(PieceDef {
        id: "test.invalid_variadic_types".into(),
        label: "invalid variadic types".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
        params: vec![
            grouped_custom_param("text", TileSide::LEFT, PortType::text()),
            ParamDef {
                schema: ParamSchema::Custom {
                    port_type: PortType::number(),
                    value_kind: ParamValueKind::Number,
                    default: None,
                    can_inline: true,
                    inline_mode: ParamInlineMode::Literal,
                    min: None,
                    max: None,
                },
                ..grouped_custom_param("number", TileSide::BOTTOM, PortType::number())
            },
        ],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    let analyzed = semantic_pass(
        &Graph {
            nodes: BTreeMap::from([(
                GridPos { col: 0, row: 0 },
                node("test.invalid_variadic_types"),
            )]),
            edges: BTreeMap::new(),
            name: "invalid variadic types".into(),
            cols: 1,
            rows: 1,
        },
        &registry,
    );

    assert!(has_invalid_registry_reason(
        &analyzed,
        "invalid_registry: piece 'test.invalid_variadic_types' variadic group 'args' mixes port types across params: text, number"
    ));
}

#[test]
fn semantic_pass_reports_variadic_group_mixed_roles() {
    let mut registry = PieceRegistry::new();
    registry.register(StaticPiece::new(PieceDef {
        id: "test.invalid_variadic_roles".into(),
        label: "invalid variadic roles".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
        params: vec![
            grouped_custom_param("value", TileSide::LEFT, PortType::text()),
            ParamDef {
                role: PortRole::Gate,
                ..grouped_custom_param("gate", TileSide::BOTTOM, PortType::text())
            },
        ],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    let analyzed = semantic_pass(
        &Graph {
            nodes: BTreeMap::from([(
                GridPos { col: 0, row: 0 },
                node("test.invalid_variadic_roles"),
            )]),
            edges: BTreeMap::new(),
            name: "invalid variadic roles".into(),
            cols: 1,
            rows: 1,
        },
        &registry,
    );

    assert!(has_invalid_registry_reason(
        &analyzed,
        "invalid_registry: piece 'test.invalid_variadic_roles' variadic group 'args' mixes roles across params: value, gate"
    ));
}

#[test]
fn semantic_pass_reports_variadic_group_mixed_text_semantics() {
    let mut registry = PieceRegistry::new();
    registry.register(StaticPiece::new(PieceDef {
        id: "test.invalid_variadic_text".into(),
        label: "invalid variadic text".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
        params: vec![
            grouped_custom_param("plain", TileSide::LEFT, PortType::text()),
            ParamDef {
                text_semantics: ParamTextSemantics::mini(),
                ..grouped_custom_param("mini", TileSide::BOTTOM, PortType::text())
            },
        ],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    let analyzed = semantic_pass(
        &Graph {
            nodes: BTreeMap::from([(
                GridPos { col: 0, row: 0 },
                node("test.invalid_variadic_text"),
            )]),
            edges: BTreeMap::new(),
            name: "invalid variadic text".into(),
            cols: 1,
            rows: 1,
        },
        &registry,
    );

    assert!(has_invalid_registry_reason(
        &analyzed,
        "invalid_registry: piece 'test.invalid_variadic_text' variadic group 'args' mixes text semantics across params: plain, mini"
    ));
}

#[test]
fn semantic_pass_reports_variadic_group_mixed_inline_capability() {
    let mut registry = PieceRegistry::new();
    registry.register(StaticPiece::new(PieceDef {
        id: "test.invalid_variadic_inline".into(),
        label: "invalid variadic inline".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
        params: vec![
            grouped_custom_param("inline", TileSide::LEFT, PortType::text()),
            ParamDef {
                schema: ParamSchema::Custom {
                    port_type: PortType::text(),
                    value_kind: ParamValueKind::Text,
                    default: None,
                    can_inline: false,
                    inline_mode: ParamInlineMode::Literal,
                    min: None,
                    max: None,
                },
                ..grouped_custom_param("edge_only", TileSide::BOTTOM, PortType::text())
            },
        ],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    let analyzed = semantic_pass(
        &Graph {
            nodes: BTreeMap::from([(
                GridPos { col: 0, row: 0 },
                node("test.invalid_variadic_inline"),
            )]),
            edges: BTreeMap::new(),
            name: "invalid variadic inline".into(),
            cols: 1,
            rows: 1,
        },
        &registry,
    );

    assert!(has_invalid_registry_reason(
        &analyzed,
        "invalid_registry: piece 'test.invalid_variadic_inline' variadic group 'args' mixes inline capability across params: inline, edge_only"
    ));
}

#[test]
fn semantic_pass_reports_required_variadic_params() {
    let mut registry = PieceRegistry::new();
    registry.register(StaticPiece::new(PieceDef {
        id: "test.invalid_variadic_required".into(),
        label: "invalid variadic required".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
        params: vec![
            ParamDef {
                required: true,
                ..grouped_custom_param("required", TileSide::LEFT, PortType::text())
            },
            grouped_custom_param("optional", TileSide::BOTTOM, PortType::text()),
        ],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    let analyzed = semantic_pass(
        &Graph {
            nodes: BTreeMap::from([(
                GridPos { col: 0, row: 0 },
                node("test.invalid_variadic_required"),
            )]),
            edges: BTreeMap::new(),
            name: "invalid variadic required".into(),
            cols: 1,
            rows: 1,
        },
        &registry,
    );

    assert!(has_invalid_registry_reason(
        &analyzed,
        "invalid_registry: piece 'test.invalid_variadic_required' variadic group 'args' cannot contain required params: required, optional"
    ));
}

#[test]
fn semantic_pass_allows_singleton_variadic_group() {
    let mut registry = PieceRegistry::new();
    registry.register(StaticPiece::new(PieceDef {
        id: "test.singleton_variadic".into(),
        label: "singleton variadic".into(),
        category: PieceCategory::Transform,
        semantic_kind: PieceSemanticKind::Operator,
        namespace: "core".into(),
        params: vec![grouped_custom_param(
            "only",
            TileSide::LEFT,
            PortType::text(),
        )],
        output_type: Some(PortType::text()),
        output_side: Some(TileSide::RIGHT),
        output_role: Default::default(),
        temporal_kind: Default::default(),
        fan_in: Default::default(),
        fan_out: Default::default(),
        description: None,
        tags: vec![],
    }));

    let analyzed = semantic_pass(
        &Graph {
            nodes: BTreeMap::from([(GridPos { col: 0, row: 0 }, node("test.singleton_variadic"))]),
            edges: BTreeMap::new(),
            name: "singleton variadic".into(),
            cols: 1,
            rows: 1,
        },
        &registry,
    );

    assert!(!analyzed.diagnostics.iter().any(|diagnostic| {
        matches!(
            &diagnostic.kind,
            DiagnosticKind::InvalidOperation { reason }
                if reason.contains("test.singleton_variadic") && reason.contains("variadic group")
        )
    }));
}

#[test]
fn semantic_pass_preserves_variadic_group_declaration_order_for_mixed_sources() {
    let mut registry = registry();
    registry.register(StaticPiece::new(variadic_order_piece()));

    let input_edge_id = EdgeId::new();
    let output_edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 1 }, node("test.source")),
            (
                GridPos { col: 1, row: 1 },
                Node {
                    piece_id: "test.variadic_group".into(),
                    inline_params: BTreeMap::from([("a_inline".into(), json!("manual"))]),
                    pattern_source: None,
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (GridPos { col: 2, row: 1 }, node("test.output")),
        ]),
        edges: BTreeMap::from([
            (
                input_edge_id.clone(),
                Edge {
                    id: input_edge_id,
                    from: GridPos { col: 0, row: 1 },
                    to_node: GridPos { col: 1, row: 1 },
                    to_param: "z_edge".into(),
                },
            ),
            (
                output_edge_id.clone(),
                Edge {
                    id: output_edge_id,
                    from: GridPos { col: 1, row: 1 },
                    to_node: GridPos { col: 2, row: 1 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "variadic mixed ordering".into(),
        cols: 3,
        rows: 2,
    };

    let analyzed = semantic_pass(&graph, &registry);
    let group = analyzed.nodes[&GridPos { col: 1, row: 1 }]
        .variadic_group("args")
        .expect("variadic args group");

    assert_eq!(group.len(), 4);
    assert!(matches!(group[0].source, ResolvedInputSource::Edge { .. }));
    assert!(matches!(
        &group[1].source,
        ResolvedInputSource::Inline { value } if value == &json!("manual")
    ));
    assert!(matches!(
        &group[2].source,
        ResolvedInputSource::Default { value } if value == &json!("fallback")
    ));
    assert!(matches!(group[3].source, ResolvedInputSource::Missing));
}

#[test]
fn analyze_cached_returns_cached_result_when_clean() {
    let registry = registry();
    let edge_id = EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.source")),
            (GridPos { col: 1, row: 0 }, node("test.output")),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id,
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "clean cache".into(),
        cols: 2,
        rows: 1,
    };
    let mut cache = AnalysisCache::new();

    let first = assert_cached_matches_fresh(&graph, &registry, &mut cache);
    assert!(cache.dirty.is_empty());

    let second = analyze_cached(&graph, &registry, &mut cache);
    assert_eq!(format!("{first:#?}"), format!("{second:#?}"));
}

#[test]
fn analyze_cached_matches_fresh_after_inline_edit() {
    let registry = registry();
    let edge_id = EdgeId::new();
    let mut graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.inline_default")),
            (GridPos { col: 1, row: 0 }, node("test.output")),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id,
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "inline edit".into(),
        cols: 2,
        rows: 1,
    };
    let mut cache = AnalysisCache::new();
    assert_cached_matches_fresh(&graph, &registry, &mut cache);

    apply_ops_to_graph_cached(
        &mut graph,
        &registry,
        &[GraphOp::ParamSetInline {
            position: GridPos { col: 0, row: 0 },
            param_id: "text".into(),
            value: json!("hello"),
        }],
        &mut cache,
    )
    .expect("set inline param");

    assert_cached_matches_fresh(&graph, &registry, &mut cache);
}

#[test]
fn analyze_cached_matches_fresh_after_edge_add_and_remove() {
    let registry = registry();
    let forward_edge_id = EdgeId::new();
    let mut graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.source")),
            (GridPos { col: 1, row: 0 }, node("test.forward")),
            (GridPos { col: 2, row: 0 }, node("test.output")),
        ]),
        edges: BTreeMap::from([(
            forward_edge_id.clone(),
            Edge {
                id: forward_edge_id,
                from: GridPos { col: 1, row: 0 },
                to_node: GridPos { col: 2, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "edge add remove".into(),
        cols: 3,
        rows: 1,
    };
    let mut cache = AnalysisCache::new();
    assert_cached_matches_fresh(&graph, &registry, &mut cache);

    apply_ops_to_graph_cached(
        &mut graph,
        &registry,
        &[GraphOp::EdgeConnect {
            edge_id: None,
            from: GridPos { col: 0, row: 0 },
            to_node: GridPos { col: 1, row: 0 },
            to_param: "value".into(),
        }],
        &mut cache,
    )
    .expect("connect source to forward");
    assert_cached_matches_fresh(&graph, &registry, &mut cache);

    let added_edge_id = graph
        .edges
        .values()
        .find(|edge| {
            edge.from == GridPos { col: 0, row: 0 } && edge.to_node == GridPos { col: 1, row: 0 }
        })
        .map(|edge| edge.id.clone())
        .expect("added edge id");

    apply_ops_to_graph_cached(
        &mut graph,
        &registry,
        &[GraphOp::EdgeDisconnect {
            edge_id: added_edge_id,
        }],
        &mut cache,
    )
    .expect("disconnect source from forward");

    assert_cached_matches_fresh(&graph, &registry, &mut cache);
}

#[test]
fn analyze_cached_matches_fresh_after_connector_reroute() {
    let registry = registry();
    let source_edge_id = EdgeId::new();
    let output_edge_id = EdgeId::new();
    let mut graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 1 }, node("test.source")),
            (
                GridPos { col: 1, row: 0 },
                Node {
                    output_side: Some(TileSide::BOTTOM),
                    ..node("test.source")
                },
            ),
            (GridPos { col: 1, row: 1 }, node("test.connector")),
            (GridPos { col: 2, row: 1 }, node("test.output")),
        ]),
        edges: BTreeMap::from([
            (
                source_edge_id.clone(),
                Edge {
                    id: source_edge_id.clone(),
                    from: GridPos { col: 0, row: 1 },
                    to_node: GridPos { col: 1, row: 1 },
                    to_param: "value".into(),
                },
            ),
            (
                output_edge_id.clone(),
                Edge {
                    id: output_edge_id,
                    from: GridPos { col: 1, row: 1 },
                    to_node: GridPos { col: 2, row: 1 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "connector reroute".into(),
        cols: 3,
        rows: 2,
    };
    let mut cache = AnalysisCache::new();
    assert_cached_matches_fresh(&graph, &registry, &mut cache);

    apply_ops_to_graph_cached(
        &mut graph,
        &registry,
        &[
            GraphOp::EdgeDisconnect {
                edge_id: source_edge_id,
            },
            GraphOp::ParamSetSide {
                position: GridPos { col: 1, row: 1 },
                param_id: "value".into(),
                side: TileSide::TOP,
            },
            GraphOp::EdgeConnect {
                edge_id: None,
                from: GridPos { col: 1, row: 0 },
                to_node: GridPos { col: 1, row: 1 },
                to_param: "value".into(),
            },
        ],
        &mut cache,
    )
    .expect("reroute connector input");

    assert_cached_matches_fresh(&graph, &registry, &mut cache);
}

#[test]
fn analyze_cached_matches_fresh_after_node_removal_and_mixed_edits() {
    let registry = registry();
    let branch_a_edge = EdgeId::new();
    let branch_b_edge = EdgeId::new();
    let mut graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 0 }, node("test.source")),
            (GridPos { col: 1, row: 0 }, node("test.forward")),
            (GridPos { col: 2, row: 0 }, node("test.output")),
            (GridPos { col: 0, row: 1 }, node("test.inline_default")),
            (GridPos { col: 1, row: 1 }, node("test.output")),
        ]),
        edges: BTreeMap::from([
            (
                branch_a_edge.clone(),
                Edge {
                    id: branch_a_edge,
                    from: GridPos { col: 1, row: 0 },
                    to_node: GridPos { col: 2, row: 0 },
                    to_param: "value".into(),
                },
            ),
            (
                branch_b_edge.clone(),
                Edge {
                    id: branch_b_edge,
                    from: GridPos { col: 0, row: 1 },
                    to_node: GridPos { col: 1, row: 1 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "mixed edits".into(),
        cols: 3,
        rows: 2,
    };
    let mut cache = AnalysisCache::new();
    assert_cached_matches_fresh(&graph, &registry, &mut cache);

    apply_ops_to_graph_cached(
        &mut graph,
        &registry,
        &[
            GraphOp::NodeSetLabel {
                position: GridPos { col: 0, row: 0 },
                label: Some("lead".into()),
            },
            GraphOp::ParamSetInline {
                position: GridPos { col: 0, row: 1 },
                param_id: "text".into(),
                value: json!("branch-b"),
            },
            GraphOp::NodeRemove {
                position: GridPos { col: 1, row: 0 },
            },
        ],
        &mut cache,
    )
    .expect("apply mixed edits");

    let analyzed = assert_cached_matches_fresh(&graph, &registry, &mut cache);
    assert!(!analyzed.nodes.contains_key(&GridPos { col: 1, row: 0 }));
    assert!(
        !analyzed
            .output_types
            .contains_key(&GridPos { col: 1, row: 0 })
    );
}

#[test]
fn analyze_cached_matches_fresh_after_editing_variadic_member() {
    let mut registry = registry();
    registry.register(StaticPiece::new(variadic_order_piece()));

    let input_edge_id = EdgeId::new();
    let output_edge_id = EdgeId::new();
    let mut graph = Graph {
        nodes: BTreeMap::from([
            (GridPos { col: 0, row: 1 }, node("test.source")),
            (
                GridPos { col: 1, row: 1 },
                Node {
                    piece_id: "test.variadic_group".into(),
                    inline_params: BTreeMap::from([("a_inline".into(), json!("manual"))]),
                    pattern_source: None,
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (GridPos { col: 2, row: 1 }, node("test.output")),
        ]),
        edges: BTreeMap::from([
            (
                input_edge_id.clone(),
                Edge {
                    id: input_edge_id,
                    from: GridPos { col: 0, row: 1 },
                    to_node: GridPos { col: 1, row: 1 },
                    to_param: "z_edge".into(),
                },
            ),
            (
                output_edge_id.clone(),
                Edge {
                    id: output_edge_id,
                    from: GridPos { col: 1, row: 1 },
                    to_node: GridPos { col: 2, row: 1 },
                    to_param: "value".into(),
                },
            ),
        ]),
        name: "variadic cache".into(),
        cols: 3,
        rows: 2,
    };
    let mut cache = AnalysisCache::new();
    assert_cached_matches_fresh(&graph, &registry, &mut cache);

    apply_ops_to_graph_cached(
        &mut graph,
        &registry,
        &[GraphOp::ParamSetInline {
            position: GridPos { col: 1, row: 1 },
            param_id: "b_missing".into(),
            value: json!("late"),
        }],
        &mut cache,
    )
    .expect("set variadic member inline");

    assert_cached_matches_fresh(&graph, &registry, &mut cache);
}
