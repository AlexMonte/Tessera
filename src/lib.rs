//! Tessera: a host-agnostic typed graph engine for visual music/program editors.
//!
//! Architectural rule: target-specific AST adaptation belongs in host/target
//! layers. The core library stays target-agnostic.

pub mod activity;
pub mod ast;
pub mod backend;
pub mod compiler;
pub mod core_pieces;
pub mod diagnostics;
pub mod graph;
pub mod host;
pub mod ops;
pub mod piece;
pub mod piece_registry;
pub mod semantic;
pub mod subgraph;
pub mod types;

pub use activity::{
    ActivityEvent, ActivityKind, ActivitySnapshot, HostTime, PreviewTimeline, ProbeEvent,
    ProbeSnapshot, TimelineStep,
};
pub use ast::{
    BinOp, Expr, ExprKind, Lit, OptLevel, Origin, StringSyntax, UnaryOp, is_valid_ident_path,
    is_valid_ident_segment, parse_ident_path,
};
pub use backend::{Backend, JsBackend, LuaBackend};
pub use compiler::{
    CompileCache, CompileMode, CompileProgram, DelaySlot, NodeStateUpdate, compile_graph,
    compile_graph_cached, compile_graph_cached_with_opts, compile_graph_with_opts,
    compile_node_expr, compile_node_expr_with_opts,
};
pub use core_pieces::{DELAY_PIECE_ID, core_expression_pieces};
pub use diagnostics::{Diagnostic, DiagnosticKind, DiagnosticSeverity, SemanticResult};
pub use graph::{
    BatchPlaceEdge, BatchPlaceEntry, Edge, Graph, GraphOp, GraphOpRecord, Node, ProjectDocument,
};
pub use host::{GraphEngine, HostAdapter};
pub use ops::{
    ApplyOpsOutcome, EdgeConnectProbeReason, EdgeTargetParamProbe, RepairSuggestion,
    apply_ops_to_graph, apply_ops_to_graph_cached, pick_target_param_for_edge, probe_edge_connect,
    validate_edge_connect,
};
pub use piece::{
    ParamDef, ParamInlineMode, ParamSchema, ParamValueKind, Piece, PieceDef, PieceInputs,
    ResolvedPieceTypes,
};
pub use piece_registry::PieceRegistry;
pub use semantic::semantic_pass;
pub use subgraph::{
    CompiledSubgraph, GeneratedSubgraphPiece, SUBGRAPH_INPUT_1_ID, SUBGRAPH_INPUT_2_ID,
    SUBGRAPH_INPUT_3_ID, SUBGRAPH_OUTPUT_ID, SubgraphDef, SubgraphInput, SubgraphInputPiece,
    SubgraphOutputPiece, SubgraphSignature, analyze_subgraph, compile_subgraph, compile_subgraphs,
    subgraph_editor_pieces, subgraph_pieces,
};
pub use types::{
    DomainBridge, DomainBridgeKind, EdgeId, ExecutionDomain, GridPos, PieceCategory,
    PieceSemanticKind, PortType, TileSide, adjacent_in_direction,
};

#[cfg(test)]
mod tests {
    use super::{CompileCache, GridPos, ProbeEvent, ProbeSnapshot};
    use serde_json::json;

    #[test]
    fn crate_reexports_probe_types() {
        let pos = GridPos { col: 0, row: 0 };
        let expected = json!(0.75);
        let event = ProbeEvent::new(pos, expected.clone());
        let mut snapshot = ProbeSnapshot::new();
        snapshot.values.insert(pos, expected.clone());

        assert_eq!(event.site, pos);
        assert_eq!(event.value, expected);
        assert_eq!(snapshot.value_at(&pos), Some(&event.value));
    }

    #[test]
    fn crate_reexports_compile_cache() {
        let cache = CompileCache::new();
        assert!(cache.is_empty());
    }
}
