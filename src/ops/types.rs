use crate::graph::Edge;
use serde::{Deserialize, Serialize};

use crate::graph::GraphOp;
use crate::types::{DomainBridgeKind, EdgeId, GridPos, TileSide};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RepairSuggestion {
    /// Move a node to a new position to satisfy adjacency.
    MoveNode { node: GridPos, to: GridPos },
    /// Rotate the source node's output side to face the target.
    SetOutputSide { position: GridPos, side: TileSide },
    /// Move a param's input side so it faces the source.
    SetParamSide {
        position: GridPos,
        param_id: String,
        side: TileSide,
    },
    /// Disconnect an existing edge to free up a param slot.
    DisconnectEdge { edge_id: EdgeId },
}

impl RepairSuggestion {
    /// Convert this suggestion into the graph operations needed to apply it.
    pub fn to_ops(&self) -> Vec<GraphOp> {
        match self {
            RepairSuggestion::MoveNode { node, to } => vec![GraphOp::NodeMove {
                from: *node,
                to: *to,
            }],
            RepairSuggestion::SetOutputSide { position, side } => {
                vec![GraphOp::OutputSetSide {
                    position: *position,
                    side: *side,
                }]
            }
            RepairSuggestion::SetParamSide {
                position,
                param_id,
                side,
            } => vec![GraphOp::ParamSetSide {
                position: *position,
                param_id: param_id.clone(),
                side: *side,
            }],
            RepairSuggestion::DisconnectEdge { edge_id } => vec![GraphOp::EdgeDisconnect {
                edge_id: edge_id.clone(),
            }],
        }
    }
}

#[derive(Debug, Default)]
pub struct ApplyOpsOutcome {
    /// Edges removed as a side effect of the mutation batch.
    pub removed_edges: Vec<Edge>,
    /// Canonicalized ops that were actually applied.
    pub applied_ops: Vec<GraphOp>,
    /// Inverse ops that can restore the previous graph state.
    pub undo_ops: Vec<GraphOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Structured reason why an attempted edge connection was rejected.
pub enum EdgeConnectProbeReason {
    UnknownSourceNode,
    UnknownTargetNode,
    UnknownSourcePiece,
    UnknownTargetPiece,
    UnknownTargetParam,
    NotAdjacent,
    SideMismatch,
    OutputFromTerminal,
    NoParamOnTargetSide,
    TargetParamOccupied,
    TypeMismatch,
    UnsupportedDomain,
    NoCompatibleParam,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Result of probing an edge connection, with optional repair suggestions.
pub struct EdgeTargetParamProbe {
    pub to_param: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implicit_bridge: Option<DomainBridgeKind>,
    pub reason: Option<EdgeConnectProbeReason>,
    pub detail: Option<String>,
    /// Machine-readable repair suggestions. Each entry maps to graph ops via `to_ops()`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<RepairSuggestion>,
}

impl EdgeTargetParamProbe {
    pub(super) fn accept(param: String) -> Self {
        Self::accept_with_bridge(param, None)
    }

    pub(super) fn accept_with_bridge(
        param: String,
        implicit_bridge: Option<DomainBridgeKind>,
    ) -> Self {
        Self {
            to_param: Some(param),
            implicit_bridge,
            reason: None,
            detail: None,
            suggestions: Vec::new(),
        }
    }

    pub(super) fn reject(reason: EdgeConnectProbeReason, detail: impl Into<String>) -> Self {
        Self {
            to_param: None,
            implicit_bridge: None,
            reason: Some(reason),
            detail: Some(detail.into()),
            suggestions: Vec::new(),
        }
    }

    pub(super) fn reject_with(
        reason: EdgeConnectProbeReason,
        detail: impl Into<String>,
        suggestions: Vec<RepairSuggestion>,
    ) -> Self {
        Self {
            to_param: None,
            implicit_bridge: None,
            reason: Some(reason),
            detail: Some(detail.into()),
            suggestions,
        }
    }
}
