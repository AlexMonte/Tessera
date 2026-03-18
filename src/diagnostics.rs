use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{DomainBridge, EdgeId, GridPos, PortType, TileSide};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiagnosticKind {
    UnknownPiece {
        piece_id: String,
    },
    UnknownNode {
        pos: GridPos,
    },
    UnknownParam {
        piece_id: String,
        param: String,
    },
    InvalidOperation {
        reason: String,
    },
    DuplicateConnection {
        to_node: GridPos,
        to_param: String,
    },
    Cycle {
        involved: Vec<GridPos>,
    },
    NoTerminalNode,
    MultipleTerminalNodes {
        positions: Vec<GridPos>,
    },
    UnreachableNode {
        position: GridPos,
    },
    TypeMismatch {
        expected: PortType,
        got: PortType,
        param: String,
    },
    UnsupportedDomainCrossing {
        expected: PortType,
        got: PortType,
        param: String,
    },
    DelayTypeMismatch {
        default: PortType,
        feedback: PortType,
    },
    SideMismatch {
        from_pos: GridPos,
        to_pos: GridPos,
        expected_side: TileSide,
    },
    NotAdjacent {
        from_pos: GridPos,
        to_pos: GridPos,
    },
    OutputFromTerminal {
        position: GridPos,
    },
    MissingRequiredParam {
        param: String,
    },
    InlineNotAllowed {
        param: String,
    },
    InlineTypeMismatch {
        param: String,
        expected: PortType,
        got_value: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub site: Option<GridPos>,
    pub edge_id: Option<EdgeId>,
    pub severity: DiagnosticSeverity,
}

impl Diagnostic {
    pub fn error(kind: DiagnosticKind, site: Option<GridPos>) -> Self {
        Self {
            kind,
            site,
            edge_id: None,
            severity: DiagnosticSeverity::Error,
        }
    }

    pub fn warning(kind: DiagnosticKind, site: Option<GridPos>) -> Self {
        Self {
            kind,
            site,
            edge_id: None,
            severity: DiagnosticSeverity::Warning,
        }
    }

    pub fn with_edge(mut self, edge_id: EdgeId) -> Self {
        self.edge_id = Some(edge_id);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticResult {
    pub diagnostics: Vec<Diagnostic>,
    pub eval_order: Vec<GridPos>,
    /// All terminal nodes found.
    pub terminals: Vec<GridPos>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub output_types: BTreeMap<GridPos, PortType>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub domain_bridges: BTreeMap<EdgeId, DomainBridge>,
    /// Edges classified as delay (feedback) edges.
    ///
    /// These edges target a `core.delay` node's `"value"` param and are
    /// excluded from the topological sort so that cycles through delay
    /// nodes are legal.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub delay_edges: BTreeSet<EdgeId>,
}

impl SemanticResult {
    pub fn is_valid(&self) -> bool {
        let has_error = self
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error);
        !has_error && !self.terminals.is_empty()
    }
}
