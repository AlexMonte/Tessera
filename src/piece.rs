//! Piece and parameter definitions that describe what can live on a graph.
//!
//! Architecture rule: this crate stays target-agnostic. Target-specific AST
//! rewrites or lowering quirks belong in host/target adapters, not in the
//! core piece model.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::activity::PreviewTimeline;
use crate::ast::{Expr, parse_ident_path};
use crate::types::{PieceCategory, PieceSemanticKind, PortType, TileSide};

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
/// How an inline JSON value should be converted into an [`Expr`].
pub enum ParamInlineMode {
    Literal,
    #[serde(alias = "raw")]
    Ident,
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
            ParamSchema::Bool { .. } => PortType::bool()
                .resolve_connection(port_type)
                .or_else(|_| PortType::number().resolve_connection(port_type)),
            ParamSchema::Custom {
                port_type: expected,
                ..
            } => expected.resolve_connection(port_type),
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
    pub fn default_expr(&self) -> Option<Expr> {
        match self {
            ParamSchema::Number { default, .. } => Some(Expr::float(*default)),
            ParamSchema::Text { default, .. } => Some(Expr::str_lit(default.as_str())),
            ParamSchema::Enum { default, .. } => Some(Expr::str_lit(default.as_str())),
            ParamSchema::Bool { default, .. } => Some(Expr::bool_lit(*default)),
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

    /// Infer the effective port type for a validated inline value.
    pub fn infer_inline_port_type(&self, value: &Value) -> Option<PortType> {
        if !self.validate_inline_value(value) {
            return None;
        }

        Some(match self {
            ParamSchema::Number { .. } => PortType::number(),
            ParamSchema::Text { .. } | ParamSchema::Enum { .. } => PortType::text(),
            ParamSchema::Bool { .. } => PortType::bool(),
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
                    ParamValueKind::Json => {
                        infer_port_type_from_json(value).unwrap_or_else(|| port_type.clone())
                    }
                    ParamValueKind::None => port_type.clone(),
                }
            }
        })
    }

    /// Infer the effective port type when the parameter resolves from either
    /// an inline value or its schema default.
    pub fn resolved_port_type(&self, inline_value: Option<&Value>) -> Option<PortType> {
        if let Some(value) = inline_value {
            return self.infer_inline_port_type(value);
        }

        match self {
            ParamSchema::Number { .. } => Some(PortType::number()),
            ParamSchema::Text { .. } | ParamSchema::Enum { .. } => Some(PortType::text()),
            ParamSchema::Bool { .. } => Some(PortType::bool()),
            ParamSchema::Custom { default, .. } => default
                .as_ref()
                .and_then(|value| self.infer_inline_port_type(value)),
        }
    }

    /// Validate a JSON inline value against this schema.
    pub fn validate_inline_value(&self, value: &Value) -> bool {
        match self {
            ParamSchema::Number { min, max, .. } => validate_number(value, *min, *max),
            ParamSchema::Text { .. } => value.is_string(),
            ParamSchema::Enum { options, .. } => value
                .as_str()
                .is_some_and(|s| options.contains(&s.to_string())),
            ParamSchema::Bool { .. } => value.is_boolean(),
            ParamSchema::Custom {
                value_kind,
                min,
                max,
                inline_mode,
                ..
            } => {
                if matches!(inline_mode, ParamInlineMode::Ident) {
                    return value
                        .as_str()
                        .is_some_and(|s| parse_ident_path(s).is_some());
                }
                match value_kind {
                    ParamValueKind::Number => validate_number(value, *min, *max),
                    ParamValueKind::Text => value.is_string(),
                    ParamValueKind::Bool => value.is_boolean(),
                    ParamValueKind::Json => true,
                    ParamValueKind::None => false,
                }
            }
        }
    }

    /// Convert an already-validated inline value into a compile-time expression.
    pub fn inline_expr(&self, value: &Value) -> Option<Expr> {
        if !self.validate_inline_value(value) {
            return None;
        }
        match self {
            ParamSchema::Custom { inline_mode, .. } => value_to_expr(value, inline_mode),
            _ => Some(Expr::from_json_value(value)),
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

fn value_to_expr(value: &Value, inline_mode: &ParamInlineMode) -> Option<Expr> {
    match inline_mode {
        ParamInlineMode::Literal => Some(Expr::from_json_value(value)),
        ParamInlineMode::Ident => value.as_str().and_then(parse_ident_path),
    }
}

fn infer_port_type_from_json(value: &Value) -> Option<PortType> {
    match value {
        Value::Number(_) => Some(PortType::number()),
        Value::String(_) => Some(PortType::text()),
        Value::Bool(_) => Some(PortType::bool()),
        _ => None,
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

#[derive(Debug, Clone, Serialize)]
/// Static description of a placeable piece.
pub struct PieceDef {
    pub id: String,
    pub label: String,
    pub category: PieceCategory,
    pub semantic_kind: PieceSemanticKind,
    #[serde(default = "default_piece_namespace")]
    pub namespace: String,
    #[serde(default)]
    pub params: Vec<ParamDef>,
    pub output_type: Option<PortType>,
    pub output_side: Option<TileSide>,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl PieceDef {
    /// Return whether this piece terminates compilation rather than producing output.
    pub fn is_terminal(&self) -> bool {
        self.output_type.is_none() || matches!(self.semantic_kind, PieceSemanticKind::Output)
    }

    /// Return whether this piece should be visible for the requested target namespace.
    pub fn is_visible_in_namespace(&self, namespace: &str) -> bool {
        self.namespace == "core"
            || self.namespace == namespace
            || matches!(self.semantic_kind, PieceSemanticKind::Trick)
    }
}

fn default_piece_namespace() -> String {
    "core".into()
}

fn infer_semantic_kind(namespace: &str, category: PieceCategory) -> PieceSemanticKind {
    if namespace != "core" {
        return PieceSemanticKind::Intrinsic;
    }

    match category {
        PieceCategory::Constant => PieceSemanticKind::Literal,
        PieceCategory::Control => PieceSemanticKind::Construct,
        PieceCategory::Output => PieceSemanticKind::Output,
        PieceCategory::Connector => PieceSemanticKind::Connector,
        PieceCategory::Transform | PieceCategory::Generator => PieceSemanticKind::Operator,
        PieceCategory::Trick => PieceSemanticKind::Trick,
    }
}

impl<'de> Deserialize<'de> for PieceDef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PieceDefSerde {
            id: String,
            label: String,
            category: PieceCategory,
            semantic_kind: Option<PieceSemanticKind>,
            namespace: Option<String>,
            #[serde(default)]
            params: Vec<ParamDef>,
            output_type: Option<PortType>,
            output_side: Option<TileSide>,
            description: Option<String>,
            #[serde(default)]
            tags: Option<Vec<String>>,
        }

        let raw = PieceDefSerde::deserialize(deserializer)?;
        let namespace = raw.namespace.unwrap_or_else(default_piece_namespace);
        let semantic_kind = raw
            .semantic_kind
            .unwrap_or_else(|| infer_semantic_kind(namespace.as_str(), raw.category));

        Ok(Self {
            id: raw.id,
            label: raw.label,
            category: raw.category,
            semantic_kind,
            namespace,
            params: raw.params,
            output_type: raw.output_type,
            output_side: raw.output_side,
            description: raw.description,
            tags: raw.tags.unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Inputs resolved for a node at compile time.
pub struct PieceInputs {
    #[serde(default)]
    pub scalar: BTreeMap<String, Expr>,
    #[serde(default)]
    pub variadic: BTreeMap<String, Vec<Expr>>,
}

impl PieceInputs {
    /// Return a resolved scalar input by id.
    pub fn get(&self, key: &str) -> Option<&Expr> {
        self.scalar.get(key)
    }

    /// Return a resolved variadic input group by id.
    pub fn get_variadic(&self, key: &str) -> Option<&Vec<Expr>> {
        self.variadic.get(key)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
/// Resolved port types available while compiling a node.
///
/// These types come from semantic inference, so pieces can safely dispatch on
/// the effective types of their inputs instead of repeating brittle,
/// piece-specific type resolution logic inside `compile`.
pub struct ResolvedPieceTypes {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_types: BTreeMap<String, PortType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_type: Option<PortType>,
}

impl ResolvedPieceTypes {
    /// Return the inferred type for a resolved input by id.
    pub fn input_type(&self, key: &str) -> Option<&PortType> {
        self.input_types.get(key)
    }

    /// Return this node's inferred output type, if known.
    pub fn inferred_output_type(&self) -> Option<&PortType> {
        self.output_type.as_ref()
    }
}

/// Trait implemented by every placeable piece type.
pub trait Piece: Send + Sync {
    /// Return the static piece definition used by UI, validation, and compilation.
    fn def(&self) -> &PieceDef;

    /// Compile this node using its resolved graph inputs and inline params.
    ///
    /// Most pieces can implement this untyped hook directly. Pieces that need
    /// ad-hoc polymorphism based on inferred input/output types should instead
    /// override [`Piece::compile_with_types`].
    fn compile(&self, inputs: &PieceInputs, inline_params: &BTreeMap<String, Value>) -> Expr;

    /// Compile this node with access to resolved input and output port types.
    ///
    /// The compiler calls this hook so pieces can dispatch on semantic
    /// inference results. The default implementation preserves existing
    /// behavior by delegating to [`Piece::compile`].
    fn compile_with_types(
        &self,
        inputs: &PieceInputs,
        inline_params: &BTreeMap<String, Value>,
        _resolved_types: &ResolvedPieceTypes,
    ) -> Expr {
        self.compile(inputs, inline_params)
    }

    /// Infer this node's effective output type from resolved input port types.
    ///
    /// The default implementation falls back to the static output type declared
    /// on [`PieceDef`]. Generic pieces can override this to propagate or refine
    /// types based on their inputs.
    fn infer_output_type(
        &self,
        _input_types: &BTreeMap<String, PortType>,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Option<PortType> {
        self.def().output_type.clone()
    }

    /// Return per-side output expressions for multi-output pieces (e.g. cross connectors).
    /// When `Some`, the compiler resolves edges by matching the exit direction to the
    /// appropriate side key, falling back to the primary `compile()` result.
    fn compile_multi_output(
        &self,
        _inputs: &PieceInputs,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Option<BTreeMap<TileSide, Expr>> {
        None
    }

    /// Typed variant of [`Piece::compile_multi_output`].
    fn compile_multi_output_with_types(
        &self,
        inputs: &PieceInputs,
        inline_params: &BTreeMap<String, Value>,
        _resolved_types: &ResolvedPieceTypes,
    ) -> Option<BTreeMap<TileSide, Expr>> {
        self.compile_multi_output(inputs, inline_params)
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
    ) -> (Expr, Value) {
        (self.compile(inputs, inline_params), state.clone())
    }

    /// Typed variant of [`Piece::compile_stateful`].
    ///
    /// Stateful pieces that also dispatch on inferred types should override
    /// this hook so they can use both the current state and semantic type
    /// information in one place.
    fn compile_stateful_with_types(
        &self,
        inputs: &PieceInputs,
        inline_params: &BTreeMap<String, Value>,
        state: &Value,
        _resolved_types: &ResolvedPieceTypes,
    ) -> (Expr, Value) {
        self.compile_stateful(inputs, inline_params, state)
    }

    /// Return a static preview timeline for this piece, if it knows its
    /// temporal structure at compile time.
    ///
    /// This allows the UI to animate nodes in preview mode without a running
    /// runtime. All step positions are normalised over one abstract preview
    /// window `[0.0, 1.0)`.
    ///
    /// Takes `inline_params` only (not [`PieceInputs`]) so preview works for
    /// disconnected nodes without full compilation.
    ///
    /// Default: `None` (no preview animation).
    fn preview_timeline(
        &self,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Option<PreviewTimeline> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ParamDef, ParamInlineMode, ParamSchema, ParamTextSemantics, ParamValueKind, PieceDef,
    };
    use crate::ast::Expr;
    use crate::types::{PieceCategory, PieceSemanticKind, PortType, TileSide};
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

    #[test]
    fn inline_ident_parses_dotted_path_into_fields() {
        let schema = ParamSchema::Custom {
            port_type: "any".into(),
            value_kind: ParamValueKind::Text,
            default: None,
            can_inline: true,
            inline_mode: ParamInlineMode::Ident,
            min: None,
            max: None,
        };

        let expr = schema
            .inline_expr(&json!("foo.bar.baz"))
            .expect("identifier path");
        assert_eq!(
            expr,
            Expr::field(Expr::field(Expr::ident("foo"), "bar"), "baz")
        );
    }

    #[test]
    fn inline_ident_rejects_invalid_path() {
        let schema = ParamSchema::Custom {
            port_type: "any".into(),
            value_kind: ParamValueKind::Text,
            default: None,
            can_inline: true,
            inline_mode: ParamInlineMode::Ident,
            min: None,
            max: None,
        };

        assert!(!schema.validate_inline_value(&json!("foo..bar")));
        assert!(schema.inline_expr(&json!("foo..bar")).is_none());
    }

    #[test]
    fn inline_mode_raw_alias_deserializes_to_ident() {
        let mode: ParamInlineMode = serde_json::from_value(json!("raw")).expect("alias");
        assert_eq!(mode, ParamInlineMode::Ident);
    }

    #[test]
    fn piece_def_defaults_core_namespace_and_semantic_kind() {
        let value = json!({
            "id": "core.not",
            "label": "not",
            "category": "transform",
            "params": [],
            "output_type": "bool",
            "output_side": "right",
            "description": null
        });

        let parsed: PieceDef = serde_json::from_value(value).expect("deserialize piece def");
        assert_eq!(parsed.namespace, "core");
        assert_eq!(parsed.semantic_kind, PieceSemanticKind::Operator);
    }

    #[test]
    fn piece_def_defaults_non_core_namespace_to_intrinsic() {
        let value = json!({
            "id": "strudel.fast",
            "label": "fast",
            "category": "generator",
            "namespace": "strudel",
            "params": [],
            "output_type": "pattern",
            "output_side": "right",
            "description": null
        });

        let parsed: PieceDef = serde_json::from_value(value).expect("deserialize piece def");
        assert_eq!(parsed.namespace, "strudel");
        assert_eq!(parsed.semantic_kind, PieceSemanticKind::Intrinsic);
    }

    #[test]
    fn piece_def_explicit_semantic_kind_overrides_defaults() {
        let value = json!({
            "id": "user.twist",
            "label": "twist",
            "category": "trick",
            "semantic_kind": "trick",
            "namespace": "user",
            "params": [],
            "output_type": "any",
            "output_side": "right",
            "description": null
        });

        let parsed: PieceDef = serde_json::from_value(value).expect("deserialize piece def");
        assert_eq!(parsed.semantic_kind, PieceSemanticKind::Trick);
    }

    #[test]
    fn piece_def_visibility_includes_core_matching_namespace_and_tricks() {
        let core_piece = PieceDef {
            id: "core.lt".into(),
            label: "lt".into(),
            category: PieceCategory::Transform,
            semantic_kind: PieceSemanticKind::Operator,
            namespace: "core".into(),
            params: vec![],
            output_type: Some("bool".into()),
            output_side: Some(TileSide::RIGHT),
            description: None,
            tags: vec![],
        };
        let intrinsic_piece = PieceDef {
            id: "strudel.fast".into(),
            label: "fast".into(),
            category: PieceCategory::Transform,
            semantic_kind: PieceSemanticKind::Intrinsic,
            namespace: "strudel".into(),
            params: vec![],
            output_type: Some("pattern".into()),
            output_side: Some(TileSide::RIGHT),
            description: None,
            tags: vec![],
        };
        let trick_piece = PieceDef {
            id: "user.twist".into(),
            label: "twist".into(),
            category: PieceCategory::Trick,
            semantic_kind: PieceSemanticKind::Trick,
            namespace: "user".into(),
            params: vec![],
            output_type: Some("any".into()),
            output_side: Some(TileSide::RIGHT),
            description: None,
            tags: vec![],
        };

        assert!(core_piece.is_visible_in_namespace("lua"));
        assert!(intrinsic_piece.is_visible_in_namespace("strudel"));
        assert!(!intrinsic_piece.is_visible_in_namespace("lua"));
        assert!(trick_piece.is_visible_in_namespace("lua"));
    }

    #[test]
    fn custom_any_schema_infers_number_from_inline_value() {
        let schema = ParamSchema::Custom {
            port_type: PortType::any(),
            value_kind: ParamValueKind::Json,
            default: None,
            can_inline: true,
            inline_mode: ParamInlineMode::Literal,
            min: None,
            max: None,
        };

        assert_eq!(
            schema.infer_inline_port_type(&json!(42)),
            Some(PortType::number())
        );
    }

    #[test]
    fn ident_inline_mode_keeps_declared_port_type_for_inference() {
        let schema = ParamSchema::Custom {
            port_type: PortType::new("pattern"),
            value_kind: ParamValueKind::Text,
            default: None,
            can_inline: true,
            inline_mode: ParamInlineMode::Ident,
            min: None,
            max: None,
        };

        assert_eq!(
            schema.infer_inline_port_type(&json!("foo.bar")),
            Some(PortType::new("pattern"))
        );
    }
}
