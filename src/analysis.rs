use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diagnostics::{Diagnostic, DiagnosticSeverity};
use crate::graph::{Edge, Graph, GraphOp};
use crate::types::{DomainBridge, DomainBridgeKind, EdgeId, GridPos, PortRole, PortType, TileSide};

/// Fully analyzed view of a graph.
///
/// Host crates use this as the stable lowering contract: explicit output roots,
/// deterministic evaluation order, normalized input sources, inferred types,
/// and bridge metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedGraph {
    /// All diagnostics emitted during semantic analysis.
    pub diagnostics: Vec<Diagnostic>,
    /// Deterministic topological walk order, excluding delay edges.
    pub eval_order: Vec<GridPos>,
    /// Explicit output roots in deterministic grid order.
    pub outputs: Vec<GridPos>,
    /// Per-node analyzed facts keyed by position.
    pub nodes: BTreeMap<GridPos, AnalyzedNode>,
    /// Final inferred output type per node when known.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub output_types: BTreeMap<GridPos, PortType>,
    /// Implicit domain bridges required by accepted edges.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub domain_bridges: BTreeMap<EdgeId, DomainBridge>,
    /// Edges treated as feedback inputs to delay nodes.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub delay_edges: BTreeSet<EdgeId>,
}

impl AnalyzedGraph {
    /// Returns true when analysis found no errors and at least one output root.
    pub fn is_valid(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            && !self.outputs.is_empty()
    }

    /// Returns the analyzed node at `pos`, if present.
    pub fn node(&self, pos: &GridPos) -> Option<&AnalyzedNode> {
        self.nodes.get(pos)
    }

    /// Iterates explicit output roots with their analyzed node records.
    pub fn output_nodes(&self) -> impl Iterator<Item = (GridPos, &AnalyzedNode)> + '_ {
        self.outputs
            .iter()
            .filter_map(|pos| self.nodes.get(pos).map(|node| (*pos, node)))
    }
}

/// Analyzed facts for one placed node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedNode {
    /// Registered piece id used by this node instance.
    pub piece_id: String,
    /// Inline values stored directly on the node.
    #[serde(default)]
    pub inline_params: BTreeMap<String, Value>,
    /// Resolved non-variadic inputs keyed by parameter id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scalar_inputs: BTreeMap<String, ResolvedInput>,
    /// Resolved variadic inputs keyed by variadic group id.
    ///
    /// Entries inside each group preserve the declaration order of the
    /// corresponding params in the piece definition.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variadic_inputs: BTreeMap<String, Vec<ResolvedInput>>,
    /// Effective input type per parameter after normalization.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_types: BTreeMap<String, PortType>,
    /// Final inferred output type when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_type: Option<PortType>,
    /// Non-value roles declared by the piece for its inputs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_roles: BTreeMap<String, PortRole>,
    /// Non-value role declared by the piece for its output.
    #[serde(default, skip_serializing_if = "PortRole::is_value")]
    pub output_role: PortRole,
    /// Effective output side after node overrides are applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_side: Option<TileSide>,
    /// Opaque host-owned state stored on the node instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_state: Option<Value>,
}

impl AnalyzedNode {
    /// Returns the resolved scalar input for `param_id`, if present.
    pub fn input(&self, param_id: &str) -> Option<&ResolvedInput> {
        self.scalar_inputs.get(param_id)
    }

    /// Returns the resolved variadic group for `group_id`, if present.
    pub fn variadic_group(&self, group_id: &str) -> Option<&[ResolvedInput]> {
        self.variadic_inputs.get(group_id).map(Vec::as_slice)
    }
}

/// Normalized input facts for a single parameter site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedInput {
    /// Where this value came from.
    pub source: ResolvedInputSource,
    /// Effective type after inline/default resolution and bridge normalization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_type: Option<PortType>,
    /// Required bridge when the source crosses execution domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge_kind: Option<DomainBridgeKind>,
}

impl ResolvedInput {
    /// Returns true when the analysis result has no source for this input.
    pub fn is_missing(&self) -> bool {
        matches!(self.source, ResolvedInputSource::Missing)
    }
}

/// Stable source categories for resolved inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolvedInputSource {
    /// Value arrives from another node over an edge.
    Edge {
        edge_id: EdgeId,
        from: GridPos,
        exit_side: TileSide,
        /// Connector nodes traversed between `from` and the current input.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        via: Vec<GridPos>,
    },
    /// Value is stored inline on the node.
    Inline { value: Value },
    /// Value comes from the parameter schema default.
    Default { value: Value },
    /// No edge, inline value, or default exists for this input.
    Missing,
}

/// Incremental cache for repeated analysis across graph edits.
#[derive(Debug, Clone, Default)]
pub struct AnalysisCache {
    pub(crate) analyzed: Option<AnalyzedGraph>,
    pub(crate) dirty: BTreeSet<GridPos>,
    pub(crate) downstream_dependents: BTreeMap<GridPos, BTreeSet<GridPos>>,
    pub(crate) incoming_targets_by_source: BTreeMap<GridPos, BTreeSet<GridPos>>,
    pub(crate) component_members: BTreeMap<GridPos, BTreeSet<GridPos>>,
    pub(crate) edge_ids: BTreeSet<EdgeId>,
    pub(crate) node_positions: BTreeSet<GridPos>,
}

impl AnalysisCache {
    /// Creates an empty analysis cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears all cached analysis state.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Returns true when no cached analysis or dirty state is present.
    pub fn is_empty(&self) -> bool {
        self.analyzed.is_none() && self.dirty.is_empty()
    }

    pub(crate) fn invalidate_from_apply_outcome(
        &mut self,
        _graph: &Graph,
        applied_ops: &[GraphOp],
        removed_edges: &[Edge],
        explicit_disconnect_targets: &BTreeMap<EdgeId, GridPos>,
    ) {
        for op in applied_ops {
            match op {
                GraphOp::NodePlace { position, .. }
                | GraphOp::NodeRemove { position }
                | GraphOp::NodeSetState { position, .. }
                | GraphOp::ParamSetInline { position, .. }
                | GraphOp::ParamClearInline { position, .. }
                | GraphOp::ParamSetSide { position, .. }
                | GraphOp::ParamClearSide { position, .. }
                | GraphOp::OutputSetSide { position, .. }
                | GraphOp::OutputClearSide { position }
                | GraphOp::NodeAutoWire { position }
                | GraphOp::NodeSetLabel { position, .. } => {
                    self.dirty.insert(*position);
                }
                GraphOp::NodeMove { from, to } | GraphOp::NodeSwap { a: from, b: to } => {
                    self.dirty.insert(*from);
                    self.dirty.insert(*to);
                }
                GraphOp::NodeBatchPlace { nodes, .. } => {
                    for entry in nodes {
                        self.dirty.insert(entry.position);
                    }
                }
                GraphOp::EdgeConnect { to_node, .. } => {
                    self.dirty.insert(*to_node);
                }
                GraphOp::EdgeDisconnect { edge_id } => {
                    if let Some(target) = explicit_disconnect_targets.get(edge_id) {
                        self.dirty.insert(*target);
                    }
                }
                GraphOp::ResizeGrid { .. } => {}
            }
        }

        for edge in removed_edges {
            self.dirty.insert(edge.to_node);
        }
    }

    pub(crate) fn store_analysis(
        &mut self,
        graph: &Graph,
        analyzed: AnalyzedGraph,
        component_members: BTreeMap<GridPos, BTreeSet<GridPos>>,
    ) -> AnalyzedGraph {
        self.downstream_dependents = build_downstream_dependents(&analyzed);
        self.incoming_targets_by_source = build_incoming_targets_by_source(graph);
        self.component_members = component_members;
        self.edge_ids = graph.edges.keys().cloned().collect();
        self.node_positions = graph.nodes.keys().copied().collect();
        self.dirty.clear();
        self.analyzed = Some(analyzed.clone());
        analyzed
    }
}

fn build_downstream_dependents(analyzed: &AnalyzedGraph) -> BTreeMap<GridPos, BTreeSet<GridPos>> {
    let mut downstream = BTreeMap::<GridPos, BTreeSet<GridPos>>::new();

    for (pos, node) in &analyzed.nodes {
        for input in node.scalar_inputs.values() {
            if let ResolvedInputSource::Edge { from, .. } = input.source {
                downstream.entry(from).or_default().insert(*pos);
            }
        }
        for inputs in node.variadic_inputs.values() {
            for input in inputs {
                if let ResolvedInputSource::Edge { from, .. } = input.source {
                    downstream.entry(from).or_default().insert(*pos);
                }
            }
        }
    }

    downstream
}

fn build_incoming_targets_by_source(graph: &Graph) -> BTreeMap<GridPos, BTreeSet<GridPos>> {
    let mut targets = BTreeMap::<GridPos, BTreeSet<GridPos>>::new();

    for edge in graph.edges.values() {
        targets.entry(edge.from).or_default().insert(edge.to_node);
    }

    targets
}
