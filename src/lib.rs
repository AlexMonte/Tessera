//! Tessera: a host-agnostic graph analysis kernel for visual editors.
//!
//! Tessera owns editable graph structure, mutation ops, validation, and
//! deterministic semantic analysis. Host crates own domain-specific lowering
//! from analyzed graphs into their own representations.

pub mod analysis;
pub mod diagnostics;
pub mod graph;
mod internal;
pub mod ops;
pub mod piece;
pub mod piece_registry;
pub mod semantic;
pub mod subgraph;
pub mod types;

pub mod prelude {
    pub use crate::{
        AnalysisCache, AnalyzedGraph, AnalyzedNode, Diagnostic, Edge, EdgeId, Graph, GridPos, Node,
        ParamDef, ParamSchema, Piece, PieceCategory, PieceDef, PieceExecutionKind, PieceRegistry,
        PieceSemanticKind, PortRole, PortType, Rational, TileSide,
    };
}

pub use analysis::{
    AnalysisCache, AnalyzedGraph, AnalyzedNode, ResolvedInput, ResolvedInputSource,
};
pub use diagnostics::{Diagnostic, DiagnosticKind, DiagnosticSeverity};
pub use graph::{BatchPlaceEdge, BatchPlaceEntry, Edge, Graph, GraphOp, GraphOpRecord, Node};
pub use ops::{
    ApplyOpsOutcome, EdgeConnectProbeReason, EdgeTargetParamProbe, RepairSuggestion,
    apply_ops_to_graph, apply_ops_to_graph_cached, pick_target_param_for_edge, probe_edge_connect,
    validate_edge_connect,
};
pub use piece::{
    ParamDef, ParamInlineMode, ParamSchema, ParamTextSemantics, ParamValueKind, Piece, PieceDef,
    PieceExecutionKind, is_valid_ident_path, is_valid_ident_segment,
};
pub use piece_registry::PieceRegistry;
pub use semantic::{analyze_cached, semantic_pass};
pub use subgraph::{
    GeneratedSubgraphPiece, SUBGRAPH_INPUT_1_ID, SUBGRAPH_INPUT_2_ID, SUBGRAPH_INPUT_3_ID,
    SUBGRAPH_OUTPUT_ID, SubgraphDef, SubgraphInput, SubgraphInputPiece, SubgraphOutputPiece,
    SubgraphSignature, analyze_subgraph, subgraph_editor_pieces, subgraph_pieces,
};
pub use types::{
    DELAY_PIECE_ID, DomainBridge, DomainBridgeKind, EdgeId, ExecutionDomain, GridPos,
    PieceCategory, PieceSemanticKind, PortRole, PortType, Rational, TileSide,
    adjacent_in_direction,
};

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{AnalysisCache, Graph};

    #[test]
    fn crate_reexports_analysis_cache() {
        let cache = AnalysisCache::new();
        assert!(cache.is_empty());
    }

    #[test]
    fn crate_reexports_graph_as_live_document_shape() {
        let graph = Graph {
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            name: "live".into(),
            cols: 4,
            rows: 2,
        };

        assert_eq!(graph.name, "live");
        assert_eq!(graph.cols, 4);
        assert_eq!(graph.rows, 2);
    }

    #[test]
    fn readme_avoids_removed_lowering_api_names() {
        let readme = include_str!("../README.md");
        for needle in [
            "compile",
            "render_terminals",
            "Backend",
            "Expr",
            "JsBackend",
            "LuaBackend",
            "GraphEngine",
            "HostAdapter",
        ] {
            assert!(
                !readme.contains(needle),
                "README.md still contains stale lowering reference: {needle}"
            );
        }
    }

    #[test]
    fn agents_reference_avoids_removed_lowering_api_names() {
        let agents = include_str!("../AGENTS.md");
        for needle in [
            "compile",
            "render_terminals",
            "Backend",
            "Expr",
            "JsBackend",
            "LuaBackend",
            "GraphEngine",
            "HostAdapter",
        ] {
            assert!(
                !agents.contains(needle),
                "AGENTS.md still contains stale lowering reference: {needle}"
            );
        }
    }

    #[test]
    fn docs_avoid_removed_activity_api_names() {
        let readme = include_str!("../README.md");
        let agents = include_str!("../AGENTS.md");

        for needle in [
            "preview_timeline",
            "preview_timelines",
            "build_activity_snapshot",
            "build_probe_snapshot",
            "PreviewTimeline",
            "ActivitySnapshot",
            "ProbeSnapshot",
            "HostTime",
        ] {
            assert!(
                !readme.contains(needle),
                "README.md still contains removed activity API reference: {needle}"
            );
            assert!(
                !agents.contains(needle),
                "AGENTS.md still contains removed activity API reference: {needle}"
            );
        }
    }

    #[test]
    fn production_ops_and_subgraph_modules_do_not_call_public_semantic_pass() {
        for (path, source) in [
            ("ops/validation.rs", include_str!("ops/validation.rs")),
            ("ops/pruning.rs", include_str!("ops/pruning.rs")),
            ("subgraph/analysis.rs", include_str!("subgraph/analysis.rs")),
        ] {
            assert!(
                !source.contains("use crate::semantic::semantic_pass"),
                "{path} still imports the public semantic_pass wrapper",
            );
            assert!(
                !source.contains("semantic_pass("),
                "{path} still calls the public semantic_pass wrapper",
            );
        }
    }
}
