use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{EdgeId, GridPos, PortType, TileSide};

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
    PieceSemantic {
        piece_id: String,
        code: String,
        message: String,
    },
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
    DuplicateInputSide {
        side: TileSide,
        params: Vec<String>,
    },
    Cycle {
        involved: Vec<GridPos>,
    },
    NoOutputNode,
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

    pub fn info(kind: DiagnosticKind, site: Option<GridPos>) -> Self {
        Self {
            kind,
            site,
            edge_id: None,
            severity: DiagnosticSeverity::Info,
        }
    }

    pub fn piece_semantic(
        severity: DiagnosticSeverity,
        piece_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        site: Option<GridPos>,
    ) -> Self {
        let kind = DiagnosticKind::PieceSemantic {
            piece_id: piece_id.into(),
            code: code.into(),
            message: message.into(),
        };

        match severity {
            DiagnosticSeverity::Error => Self::error(kind, site),
            DiagnosticSeverity::Warning => Self::warning(kind, site),
            DiagnosticSeverity::Info => Self::info(kind, site),
        }
    }

    pub fn piece_semantic_error(
        piece_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        site: Option<GridPos>,
    ) -> Self {
        Self::piece_semantic(DiagnosticSeverity::Error, piece_id, code, message, site)
    }

    pub fn piece_semantic_warning(
        piece_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        site: Option<GridPos>,
    ) -> Self {
        Self::piece_semantic(DiagnosticSeverity::Warning, piece_id, code, message, site)
    }

    pub fn piece_semantic_info(
        piece_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        site: Option<GridPos>,
    ) -> Self {
        Self::piece_semantic(DiagnosticSeverity::Info, piece_id, code, message, site)
    }

    pub fn with_edge(mut self, edge_id: EdgeId) -> Self {
        self.edge_id = Some(edge_id);
        self
    }
}
