use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ast::{Expr, Origin};
use crate::compiler::{CompileMode, compile_node_expr};
use crate::diagnostics::{Diagnostic, DiagnosticKind};
use crate::graph::Graph;
use crate::piece::{
    ParamDef, ParamInlineMode, ParamSchema, ParamValueKind, Piece, PieceDef, PieceInputs,
};
use crate::piece_registry::PieceRegistry;
use crate::semantic::semantic_pass;
use crate::types::{GridPos, PieceCategory, PieceSemanticKind, PortType, TileSide};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const SUBGRAPH_INPUT_1_ID: &str = "tessera.subgraph_input_1";
pub const SUBGRAPH_INPUT_2_ID: &str = "tessera.subgraph_input_2";
pub const SUBGRAPH_INPUT_3_ID: &str = "tessera.subgraph_input_3";
pub const SUBGRAPH_OUTPUT_ID: &str = "tessera.subgraph_output";

const MAX_SUBGRAPH_INPUTS: usize = 3;

fn is_subgraph_input_id(piece_id: &str) -> bool {
    matches!(
        piece_id,
        SUBGRAPH_INPUT_1_ID | SUBGRAPH_INPUT_2_ID | SUBGRAPH_INPUT_3_ID
    )
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A reusable subgraph definition (graph macro).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubgraphDef {
    pub id: String,
    pub name: String,
    pub graph: Graph,
}

/// An analysed input port inside a subgraph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubgraphInput {
    pub slot: u8,
    pub pos: GridPos,
    pub label: String,
    pub port_type: PortType,
    pub required: bool,
    pub is_receiver: bool,
    pub default_value: Option<Value>,
}

impl SubgraphInput {
    /// Sanitise the label into a valid identifier, falling back to `arg{slot}`.
    pub fn param_name(&self) -> String {
        let sanitized = sanitize_identifier(&self.label);
        if sanitized.is_empty() {
            format!("arg{}", self.slot)
        } else {
            sanitized
        }
    }
}

/// Signature extracted from a subgraph after analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubgraphSignature {
    pub inputs: Vec<SubgraphInput>,
    pub output_pos: GridPos,
}

/// A fully compiled subgraph ready to be used as a piece.
#[derive(Debug, Clone)]
pub struct CompiledSubgraph {
    pub subgraph_id: String,
    pub display_name: String,
    pub binding_name: String,
    pub signature: SubgraphSignature,
    pub body: Expr,
}

impl CompiledSubgraph {
    /// Create a [`GeneratedSubgraphPiece`] from this compiled subgraph.
    pub fn to_piece(&self) -> GeneratedSubgraphPiece {
        GeneratedSubgraphPiece::new(
            &self.subgraph_id,
            &self.display_name,
            &self.binding_name,
            &self.signature.inputs,
        )
    }
}

// ---------------------------------------------------------------------------
// Built-in pieces: SubgraphInputPiece + SubgraphOutputPiece
// ---------------------------------------------------------------------------

/// A subgraph boundary input piece (slots 1–3).
pub struct SubgraphInputPiece {
    def: PieceDef,
    slot: u8,
}

impl SubgraphInputPiece {
    pub fn new(slot: u8) -> Self {
        let id = match slot {
            1 => SUBGRAPH_INPUT_1_ID,
            2 => SUBGRAPH_INPUT_2_ID,
            3 => SUBGRAPH_INPUT_3_ID,
            _ => SUBGRAPH_INPUT_1_ID,
        };
        Self {
            def: PieceDef {
                id: id.into(),
                label: format!("arg{slot}"),
                category: PieceCategory::Trick,
                semantic_kind: PieceSemanticKind::Trick,
                namespace: "core".into(),
                params: vec![
                    ParamDef {
                        id: "label".into(),
                        label: "label".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Text {
                            default: format!("input {slot}"),
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                    },
                    ParamDef {
                        id: "port_type".into(),
                        label: "type".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Text {
                            default: "any".into(),
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                    },
                    ParamDef {
                        id: "required".into(),
                        label: "required".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Bool {
                            default: true,
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                    },
                    ParamDef {
                        id: "is_receiver".into(),
                        label: "receiver".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Bool {
                            default: false,
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                    },
                    ParamDef {
                        id: "default_value".into(),
                        label: "default".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Custom {
                            port_type: PortType::any(),
                            value_kind: ParamValueKind::Json,
                            default: None,
                            can_inline: true,
                            inline_mode: ParamInlineMode::Literal,
                            min: None,
                            max: None,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                    },
                ],
                output_type: Some(PortType::any()),
                output_side: Some(TileSide::RIGHT),
                description: Some(
                    "Subgraph boundary input. Configure its metadata via inline params; \
                     used as a formal parameter when compiling the subgraph."
                        .into(),
                ),
            },
            slot,
        }
    }
}

impl Piece for SubgraphInputPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
        Expr::ident(format!("arg{}", self.slot))
    }
}

/// A subgraph boundary output piece.
pub struct SubgraphOutputPiece {
    def: PieceDef,
}

impl SubgraphOutputPiece {
    pub fn new() -> Self {
        Self {
            def: PieceDef {
                id: SUBGRAPH_OUTPUT_ID.into(),
                label: "return".into(),
                category: PieceCategory::Output,
                semantic_kind: PieceSemanticKind::Output,
                namespace: "core".into(),
                params: vec![ParamDef {
                    id: "input".into(),
                    label: "input".into(),
                    side: TileSide::LEFT,
                    schema: ParamSchema::Custom {
                        port_type: PortType::any(),
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
                description: Some(
                    "Subgraph boundary output. Connect the expression this subgraph should return."
                        .into(),
                ),
            },
        }
    }
}

impl Piece for SubgraphOutputPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
        inputs
            .get("input")
            .cloned()
            .unwrap_or_else(|| Expr::error("missing subgraph output"))
    }
}

// ---------------------------------------------------------------------------
// Generated piece: created from a compiled subgraph for use in the main graph
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct GeneratedSubgraphPiece {
    def: PieceDef,
    binding_name: String,
    ordered_inputs: Vec<SubgraphInput>,
}

impl GeneratedSubgraphPiece {
    pub fn new(
        subgraph_id: &str,
        label: &str,
        binding_name: &str,
        ordered_inputs: &[SubgraphInput],
    ) -> Self {
        let params = build_generated_params(ordered_inputs);
        Self {
            def: PieceDef {
                id: format!("tessera.subgraph.{subgraph_id}"),
                label: label.into(),
                category: PieceCategory::Trick,
                semantic_kind: PieceSemanticKind::Trick,
                namespace: "user".into(),
                params,
                output_type: Some(PortType::any()),
                output_side: Some(TileSide::RIGHT),
                description: Some("User-defined subgraph macro.".into()),
            },
            binding_name: binding_name.to_lowercase(),
            ordered_inputs: ordered_inputs.to_vec(),
        }
    }
}

impl Piece for GeneratedSubgraphPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn compile(&self, inputs: &PieceInputs, inline_params: &BTreeMap<String, Value>) -> Expr {
        if self.ordered_inputs.is_empty() {
            return Expr::ident(self.binding_name.clone());
        }

        let resolved: Vec<_> = self
            .ordered_inputs
            .iter()
            .map(|input| resolve_input(input, inputs, inline_params))
            .collect();

        let receiver = resolved.iter().find(|r| r.input.is_receiver);
        if let Some(receiver) = receiver {
            if receiver.expr.is_none() {
                let has_explicit_partial = resolved
                    .iter()
                    .any(|r| !r.input.is_receiver && r.is_explicit);
                if !has_explicit_partial {
                    return Expr::ident(self.binding_name.clone());
                }
                // Build a structural lambda for partial application.
                let placeholder = "pattern";
                let lambda_args: Vec<Expr> = resolved
                    .iter()
                    .map(|r| {
                        if r.input.is_receiver {
                            Expr::ident(placeholder)
                        } else {
                            r.expr.clone().unwrap_or_else(Expr::nil)
                        }
                    })
                    .collect();
                return Expr::lambda(
                    vec![placeholder.into()],
                    Expr::call_named(&self.binding_name, lambda_args),
                );
            }
        }

        Expr::call_named(
            &self.binding_name,
            strip_trailing_none(
                resolved
                    .into_iter()
                    .map(|r| r.expr)
                    .collect::<Vec<Option<Expr>>>(),
            ),
        )
    }
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

/// Analyse a subgraph, returning its [`SubgraphSignature`] or diagnostics.
pub fn analyze_subgraph(
    graph: &Graph,
    registry: &PieceRegistry,
) -> Result<SubgraphSignature, Vec<Diagnostic>> {
    let mut inputs = Vec::<SubgraphInput>::new();
    let mut output_positions = Vec::<GridPos>::new();
    let mut diagnostics = Vec::<Diagnostic>::new();

    for (pos, node) in &graph.nodes {
        if is_subgraph_input_id(node.piece_id.as_str()) {
            let slot = slot_from_piece_id(node.piece_id.as_str()).unwrap_or(1);
            let label = node
                .inline_params
                .get("label")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("input {slot}"));
            let port_type = node
                .inline_params
                .get("port_type")
                .and_then(Value::as_str)
                .map(PortType::from)
                .unwrap_or_else(PortType::any);
            let required = node
                .inline_params
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let is_receiver = node
                .inline_params
                .get("is_receiver")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let default_value = node.inline_params.get("default_value").cloned();

            inputs.push(SubgraphInput {
                slot,
                pos: pos.clone(),
                label,
                port_type,
                required,
                is_receiver,
                default_value,
            });
        } else if node.piece_id == SUBGRAPH_OUTPUT_ID {
            output_positions.push(pos.clone());
        }
    }

    if inputs.len() > MAX_SUBGRAPH_INPUTS {
        diagnostics.push(Diagnostic::error(
            DiagnosticKind::InvalidOperation {
                reason: format!("subgraph may declare at most {MAX_SUBGRAPH_INPUTS} inputs"),
            },
            inputs.get(MAX_SUBGRAPH_INPUTS).map(|i| i.pos.clone()),
        ));
    }

    let mut seen_slots = BTreeSet::new();
    for input in &inputs {
        if !seen_slots.insert(input.slot) {
            diagnostics.push(Diagnostic::error(
                DiagnosticKind::InvalidOperation {
                    reason: format!("duplicate subgraph input slot {}", input.slot),
                },
                Some(input.pos.clone()),
            ));
        }
    }

    if output_positions.is_empty() {
        diagnostics.push(Diagnostic::error(
            DiagnosticKind::InvalidOperation {
                reason: "subgraph requires exactly one output".into(),
            },
            None,
        ));
    } else if output_positions.len() > 1 {
        diagnostics.push(Diagnostic::error(
            DiagnosticKind::InvalidOperation {
                reason: "subgraph requires exactly one output".into(),
            },
            output_positions.get(1).cloned(),
        ));
    }

    let receiver_count = inputs.iter().filter(|i| i.is_receiver).count();
    if receiver_count > 1 {
        diagnostics.push(Diagnostic::error(
            DiagnosticKind::InvalidOperation {
                reason: "subgraph may declare at most one receiver input".into(),
            },
            inputs.iter().find(|i| i.is_receiver).map(|i| i.pos.clone()),
        ));
    }

    // Type-check edges leaving input nodes.
    for input in &inputs {
        for edge in graph.edges.values().filter(|e| e.from == input.pos) {
            let Some(target_node) = graph.nodes.get(&edge.to_node) else {
                continue;
            };
            let Some(target_piece) = registry.get(target_node.piece_id.as_str()) else {
                continue;
            };
            let Some(param_def) = target_piece
                .def()
                .params
                .iter()
                .find(|p| p.id == edge.to_param)
            else {
                continue;
            };
            if !param_def.schema.accepts(&input.port_type) {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticKind::TypeMismatch {
                            expected: param_def.schema.expected_port_type(),
                            got: input.port_type.clone(),
                            param: edge.to_param.clone(),
                        },
                        Some(edge.to_node.clone()),
                    )
                    .with_edge(edge.id.clone()),
                );
            }
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    inputs.sort_by_key(|i| i.slot);
    Ok(SubgraphSignature {
        inputs,
        output_pos: output_positions
            .into_iter()
            .next()
            .expect("checked output existence"),
    })
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

/// Compile a single subgraph definition against the given registry.
pub fn compile_subgraph(
    def: &SubgraphDef,
    registry: &PieceRegistry,
) -> Result<CompiledSubgraph, Vec<Diagnostic>> {
    let sem = semantic_pass(&def.graph, registry);
    let mut diagnostics = Vec::<Diagnostic>::new();

    let signature = match analyze_subgraph(&def.graph, registry) {
        Ok(sig) => sig,
        Err(mut errors) => {
            diagnostics.append(&mut errors);
            return Err(diagnostics);
        }
    };

    if !sem.diagnostics.is_empty() {
        diagnostics.append(&mut sem.diagnostics.clone());
    }

    let mut overrides = BTreeMap::new();
    for input in &signature.inputs {
        overrides.insert(
            input.pos.clone(),
            Expr::ident(input.param_name()).with_origin(Origin {
                node: input.pos.clone(),
                param: Some(format!("arg{}", input.slot)),
            }),
        );
    }

    let body = match compile_node_expr(
        &def.graph,
        registry,
        &sem,
        CompileMode::Preview,
        &signature.output_pos,
        &overrides,
    ) {
        Ok((expr, _)) => expr,
        Err(mut errors) => {
            diagnostics.append(&mut errors);
            return Err(diagnostics);
        }
    };

    let mut used = BTreeSet::new();
    let binding_name = unique_binding_name(&def.name, &def.id, &mut used);

    Ok(CompiledSubgraph {
        subgraph_id: def.id.clone(),
        display_name: def.name.clone(),
        binding_name,
        signature,
        body,
    })
}

/// Compile a batch of subgraph definitions, deduplicating binding names.
pub fn compile_subgraphs(
    defs: &[SubgraphDef],
    registry: &PieceRegistry,
) -> (Vec<CompiledSubgraph>, Vec<Diagnostic>) {
    let mut compiled = Vec::<CompiledSubgraph>::new();
    let mut diagnostics = Vec::<Diagnostic>::new();
    let mut used_names = BTreeSet::<String>::new();

    for def in defs {
        let sem = semantic_pass(&def.graph, registry);
        let signature = match analyze_subgraph(&def.graph, registry) {
            Ok(sig) => sig,
            Err(mut errors) => {
                diagnostics.append(&mut errors);
                continue;
            }
        };

        if !sem.diagnostics.is_empty() {
            diagnostics.append(&mut sem.diagnostics.clone());
        }

        let mut overrides = BTreeMap::new();
        for input in &signature.inputs {
            overrides.insert(
                input.pos.clone(),
                Expr::ident(input.param_name()).with_origin(Origin {
                    node: input.pos.clone(),
                    param: Some(format!("arg{}", input.slot)),
                }),
            );
        }

        let body = match compile_node_expr(
            &def.graph,
            registry,
            &sem,
            CompileMode::Preview,
            &signature.output_pos,
            &overrides,
        ) {
            Ok((expr, _)) => expr,
            Err(mut errors) => {
                diagnostics.append(&mut errors);
                continue;
            }
        };

        let binding_name = unique_binding_name(&def.name, &def.id, &mut used_names);

        compiled.push(CompiledSubgraph {
            subgraph_id: def.id.clone(),
            display_name: def.name.clone(),
            binding_name,
            signature,
            body,
        });
    }

    (compiled, diagnostics)
}

/// Create pieces from compiled subgraphs for use in a runtime registry.
pub fn subgraph_pieces(compiled: &[CompiledSubgraph]) -> Vec<GeneratedSubgraphPiece> {
    compiled.iter().map(CompiledSubgraph::to_piece).collect()
}

/// Build a registry pre-loaded with the subgraph I/O boundary pieces.
///
/// Hosts should register their own application pieces into this registry
/// before passing it to [`analyze_subgraph`] or [`compile_subgraph`].
pub fn subgraph_editor_pieces() -> Vec<Box<dyn Piece>> {
    vec![
        Box::new(SubgraphInputPiece::new(1)),
        Box::new(SubgraphInputPiece::new(2)),
        Box::new(SubgraphInputPiece::new(3)),
        Box::new(SubgraphOutputPiece::new()),
    ]
}

// ---------------------------------------------------------------------------
// Helpers: generated piece param building
// ---------------------------------------------------------------------------

fn build_generated_params(inputs: &[SubgraphInput]) -> Vec<ParamDef> {
    let mut ordered = inputs.to_vec();
    ordered.sort_by_key(|i| (if i.is_receiver { 0 } else { 1 }, i.slot));
    let mut extra_sides = vec![TileSide::BOTTOM, TileSide::TOP, TileSide::RIGHT];
    let mut params = Vec::with_capacity(ordered.len());

    for (index, input) in ordered.iter().enumerate() {
        let side = if input.is_receiver {
            TileSide::LEFT
        } else if !ordered.iter().any(|v| v.is_receiver) && index == 0 {
            TileSide::LEFT
        } else {
            extra_sides.remove(0.min(extra_sides.len().saturating_sub(1)))
        };
        params.push(ParamDef {
            id: format!("arg{}", input.slot),
            label: input.label.clone(),
            side,
            schema: schema_for_port(
                &input.port_type,
                input.default_value.clone(),
                can_inline_for_port(&input.port_type),
            ),
            text_semantics: Default::default(),
            variadic_group: None,
            required: input.required && !input.is_receiver,
        });
    }

    params
}

fn schema_for_port(port_type: &PortType, default: Option<Value>, can_inline: bool) -> ParamSchema {
    match port_type.as_str() {
        "number" => ParamSchema::Number {
            default: default.and_then(|v| v.as_f64()).unwrap_or(0.0),
            min: None,
            max: None,
            can_inline,
        },
        "text" => ParamSchema::Text {
            default: default
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            can_inline,
        },
        "bool" => ParamSchema::Bool {
            default: default.and_then(|v| v.as_bool()).unwrap_or(false),
            can_inline,
        },
        _ => ParamSchema::Custom {
            port_type: port_type.clone(),
            value_kind: ParamValueKind::Json,
            default,
            can_inline,
            inline_mode: ParamInlineMode::Literal,
            min: None,
            max: None,
        },
    }
}

fn can_inline_for_port(port_type: &PortType) -> bool {
    matches!(port_type.as_str(), "number" | "text" | "bool")
}

pub fn default_expr_for_input(input: &SubgraphInput) -> Option<Expr> {
    schema_for_port(
        &input.port_type,
        input.default_value.clone(),
        can_inline_for_port(&input.port_type),
    )
    .default_expr()
    .map(|expr| {
        expr.with_origin_if_missing(Origin {
            node: input.pos.clone(),
            param: Some(format!("arg{}", input.slot)),
        })
    })
}

// ---------------------------------------------------------------------------
// Helpers: generated piece compilation
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ResolvedInput<'a> {
    input: &'a SubgraphInput,
    expr: Option<Expr>,
    is_explicit: bool,
}

fn resolve_input<'a>(
    input: &'a SubgraphInput,
    inputs: &PieceInputs,
    inline_params: &BTreeMap<String, Value>,
) -> ResolvedInput<'a> {
    let param_id = format!("arg{}", input.slot);
    let connected = inputs.get(param_id.as_str()).cloned();
    let inline = inline_params.get(param_id.as_str()).and_then(|value| {
        schema_for_port(
            &input.port_type,
            None,
            can_inline_for_port(&input.port_type),
        )
        .inline_expr(value)
    });
    ResolvedInput {
        input,
        expr: connected.or(inline),
        is_explicit: inputs.get(param_id.as_str()).is_some()
            || inline_params.contains_key(param_id.as_str()),
    }
}

fn strip_trailing_none(args: Vec<Option<Expr>>) -> Vec<Expr> {
    let mut args = args;
    while matches!(args.last(), Some(None)) {
        args.pop();
    }
    args.into_iter()
        .map(|v| v.unwrap_or_else(Expr::nil))
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers: naming
// ---------------------------------------------------------------------------

fn slot_from_piece_id(piece_id: &str) -> Option<u8> {
    piece_id
        .rsplit('_')
        .next()
        .and_then(|v| v.parse::<u8>().ok())
}

fn unique_binding_name(name: &str, id: &str, used: &mut BTreeSet<String>) -> String {
    let mut base = sanitize_identifier(name);
    if base.is_empty() {
        let suffix: String = id
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(8)
            .collect();
        base = if suffix.is_empty() {
            "subgraph_generated".into()
        } else {
            format!("subgraph_{suffix}")
        };
    }

    let mut candidate = base.clone();
    let mut index = 2usize;
    while used.contains(&candidate) {
        candidate = format!("{base}_{index}");
        index += 1;
    }
    used.insert(candidate.clone());
    candidate
}

fn sanitize_identifier(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
            if index == 0 && ch.is_ascii_digit() {
                out.push('_');
            }
            out.push(ch);
        } else if (ch == ' ' || ch == '-' || ch == '.') && !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ExprKind;
    use crate::backend::{Backend, JsBackend};
    use crate::graph::{Edge, Node};
    use crate::types::EdgeId;

    fn make_input(slot: u8, port_type: &str, required: bool, is_receiver: bool) -> SubgraphInput {
        SubgraphInput {
            slot,
            pos: GridPos {
                col: i32::from(slot),
                row: 0,
            },
            label: format!("arg{slot}"),
            port_type: PortType::from(port_type),
            required,
            is_receiver,
            default_value: None,
        }
    }

    // -- SubgraphInputPiece / SubgraphOutputPiece --

    #[test]
    fn input_piece_compiles_to_ident() {
        let piece = SubgraphInputPiece::new(2);
        let expr = piece.compile(&PieceInputs::default(), &BTreeMap::new());
        assert_eq!(JsBackend.render(&expr), "arg2");
    }

    #[test]
    fn output_piece_passes_through() {
        let piece = SubgraphOutputPiece::new();
        let mut inputs = PieceInputs::default();
        inputs.scalar.insert("input".into(), Expr::ident("x"));
        let expr = piece.compile(&inputs, &BTreeMap::new());
        assert_eq!(JsBackend.render(&expr), "x");
    }

    #[test]
    fn output_piece_uses_error_placeholder_when_missing() {
        let piece = SubgraphOutputPiece::new();
        let expr = piece.compile(&PieceInputs::default(), &BTreeMap::new());
        assert!(matches!(expr.kind, ExprKind::Error { .. }));
        assert_eq!(JsBackend.render(&expr), "/* missing subgraph output */");
    }

    // -- GeneratedSubgraphPiece --

    #[test]
    fn zero_input_compiles_to_ident() {
        let piece = GeneratedSubgraphPiece::new("my_sub", "my sub", "my_sub", &[]);
        let expr = piece.compile(&PieceInputs::default(), &BTreeMap::new());
        assert_eq!(JsBackend.render(&expr), "my_sub");
    }

    #[test]
    fn receiver_not_connected_compiles_to_ident() {
        let piece =
            GeneratedSubgraphPiece::new("fx", "fx", "fx", &[make_input(1, "any", true, true)]);
        let expr = piece.compile(&PieceInputs::default(), &BTreeMap::new());
        assert_eq!(JsBackend.render(&expr), "fx");
    }

    #[test]
    fn receiver_connected_compiles_to_call() {
        let piece =
            GeneratedSubgraphPiece::new("fx", "fx", "fx", &[make_input(1, "any", true, true)]);
        let mut inputs = PieceInputs::default();
        inputs.scalar.insert("arg1".into(), Expr::ident("src"));
        let expr = piece.compile(&inputs, &BTreeMap::new());
        assert!(matches!(expr.kind, ExprKind::Call { .. }));
        assert_eq!(JsBackend.render(&expr), "fx(src)");
    }

    #[test]
    fn partial_application_without_receiver() {
        let piece = GeneratedSubgraphPiece::new(
            "shimmer",
            "shimmer",
            "shimmer",
            &[
                make_input(1, "any", true, true),
                make_input(2, "number", false, false),
            ],
        );
        let mut inputs = PieceInputs::default();
        inputs.scalar.insert("arg2".into(), Expr::float(0.5));
        let expr = piece.compile(&inputs, &BTreeMap::new());
        assert!(matches!(expr.kind, ExprKind::Lambda { .. }));
        assert_eq!(
            JsBackend.render(&expr),
            "(pattern) => shimmer(pattern, 0.5)"
        );
    }

    #[test]
    fn strip_trailing_none_replaces_internal_gaps_with_nil() {
        let args = strip_trailing_none(vec![Some(Expr::ident("src")), None, Some(Expr::int(2))]);
        assert_eq!(args, vec![Expr::ident("src"), Expr::nil(), Expr::int(2)]);
    }

    // -- analyze_subgraph --

    fn editor_registry() -> PieceRegistry {
        let mut reg = PieceRegistry::new();
        for piece in subgraph_editor_pieces() {
            let id = piece.def().id.clone();
            reg.register_arc(id, std::sync::Arc::from(piece));
        }
        // Add a simple transform for wiring tests.
        reg.register(SimpleTransform::new());
        reg
    }

    struct SimpleTransform {
        def: PieceDef,
    }
    impl SimpleTransform {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.transform".into(),
                    label: "xform".into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "input".into(),
                        label: "input".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Custom {
                            port_type: PortType::any(),
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
                    output_type: Some(PortType::any()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                },
            }
        }
    }
    impl Piece for SimpleTransform {
        fn def(&self) -> &PieceDef {
            &self.def
        }
        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::method_call(
                inputs
                    .get("input")
                    .cloned()
                    .unwrap_or_else(|| Expr::error("missing")),
                "xform",
                vec![],
            )
        }
    }

    fn simple_subgraph() -> (Graph, PieceRegistry) {
        let reg = editor_registry();
        let input_pos = GridPos { col: 0, row: 0 };
        let xform_pos = GridPos { col: 1, row: 0 };
        let output_pos = GridPos { col: 2, row: 0 };
        let edge_a_id = EdgeId::new();
        let edge_b_id = EdgeId::new();
        let graph = Graph {
            nodes: BTreeMap::from([
                (
                    input_pos.clone(),
                    Node {
                        piece_id: SUBGRAPH_INPUT_1_ID.into(),
                        inline_params: BTreeMap::from([(
                            "label".into(),
                            Value::String("src".into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    xform_pos.clone(),
                    Node {
                        piece_id: "test.transform".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("input".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos.clone(),
                    Node {
                        piece_id: SUBGRAPH_OUTPUT_ID.into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("input".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([
                (
                    edge_a_id.clone(),
                    Edge {
                        id: edge_a_id,
                        from: input_pos,
                        to_node: xform_pos.clone(),
                        to_param: "input".into(),
                    },
                ),
                (
                    edge_b_id.clone(),
                    Edge {
                        id: edge_b_id,
                        from: xform_pos,
                        to_node: output_pos,
                        to_param: "input".into(),
                    },
                ),
            ]),
            name: "test_sub".into(),
            cols: 4,
            rows: 1,
        };
        (graph, reg)
    }

    #[test]
    fn analyze_subgraph_extracts_signature() {
        let (graph, reg) = simple_subgraph();
        let sig = analyze_subgraph(&graph, &reg).expect("valid subgraph");
        assert_eq!(sig.inputs.len(), 1);
        assert_eq!(sig.inputs[0].slot, 1);
        assert_eq!(sig.inputs[0].label, "src");
        assert_eq!(sig.output_pos, GridPos { col: 2, row: 0 });
    }

    #[test]
    fn compile_subgraph_produces_body() {
        let (graph, reg) = simple_subgraph();
        let def = SubgraphDef {
            id: "test_sub".into(),
            name: "test sub".into(),
            graph,
        };
        let compiled = compile_subgraph(&def, &reg).expect("compile");
        assert_eq!(JsBackend.render(&compiled.body), "src.xform()");
    }
}
