//! Piece and parameter definitions that describe what can live on a graph.
//!
//! Architectural rule: the Tessera core owns graph structure, validation, and
//! type inference. Host crates own semantic compilation from analyzed graphs
//! into their own domain representations.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::analysis::AnalyzedNode;
use crate::diagnostics::Diagnostic;
use crate::types::{
    DELAY_PIECE_ID, GridPos, PieceCategory, PieceSemanticKind, PortRole, PortType, Rational,
    TileSide,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamValueKind {
    Number,
    Text,
    Bool,
    Rational,
    Json,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamInlineMode {
    Literal,
    Ident,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParamTextSemantics(String);

impl ParamTextSemantics {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn plain() -> Self {
        Self::new("plain")
    }

    pub fn mini() -> Self {
        Self::new("mini")
    }

    pub fn rhythm() -> Self {
        Self::new("rhythm")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_plain(&self) -> bool {
        self.as_str() == "plain"
    }
}

impl Default for ParamTextSemantics {
    fn default() -> Self {
        Self::plain()
    }
}

impl From<&str> for ParamTextSemantics {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ParamTextSemantics {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for ParamTextSemantics {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ParamTextSemantics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParamSchema {
    Number {
        default: f64,
        min: Option<f64>,
        max: Option<f64>,
        can_inline: bool,
    },
    Text {
        default: String,
        can_inline: bool,
    },
    Enum {
        options: Vec<String>,
        default: String,
        can_inline: bool,
    },
    Bool {
        default: bool,
        can_inline: bool,
    },
    Rational {
        default: Rational,
        can_inline: bool,
    },
    Custom {
        port_type: PortType,
        value_kind: ParamValueKind,
        default: Option<Value>,
        can_inline: bool,
        inline_mode: ParamInlineMode,
        min: Option<f64>,
        max: Option<f64>,
    },
}

impl ParamSchema {
    pub fn accepts(&self, port_type: &PortType) -> bool {
        self.resolve_connection(port_type).is_ok()
    }

    pub(crate) fn resolve_connection(
        &self,
        port_type: &PortType,
    ) -> Result<crate::types::PortTypeConnection, crate::types::PortTypeConnectionError> {
        match self {
            ParamSchema::Number { .. } => PortType::number().resolve_connection(port_type),
            ParamSchema::Text { .. } | ParamSchema::Enum { .. } => {
                PortType::text().resolve_connection(port_type)
            }
            ParamSchema::Bool { .. } => PortType::bool().resolve_connection(port_type),
            ParamSchema::Rational { .. } => PortType::rational().resolve_connection(port_type),
            ParamSchema::Custom {
                port_type: expected,
                ..
            } => expected.resolve_connection(port_type),
        }
    }

    pub fn can_inline(&self) -> bool {
        match self {
            ParamSchema::Number { can_inline, .. }
            | ParamSchema::Text { can_inline, .. }
            | ParamSchema::Enum { can_inline, .. }
            | ParamSchema::Bool { can_inline, .. }
            | ParamSchema::Rational { can_inline, .. } => *can_inline,
            ParamSchema::Custom { can_inline, .. } => *can_inline,
        }
    }

    pub fn default_value(&self) -> Option<Value> {
        match self {
            ParamSchema::Number { default, .. } => Some(Value::from(*default)),
            ParamSchema::Text { default, .. } | ParamSchema::Enum { default, .. } => {
                Some(Value::String(default.clone()))
            }
            ParamSchema::Bool { default, .. } => Some(Value::Bool(*default)),
            ParamSchema::Rational { default, .. } => Some(Value::String(default.to_string())),
            ParamSchema::Custom { default, .. } => default.clone(),
        }
    }

    pub fn expected_port_type(&self) -> PortType {
        match self {
            ParamSchema::Number { .. } => PortType::number(),
            ParamSchema::Text { .. } | ParamSchema::Enum { .. } => PortType::text(),
            ParamSchema::Bool { .. } => PortType::bool(),
            ParamSchema::Rational { .. } => PortType::rational(),
            ParamSchema::Custom { port_type, .. } => port_type.clone(),
        }
    }

    pub fn infer_inline_port_type(&self, value: &Value) -> Option<PortType> {
        if !self.validate_inline_value(value) {
            return None;
        }

        Some(match self {
            ParamSchema::Number { .. } => PortType::number(),
            ParamSchema::Text { .. } | ParamSchema::Enum { .. } => PortType::text(),
            ParamSchema::Bool { .. } => PortType::bool(),
            ParamSchema::Rational { .. } => PortType::rational(),
            ParamSchema::Custom {
                port_type,
                value_kind,
                inline_mode,
                ..
            } => {
                if matches!(inline_mode, ParamInlineMode::Ident) {
                    return Some(port_type.clone());
                }

                match value_kind {
                    ParamValueKind::Number => PortType::number(),
                    ParamValueKind::Text => PortType::text(),
                    ParamValueKind::Bool => PortType::bool(),
                    ParamValueKind::Rational => PortType::rational(),
                    ParamValueKind::Json => {
                        infer_port_type_from_json(value).unwrap_or_else(|| port_type.clone())
                    }
                    ParamValueKind::None => port_type.clone(),
                }
            }
        })
    }

    pub fn resolved_port_type(&self, inline_value: Option<&Value>) -> Option<PortType> {
        if let Some(value) = inline_value {
            return self.infer_inline_port_type(value);
        }

        match self {
            ParamSchema::Number { .. } => Some(PortType::number()),
            ParamSchema::Text { .. } | ParamSchema::Enum { .. } => Some(PortType::text()),
            ParamSchema::Bool { .. } => Some(PortType::bool()),
            ParamSchema::Rational { .. } => Some(PortType::rational()),
            ParamSchema::Custom { default, .. } => default
                .as_ref()
                .and_then(|value| self.infer_inline_port_type(value)),
        }
    }

    pub fn validate_inline_value(&self, value: &Value) -> bool {
        match self {
            ParamSchema::Number { min, max, .. } => validate_number(value, *min, *max),
            ParamSchema::Text { .. } => value.is_string(),
            ParamSchema::Enum { options, .. } => value
                .as_str()
                .is_some_and(|candidate| options.contains(&candidate.to_string())),
            ParamSchema::Bool { .. } => value.is_boolean(),
            ParamSchema::Rational { .. } => value
                .as_str()
                .is_some_and(|candidate| Rational::parse(candidate).is_some()),
            ParamSchema::Custom {
                value_kind,
                min,
                max,
                inline_mode,
                ..
            } => {
                if matches!(inline_mode, ParamInlineMode::Ident) {
                    return value.as_str().is_some_and(is_valid_ident_path);
                }

                match value_kind {
                    ParamValueKind::Number => validate_number(value, *min, *max),
                    ParamValueKind::Text => value.is_string(),
                    ParamValueKind::Bool => value.is_boolean(),
                    ParamValueKind::Rational => value
                        .as_str()
                        .is_some_and(|candidate| Rational::parse(candidate).is_some()),
                    ParamValueKind::Json => true,
                    ParamValueKind::None => false,
                }
            }
        }
    }
}

fn validate_number(value: &Value, min: Option<f64>, max: Option<f64>) -> bool {
    let Some(number) = value.as_f64() else {
        return false;
    };
    if let Some(min) = min
        && number < min
    {
        return false;
    }
    if let Some(max) = max
        && number > max
    {
        return false;
    }
    true
}

fn infer_port_type_from_json(value: &Value) -> Option<PortType> {
    match value {
        Value::Number(_) => Some(PortType::number()),
        Value::String(candidate) if Rational::parse(candidate).is_some() => {
            Some(PortType::rational())
        }
        Value::String(_) => Some(PortType::text()),
        Value::Bool(_) => Some(PortType::bool()),
        _ => None,
    }
}

pub fn is_valid_ident_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

pub fn is_valid_ident_path(path: &str) -> bool {
    let mut segments = path.split('.');
    let Some(first) = segments.next() else {
        return false;
    };
    if !is_valid_ident_segment(first) {
        return false;
    }
    segments.all(is_valid_ident_segment)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    pub id: String,
    pub label: String,
    pub side: TileSide,
    pub schema: ParamSchema,
    #[serde(default)]
    pub text_semantics: ParamTextSemantics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variadic_group: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "PortRole::is_value")]
    pub role: PortRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PieceDef {
    pub id: String,
    pub label: String,
    pub category: PieceCategory,
    pub semantic_kind: PieceSemanticKind,
    pub namespace: String,
    #[serde(default)]
    pub params: Vec<ParamDef>,
    pub output_type: Option<PortType>,
    pub output_side: Option<TileSide>,
    #[serde(default, skip_serializing_if = "PortRole::is_value")]
    pub output_role: PortRole,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceExecutionKind {
    Pure,
    State,
    Connector,
    Boundary,
}

impl PieceDef {
    pub fn is_output(&self) -> bool {
        matches!(self.semantic_kind, PieceSemanticKind::Output)
    }

    pub fn is_connector(&self) -> bool {
        matches!(self.category, PieceCategory::Connector)
            || matches!(self.semantic_kind, PieceSemanticKind::Connector)
    }

    pub fn has_output(&self) -> bool {
        self.output_type.is_some() || self.is_connector()
    }

    pub fn execution_kind(&self) -> PieceExecutionKind {
        if self.is_connector() {
            PieceExecutionKind::Connector
        } else if self.is_output() {
            PieceExecutionKind::Boundary
        } else if self.id == DELAY_PIECE_ID {
            PieceExecutionKind::State
        } else {
            PieceExecutionKind::Pure
        }
    }

    pub fn connector_param(&self) -> Option<&ParamDef> {
        if !self.is_connector() {
            return None;
        }

        match self.params.as_slice() {
            [param] => Some(param),
            _ => None,
        }
    }

    pub fn is_visible_in_namespace(&self, namespace: &str) -> bool {
        self.namespace == "core"
            || self.namespace == namespace
            || matches!(self.semantic_kind, PieceSemanticKind::Trick)
    }
}

pub trait Piece: Send + Sync {
    fn def(&self) -> &PieceDef;

    fn execution_kind(&self) -> PieceExecutionKind {
        self.def().execution_kind()
    }

    fn infer_output_type(
        &self,
        _input_types: &BTreeMap<String, PortType>,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Option<PortType> {
        self.def().output_type.clone()
    }

    fn validate_analysis(&self, _position: GridPos, _node: &AnalyzedNode) -> Vec<Diagnostic> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        ParamDef, ParamInlineMode, ParamSchema, ParamTextSemantics, ParamValueKind, Piece,
        PieceDef, PieceExecutionKind, is_valid_ident_path, is_valid_ident_segment,
    };
    use crate::types::{
        DELAY_PIECE_ID, PieceCategory, PieceSemanticKind, PortType, Rational, TileSide,
    };
    use serde_json::json;

    struct TestPiece {
        def: PieceDef,
    }

    impl Piece for TestPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }
    }

    #[test]
    fn ident_validation_accepts_dotted_paths() {
        assert!(is_valid_ident_segment("foo"));
        assert!(is_valid_ident_path("foo.bar"));
        assert!(!is_valid_ident_path("foo.123"));
        assert!(!is_valid_ident_path("foo..bar"));
    }

    #[test]
    fn param_def_text_semantics_defaults_to_plain_when_omitted() {
        let value = json!({
            "id": "value",
            "label": "value",
            "side": "bottom",
            "schema": {
                "kind": "text",
                "default": "",
                "can_inline": true
            },
            "required": false
        });

        let parsed: ParamDef = serde_json::from_value(value).expect("deserialize param def");
        assert_eq!(parsed.text_semantics, ParamTextSemantics::plain());
    }

    #[test]
    fn rational_schema_validates_inline_values() {
        let schema = ParamSchema::Rational {
            default: Rational::new(1, 4).unwrap(),
            can_inline: true,
        };

        assert!(schema.validate_inline_value(&json!("1/4")));
        assert!(!schema.validate_inline_value(&json!("1/0")));
        assert_eq!(schema.expected_port_type(), PortType::rational());
        assert_eq!(schema.default_value(), Some(json!("1/4")));
    }

    #[test]
    fn custom_rational_schema_infers_rational_port_type() {
        let schema = ParamSchema::Custom {
            port_type: PortType::rational(),
            value_kind: ParamValueKind::Rational,
            default: Some(json!("3/2")),
            can_inline: true,
            inline_mode: ParamInlineMode::Literal,
            min: None,
            max: None,
        };

        assert_eq!(
            schema.infer_inline_port_type(&json!("1/8")),
            Some(PortType::rational())
        );
        assert_eq!(schema.resolved_port_type(None), Some(PortType::rational()));
    }

    #[test]
    fn bool_schema_rejects_number_without_fallback() {
        let schema = ParamSchema::Bool {
            default: false,
            can_inline: true,
        };

        assert!(schema.resolve_connection(&PortType::bool()).is_ok());
        assert!(schema.resolve_connection(&PortType::number()).is_err());
    }

    #[test]
    fn piece_def_output_detection_is_explicit() {
        let output = PieceDef {
            id: "test.output".into(),
            label: "output".into(),
            category: PieceCategory::Output,
            semantic_kind: PieceSemanticKind::Output,
            namespace: "core".into(),
            params: vec![],
            output_type: None,
            output_side: None,
            output_role: Default::default(),
            description: None,
            tags: vec![],
        };
        let transform = PieceDef {
            id: "test.transform".into(),
            label: "transform".into(),
            category: PieceCategory::Transform,
            semantic_kind: PieceSemanticKind::Operator,
            namespace: "core".into(),
            params: vec![],
            output_type: Some(PortType::number()),
            output_side: Some(TileSide::RIGHT),
            output_role: Default::default(),
            description: None,
            tags: vec![],
        };

        assert!(output.is_output());
        assert!(!transform.is_output());
    }

    #[test]
    fn piece_execution_kind_tracks_kernel_taxonomy() {
        let pure = PieceDef {
            id: "test.literal".into(),
            label: "literal".into(),
            category: PieceCategory::Constant,
            semantic_kind: PieceSemanticKind::Literal,
            namespace: "core".into(),
            params: vec![],
            output_type: Some(PortType::number()),
            output_side: Some(TileSide::RIGHT),
            output_role: Default::default(),
            description: None,
            tags: vec![],
        };
        let output = PieceDef {
            id: "test.output".into(),
            label: "output".into(),
            category: PieceCategory::Output,
            semantic_kind: PieceSemanticKind::Output,
            namespace: "core".into(),
            params: vec![],
            output_type: None,
            output_side: None,
            output_role: Default::default(),
            description: None,
            tags: vec![],
        };
        let connector = PieceDef {
            id: "test.connector".into(),
            label: "connector".into(),
            category: PieceCategory::Connector,
            semantic_kind: PieceSemanticKind::Connector,
            namespace: "core".into(),
            params: vec![ParamDef {
                id: "value".into(),
                label: "value".into(),
                side: TileSide::LEFT,
                schema: ParamSchema::Custom {
                    port_type: PortType::any(),
                    value_kind: ParamValueKind::Json,
                    default: None,
                    can_inline: false,
                    inline_mode: ParamInlineMode::Literal,
                    min: None,
                    max: None,
                },
                text_semantics: Default::default(),
                variadic_group: None,
                required: false,
                role: Default::default(),
            }],
            output_type: None,
            output_side: Some(TileSide::RIGHT),
            output_role: Default::default(),
            description: None,
            tags: vec![],
        };
        let state = PieceDef {
            id: DELAY_PIECE_ID.into(),
            label: "delay".into(),
            category: PieceCategory::Control,
            semantic_kind: PieceSemanticKind::Construct,
            namespace: "core".into(),
            params: vec![],
            output_type: Some(PortType::any()),
            output_side: Some(TileSide::RIGHT),
            output_role: Default::default(),
            description: None,
            tags: vec![],
        };

        assert_eq!(pure.execution_kind(), PieceExecutionKind::Pure);
        assert_eq!(state.execution_kind(), PieceExecutionKind::State);
        assert_eq!(output.execution_kind(), PieceExecutionKind::Boundary);
        assert_eq!(connector.execution_kind(), PieceExecutionKind::Connector);
    }

    #[test]
    fn piece_trait_defaults_to_declared_output_type() {
        let piece = TestPiece {
            def: PieceDef {
                id: "test.number".into(),
                label: "number".into(),
                category: PieceCategory::Constant,
                semantic_kind: PieceSemanticKind::Literal,
                namespace: "core".into(),
                params: vec![],
                output_type: Some(PortType::number()),
                output_side: Some(TileSide::RIGHT),
                output_role: Default::default(),
                description: None,
                tags: vec![],
            },
        };

        assert_eq!(
            piece.infer_output_type(&BTreeMap::new(), &BTreeMap::new()),
            Some(PortType::number())
        );
    }
}
