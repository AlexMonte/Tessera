//! Core graph data structures and mutation records used by Tessera hosts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{EdgeId, GridPos, TileSide};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// One placed piece instance on the grid.
pub struct Node {
    /// Registered piece id, for example `strudel.fast`.
    pub piece_id: String,
    /// Inline parameter values owned directly by this node instance.
    #[serde(default)]
    pub inline_params: BTreeMap<String, Value>,
    /// Per-param side overrides applied by the editor.
    #[serde(default)]
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
                position: position.clone(),
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
