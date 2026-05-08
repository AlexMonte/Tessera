use std::collections::BTreeMap;

use serde_json::Value;

use crate::piece::{ParamInlineMode, ParamSchema, ParamValueKind};
use crate::types::{ExecutionDomain, PieceCategory, PortRole, PortType, Rational};

pub(super) fn schema_for_port(
    port_type: &PortType,
    default: Option<Value>,
    can_inline: bool,
) -> ParamSchema {
    if port_type.domain() == Some(ExecutionDomain::Control) {
        match port_type.as_str() {
            "number" => {
                return ParamSchema::Number {
                    default: default.and_then(|value| value.as_f64()).unwrap_or(0.0),
                    min: None,
                    max: None,
                    can_inline,
                };
            }
            "text" => {
                return ParamSchema::Text {
                    default: default
                        .and_then(|value| value.as_str().map(String::from))
                        .unwrap_or_default(),
                    can_inline,
                };
            }
            "bool" => {
                return ParamSchema::Bool {
                    default: default.and_then(|value| value.as_bool()).unwrap_or(false),
                    can_inline,
                };
            }
            "rational" => {
                let default = default
                    .and_then(|value| value.as_str().and_then(Rational::parse))
                    .unwrap_or(Rational::ONE);
                return ParamSchema::Rational {
                    default,
                    can_inline,
                };
            }
            _ => {}
        }
    }

    ParamSchema::Custom {
        port_type: port_type.clone(),
        value_kind: value_kind_for_port(port_type),
        default,
        can_inline,
        inline_mode: ParamInlineMode::Literal,
        min: None,
        max: None,
    }
}

pub(super) fn can_inline_for_port(port_type: &PortType) -> bool {
    matches!(port_type.as_str(), "number" | "text" | "bool" | "rational")
}

fn value_kind_for_port(port_type: &PortType) -> ParamValueKind {
    match port_type.as_str() {
        "number" => ParamValueKind::Number,
        "text" => ParamValueKind::Text,
        "bool" => ParamValueKind::Bool,
        "rational" => ParamValueKind::Rational,
        _ => ParamValueKind::Json,
    }
}

pub(super) fn subgraph_boundary_port_type(inline_params: &BTreeMap<String, Value>) -> PortType {
    let kind = inline_params
        .get("port_type")
        .and_then(Value::as_str)
        .unwrap_or("any");
    match subgraph_boundary_domain(inline_params) {
        Some(domain) => PortType::from(kind).with_domain(domain),
        None => PortType::from(kind).with_unspecified_domain(),
    }
}

fn subgraph_boundary_domain(inline_params: &BTreeMap<String, Value>) -> Option<ExecutionDomain> {
    match inline_params.get("domain").and_then(Value::as_str) {
        Some("audio") => Some(ExecutionDomain::Audio),
        Some("event") => Some(ExecutionDomain::Event),
        Some("unspecified") => None,
        Some("control") | None => Some(ExecutionDomain::Control),
        Some(_) => Some(ExecutionDomain::Control),
    }
}

pub(super) fn slot_from_piece_id(piece_id: &str) -> Option<u8> {
    piece_id
        .rsplit('_')
        .next()
        .and_then(|value| value.parse::<u8>().ok())
}

pub(super) fn subgraph_boundary_role(inline_params: &BTreeMap<String, Value>) -> PortRole {
    match inline_params.get("role").and_then(Value::as_str) {
        Some("gate") => PortRole::Gate,
        Some("signal") => PortRole::Signal,
        Some("callback") => PortRole::Callback,
        Some("sequence") => PortRole::Sequence,
        _ => PortRole::Value,
    }
}

pub(super) fn subgraph_piece_category(
    inline_params: &BTreeMap<String, Value>,
) -> Option<PieceCategory> {
    match inline_params.get("category").and_then(Value::as_str) {
        Some("source") => Some(PieceCategory::Source),
        Some("cadence") => Some(PieceCategory::Cadence),
        Some("flow") => Some(PieceCategory::Flow),
        Some("state") => Some(PieceCategory::State),
        Some("transform") => Some(PieceCategory::Transform),
        Some("structure") => Some(PieceCategory::Structure),
        Some("observe") => Some(PieceCategory::Observe),
        Some("control") => Some(PieceCategory::Control),
        Some("trick") => Some(PieceCategory::Trick),
        Some("constant") => Some(PieceCategory::Constant),
        Some("generator") => Some(PieceCategory::Generator),
        Some("boundary") => Some(PieceCategory::Boundary),
        Some("output") => Some(PieceCategory::Output),
        Some("connector") => Some(PieceCategory::Connector),
        _ => None,
    }
}

pub(super) fn trimmed_inline_text(
    inline_params: &BTreeMap<String, Value>,
    key: &str,
) -> Option<String> {
    inline_params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(String::from)
}

pub(super) fn split_inline_tags(inline_params: &BTreeMap<String, Value>) -> Vec<String> {
    inline_params
        .get("tags")
        .and_then(Value::as_str)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}
