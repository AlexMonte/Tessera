//! Piece and parameter definitions that describe what can live on a graph.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

use crate::code_expr::CodeExpr;
use crate::types::{PieceCategory, PortType, TileSide};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Primitive value shape used for validating inline/custom params.
pub enum ParamValueKind {
    Number,
    Text,
    Bool,
    Json,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// How an inline JSON value should be converted into a [`CodeExpr`].
pub enum ParamInlineMode {
    Literal,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
/// Optional semantic hint for richer text editing surfaces.
pub enum ParamTextSemantics {
    #[default]
    Plain,
    Mini,
    Rhythm,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
/// Type and validation rules for one piece parameter.
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
    /// Dropdown selection from a fixed set of options.
    Enum {
        options: Vec<String>,
        default: String,
        can_inline: bool,
    },
    /// Boolean toggle.
    Bool {
        default: bool,
        can_inline: bool,
    },
    /// User-defined port typing and inline/default expression semantics.
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
    /// Return whether this parameter can accept a connection of the given port type.
    pub fn accepts(&self, port_type: &PortType) -> bool {
        match self {
            ParamSchema::Number { .. } => PortType::number().accepts(port_type),
            ParamSchema::Text { .. } => PortType::text().accepts(port_type),
            ParamSchema::Enum { .. } => PortType::text().accepts(port_type),
            ParamSchema::Bool { .. } => {
                PortType::bool().accepts(port_type) || PortType::number().accepts(port_type)
            }
            ParamSchema::Custom {
                port_type: expected,
                ..
            } => expected.accepts(port_type),
        }
    }

    /// Return whether the parameter may be supplied inline on the node.
    pub fn can_inline(&self) -> bool {
        match self {
            ParamSchema::Number { can_inline, .. }
            | ParamSchema::Text { can_inline, .. }
            | ParamSchema::Enum { can_inline, .. }
            | ParamSchema::Bool { can_inline, .. } => *can_inline,
            ParamSchema::Custom { can_inline, .. } => *can_inline,
        }
    }

    /// Convert the schema's default into a compile-time expression, when one exists.
    pub fn default_expr(&self) -> Option<CodeExpr> {
        match self {
            ParamSchema::Number { default, .. } => Some(CodeExpr::Literal(Value::Number(
                Number::from_f64(*default).unwrap_or_else(|| Number::from(0)),
            ))),
            ParamSchema::Text { default, .. } => {
                Some(CodeExpr::Literal(Value::String(default.clone())))
            }
            ParamSchema::Enum { default, .. } => {
                Some(CodeExpr::Literal(Value::String(default.clone())))
            }
            ParamSchema::Bool { default, .. } => Some(CodeExpr::Literal(Value::Bool(*default))),
            ParamSchema::Custom {
                default,
                inline_mode,
                ..
            } => default
                .as_ref()
                .and_then(|value| value_to_expr(value, inline_mode)),
        }
    }

    /// Return the port type expected by this parameter.
    pub fn expected_port_type(&self) -> PortType {
        match self {
            ParamSchema::Number { .. } => PortType::number(),
            ParamSchema::Text { .. } => PortType::text(),
            ParamSchema::Enum { .. } => PortType::text(),
            ParamSchema::Bool { .. } => PortType::bool(),
            ParamSchema::Custom { port_type, .. } => port_type.clone(),
        }
    }

    /// Validate a JSON inline value against this schema.
    pub fn validate_inline_value(&self, value: &Value) -> bool {
        match self {
            ParamSchema::Number { min, max, .. } => validate_number(value, *min, *max),
            ParamSchema::Text { .. } => value.is_string(),
            ParamSchema::Enum { options, .. } => value
                .as_str()
                .map_or(false, |s| options.contains(&s.to_string())),
            ParamSchema::Bool { .. } => value.is_boolean(),
            ParamSchema::Custom {
                value_kind,
                min,
                max,
                ..
            } => match value_kind {
                ParamValueKind::Number => validate_number(value, *min, *max),
                ParamValueKind::Text => value.is_string(),
                ParamValueKind::Bool => value.is_boolean(),
                ParamValueKind::Json => true,
                ParamValueKind::None => false,
            },
        }
    }

    /// Convert an already-validated inline value into a compile-time expression.
    pub fn inline_expr(&self, value: &Value) -> Option<CodeExpr> {
        if !self.validate_inline_value(value) {
            return None;
        }
        match self {
            ParamSchema::Custom { inline_mode, .. } => value_to_expr(value, inline_mode),
            _ => Some(CodeExpr::Literal(value.clone())),
        }
    }
}

fn validate_number(value: &Value, min: Option<f64>, max: Option<f64>) -> bool {
    let Some(number) = value.as_f64() else {
        return false;
    };
    if let Some(min) = min {
        if number < min {
            return false;
        }
    }
    if let Some(max) = max {
        if number > max {
            return false;
        }
    }
    true
}

fn value_to_expr(value: &Value, inline_mode: &ParamInlineMode) -> Option<CodeExpr> {
    match inline_mode {
        ParamInlineMode::Literal => Some(CodeExpr::Literal(value.clone())),
        ParamInlineMode::Raw => value.as_str().map(|raw| CodeExpr::Raw(raw.to_string())),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// One input parameter on a piece definition.
pub struct ParamDef {
    pub id: String,
    pub label: String,
    pub side: TileSide,
    pub schema: ParamSchema,
    #[serde(default)]
    pub text_semantics: ParamTextSemantics,
    /// Optional grouping key for variadic fan-in. Params that share this key are
    /// exposed as an ordered vector to pieces during compile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variadic_group: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Static description of a placeable piece.
pub struct PieceDef {
    pub id: String,
    pub label: String,
    pub category: PieceCategory,
    #[serde(default)]
    pub params: Vec<ParamDef>,
    pub output_type: Option<PortType>,
    pub output_side: Option<TileSide>,
    pub description: Option<String>,
}

impl PieceDef {
    /// Return whether this piece terminates compilation rather than producing output.
    pub fn is_terminal(&self) -> bool {
        self.output_type.is_none() || matches!(self.category, PieceCategory::Output)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
/// Inputs resolved for a node at compile time.
pub struct PieceInputs {
    #[serde(default)]
    pub scalar: BTreeMap<String, CodeExpr>,
    #[serde(default)]
    pub variadic: BTreeMap<String, Vec<CodeExpr>>,
}

impl PieceInputs {
    /// Return a resolved scalar input by id.
    pub fn get(&self, key: &str) -> Option<&CodeExpr> {
        self.scalar.get(key)
    }

    /// Return a resolved variadic input group by id.
    pub fn get_variadic(&self, key: &str) -> Option<&Vec<CodeExpr>> {
        self.variadic.get(key)
    }
}

/// Trait implemented by every placeable piece type.
pub trait Piece: Send + Sync {
    /// Return the static piece definition used by UI, validation, and compilation.
    fn def(&self) -> &PieceDef;

    /// Compile this node using its resolved graph inputs and inline params.
    fn compile(&self, inputs: &PieceInputs, inline_params: &BTreeMap<String, Value>) -> CodeExpr;

    /// Return per-side output expressions for multi-output pieces (e.g. cross connectors).
    /// When `Some`, the compiler resolves edges by matching the exit direction to the
    /// appropriate side key, falling back to the primary `compile()` result.
    fn compile_multi_output(
        &self,
        _inputs: &PieceInputs,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Option<BTreeMap<TileSide, CodeExpr>> {
        None
    }

    /// Return the initial persisted state for a stateful piece, if any.
    fn initial_state(&self) -> Option<Value> {
        None
    }

    /// Compile a stateful piece and return the next persisted state.
    fn compile_stateful(
        &self,
        inputs: &PieceInputs,
        inline_params: &BTreeMap<String, Value>,
        state: &Value,
    ) -> (CodeExpr, Value) {
        (self.compile(inputs, inline_params), state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{ParamDef, ParamSchema, ParamTextSemantics};
    use crate::types::TileSide;
    use serde_json::json;

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
        assert_eq!(parsed.text_semantics, ParamTextSemantics::Plain);
    }

    #[test]
    fn param_def_text_semantics_round_trips() {
        let param = ParamDef {
            id: "value".into(),
            label: "value".into(),
            side: TileSide::BOTTOM,
            schema: ParamSchema::Text {
                default: String::new(),
                can_inline: true,
            },
            text_semantics: ParamTextSemantics::Mini,
            variadic_group: None,
            required: false,
        };

        let value = serde_json::to_value(&param).expect("serialize param def");
        assert_eq!(value.get("text_semantics"), Some(&json!("mini")));
    }
}
