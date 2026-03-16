//! Tessera: a host-agnostic typed graph engine for visual music/program editors.
//!
//! Architectural rule: target-specific AST adaptation belongs in host/target
//! layers. The core library stays target-agnostic.

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

pub use ast::{
    BinOp, Expr, ExprKind, Lit, Origin, StringSyntax, UnaryOp, is_valid_ident_path,
    is_valid_ident_segment, parse_ident_path,
};
pub use backend::{Backend, JsBackend, LuaBackend};
pub use compiler::{CompileMode, CompileProgram, NodeStateUpdate, compile_graph};
pub use core_pieces::core_expression_pieces;
pub use diagnostics::{Diagnostic, DiagnosticKind, DiagnosticSeverity, SemanticResult};
pub use graph::{Edge, Graph, GraphOp, GraphOpRecord, Node, ProjectDocument};
pub use host::{GraphEngine, HostAdapter};
pub use ops::{
    ApplyOpsOutcome, EdgeConnectProbeReason, EdgeTargetParamProbe, RepairSuggestion,
    apply_ops_to_graph, pick_target_param_for_edge, probe_edge_connect, validate_edge_connect,
};
pub use piece::{
    ParamDef, ParamInlineMode, ParamSchema, ParamValueKind, Piece, PieceDef, PieceInputs,
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
    EdgeId, GridPos, PieceCategory, PieceSemanticKind, PortType, TileSide, adjacent_in_direction,
};
