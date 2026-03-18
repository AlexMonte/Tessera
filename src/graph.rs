//! Core graph data structures and mutation records used by Tessera hosts.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diagnostics::{Diagnostic, DiagnosticKind};
use crate::piece_registry::PieceRegistry;
use crate::types::{EdgeId, GridPos, TileSide};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// One placed piece instance on the grid.
pub struct Node {
    /// Registered piece id, for example `strudel.fast`.
    pub piece_id: String,
    /// Inline parameter values owned directly by this node instance.
    #[serde(default)]
    pub inline_params: BTreeMap<String, Value>,
    /// Per-param side assignments applied to this placed node.
    ///
    /// Missing entries mean the param is currently unassigned. Legacy `null`
    /// entries deserialize as missing assignments.
    #[serde(default, deserialize_with = "input_sides_serde::deserialize")]
    pub input_sides: BTreeMap<String, TileSide>,
    /// Optional output-side override applied by the editor.
    #[serde(default)]
    pub output_side: Option<TileSide>,
    /// Optional user-defined display name for this node instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional per-node opaque state blob (for stateful pieces).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_state: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Directed connection from one node output to a specific target parameter.
pub struct Edge {
    pub id: EdgeId,
    pub from: GridPos,
    pub to_node: GridPos,
    pub to_param: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Rectangular grid workspace containing nodes and edges.
pub struct Graph {
    #[serde(with = "grid_nodes_serde")]
    /// Nodes keyed by grid position.
    pub nodes: BTreeMap<GridPos, Node>,
    /// Edges keyed by their stable id.
    pub edges: BTreeMap<EdgeId, Edge>,
    /// Display name for the graph/workspace.
    #[serde(default)]
    pub name: String,
    /// Grid width in columns. Defaults to 9.
    #[serde(default = "default_grid_cols")]
    pub cols: u32,
    /// Grid height in rows. Defaults to 9.
    #[serde(default = "default_grid_rows")]
    pub rows: u32,
}

fn default_grid_cols() -> u32 {
    14
}
fn default_grid_rows() -> u32 {
    6
}

impl Graph {
    pub fn validate(&self, registry: &PieceRegistry) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::<Diagnostic>::new();

        for (pos, node) in &self.nodes {
            if registry.get(node.piece_id.as_str()).is_none() {
                diagnostics.push(Diagnostic::error(
                    DiagnosticKind::UnknownPiece {
                        piece_id: node.piece_id.clone(),
                    },
                    Some(*pos),
                ));
            }

            if !self.in_bounds(pos) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticKind::InvalidOperation {
                        reason: format!(
                            "node out of bounds at ({}, {}), allowed cols=[0..{}), rows=[0..{})",
                            pos.col, pos.row, self.cols, self.rows
                        ),
                    },
                    Some(*pos),
                ));
            }
        }

        let mut incoming_slots = BTreeSet::<(GridPos, String)>::new();
        for edge in self.edges.values() {
            let Some(from_node) = self.nodes.get(&edge.from) else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticKind::UnknownNode { pos: edge.from },
                        Some(edge.to_node),
                    )
                    .with_edge(edge.id.clone()),
                );
                continue;
            };
            let Some(to_node) = self.nodes.get(&edge.to_node) else {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticKind::UnknownNode { pos: edge.to_node },
                        Some(edge.to_node),
                    )
                    .with_edge(edge.id.clone()),
                );
                continue;
            };
            if registry.get(from_node.piece_id.as_str()).is_none() {
                continue;
            }
            let Some(to_piece) = registry.get(to_node.piece_id.as_str()) else {
                continue;
            };

            if !incoming_slots.insert((edge.to_node, edge.to_param.clone())) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticKind::DuplicateConnection {
                            to_node: edge.to_node,
                            to_param: edge.to_param.clone(),
                        },
                        Some(edge.to_node),
                    )
                    .with_edge(edge.id.clone()),
                );
            }

            if !to_piece
                .def()
                .params
                .iter()
                .any(|param| param.id == edge.to_param)
            {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticKind::UnknownParam {
                            piece_id: to_piece.def().id.clone(),
                            param: edge.to_param.clone(),
                        },
                        Some(edge.to_node),
                    )
                    .with_edge(edge.id.clone()),
                );
            }
        }

        for (pos, node) in &self.nodes {
            let Some(piece) = registry.get(node.piece_id.as_str()) else {
                continue;
            };

            for inline_key in node.inline_params.keys() {
                if piece
                    .def()
                    .params
                    .iter()
                    .any(|param| &param.id == inline_key)
                {
                    continue;
                }
                diagnostics.push(Diagnostic::error(
                    DiagnosticKind::UnknownParam {
                        piece_id: piece.def().id.clone(),
                        param: inline_key.clone(),
                    },
                    Some(*pos),
                ));
            }

            for side_key in node.input_sides.keys() {
                if piece.def().params.iter().any(|param| &param.id == side_key) {
                    continue;
                }
                diagnostics.push(Diagnostic::error(
                    DiagnosticKind::UnknownParam {
                        piece_id: piece.def().id.clone(),
                        param: side_key.clone(),
                    },
                    Some(*pos),
                ));
            }
        }

        diagnostics
    }

    /// Compute the set of positions reachable by walking edges **backward**
    /// from the given root positions (typically terminal nodes).
    pub fn reachable_nodes(&self, terminals: &[GridPos]) -> BTreeSet<GridPos> {
        let mut reachable = BTreeSet::new();
        let mut frontier: Vec<GridPos> = terminals.to_vec();
        while let Some(next) = frontier.pop() {
            if !reachable.insert(next) {
                continue;
            }
            for edge in self.edges.values() {
                if edge.to_node == next {
                    frontier.push(edge.from);
                }
            }
        }
        reachable
    }

    fn in_bounds(&self, pos: &GridPos) -> bool {
        let cols = self.cols as i32;
        let rows = self.rows as i32;
        (0..cols).contains(&pos.col) && (0..rows).contains(&pos.row)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchPlaceEntry {
    pub position: GridPos,
    pub piece_id: String,
    #[serde(default)]
    pub inline_params: BTreeMap<String, Value>,
    #[serde(default)]
    pub input_sides: BTreeMap<String, TileSide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_side: Option<TileSide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchPlaceEdge {
    pub from: GridPos,
    pub to_node: GridPos,
    pub to_param: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
/// Canonical mutation language understood by the Tessera engine.
pub enum GraphOp {
    /// Place a new node at an empty grid position.
    NodePlace {
        position: GridPos,
        piece_id: String,
        #[serde(default)]
        inline_params: BTreeMap<String, Value>,
    },
    /// Place multiple nodes atomically, with optional explicit edges and auto-wiring.
    NodeBatchPlace {
        nodes: Vec<BatchPlaceEntry>,
        #[serde(default)]
        edges: Vec<BatchPlaceEdge>,
        #[serde(default)]
        auto_wire: bool,
    },
    /// Move a node from one cell to another.
    NodeMove { from: GridPos, to: GridPos },
    /// Swap the positions of two nodes.
    NodeSwap { a: GridPos, b: GridPos },
    /// Remove a node and any affected edges.
    NodeRemove { position: GridPos },
    /// Create an explicit edge connection.
    EdgeConnect {
        #[serde(default)]
        edge_id: Option<EdgeId>,
        from: GridPos,
        to_node: GridPos,
        to_param: String,
    },
    /// Remove an edge by id.
    EdgeDisconnect { edge_id: EdgeId },
    /// Set an inline parameter value on a node.
    ParamSetInline {
        position: GridPos,
        param_id: String,
        value: Value,
    },
    /// Clear an inline parameter value from a node.
    ParamClearInline { position: GridPos, param_id: String },
    /// Override which side a param should accept input from.
    ParamSetSide {
        position: GridPos,
        param_id: String,
        side: TileSide,
    },
    /// Remove a param-side override.
    ParamClearSide { position: GridPos, param_id: String },
    /// Override which side a node should emit output from.
    OutputSetSide { position: GridPos, side: TileSide },
    /// Remove an output-side override.
    OutputClearSide { position: GridPos },
    /// Re-run adjacency-based auto-wiring for the given node.
    NodeAutoWire { position: GridPos },
    /// Set or clear a user-defined display label on a node.
    NodeSetLabel {
        position: GridPos,
        label: Option<String>,
    },
    /// Set opaque state data on a node (for stateful pieces).
    NodeSetState {
        position: GridPos,
        state: Option<Value>,
    },
    /// Resize the grid bounds.
    ResizeGrid { cols: u32, rows: u32 },
}

#[derive(Debug, Clone)]
/// Undo/redo payload produced when applying a batch of graph ops.
pub struct GraphOpRecord {
    pub do_ops: Vec<GraphOp>,
    pub undo_ops: Vec<GraphOp>,
    pub removed_edges: Vec<Edge>,
}

mod grid_nodes_serde {
    use super::{GridPos, Node};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct NodeEntry {
        position: GridPos,
        node: Node,
    }

    pub fn serialize<S>(value: &BTreeMap<GridPos, Node>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let entries = value
            .iter()
            .map(|(position, node)| NodeEntry {
                position: *position,
                node: node.clone(),
            })
            .collect::<Vec<_>>();
        entries.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<GridPos, Node>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries = Vec::<NodeEntry>::deserialize(deserializer)?;
        let mut nodes = BTreeMap::new();
        for entry in entries {
            nodes.insert(entry.position, entry.node);
        }
        Ok(nodes)
    }
}

mod input_sides_serde {
    use crate::types::TileSide;
    use serde::Deserialize;
    use serde::Deserializer;
    use std::collections::BTreeMap;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<String, TileSide>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = BTreeMap::<String, Option<TileSide>>::deserialize(deserializer)?;
        Ok(raw
            .into_iter()
            .filter_map(|(param, side)| side.map(|side| (param, side)))
            .collect())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Legacy standalone project document retained for schema-v2 migration.
pub struct ProjectDocument {
    pub schema_version: u32,
    pub name: String,
    pub graph: Graph,
}

impl ProjectDocument {
    /// Schema version used by the legacy document wrapper.
    pub const SCHEMA_VERSION: u32 = 2;

    /// Build a legacy project wrapper around a graph.
    pub fn new(name: String, graph: Graph) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            name,
            graph,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Edge, Graph, GraphOp, Node};
    use crate::ast::Expr;
    use crate::diagnostics::DiagnosticKind;
    use crate::piece::{
        ParamDef, ParamInlineMode, ParamSchema, ParamValueKind, Piece, PieceDef, PieceInputs,
    };
    use crate::piece_registry::PieceRegistry;
    use crate::types::{EdgeId, GridPos, PieceCategory, PieceSemanticKind, PortType, TileSide};
    use serde_json::Value;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn node_input_sides_drop_legacy_null_entries() {
        let node: Node = serde_json::from_value(json!({
            "piece_id": "test.node",
            "inline_params": {},
            "input_sides": {
                "left": "left",
                "pattern": null
            },
            "output_side": null,
            "label": null,
            "node_state": null
        }))
        .expect("deserialize node");

        assert_eq!(
            node.input_sides,
            BTreeMap::from([("left".into(), TileSide::LEFT)])
        );
    }

    #[test]
    fn param_set_side_serializes_without_nullable_side() {
        let op = GraphOp::ParamSetSide {
            position: GridPos { col: 1, row: 2 },
            param_id: "pattern".into(),
            side: TileSide::LEFT,
        };

        let value = serde_json::to_value(op).expect("serialize op");
        assert_eq!(value.get("side"), Some(&json!("left")));
    }

    struct TestPiece {
        def: PieceDef,
    }

    impl TestPiece {
        fn source(id: &str) -> Self {
            Self {
                def: PieceDef {
                    id: id.into(),
                    label: id.into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Intrinsic,
                    namespace: "strudel".into(),
                    params: vec![],
                    output_type: Some(PortType::new("pattern")),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }

        fn sink(id: &str) -> Self {
            Self {
                def: PieceDef {
                    id: id.into(),
                    label: id.into(),
                    category: PieceCategory::Output,
                    semantic_kind: PieceSemanticKind::Output,
                    namespace: "strudel".into(),
                    params: vec![ParamDef {
                        id: "input".into(),
                        label: "input".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Custom {
                            port_type: PortType::new("pattern"),
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
                    }],
                    output_type: None,
                    output_side: None,
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

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::nil()
        }
    }

    fn registry() -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(TestPiece::source("test.source"));
        registry.register(TestPiece::source("test.source_2"));
        registry.register(TestPiece::sink("test.output"));
        registry
    }

    fn valid_graph() -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let output_pos = GridPos { col: 1, row: 0 };
        let edge = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: output_pos,
            to_param: "input".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
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
                    output_pos,
                    Node {
                        piece_id: "test.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("input".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge.id.clone(), edge)]),
            name: "validate".into(),
            cols: 2,
            rows: 1,
        }
    }

    #[test]
    fn validate_returns_empty_for_structurally_valid_graph() {
        let graph = valid_graph();
        assert!(graph.validate(&registry()).is_empty());
    }

    #[test]
    fn validate_reports_unknown_piece() {
        let mut graph = valid_graph();
        graph
            .nodes
            .get_mut(&GridPos { col: 0, row: 0 })
            .unwrap()
            .piece_id = "missing".into();

        let diagnostics = graph.validate(&registry());
        assert!(diagnostics.iter().any(|diag| {
            matches!(
                &diag.kind,
                DiagnosticKind::UnknownPiece { piece_id } if piece_id == "missing"
            )
        }));
    }

    #[test]
    fn validate_reports_out_of_bounds_node() {
        let mut graph = valid_graph();
        let node = graph.nodes.remove(&GridPos { col: 0, row: 0 }).unwrap();
        graph.nodes.insert(GridPos { col: 5, row: 0 }, node);

        let diagnostics = graph.validate(&registry());
        assert!(
            diagnostics
                .iter()
                .any(|diag| matches!(diag.kind, DiagnosticKind::InvalidOperation { .. }))
        );
    }

    #[test]
    fn validate_reports_dangling_edge_endpoint() {
        let mut graph = valid_graph();
        graph.edges.values_mut().next().unwrap().from = GridPos { col: 9, row: 9 };

        let diagnostics = graph.validate(&registry());
        assert!(
            diagnostics
                .iter()
                .any(|diag| matches!(diag.kind, DiagnosticKind::UnknownNode { .. }))
        );
    }

    #[test]
    fn validate_reports_unknown_edge_param() {
        let mut graph = valid_graph();
        graph.edges.values_mut().next().unwrap().to_param = "other".into();

        let diagnostics = graph.validate(&registry());
        assert!(diagnostics.iter().any(|diag| {
            matches!(
                &diag.kind,
                DiagnosticKind::UnknownParam { piece_id, param }
                    if piece_id == "test.output" && param == "other"
            )
        }));
    }

    #[test]
    fn validate_reports_duplicate_connection() {
        let mut graph = valid_graph();
        graph.nodes.insert(
            GridPos { col: 0, row: 1 },
            Node {
                piece_id: "test.source_2".into(),
                inline_params: BTreeMap::new(),
                input_sides: BTreeMap::new(),
                output_side: None,
                label: None,
                node_state: None,
            },
        );
        graph.rows = 2;
        let second = Edge {
            id: EdgeId::new(),
            from: GridPos { col: 0, row: 1 },
            to_node: GridPos { col: 1, row: 0 },
            to_param: "input".into(),
        };
        graph.edges.insert(second.id.clone(), second);

        let diagnostics = graph.validate(&registry());
        assert!(diagnostics.iter().any(|diag| {
            matches!(
                &diag.kind,
                DiagnosticKind::DuplicateConnection { to_param, .. } if to_param == "input"
            )
        }));
    }

    #[test]
    fn validate_reports_unknown_inline_param() {
        let mut graph = valid_graph();
        graph
            .nodes
            .get_mut(&GridPos { col: 1, row: 0 })
            .unwrap()
            .inline_params
            .insert("mystery".into(), Value::String("x".into()));

        let diagnostics = graph.validate(&registry());
        assert!(diagnostics.iter().any(|diag| {
            matches!(
                &diag.kind,
                DiagnosticKind::UnknownParam { piece_id, param }
                    if piece_id == "test.output" && param == "mystery"
            )
        }));
    }

    #[test]
    fn validate_reports_unknown_input_side_key() {
        let mut graph = valid_graph();
        graph
            .nodes
            .get_mut(&GridPos { col: 1, row: 0 })
            .unwrap()
            .input_sides
            .insert("mystery".into(), TileSide::TOP);

        let diagnostics = graph.validate(&registry());
        assert!(diagnostics.iter().any(|diag| {
            matches!(
                &diag.kind,
                DiagnosticKind::UnknownParam { piece_id, param }
                    if piece_id == "test.output" && param == "mystery"
            )
        }));
    }

    // -- reachable_nodes tests --

    use std::collections::BTreeSet;

    fn make_node(piece_id: &str) -> Node {
        Node {
            piece_id: piece_id.into(),
            inline_params: BTreeMap::new(),
            input_sides: BTreeMap::new(),
            output_side: None,
            label: None,
            node_state: None,
        }
    }

    #[test]
    fn reachable_returns_terminals_themselves() {
        let pos = GridPos { col: 0, row: 0 };
        let graph = Graph {
            nodes: BTreeMap::from([(pos, make_node("a"))]),
            edges: BTreeMap::new(),
            name: "test".into(),
            cols: 1,
            rows: 1,
        };
        assert_eq!(graph.reachable_nodes(&[pos]), BTreeSet::from([pos]));
    }

    #[test]
    fn reachable_follows_edges_backward() {
        let p0 = GridPos { col: 0, row: 0 };
        let p1 = GridPos { col: 1, row: 0 };
        let p2 = GridPos { col: 2, row: 0 };
        let edge_01 = EdgeId::new();
        let edge_12 = EdgeId::new();

        let graph = Graph {
            nodes: BTreeMap::from([
                (p0, make_node("src")),
                (p1, make_node("mid")),
                (p2, make_node("out")),
            ]),
            edges: BTreeMap::from([
                (
                    edge_01.clone(),
                    Edge {
                        id: edge_01,
                        from: p0,
                        to_node: p1,
                        to_param: "in".into(),
                    },
                ),
                (
                    edge_12.clone(),
                    Edge {
                        id: edge_12,
                        from: p1,
                        to_node: p2,
                        to_param: "in".into(),
                    },
                ),
            ]),
            name: "test".into(),
            cols: 3,
            rows: 1,
        };

        assert_eq!(graph.reachable_nodes(&[p2]), BTreeSet::from([p0, p1, p2]));
    }

    #[test]
    fn reachable_excludes_disconnected_nodes() {
        let p0 = GridPos { col: 0, row: 0 };
        let p1 = GridPos { col: 1, row: 0 };
        let p_isolated = GridPos { col: 2, row: 0 };

        let edge_id = EdgeId::new();
        let graph = Graph {
            nodes: BTreeMap::from([
                (p0, make_node("src")),
                (p1, make_node("out")),
                (p_isolated, make_node("lonely")),
            ]),
            edges: BTreeMap::from([(
                edge_id.clone(),
                Edge {
                    id: edge_id,
                    from: p0,
                    to_node: p1,
                    to_param: "in".into(),
                },
            )]),
            name: "test".into(),
            cols: 3,
            rows: 1,
        };

        let reachable = graph.reachable_nodes(&[p1]);
        assert_eq!(reachable, BTreeSet::from([p0, p1]));
        assert!(!reachable.contains(&p_isolated));
    }

    #[test]
    fn reachable_empty_terminals_returns_empty() {
        let graph = Graph {
            nodes: BTreeMap::from([(GridPos { col: 0, row: 0 }, make_node("a"))]),
            edges: BTreeMap::new(),
            name: "test".into(),
            cols: 1,
            rows: 1,
        };
        assert!(graph.reachable_nodes(&[]).is_empty());
    }
}
