use crate::ast::Expr;
use crate::backend::{Backend, JsBackend};
use crate::compiler::{CompileMode, CompileProgram, NodeStateUpdate, compile_graph};
use crate::diagnostics::{Diagnostic, SemanticResult};
use crate::graph::{Graph, GraphOp};
use crate::ops::{ApplyOpsOutcome, EdgeTargetParamProbe, apply_ops_to_graph, probe_edge_connect};
use crate::piece_registry::PieceRegistry;
use crate::semantic::semantic_pass;
use crate::types::GridPos;

/// Trait for host applications to plug into Tessera's graph engine.
///
/// Implement this trait to provide host-specific registry construction,
/// terminal rendering, and runtime behavior. Different hosts can supply
/// their own piece registries, compile targets, and state-update handling
/// without dragging application-specific assumptions through the engine.
pub trait HostAdapter: Send + Sync {
    /// Build the piece registry for this host.
    ///
    /// Called during engine construction and when [`GraphEngine::reload_registry`]
    /// is invoked.
    fn create_registry(&self) -> PieceRegistry;

    /// Return the rendering backend for this host.
    ///
    /// Override to target a different language (e.g. Lua). Default: JavaScript.
    fn backend(&self) -> &'static dyn Backend {
        &JsBackend
    }

    /// Render compiled terminal expressions into output strings.
    ///
    /// Override this to change how terminals are combined (e.g. stack, concurrent
    /// voices, single output). Default: renders each terminal individually using
    /// the host's backend.
    fn render_terminals(&self, terminals: &[Expr]) -> Vec<String> {
        let backend = self.backend();
        terminals.iter().map(|expr| backend.render(expr)).collect()
    }

    /// Convert node state updates into graph operations for persistence.
    ///
    /// Override this to customise how runtime state changes flow back into the
    /// graph. Default: creates a `NodeSetState` op per update.
    fn state_update_ops(&self, updates: &[NodeStateUpdate]) -> Vec<GraphOp> {
        updates
            .iter()
            .map(|update| GraphOp::NodeSetState {
                position: update.position.clone(),
                state: Some(update.state.clone()),
            })
            .collect()
    }
}

/// A unified engine that ties a [`HostAdapter`] to Tessera's graph operations.
///
/// `GraphEngine` provides a single entry point for analysis, compilation,
/// op application, and rendering—abstracting the multi-step pipeline into
/// a cohesive API that different hosts can share.
pub struct GraphEngine<H: HostAdapter> {
    adapter: H,
    registry: PieceRegistry,
}

impl<H: HostAdapter> GraphEngine<H> {
    pub fn new(adapter: H) -> Self {
        let registry = adapter.create_registry();
        Self { adapter, registry }
    }

    pub fn adapter(&self) -> &H {
        &self.adapter
    }

    pub fn registry(&self) -> &PieceRegistry {
        &self.registry
    }

    /// Rebuild the registry from the adapter (e.g. after registering new subgraphs).
    pub fn reload_registry(&mut self) {
        self.registry = self.adapter.create_registry();
    }

    /// Run semantic analysis on a graph.
    pub fn analyze(&self, graph: &Graph) -> SemanticResult {
        semantic_pass(graph, &self.registry)
    }

    /// Compile a graph into a program.
    pub fn compile(
        &self,
        graph: &Graph,
        mode: CompileMode,
    ) -> Result<CompileProgram, Vec<Diagnostic>> {
        let sem = self.analyze(graph);
        compile_graph(graph, &self.registry, &sem, mode)
    }

    /// Apply a batch of graph operations.
    pub fn apply_ops(
        &self,
        graph: &mut Graph,
        ops: &[GraphOp],
    ) -> Result<ApplyOpsOutcome, Vec<Diagnostic>> {
        apply_ops_to_graph(graph, &self.registry, ops)
    }

    /// Render terminal expressions using the host adapter.
    pub fn render_terminals(&self, terminals: &[Expr]) -> Vec<String> {
        self.adapter.render_terminals(terminals)
    }

    /// Convert state updates to graph operations using the host adapter.
    pub fn state_update_ops(&self, updates: &[NodeStateUpdate]) -> Vec<GraphOp> {
        self.adapter.state_update_ops(updates)
    }

    /// Probe whether an edge connection is valid, with repair suggestions.
    pub fn probe_edge(
        &self,
        graph: &Graph,
        from: &GridPos,
        to_node: &GridPos,
        to_param: Option<&str>,
    ) -> EdgeTargetParamProbe {
        probe_edge_connect(graph, &self.registry, from, to_node, to_param)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::Value;

    use super::*;
    use crate::ast::Expr;
    use crate::graph::{Edge, Node};
    use crate::piece::{ParamDef, ParamSchema, Piece, PieceDef, PieceInputs};
    use crate::types::{EdgeId, PieceCategory, PieceSemanticKind, TileSide};

    struct TestSource {
        def: PieceDef,
    }
    impl TestSource {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.source".into(),
                    label: "source".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                },
            }
        }
    }
    impl Piece for TestSource {
        fn def(&self) -> &PieceDef {
            &self.def
        }
        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::str_lit("bd")
        }
    }

    struct TestOutput {
        def: PieceDef,
    }
    impl TestOutput {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.output".into(),
                    label: "output".into(),
                    category: PieceCategory::Output,
                    semantic_kind: PieceSemanticKind::Output,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "pattern".into(),
                        label: "pattern".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Text {
                            default: String::new(),
                            can_inline: false,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: true,
                    }],
                    output_type: None,
                    output_side: None,
                    description: None,
                },
            }
        }
    }
    impl Piece for TestOutput {
        fn def(&self) -> &PieceDef {
            &self.def
        }
        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            inputs
                .get("pattern")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing"))
        }
    }

    struct TestAdapter;
    impl HostAdapter for TestAdapter {
        fn create_registry(&self) -> PieceRegistry {
            let mut reg = PieceRegistry::new();
            reg.register(TestSource::new());
            reg.register(TestOutput::new());
            reg
        }
    }

    #[test]
    fn engine_compile_produces_terminal_output() {
        let engine = GraphEngine::new(TestAdapter);
        let graph = Graph {
            nodes: BTreeMap::from([
                (
                    GridPos { col: 0, row: 0 },
                    Node {
                        piece_id: "test.source".into(),
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
                        piece_id: "test.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(
                EdgeId::new(),
                Edge {
                    id: EdgeId::new(),
                    from: GridPos { col: 0, row: 0 },
                    to_node: GridPos { col: 1, row: 0 },
                    to_param: "pattern".into(),
                },
            )]),
            name: "test".into(),
            cols: 4,
            rows: 1,
        };

        let program = engine.compile(&graph, CompileMode::Preview).unwrap();
        let rendered = engine.render_terminals(&program.terminals);
        assert_eq!(rendered, vec!["'bd'"]);
    }
}
