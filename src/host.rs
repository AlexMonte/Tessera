use std::collections::BTreeMap;

use crate::activity::{
    ActivityEvent, ActivitySnapshot, HostTime, PreviewTimeline, ProbeEvent, ProbeSnapshot,
};
use crate::ast::{Expr, OptLevel};
use crate::backend::{Backend, JsBackend};
use crate::compiler::{
    CompileCache, CompileMode, CompileProgram, NodeStateUpdate, compile_graph,
    compile_graph_cached, compile_graph_cached_with_opts, compile_graph_with_opts,
};
use crate::diagnostics::{Diagnostic, SemanticResult};
use crate::graph::{Graph, GraphOp};
use crate::ops::{
    ApplyOpsOutcome, EdgeTargetParamProbe, apply_ops_to_graph, apply_ops_to_graph_cached,
    probe_edge_connect,
};
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
                position: update.position,
                state: Some(update.state.clone()),
            })
            .collect()
    }

    /// Build an activity snapshot from a batch of raw activity events.
    ///
    /// Hosts override this to filter, transform, or enrich events before
    /// they reach the UI (e.g. decay logic, event coalescing).
    /// Default: groups events by `site` into a snapshot.
    fn build_activity_snapshot(
        &self,
        events: &[ActivityEvent],
        position: Option<HostTime>,
    ) -> ActivitySnapshot {
        let mut snapshot = ActivitySnapshot::new();
        snapshot.position = position;
        for event in events {
            snapshot
                .events
                .entry(event.site)
                .or_default()
                .push(event.clone());
        }
        snapshot
    }

    /// Build a probe snapshot from a batch of raw probe events.
    ///
    /// Hosts override this to coalesce or transform runtime values before they
    /// reach the UI. Default: stores the last probe seen for each `site`.
    fn build_probe_snapshot(
        &self,
        probes: &[ProbeEvent],
        position: Option<HostTime>,
    ) -> ProbeSnapshot {
        let mut snapshot = ProbeSnapshot::new();
        snapshot.position = position;
        for probe in probes {
            snapshot.values.insert(probe.site, probe.value.clone());
        }
        snapshot
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

    /// Compile a graph with an explicit optimization level.
    pub fn compile_with_opts(
        &self,
        graph: &Graph,
        mode: CompileMode,
        opt_level: OptLevel,
    ) -> Result<CompileProgram, Vec<Diagnostic>> {
        let sem = self.analyze(graph);
        compile_graph_with_opts(graph, &self.registry, &sem, mode, opt_level)
    }

    /// Compile a graph using a host-owned incremental cache.
    pub fn compile_cached(
        &self,
        graph: &Graph,
        mode: CompileMode,
        cache: &mut CompileCache,
    ) -> Result<CompileProgram, Vec<Diagnostic>> {
        compile_graph_cached(graph, &self.registry, mode, cache)
    }

    /// Compile a graph with an explicit optimization level and a host-owned cache.
    pub fn compile_cached_with_opts(
        &self,
        graph: &Graph,
        mode: CompileMode,
        opt_level: OptLevel,
        cache: &mut CompileCache,
    ) -> Result<CompileProgram, Vec<Diagnostic>> {
        compile_graph_cached_with_opts(graph, &self.registry, mode, opt_level, cache)
    }

    /// Apply a batch of graph operations.
    pub fn apply_ops(
        &self,
        graph: &mut Graph,
        ops: &[GraphOp],
    ) -> Result<ApplyOpsOutcome, Vec<Diagnostic>> {
        apply_ops_to_graph(graph, &self.registry, ops)
    }

    /// Apply a batch of graph operations and update a host-owned compile cache.
    pub fn apply_ops_cached(
        &self,
        graph: &mut Graph,
        ops: &[GraphOp],
        cache: &mut CompileCache,
    ) -> Result<ApplyOpsOutcome, Vec<Diagnostic>> {
        apply_ops_to_graph_cached(graph, &self.registry, ops, cache)
    }

    /// Render terminal expressions using the host adapter.
    pub fn render_terminals(&self, terminals: &[Expr]) -> Vec<String> {
        self.adapter.render_terminals(terminals)
    }

    /// Convert state updates to graph operations using the host adapter.
    pub fn state_update_ops(&self, updates: &[NodeStateUpdate]) -> Vec<GraphOp> {
        self.adapter.state_update_ops(updates)
    }

    /// Build an activity snapshot from raw events using the host adapter.
    pub fn build_activity_snapshot(
        &self,
        events: &[ActivityEvent],
        position: Option<HostTime>,
    ) -> ActivitySnapshot {
        self.adapter.build_activity_snapshot(events, position)
    }

    /// Build a probe snapshot from raw probe events using the host adapter.
    pub fn build_probe_snapshot(
        &self,
        probes: &[ProbeEvent],
        position: Option<HostTime>,
    ) -> ProbeSnapshot {
        self.adapter.build_probe_snapshot(probes, position)
    }

    /// Collect preview timelines for all nodes in the graph.
    ///
    /// Returns a map from grid position to the piece's preview timeline,
    /// for nodes whose pieces implement
    /// [`preview_timeline`](crate::piece::Piece::preview_timeline).
    pub fn preview_timelines(&self, graph: &Graph) -> BTreeMap<GridPos, PreviewTimeline> {
        let mut timelines = BTreeMap::new();
        for (pos, node) in &graph.nodes {
            if let Some(timeline) = self
                .registry
                .get(node.piece_id.as_str())
                .and_then(|piece| piece.preview_timeline(&node.inline_params))
            {
                timelines.insert(*pos, timeline);
            }
        }
        timelines
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
                    tags: vec![],
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
                    tags: vec![],
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

    #[test]
    fn engine_compile_cached_reuses_host_owned_cache() {
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
        let mut cache = CompileCache::new();

        let first = engine
            .compile_cached(&graph, CompileMode::Preview, &mut cache)
            .unwrap();
        let second = engine
            .compile_cached(&graph, CompileMode::Preview, &mut cache)
            .unwrap();

        assert_eq!(engine.render_terminals(&first.terminals), vec!["'bd'"]);
        assert_eq!(first.terminals, second.terminals);
        assert!(!cache.is_empty());
    }

    #[test]
    fn engine_apply_ops_cached_threads_invalidation_through_engine_api() {
        let engine = GraphEngine::new(TestAdapter);
        let mut graph = Graph {
            nodes: BTreeMap::from([(
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.source".into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            )]),
            edges: BTreeMap::new(),
            name: "test".into(),
            cols: 4,
            rows: 1,
        };
        let mut cache = CompileCache::new();

        engine
            .apply_ops_cached(
                &mut graph,
                &[GraphOp::NodeSetLabel {
                    position: GridPos { col: 0, row: 0 },
                    label: Some("kick".into()),
                }],
                &mut cache,
            )
            .unwrap();

        assert_eq!(
            graph.nodes[&GridPos { col: 0, row: 0 }].label.as_deref(),
            Some("kick")
        );
        assert!(cache.is_empty());
    }

    // -- Activity tests --

    use crate::activity::{ActivityEvent, HostTime, PreviewTimeline, ProbeEvent, ProbeSnapshot};
    use serde_json::json;

    /// A test piece that provides a preview timeline.
    struct TestTimelineSource {
        def: PieceDef,
    }
    impl TestTimelineSource {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.timeline_source".into(),
                    label: "timeline_source".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }
    impl Piece for TestTimelineSource {
        fn def(&self) -> &PieceDef {
            &self.def
        }
        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::str_lit("bd sd")
        }
        fn preview_timeline(
            &self,
            _inline_params: &BTreeMap<String, Value>,
        ) -> Option<PreviewTimeline> {
            Some(PreviewTimeline::uniform(&["bd", "sd"]))
        }
    }

    struct TimelineAdapter;
    impl HostAdapter for TimelineAdapter {
        fn create_registry(&self) -> PieceRegistry {
            let mut reg = PieceRegistry::new();
            reg.register(TestSource::new());
            reg.register(TestTimelineSource::new());
            reg.register(TestOutput::new());
            reg
        }
    }

    #[test]
    fn preview_timelines_skips_pieces_without_timeline() {
        let engine = GraphEngine::new(TimelineAdapter);
        let graph = Graph {
            nodes: BTreeMap::from([(
                GridPos { col: 0, row: 0 },
                Node {
                    piece_id: "test.source".into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            )]),
            edges: BTreeMap::new(),
            name: "test".into(),
            cols: 4,
            rows: 1,
        };

        let timelines = engine.preview_timelines(&graph);
        assert!(timelines.is_empty());
    }

    #[test]
    fn preview_timelines_includes_pieces_with_timeline() {
        let engine = GraphEngine::new(TimelineAdapter);
        let pos = GridPos { col: 0, row: 0 };
        let graph = Graph {
            nodes: BTreeMap::from([(
                pos,
                Node {
                    piece_id: "test.timeline_source".into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            )]),
            edges: BTreeMap::new(),
            name: "test".into(),
            cols: 4,
            rows: 1,
        };

        let timelines = engine.preview_timelines(&graph);
        assert_eq!(timelines.len(), 1);
        let tl = &timelines[&pos];
        assert_eq!(tl.steps.len(), 2);
        assert_eq!(tl.steps[0].label.as_deref(), Some("bd"));
        assert_eq!(tl.steps[1].label.as_deref(), Some("sd"));
    }

    #[test]
    fn build_activity_snapshot_groups_by_site() {
        let engine = GraphEngine::new(TestAdapter);
        let p1 = GridPos { col: 0, row: 0 };
        let p2 = GridPos { col: 1, row: 0 };
        let events = vec![
            ActivityEvent::trigger(p1).with_label("bd"),
            ActivityEvent::trigger(p2).with_label("sd"),
            ActivityEvent::trigger(p1).with_label("hh"),
        ];
        let snap = engine.build_activity_snapshot(&events, Some(HostTime::cycle(0.5, 1)));

        assert_eq!(snap.events_at(&p1).len(), 2);
        assert_eq!(snap.events_at(&p2).len(), 1);
        assert_eq!(
            snap.position,
            Some(HostTime::Cycle {
                phase: 0.5,
                cycle: 1
            })
        );
    }

    #[test]
    fn build_probe_snapshot_keeps_last_value_per_site() {
        let engine = GraphEngine::new(TestAdapter);
        let p1 = GridPos { col: 0, row: 0 };
        let p2 = GridPos { col: 1, row: 0 };
        let probes = vec![
            ProbeEvent::new(p1, json!(0.25)),
            ProbeEvent::new(p2, json!("sd")),
            ProbeEvent::new(p1, json!(0.75)),
        ];
        let expected_p1 = json!(0.75);
        let expected_p2 = json!("sd");

        let snap = engine.build_probe_snapshot(&probes, Some(HostTime::cycle(0.5, 1)));

        assert_eq!(snap.value_at(&p1), Some(&expected_p1));
        assert_eq!(snap.value_at(&p2), Some(&expected_p2));
        assert_eq!(
            snap.position,
            Some(HostTime::Cycle {
                phase: 0.5,
                cycle: 1
            })
        );
    }

    struct ProbeAdapter;
    impl HostAdapter for ProbeAdapter {
        fn create_registry(&self) -> PieceRegistry {
            PieceRegistry::new()
        }

        fn build_probe_snapshot(
            &self,
            probes: &[ProbeEvent],
            _position: Option<HostTime>,
        ) -> ProbeSnapshot {
            let mut snapshot = ProbeSnapshot::new();
            if let Some(first) = probes.first() {
                snapshot
                    .values
                    .insert(first.site, json!("adapter_override"));
            }
            snapshot
        }
    }

    #[test]
    fn build_probe_snapshot_uses_adapter_override() {
        let engine = GraphEngine::new(ProbeAdapter);
        let pos = GridPos { col: 2, row: 3 };
        let expected = json!("adapter_override");

        let snap = engine.build_probe_snapshot(
            &[ProbeEvent::new(pos, json!(0.25))],
            Some(HostTime::seconds(1.0)),
        );

        assert_eq!(snap.position, None);
        assert_eq!(snap.value_at(&pos), Some(&expected));
    }
}
