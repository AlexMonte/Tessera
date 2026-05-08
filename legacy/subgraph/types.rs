use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::graph::Graph;
use crate::types::{GridPos, PieceCategory, PortRole, PortType};

pub const SUBGRAPH_INPUT_1_ID: &str = "tessera.subgraph_input_1";
pub const SUBGRAPH_INPUT_2_ID: &str = "tessera.subgraph_input_2";
pub const SUBGRAPH_INPUT_3_ID: &str = "tessera.subgraph_input_3";
pub const SUBGRAPH_OUTPUT_ID: &str = "tessera.subgraph_output";

/// Stored definition for a reusable subgraph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubgraphDef {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<PieceCategory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub graph: Graph,
}

/// One declared input boundary in a subgraph signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubgraphInput {
    /// Stable input slot number exposed by the boundary marker piece.
    pub slot: u8,
    /// Position of the input marker inside the subgraph graph.
    pub pos: GridPos,
    /// Host-facing label for the input.
    pub label: String,
    /// Effective port type declared by the boundary marker.
    pub port_type: PortType,
    /// Semantic role expected for values flowing into this input.
    #[serde(default, skip_serializing_if = "PortRole::is_value")]
    pub role: PortRole,
    /// Whether callers must supply a value.
    pub required: bool,
    /// Whether this input acts as the receiver in host-specific lowering.
    pub is_receiver: bool,
    /// Optional default value surfaced to the host.
    pub default_value: Option<Value>,
}

/// Stable subgraph boundary facts exposed to host crates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubgraphSignature {
    /// Ordered input boundary declarations.
    pub inputs: Vec<SubgraphInput>,
    /// Position of the unique output marker node.
    pub output_pos: GridPos,
    /// Effective type of the value wired into the output marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_type: Option<PortType>,
    /// Semantic role of the emitted value.
    #[serde(default, skip_serializing_if = "PortRole::is_value")]
    pub output_role: PortRole,
    /// Optional palette category for generated pieces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<PieceCategory>,
    /// Optional inline description for palette/help surfaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Search/discovery tags for generated pieces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}
