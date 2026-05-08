use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::analysis::AnalysisCache;
use crate::diagnostics::DiagnosticKind;
use crate::graph::{Edge, Graph, GraphOp, Node};
use crate::pattern::{
    PatternAtom, PatternContainer, PatternContainerKind, PatternItem, PatternItemKind, PatternPos,
    PatternRoot, PatternSurface,
};
use crate::piece::{ParamDef, ParamInlineMode, ParamSchema, ParamValueKind, Piece, PieceDef};
use crate::piece_registry::PieceRegistry;
use crate::types::{
    DomainBridgeKind, ExecutionDomain, GridPos, PieceCategory, PieceSemanticKind, PortRole,
    PortType, TileSide,
};

struct TestPiece {
    def: PieceDef,
}

impl TestPiece {
    fn source() -> Self {
        Self {
            def: PieceDef {
                id: "test.source".into(),
                label: "source".into(),
                category: PieceCategory::Generator,
                semantic_kind: PieceSemanticKind::Intrinsic,
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
            },
        }
    }

    fn output() -> Self {
        Self {
            def: PieceDef {
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
            },
        }
    }

    fn audio_source() -> Self {
        Self {
            def: PieceDef {
                id: "test.audio_source".into(),
                label: "audio_source".into(),
                category: PieceCategory::Generator,
                semantic_kind: PieceSemanticKind::Intrinsic,
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
            },
        }
    }

    fn number_source() -> Self {
        Self {
            def: PieceDef {
                id: "test.number_source".into(),
                label: "number_source".into(),
                category: PieceCategory::Generator,
                semantic_kind: PieceSemanticKind::Intrinsic,
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
            },
        }
    }

    fn number_output() -> Self {
        Self {
            def: PieceDef {
                id: "test.number_output".into(),
                label: "number_output".into(),
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
            },
        }
    }

    fn bool_output() -> Self {
        Self {
            def: PieceDef {
                id: "test.bool_output".into(),
                label: "bool_output".into(),
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
            },
        }
    }

    fn multi_input() -> Self {
        Self {
            def: PieceDef {
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
            },
        }
    }

    fn connector() -> Self {
        Self {
            def: PieceDef {
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
            },
        }
    }

    fn signal_source() -> Self {
        Self {
            def: PieceDef {
                id: "test.signal_source".into(),
                label: "signal_source".into(),
                category: PieceCategory::Generator,
                semantic_kind: PieceSemanticKind::Intrinsic,
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
            },
        }
    }

    fn gate_output() -> Self {
        Self {
            def: PieceDef {
                id: "test.gate_output".into(),
                label: "gate_output".into(),
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
            },
        }
    }
}

impl Piece for TestPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }
}

fn registry() -> PieceRegistry {
    let mut registry = PieceRegistry::new();
    registry.register(TestPiece::source());
    registry.register(TestPiece::output());
    registry.register(TestPiece::audio_source());
    registry.register(TestPiece::number_source());
    registry.register(TestPiece::number_output());
    registry.register(TestPiece::bool_output());
    registry.register(TestPiece::multi_input());
    registry.register(TestPiece::connector());
    registry.register(TestPiece::signal_source());
    registry.register(TestPiece::gate_output());
    registry
}

#[test]
fn apply_ops_places_nodes_and_connects_edges() {
    let registry = registry();
    let mut graph = Graph {
        nodes: BTreeMap::new(),
        edges: BTreeMap::new(),
        name: "ops".into(),
        cols: 4,
        rows: 1,
    };

    apply_ops_to_graph(
        &mut graph,
        &registry,
        &[
            GraphOp::NodePlace {
                position: GridPos { col: 0, row: 0 },
                piece_id: "test.source".into(),
                inline_params: BTreeMap::new(),
                pattern_source: None,
            },
            GraphOp::NodePlace {
                position: GridPos { col: 1, row: 0 },
                piece_id: "test.output".into(),
                inline_params: BTreeMap::new(),
                pattern_source: None,
            },
            GraphOp::ParamSetSide {
                position: GridPos { col: 1, row: 0 },
                param_id: "value".into(),
                side: TileSide::LEFT,
            },
        ],
    )
    .expect("place nodes");

    let outcome = apply_ops_to_graph(
        &mut graph,
        &registry,
        &[GraphOp::EdgeConnect {
            edge_id: None,
            from: GridPos { col: 0, row: 0 },
            to_node: GridPos { col: 1, row: 0 },
            to_param: "value".into(),
        }],
    )
    .expect("connect edge");

    assert_eq!(graph.edges.len(), 1);
    assert_eq!(outcome.applied_ops.len(), 1);
}

#[test]
fn apply_ops_cached_invalidates_analysis_cache() {
    let registry = registry();
    let mut graph = Graph {
        nodes: BTreeMap::from([(
            GridPos { col: 0, row: 0 },
            Node {
                piece_id: "test.source".into(),
                inline_params: BTreeMap::new(),
                pattern_source: None,
                input_sides: BTreeMap::new(),
                output_side: None,
                label: None,
                node_state: None,
            },
        )]),
        edges: BTreeMap::new(),
        name: "ops".into(),
        cols: 4,
        rows: 1,
    };
    let mut cache = AnalysisCache::new();
    cache.analyzed = Some(crate::analysis::AnalyzedGraph {
        diagnostics: vec![],
        eval_order: vec![],
        outputs: vec![],
        nodes: BTreeMap::new(),
        output_types: BTreeMap::new(),
        domain_bridges: BTreeMap::new(),
        delay_edges: Default::default(),
        temporal_nodes: BTreeMap::new(),
        temporal_edges: BTreeMap::new(),
    });

    apply_ops_to_graph_cached(
        &mut graph,
        &registry,
        &[GraphOp::NodeSetLabel {
            position: GridPos { col: 0, row: 0 },
            label: Some("kick".into()),
        }],
        &mut cache,
    )
    .expect("apply cached");

    assert!(cache.analyzed.is_some());
    assert!(cache.dirty.contains(&GridPos { col: 0, row: 0 }));
}

#[test]
fn apply_ops_sets_pattern_surface_on_node() {
    let registry = registry();
    let mut graph = Graph {
        nodes: BTreeMap::from([(
            GridPos { col: 0, row: 0 },
            Node {
                piece_id: "test.source".into(),
                inline_params: BTreeMap::new(),
                pattern_source: None,
                input_sides: BTreeMap::new(),
                output_side: None,
                label: None,
                node_state: None,
            },
        )]),
        edges: BTreeMap::new(),
        name: "pattern-surface".into(),
        cols: 2,
        rows: 1,
    };

    let surface = PatternSurface {
        roots: vec![PatternRoot {
            position: PatternPos { col: 0, row: 0 },
            container: PatternContainer {
                kind: PatternContainerKind::Subdivide,
                items: vec![PatternItem {
                    position: PatternPos { col: 0, row: 0 },
                    kind: PatternItemKind::Atom(PatternAtom::Note { value: "b4".into() }),
                }],
            },
        }],
    };

    apply_ops_to_graph(
        &mut graph,
        &registry,
        &[GraphOp::NodeSetPatternSurface {
            position: GridPos { col: 0, row: 0 },
            pattern_source: Some(surface.clone()),
        }],
    )
    .expect("set pattern surface");

    assert_eq!(
        graph.nodes[&GridPos { col: 0, row: 0 }].pattern_source,
        Some(surface)
    );
}

#[test]
fn probe_edge_connect_reports_domain_bridges() {
    let registry = registry();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.audio_source".into(),
                    inline_params: BTreeMap::new(),
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
                    piece_id: "test.number_output".into(),
                    inline_params: BTreeMap::new(),
                    pattern_source: None,
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
        ]),
        edges: BTreeMap::new(),
        name: "probe".into(),
        cols: 2,
        rows: 1,
    };

    let probe = probe_edge_connect(
        &graph,
        &registry,
        &GridPos { col: 0, row: 0 },
        &GridPos { col: 1, row: 0 },
        Some("value"),
    );

    assert_eq!(probe.to_param.as_deref(), Some("value"));
    assert_eq!(
        probe.implicit_bridge,
        Some(DomainBridgeKind::AudioToControl)
    );
}

#[test]
fn probe_edge_connect_rejects_number_to_bool_without_coercion() {
    let registry = registry();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.number_source".into(),
                    inline_params: BTreeMap::new(),
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
                    piece_id: "test.bool_output".into(),
                    inline_params: BTreeMap::new(),
                    pattern_source: None,
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
        ]),
        edges: BTreeMap::new(),
        name: "bool probe".into(),
        cols: 2,
        rows: 1,
    };

    let probe = probe_edge_connect(
        &graph,
        &registry,
        &GridPos { col: 0, row: 0 },
        &GridPos { col: 1, row: 0 },
        Some("value"),
    );

    assert_eq!(probe.reason, Some(EdgeConnectProbeReason::TypeMismatch));
}

#[test]
fn probe_edge_connect_traverses_connector_sources() {
    let registry = registry();
    let upstream_edge_id = crate::types::EdgeId::new();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.audio_source".into(),
                    inline_params: BTreeMap::new(),
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
                    piece_id: "test.connector".into(),
                    inline_params: BTreeMap::new(),
                    pattern_source: None,
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (
                GridPos { col: 2, row: 0 },
                Node {
                    piece_id: "test.number_output".into(),
                    inline_params: BTreeMap::new(),
                    pattern_source: None,
                    input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
        ]),
        edges: BTreeMap::from([(
            upstream_edge_id.clone(),
            crate::graph::Edge {
                id: upstream_edge_id,
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "value".into(),
            },
        )]),
        name: "connector probe".into(),
        cols: 3,
        rows: 1,
    };

    let probe = probe_edge_connect(
        &graph,
        &registry,
        &GridPos { col: 1, row: 0 },
        &GridPos { col: 2, row: 0 },
        Some("value"),
    );

    assert_eq!(probe.to_param.as_deref(), Some("value"));
    assert_eq!(
        probe.implicit_bridge,
        Some(DomainBridgeKind::AudioToControl)
    );
}

#[test]
fn apply_ops_rejects_invalid_inline_values() {
    let registry = registry();
    let mut graph = Graph {
        nodes: BTreeMap::new(),
        edges: BTreeMap::new(),
        name: "ops".into(),
        cols: 2,
        rows: 1,
    };

    let errors = apply_ops_to_graph(
        &mut graph,
        &registry,
        &[GraphOp::NodePlace {
            position: GridPos { col: 0, row: 0 },
            piece_id: "test.output".into(),
            inline_params: BTreeMap::from([("value".into(), json!("bd"))]),
            pattern_source: None,
        }],
    )
    .expect_err("inline not allowed");

    assert!(
        errors.iter().any(|diagnostic| {
            matches!(diagnostic.kind, DiagnosticKind::InvalidOperation { .. })
        })
    );
}

#[test]
fn apply_ops_rejects_duplicate_side_assignment() {
    let registry = registry();
    let mut graph = Graph {
        nodes: BTreeMap::from([(
            GridPos { col: 0, row: 0 },
            Node {
                piece_id: "test.multi_input".into(),
                inline_params: BTreeMap::new(),
                pattern_source: None,
                input_sides: BTreeMap::from([("left_a".into(), TileSide::LEFT)]),
                output_side: None,
                label: None,
                node_state: None,
            },
        )]),
        edges: BTreeMap::new(),
        name: "ops".into(),
        cols: 1,
        rows: 1,
    };

    let errors = apply_ops_to_graph(
        &mut graph,
        &registry,
        &[GraphOp::ParamSetSide {
            position: GridPos { col: 0, row: 0 },
            param_id: "left_b".into(),
            side: TileSide::LEFT,
        }],
    )
    .expect_err("duplicate side assignment should fail");

    assert!(
        errors.iter().any(|diagnostic| {
            matches!(diagnostic.kind, DiagnosticKind::InvalidOperation { .. })
        })
    );
    assert_eq!(
        graph.nodes[&GridPos { col: 0, row: 0 }].input_sides,
        BTreeMap::from([("left_a".into(), TileSide::LEFT)])
    );
}

#[test]
fn apply_ops_rejects_batch_place_with_duplicate_input_sides_atomically() {
    let registry = registry();
    let mut graph = Graph {
        nodes: BTreeMap::new(),
        edges: BTreeMap::new(),
        name: "ops".into(),
        cols: 2,
        rows: 1,
    };

    let errors = apply_ops_to_graph(
        &mut graph,
        &registry,
        &[GraphOp::NodeBatchPlace {
            nodes: vec![crate::graph::BatchPlaceEntry {
                position: GridPos { col: 0, row: 0 },
                piece_id: "test.multi_input".into(),
                inline_params: BTreeMap::new(),
                pattern_source: None,
                input_sides: BTreeMap::from([
                    ("left_a".into(), TileSide::LEFT),
                    ("left_b".into(), TileSide::LEFT),
                ]),
                output_side: None,
                label: None,
            }],
            edges: vec![],
            auto_wire: false,
        }],
    )
    .expect_err("duplicate sides in batch place should fail");

    assert!(
        errors.iter().any(|diagnostic| {
            matches!(diagnostic.kind, DiagnosticKind::InvalidOperation { .. })
        })
    );
    assert!(graph.nodes.is_empty());
    assert!(graph.edges.is_empty());
}

#[test]
fn probe_edge_connect_never_needs_to_guess_after_duplicate_side_rejection() {
    let registry = registry();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.source".into(),
                    inline_params: BTreeMap::new(),
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
                    piece_id: "test.multi_input".into(),
                    inline_params: BTreeMap::new(),
                    pattern_source: None,
                    input_sides: BTreeMap::from([("left_a".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
        ]),
        edges: BTreeMap::new(),
        name: "probe".into(),
        cols: 2,
        rows: 1,
    };

    let probe = probe_edge_connect(
        &graph,
        &registry,
        &GridPos { col: 0, row: 0 },
        &GridPos { col: 1, row: 0 },
        None,
    );

    assert_eq!(probe.to_param.as_deref(), Some("left_a"));
}

#[test]
fn auto_wire_uses_the_only_remaining_param_side() {
    let registry = registry();
    let mut graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.source".into(),
                    inline_params: BTreeMap::new(),
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
                    piece_id: "test.multi_input".into(),
                    inline_params: BTreeMap::new(),
                    pattern_source: None,
                    input_sides: BTreeMap::from([("left_a".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
        ]),
        edges: BTreeMap::new(),
        name: "autowire".into(),
        cols: 2,
        rows: 1,
    };

    apply_ops_to_graph(
        &mut graph,
        &registry,
        &[GraphOp::NodeAutoWire {
            position: GridPos { col: 1, row: 0 },
        }],
    )
    .expect("auto wire should succeed");

    let edge = graph.edges.values().next().expect("auto-wired edge");
    assert_eq!(edge.to_param, "left_a");
}

#[test]
fn probe_edge_connect_uses_piece_default_input_side() {
    let registry = registry();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.source".into(),
                    inline_params: BTreeMap::new(),
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
                    piece_id: "test.output".into(),
                    inline_params: BTreeMap::new(),
                    pattern_source: None,
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
        ]),
        edges: BTreeMap::new(),
        name: "default-side-probe".into(),
        cols: 2,
        rows: 1,
    };

    let probe = probe_edge_connect(
        &graph,
        &registry,
        &GridPos { col: 0, row: 0 },
        &GridPos { col: 1, row: 0 },
        None,
    );

    assert_eq!(probe.to_param.as_deref(), Some("value"));
}

#[test]
fn probe_edge_connect_rejects_role_mismatch() {
    let registry = registry();
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.signal_source".into(),
                    inline_params: BTreeMap::new(),
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
                    piece_id: "test.gate_output".into(),
                    inline_params: BTreeMap::new(),
                    pattern_source: None,
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
        ]),
        edges: BTreeMap::new(),
        name: "role-probe".into(),
        cols: 2,
        rows: 1,
    };

    let probe = probe_edge_connect(
        &graph,
        &registry,
        &GridPos { col: 0, row: 0 },
        &GridPos { col: 1, row: 0 },
        Some("value"),
    );

    assert_eq!(probe.reason, Some(EdgeConnectProbeReason::TypeMismatch));
    assert!(
        probe
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("role_mismatch:"))
    );
}

#[test]
fn node_auto_wire_prunes_existing_role_mismatch_edges() {
    let registry = registry();
    let edge_id = crate::types::EdgeId::new();
    let mut graph = Graph {
        nodes: BTreeMap::from([
            (
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.signal_source".into(),
                    inline_params: BTreeMap::new(),
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
                    piece_id: "test.gate_output".into(),
                    inline_params: BTreeMap::new(),
                    pattern_source: None,
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
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
        name: "role-prune".into(),
        cols: 2,
        rows: 1,
    };

    let outcome = apply_ops_to_graph(
        &mut graph,
        &registry,
        &[GraphOp::NodeAutoWire {
            position: GridPos { col: 1, row: 0 },
        }],
    )
    .expect("auto wire should prune invalid role edge");

    assert!(graph.edges.is_empty());
    assert_eq!(outcome.removed_edges.len(), 1);
    assert!(outcome.applied_ops.iter().any(
        |op| matches!(op, GraphOp::EdgeDisconnect { edge_id: removed } if removed == &edge_id)
    ));
}

#[test]
fn apply_ops_rejects_side_override_that_collides_with_default_side() {
    let registry = registry();
    let mut graph = Graph {
        nodes: BTreeMap::from([(
            GridPos { col: 1, row: 0 },
            Node {
                piece_id: "test.multi_input".into(),
                inline_params: BTreeMap::new(),
                pattern_source: None,
                input_sides: BTreeMap::new(),
                output_side: None,
                label: None,
                node_state: None,
            },
        )]),
        edges: BTreeMap::new(),
        name: "override-collision".into(),
        cols: 2,
        rows: 1,
    };

    let result = apply_ops_to_graph(
        &mut graph,
        &registry,
        &[GraphOp::ParamSetSide {
            position: GridPos { col: 1, row: 0 },
            param_id: "left_b".into(),
            side: TileSide::LEFT,
        }],
    );

    assert!(matches!(
        result,
        Err(ref diagnostics)
            if diagnostics.iter().any(|diagnostic| matches!(
                &diagnostic.kind,
                DiagnosticKind::InvalidOperation { reason }
                    if reason.contains("already assigned to param 'left_a'")
            ))
    ));
}
