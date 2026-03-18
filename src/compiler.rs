use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::activity::ActivityEvent;
use crate::ast::{Expr, ExprKind, OptLevel, Origin};
use crate::diagnostics::{Diagnostic, DiagnosticKind, SemanticResult};
use crate::graph::{Edge, Graph, GraphOp, Node};
use crate::piece::{Piece, PieceInputs, ResolvedPieceTypes};
use crate::piece_registry::PieceRegistry;
use crate::semantic::{incoming_edge_for_param, resolved_input_types_for_piece, semantic_pass};
use crate::types::{DomainBridge, EdgeId, GridPos, PortType, TileSide};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompileMode {
    Preview,
    Runtime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStateUpdate {
    pub position: GridPos,
    pub state: Value,
}

/// Metadata for a host-managed delay (feedback) buffer.
///
/// Hosts allocate a frame buffer for each slot and provide the previous
/// frame's value at runtime via `__delay(slot, default)` (or equivalent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelaySlot {
    /// Unique slot identifier, derived from the delay node's grid position
    /// (e.g. `"d_3_2"`).
    pub slot: String,
    /// The delay node's position in the graph.
    pub node: GridPos,
    /// The default expression emitted for frame 0.
    pub default_expr: Expr,
    /// Inferred output type of the delay node, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_type: Option<PortType>,
}

/// Result of compiling a graph, including expressions plus host-facing
/// runtime metadata for feedback buffers and implicit domain bridges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileProgram {
    pub terminals: Vec<Expr>,
    pub state_updates: Vec<NodeStateUpdate>,
    /// Activity events generated during compilation. Forward-compatible carry
    /// channel; empty in core v1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity_events: Vec<ActivityEvent>,
    /// Delay buffer slots the host must allocate for feedback loops.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delay_slots: Vec<DelaySlot>,
    /// Implicit execution-domain bridges inserted while resolving edges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain_bridges: Vec<DomainBridge>,
    /// Diagnostics collected during compilation.
    ///
    /// In best-effort (Preview) mode this may contain errors and warnings
    /// for broken subgraphs while the rest of the program is still valid.
    /// In Runtime mode the program is only returned when terminals are
    /// error-free, so diagnostics here are warnings only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
struct CompiledNodes {
    compiled: BTreeMap<GridPos, Expr>,
    state_updates: Vec<NodeStateUpdate>,
}

#[derive(Debug, Clone)]
struct CachedNodeExpr {
    expr: Expr,
    inputs: PieceInputs,
    inline_params: BTreeMap<String, Value>,
    node_state: Option<Value>,
    resolved_types: ResolvedPieceTypes,
    side_outputs: BTreeMap<TileSide, Expr>,
}

#[derive(Debug, Clone, Default)]
/// Host-owned incremental compilation cache.
///
/// Hosts should treat this as ephemeral process state. Clear it when the
/// registry changes or when switching to a different graph instance without
/// replaying mutations through `apply_ops_cached`.
pub struct CompileCache {
    semantic: Option<SemanticResult>,
    node_exprs: BTreeMap<GridPos, CachedNodeExpr>,
    dirty: BTreeSet<GridPos>,
    last_mode: Option<CompileMode>,
    last_opt_level: Option<OptLevel>,
}

impl CompileCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_empty(&self) -> bool {
        self.semantic.is_none()
            && self.node_exprs.is_empty()
            && self.dirty.is_empty()
            && self.last_mode.is_none()
            && self.last_opt_level.is_none()
    }

    pub(crate) fn invalidate_from_apply_outcome(
        &mut self,
        graph: &Graph,
        applied_ops: &[GraphOp],
        removed_edges: &[Edge],
        explicit_disconnect_targets: &BTreeMap<EdgeId, GridPos>,
    ) {
        let mut invalidate_semantic = false;
        let mut dropped_positions = BTreeSet::<GridPos>::new();

        for op in applied_ops {
            match op {
                GraphOp::NodePlace { position, .. } => {
                    invalidate_semantic = true;
                    self.dirty.insert(*position);
                    dropped_positions.insert(*position);
                }
                GraphOp::NodeBatchPlace { nodes, .. } => {
                    invalidate_semantic = true;
                    for entry in nodes {
                        self.dirty.insert(entry.position);
                        dropped_positions.insert(entry.position);
                    }
                }
                GraphOp::NodeMove { from, to } => {
                    invalidate_semantic = true;
                    self.dirty.insert(*from);
                    self.dirty.insert(*to);
                    dropped_positions.insert(*from);
                    dropped_positions.insert(*to);
                }
                GraphOp::NodeSwap { a, b } => {
                    invalidate_semantic = true;
                    self.dirty.insert(*a);
                    self.dirty.insert(*b);
                    dropped_positions.insert(*a);
                    dropped_positions.insert(*b);
                }
                GraphOp::NodeRemove { position } => {
                    invalidate_semantic = true;
                    self.dirty.insert(*position);
                    dropped_positions.insert(*position);
                }
                GraphOp::EdgeConnect { to_node, .. } => {
                    invalidate_semantic = true;
                    self.dirty.insert(*to_node);
                }
                GraphOp::EdgeDisconnect { edge_id } => {
                    invalidate_semantic = true;
                    if let Some(target) = explicit_disconnect_targets.get(edge_id) {
                        self.dirty.insert(*target);
                    }
                }
                GraphOp::ParamSetInline { position, .. }
                | GraphOp::ParamClearInline { position, .. } => {
                    invalidate_semantic = true;
                    self.dirty.insert(*position);
                }
                GraphOp::NodeSetState { position, .. } => {
                    self.dirty.insert(*position);
                }
                GraphOp::ParamSetSide { position, .. }
                | GraphOp::ParamClearSide { position, .. }
                | GraphOp::OutputSetSide { position, .. }
                | GraphOp::OutputClearSide { position } => {
                    invalidate_semantic = true;
                    self.dirty.insert(*position);
                }
                GraphOp::NodeSetLabel { .. } => {}
                GraphOp::ResizeGrid { .. } => {
                    invalidate_semantic = true;
                }
                GraphOp::NodeAutoWire { .. } => {
                    invalidate_semantic = true;
                }
            }
        }

        for edge in removed_edges {
            self.dirty.insert(edge.to_node);
        }

        if invalidate_semantic {
            self.semantic = None;
        }

        for position in dropped_positions {
            self.node_exprs.remove(&position);
        }

        self.node_exprs
            .retain(|position, _| graph.nodes.contains_key(position));
        self.dirty
            .retain(|position| graph.nodes.contains_key(position));
    }
}

fn error(kind: DiagnosticKind, site: Option<GridPos>, edge_id: Option<EdgeId>) -> Diagnostic {
    let diagnostic = Diagnostic::error(kind, site);
    if let Some(edge_id) = edge_id {
        diagnostic.with_edge(edge_id)
    } else {
        diagnostic
    }
}

fn node_origin(pos: &GridPos) -> Origin {
    Origin {
        node: *pos,
        param: None,
    }
}

fn param_origin(pos: &GridPos, param: &str) -> Origin {
    Origin {
        node: *pos,
        param: Some(param.to_string()),
    }
}

fn unresolved_error_diagnostic(expr: &Expr, fallback: &GridPos) -> Diagnostic {
    let Some(error_expr) = expr.first_error() else {
        return error(
            DiagnosticKind::InvalidOperation {
                reason: "terminal expression contains unresolved error placeholder".into(),
            },
            Some(*fallback),
            None,
        );
    };

    let message = match &error_expr.kind {
        ExprKind::Error { message } => message.clone(),
        _ => "terminal expression contains unresolved error placeholder".into(),
    };
    let site = error_expr
        .origin
        .as_ref()
        .map(|origin| origin.node)
        .or(Some(*fallback));

    error(
        DiagnosticKind::InvalidOperation {
            reason: format!("terminal expression contains unresolved error placeholder: {message}"),
        },
        site,
        None,
    )
}

pub fn compile_graph(
    graph: &Graph,
    registry: &PieceRegistry,
    sem: &SemanticResult,
    mode: CompileMode,
) -> Result<CompileProgram, Vec<Diagnostic>> {
    compile_graph_with_opts(graph, registry, sem, mode, OptLevel::default())
}

/// Compile the graph with a specific optimization level.
///
/// Best-effort in Preview mode: broken nodes degrade to error placeholders
/// while valid subgraphs compile normally. Diagnostics are carried on the
/// returned `CompileProgram`. Runtime mode still fails if any terminal
/// contains an error.
pub fn compile_graph_with_opts(
    graph: &Graph,
    registry: &PieceRegistry,
    sem: &SemanticResult,
    mode: CompileMode,
    opt_level: OptLevel,
) -> Result<CompileProgram, Vec<Diagnostic>> {
    let (compiled_nodes, mut diagnostics) =
        compile_nodes(graph, registry, sem, mode, &BTreeMap::new(), opt_level);
    diagnostics.extend(sem.diagnostics.iter().cloned());
    build_compile_program(
        &compiled_nodes.compiled,
        sem,
        mode,
        compiled_nodes.state_updates,
        opt_level,
        diagnostics,
    )
}

pub fn compile_graph_cached(
    graph: &Graph,
    registry: &PieceRegistry,
    mode: CompileMode,
    cache: &mut CompileCache,
) -> Result<CompileProgram, Vec<Diagnostic>> {
    compile_graph_cached_with_opts(graph, registry, mode, OptLevel::default(), cache)
}

/// Cached compilation with a specific optimization level.
///
/// Best-effort: broken nodes get error placeholders while valid subgraphs
/// compile normally. Diagnostics are carried on the returned `CompileProgram`.
pub fn compile_graph_cached_with_opts(
    graph: &Graph,
    registry: &PieceRegistry,
    mode: CompileMode,
    opt_level: OptLevel,
    cache: &mut CompileCache,
) -> Result<CompileProgram, Vec<Diagnostic>> {
    cache
        .node_exprs
        .retain(|pos, _| graph.nodes.contains_key(pos));
    cache.dirty.retain(|pos| graph.nodes.contains_key(pos));

    let force_recompile_all =
        cache.last_mode != Some(mode) || cache.last_opt_level != Some(opt_level);
    if force_recompile_all {
        cache.dirty.extend(graph.nodes.keys().copied());
    }
    cache.last_mode = Some(mode);
    cache.last_opt_level = Some(opt_level);

    let needs_semantic = cache.semantic.is_none()
        || cache
            .semantic
            .as_ref()
            .is_some_and(|sem| !sem.is_valid() && !cache.dirty.is_empty());
    if needs_semantic {
        cache.semantic = Some(semantic_pass(graph, registry));
    }

    let sem = cache
        .semantic
        .as_ref()
        .expect("compile cache populates semantic result before use")
        .clone();

    let mut compiled = BTreeMap::<GridPos, Expr>::new();
    let mut multi_outputs = BTreeMap::<(GridPos, TileSide), Expr>::new();
    let mut state_updates = Vec::<NodeStateUpdate>::new();
    let mut diagnostics = sem.diagnostics.clone();

    for pos in &sem.eval_order {
        let cached_record = cache.node_exprs.get(pos).cloned();
        let is_dirty = force_recompile_all || cache.dirty.contains(pos);

        let Some(node) = graph.nodes.get(pos) else {
            diagnostics.push(error(
                DiagnosticKind::UnknownNode { pos: *pos },
                Some(*pos),
                None,
            ));
            compiled.insert(
                *pos,
                Expr::error(format!("unknown node at ({}, {})", pos.col, pos.row))
                    .with_origin(node_origin(pos)),
            );
            cache.node_exprs.remove(pos);
            mark_direct_downstream_dirty(graph, pos, &mut cache.dirty);
            continue;
        };

        let Some(piece) = registry.get(node.piece_id.as_str()) else {
            diagnostics.push(error(
                DiagnosticKind::UnknownPiece {
                    piece_id: node.piece_id.clone(),
                },
                Some(*pos),
                None,
            ));
            compiled.insert(
                *pos,
                Expr::error(format!("unknown piece: {}", node.piece_id))
                    .with_origin(node_origin(pos)),
            );
            cache.node_exprs.remove(pos);
            mark_direct_downstream_dirty(graph, pos, &mut cache.dirty);
            continue;
        };

        let resolved_types = resolved_piece_types(piece.as_ref(), node, pos, graph, &sem);

        if !is_dirty {
            if let Some(cached) = cached_record.as_ref()
                && cached.resolved_types == resolved_types
            {
                hydrate_cached_outputs(pos, cached, &mut compiled, &mut multi_outputs);
                continue;
            }
            cache.dirty.insert(*pos);
        }

        let (inputs, mut param_diags) = resolve_param_inputs(
            piece.as_ref(),
            node,
            pos,
            graph,
            &sem,
            &compiled,
            &multi_outputs,
        );
        diagnostics.append(&mut param_diags);

        if !force_recompile_all
            && cached_record.as_ref().is_some_and(|cached| {
                cache_signature_matches(cached, &inputs, node, &resolved_types)
            })
        {
            let cached = cached_record
                .as_ref()
                .expect("signature match requires cached node record");
            hydrate_cached_outputs(pos, cached, &mut compiled, &mut multi_outputs);
            cache.dirty.remove(pos);
            continue;
        }

        let (next_record, state_update) = compile_node_outputs(
            piece.as_ref(),
            &inputs,
            &resolved_types,
            node,
            pos,
            mode,
            opt_level,
        );

        if let Some(update) = state_update {
            state_updates.push(update);
        }

        let outputs_changed = cached_record.as_ref().is_none_or(|cached| {
            cached.expr != next_record.expr || cached.side_outputs != next_record.side_outputs
        });
        let output_type_changed = cached_record.as_ref().is_none_or(|cached| {
            cached.resolved_types.output_type != next_record.resolved_types.output_type
        });

        hydrate_cached_outputs(pos, &next_record, &mut compiled, &mut multi_outputs);
        cache.node_exprs.insert(*pos, next_record);
        cache.dirty.remove(pos);

        if outputs_changed || output_type_changed {
            mark_direct_downstream_dirty(graph, pos, &mut cache.dirty);
        }
    }

    build_compile_program(&compiled, &sem, mode, state_updates, opt_level, diagnostics)
}

pub fn compile_node_expr(
    graph: &Graph,
    registry: &PieceRegistry,
    sem: &SemanticResult,
    mode: CompileMode,
    root: &GridPos,
    overrides: &BTreeMap<GridPos, Expr>,
) -> Result<(Expr, Vec<NodeStateUpdate>), Vec<Diagnostic>> {
    compile_node_expr_with_opts(
        graph,
        registry,
        sem,
        mode,
        root,
        overrides,
        OptLevel::default(),
    )
}

pub fn compile_node_expr_with_opts(
    graph: &Graph,
    registry: &PieceRegistry,
    sem: &SemanticResult,
    mode: CompileMode,
    root: &GridPos,
    overrides: &BTreeMap<GridPos, Expr>,
    opt_level: OptLevel,
) -> Result<(Expr, Vec<NodeStateUpdate>), Vec<Diagnostic>> {
    let (compiled_nodes, diagnostics) =
        compile_nodes(graph, registry, sem, mode, overrides, opt_level);
    let Some(expr) = compiled_nodes.compiled.get(root).cloned() else {
        let mut errs = diagnostics;
        errs.push(error(
            DiagnosticKind::UnknownNode { pos: *root },
            Some(*root),
            None,
        ));
        return Err(errs);
    };

    Ok((expr, compiled_nodes.state_updates))
}

/// Determine which side of `from` faces `to` based on grid adjacency.
fn direction_from_to(from: &GridPos, to: &GridPos) -> Option<TileSide> {
    match (to.col - from.col, to.row - from.row) {
        (1, 0) => Some(TileSide::RIGHT),
        (-1, 0) => Some(TileSide::LEFT),
        (0, -1) => Some(TileSide::TOP),
        (0, 1) => Some(TileSide::BOTTOM),
        _ => None,
    }
}

/// Resolve all parameter inputs for a single node by checking, in priority
/// order: connected upstream expression → inline parameter value → schema
/// default → required-param error placeholder.
///
/// Best-effort: always returns an input set. When a param cannot be resolved
/// an `Expr::error(...)` placeholder is inserted so compilation can continue
/// and the error propagates naturally through downstream expressions.
fn resolve_param_inputs(
    piece: &dyn Piece,
    node: &Node,
    pos: &GridPos,
    graph: &Graph,
    sem: &SemanticResult,
    compiled: &BTreeMap<GridPos, Expr>,
    multi_outputs: &BTreeMap<(GridPos, TileSide), Expr>,
) -> (PieceInputs, Vec<Diagnostic>) {
    let mut inputs = PieceInputs::default();
    let mut diagnostics = Vec::<Diagnostic>::new();

    for param in &piece.def().params {
        let connected = incoming_edge_for_param(graph, pos, param.id.as_str()).and_then(|edge| {
            let source_expr = direction_from_to(&edge.from, &edge.to_node)
                .and_then(|exit_side| multi_outputs.get(&(edge.from, exit_side)))
                .or_else(|| compiled.get(&edge.from))
                .cloned()?;
            Some(match sem.domain_bridges.get(&edge.id) {
                Some(bridge) => Expr::domain_convert(bridge.kind, source_expr),
                None => source_expr,
            })
        });

        let resolved = if let Some(expr) = connected {
            Some(expr.with_origin_if_missing(param_origin(pos, param.id.as_str())))
        } else if let Some(value) = node.inline_params.get(param.id.as_str()) {
            if !param.schema.can_inline() {
                diagnostics.push(error(
                    DiagnosticKind::InlineNotAllowed {
                        param: param.id.clone(),
                    },
                    Some(*pos),
                    None,
                ));
                Some(
                    Expr::error(format!("inline not allowed for param '{}'", param.id))
                        .with_origin(param_origin(pos, param.id.as_str())),
                )
            } else if let Some(expr) = param.schema.inline_expr(value) {
                Some(expr.with_origin_if_missing(param_origin(pos, param.id.as_str())))
            } else {
                diagnostics.push(error(
                    DiagnosticKind::InlineTypeMismatch {
                        param: param.id.clone(),
                        expected: param.schema.expected_port_type(),
                        got_value: value.clone(),
                    },
                    Some(*pos),
                    None,
                ));
                Some(
                    Expr::error(format!("inline type mismatch for param '{}'", param.id))
                        .with_origin(param_origin(pos, param.id.as_str())),
                )
            }
        } else if let Some(default_expr) = param.schema.default_expr() {
            Some(default_expr.with_origin_if_missing(param_origin(pos, param.id.as_str())))
        } else if param.required {
            diagnostics.push(error(
                DiagnosticKind::MissingRequiredParam {
                    param: param.id.clone(),
                },
                Some(*pos),
                None,
            ));
            Some(
                Expr::error(format!("missing {}", param.id))
                    .with_origin(param_origin(pos, param.id.as_str())),
            )
        } else {
            None
        };

        if let Some(expr) = resolved {
            if let Some(group) = param.variadic_group.as_ref() {
                inputs
                    .variadic
                    .entry(group.clone())
                    .or_default()
                    .push(expr.clone());
            }
            inputs.scalar.insert(param.id.clone(), expr);
        }
    }

    (inputs, diagnostics)
}

fn resolved_piece_types(
    piece: &dyn Piece,
    node: &Node,
    pos: &GridPos,
    graph: &Graph,
    sem: &SemanticResult,
) -> ResolvedPieceTypes {
    ResolvedPieceTypes {
        input_types: resolved_input_types_for_piece(
            graph,
            node,
            pos,
            piece.def(),
            &sem.output_types,
        ),
        output_type: sem
            .output_types
            .get(pos)
            .cloned()
            .or_else(|| piece.def().output_type.clone()),
    }
}

/// Compile a single piece, handling the three-way branch for stateful vs
/// stateless pieces and recording any state transitions.
fn compile_piece_expr(
    piece: &dyn Piece,
    inputs: &PieceInputs,
    resolved_types: &ResolvedPieceTypes,
    node: &Node,
    pos: &GridPos,
    mode: CompileMode,
) -> (Expr, Option<NodeStateUpdate>) {
    let (expr, state_update) = if let Some(state) = node.node_state.as_ref() {
        let (expr, next_state) =
            piece.compile_stateful_with_types(inputs, &node.inline_params, state, resolved_types);
        let update = if mode == CompileMode::Runtime && &next_state != state {
            Some(NodeStateUpdate {
                position: *pos,
                state: next_state,
            })
        } else {
            None
        };
        (expr, update)
    } else if let Some(initial) = piece.initial_state() {
        let (expr, next_state) = piece.compile_stateful_with_types(
            inputs,
            &node.inline_params,
            &initial,
            resolved_types,
        );
        let update = if mode == CompileMode::Runtime {
            Some(NodeStateUpdate {
                position: *pos,
                state: next_state,
            })
        } else {
            None
        };
        (expr, update)
    } else {
        (
            piece.compile_with_types(inputs, &node.inline_params, resolved_types),
            None,
        )
    };

    (expr.with_origin_if_missing(node_origin(pos)), state_update)
}

/// Assign the canonical delay slot name for a node at `pos`.
fn delay_slot_name(pos: &GridPos) -> String {
    format!("d_{}_{}", pos.col, pos.row)
}

/// If `expr` is a `DelayRef` with an empty slot, fill in the slot name.
fn stamp_delay_slot(expr: Expr, pos: &GridPos) -> Expr {
    if let ExprKind::DelayRef { slot, default } = &expr.kind
        && slot.is_empty()
    {
        let origin = expr.origin.clone();
        let stamped = Expr::delay_ref(delay_slot_name(pos), default.as_ref().clone());
        return match origin {
            Some(o) => stamped.with_origin(o),
            None => stamped,
        };
    }
    expr
}

fn compile_node_outputs(
    piece: &dyn Piece,
    inputs: &PieceInputs,
    resolved_types: &ResolvedPieceTypes,
    node: &Node,
    pos: &GridPos,
    mode: CompileMode,
    opt_level: OptLevel,
) -> (CachedNodeExpr, Option<NodeStateUpdate>) {
    let (expr, state_update) = compile_piece_expr(piece, inputs, resolved_types, node, pos, mode);
    let expr = stamp_delay_slot(expr.optimize_at(opt_level), pos);
    let mut side_outputs = BTreeMap::<TileSide, Expr>::new();

    if let Some(side_exprs) =
        piece.compile_multi_output_with_types(inputs, &node.inline_params, resolved_types)
    {
        for (side, side_expr) in side_exprs {
            side_outputs.insert(
                side,
                side_expr
                    .with_origin_if_missing(node_origin(pos))
                    .optimize_at(opt_level),
            );
        }
    }

    (
        CachedNodeExpr {
            expr,
            inputs: inputs.clone(),
            inline_params: node.inline_params.clone(),
            node_state: node.node_state.clone(),
            resolved_types: resolved_types.clone(),
            side_outputs,
        },
        state_update,
    )
}

fn cache_signature_matches(
    cached: &CachedNodeExpr,
    inputs: &PieceInputs,
    node: &Node,
    resolved_types: &ResolvedPieceTypes,
) -> bool {
    cached.inputs == *inputs
        && cached.inline_params == node.inline_params
        && cached.node_state == node.node_state
        && cached.resolved_types == *resolved_types
}

fn hydrate_cached_outputs(
    pos: &GridPos,
    cached: &CachedNodeExpr,
    compiled: &mut BTreeMap<GridPos, Expr>,
    multi_outputs: &mut BTreeMap<(GridPos, TileSide), Expr>,
) {
    compiled.insert(*pos, cached.expr.clone());
    for (side, expr) in &cached.side_outputs {
        multi_outputs.insert((*pos, *side), expr.clone());
    }
}

fn mark_direct_downstream_dirty(graph: &Graph, pos: &GridPos, dirty: &mut BTreeSet<GridPos>) {
    for edge in graph.edges.values() {
        if edge.from == *pos {
            dirty.insert(edge.to_node);
        }
    }
}

fn build_compile_program(
    compiled: &BTreeMap<GridPos, Expr>,
    sem: &SemanticResult,
    mode: CompileMode,
    state_updates: Vec<NodeStateUpdate>,
    opt_level: OptLevel,
    mut diagnostics: Vec<Diagnostic>,
) -> Result<CompileProgram, Vec<Diagnostic>> {
    let mut terminals = Vec::with_capacity(sem.terminals.len());
    for terminal in &sem.terminals {
        let expr = compiled.get(terminal).cloned().unwrap_or_else(|| {
            diagnostics.push(error(
                DiagnosticKind::UnknownNode { pos: *terminal },
                Some(*terminal),
                None,
            ));
            Expr::error(format!(
                "terminal not compiled at ({}, {})",
                terminal.col, terminal.row
            ))
            .with_origin(node_origin(terminal))
        });
        terminals.push(expr);
    }

    if opt_level == OptLevel::Full {
        terminals = Expr::hoist_common_subexprs(&terminals);
    }

    if mode == CompileMode::Runtime {
        for (terminal, expr) in sem.terminals.iter().zip(terminals.iter()) {
            if expr.contains_error() {
                diagnostics.push(unresolved_error_diagnostic(expr, terminal));
                return Err(diagnostics);
            }
        }
    }

    // Collect delay slots from compiled delay nodes.
    let mut delay_slots = Vec::new();
    for (pos, expr) in compiled {
        if let ExprKind::DelayRef { slot, default } = &expr.kind {
            delay_slots.push(DelaySlot {
                slot: slot.clone(),
                node: *pos,
                default_expr: default.as_ref().clone(),
                port_type: sem.output_types.get(pos).cloned(),
            });
        }
    }

    Ok(CompileProgram {
        terminals,
        state_updates,
        activity_events: vec![],
        delay_slots,
        domain_bridges: sem.domain_bridges.values().cloned().collect(),
        diagnostics,
    })
}

/// Best-effort node compilation. Always returns compiled nodes — broken
/// nodes get `Expr::error(...)` placeholders so downstream expressions
/// degrade gracefully rather than aborting the entire graph.
fn compile_nodes(
    graph: &Graph,
    registry: &PieceRegistry,
    sem: &SemanticResult,
    mode: CompileMode,
    overrides: &BTreeMap<GridPos, Expr>,
    opt_level: OptLevel,
) -> (CompiledNodes, Vec<Diagnostic>) {
    let mut compiled = overrides
        .iter()
        .map(|(pos, expr)| (*pos, expr.clone().with_origin_if_missing(node_origin(pos))))
        .collect::<BTreeMap<_, _>>();
    let mut multi_outputs: BTreeMap<(GridPos, TileSide), Expr> = BTreeMap::new();
    let mut state_updates = Vec::<NodeStateUpdate>::new();
    let mut diagnostics = Vec::<Diagnostic>::new();

    for pos in &sem.eval_order {
        if compiled.contains_key(pos) {
            continue;
        }

        let Some(node) = graph.nodes.get(pos) else {
            diagnostics.push(error(
                DiagnosticKind::UnknownNode { pos: *pos },
                Some(*pos),
                None,
            ));
            compiled.insert(
                *pos,
                Expr::error(format!("unknown node at ({}, {})", pos.col, pos.row))
                    .with_origin(node_origin(pos)),
            );
            continue;
        };

        let Some(piece) = registry.get(node.piece_id.as_str()) else {
            diagnostics.push(error(
                DiagnosticKind::UnknownPiece {
                    piece_id: node.piece_id.clone(),
                },
                Some(*pos),
                None,
            ));
            compiled.insert(
                *pos,
                Expr::error(format!("unknown piece: {}", node.piece_id))
                    .with_origin(node_origin(pos)),
            );
            continue;
        };

        let resolved_types = resolved_piece_types(piece.as_ref(), node, pos, graph, sem);
        let (inputs, mut param_diags) = resolve_param_inputs(
            piece.as_ref(),
            node,
            pos,
            graph,
            sem,
            &compiled,
            &multi_outputs,
        );
        diagnostics.append(&mut param_diags);

        let (cached_outputs, state_update) = compile_node_outputs(
            piece.as_ref(),
            &inputs,
            &resolved_types,
            node,
            pos,
            mode,
            opt_level,
        );

        if let Some(update) = state_update {
            state_updates.push(update);
        }

        hydrate_cached_outputs(pos, &cached_outputs, &mut compiled, &mut multi_outputs);
    }

    (
        CompiledNodes {
            compiled,
            state_updates,
        },
        diagnostics,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use serde_json::Value;
    use serde_json::json;

    use super::*;
    use crate::ast::{BinOp, Expr, ExprKind, Origin};
    use crate::backend::{Backend, JsBackend};
    use crate::graph::{Edge, Graph, GraphOp, Node};
    use crate::ops::apply_ops_to_graph_cached;
    use crate::piece::{
        ParamDef, ParamInlineMode, ParamSchema, ParamValueKind, Piece, PieceDef, PieceInputs,
        ResolvedPieceTypes,
    };
    use crate::piece_registry::PieceRegistry;
    use crate::semantic::semantic_pass;
    use crate::types::{
        DomainBridgeKind, EdgeId, ExecutionDomain, GridPos, PieceCategory, PieceSemanticKind,
        PortType, TileSide,
    };

    struct SourcePiece {
        def: PieceDef,
    }

    impl SourcePiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.source".into(),
                    label: "source".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "value".into(),
                        label: "value".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Text {
                            default: "a".into(),
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                    }],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for SourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, inputs: &PieceInputs, inline_params: &BTreeMap<String, Value>) -> Expr {
            inputs
                .get("value")
                .cloned()
                .or_else(|| inline_params.get("value").map(Expr::from_json_value))
                .unwrap_or_else(|| Expr::str_lit("a"))
        }
    }

    struct MethodPiece {
        def: PieceDef,
    }

    impl MethodPiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.method".into(),
                    label: "method".into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "pattern".into(),
                        label: "pattern".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Text {
                            default: String::new(),
                            can_inline: false,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: true,
                    }],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for MethodPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::method_call(
                inputs
                    .get("pattern")
                    .cloned()
                    .unwrap_or_else(|| Expr::error("missing")),
                "fast",
                Vec::new(),
            )
        }
    }

    struct TerminalPiece {
        def: PieceDef,
    }

    impl TerminalPiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.output".into(),
                    label: "output".into(),
                    category: PieceCategory::Output,
                    semantic_kind: PieceSemanticKind::Output,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "pattern".into(),
                        label: "pattern".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Text {
                            default: String::new(),
                            can_inline: false,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: true,
                    }],
                    output_type: None,
                    output_side: None,
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for TerminalPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            inputs
                .get("pattern")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing"))
        }
    }

    fn registry() -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(SourcePiece::new());
        registry.register(MethodPiece::new());
        registry.register(TerminalPiece::new());
        registry
    }

    fn graph() -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let method_pos = GridPos { col: 1, row: 0 };
        let output_pos = GridPos { col: 2, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: method_pos,
            to_param: "pattern".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: method_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };
        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.source".into(),
                        inline_params: BTreeMap::from([(
                            "value".into(),
                            Value::String("bd".into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    method_pos,
                    Node {
                        piece_id: "test.method".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "test.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a), (edge_b.id.clone(), edge_b)]),
            name: "compiler".into(),
            cols: 4,
            rows: 1,
        }
    }

    fn inferred_dispatch_registry(dispatch_count: Arc<AtomicUsize>) -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(SourcePiece::new());
        registry.register(NumberSourcePiece::new());
        registry.register(GenericForwardPiece::new());
        registry.register(AdHocDispatchPiece::new("test.dispatch", dispatch_count));
        registry.register(TerminalPiece::new());
        registry
    }

    fn inferred_dispatch_graph(source_piece_id: &str) -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let forward_pos = GridPos { col: 1, row: 0 };
        let dispatch_pos = GridPos { col: 2, row: 0 };
        let output_pos = GridPos { col: 3, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: forward_pos,
            to_param: "value".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: forward_pos,
            to_node: dispatch_pos,
            to_param: "value".into(),
        };
        let edge_c = Edge {
            id: EdgeId::new(),
            from: dispatch_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };

        let inline_params = if source_piece_id == "test.source" {
            BTreeMap::from([("value".into(), Value::String("bd".into()))])
        } else {
            BTreeMap::new()
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: source_piece_id.into(),
                        inline_params,
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    forward_pos,
                    Node {
                        piece_id: "test.forward".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    dispatch_pos,
                    Node {
                        piece_id: "test.dispatch".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "test.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([
                (edge_a.id.clone(), edge_a),
                (edge_b.id.clone(), edge_b),
                (edge_c.id.clone(), edge_c),
            ]),
            name: format!("dispatch_{source_piece_id}"),
            cols: 4,
            rows: 1,
        }
    }

    #[test]
    fn compile_node_expr_returns_selected_expression() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);

        let (expr, updates) = compile_node_expr(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            &GridPos { col: 1, row: 0 },
            &BTreeMap::new(),
        )
        .expect("compile method");

        assert!(updates.is_empty());
        assert_eq!(JsBackend.render(&expr), "'bd'.fast()");
    }

    #[test]
    fn compile_node_expr_uses_overrides_for_downstream_nodes() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);
        let overrides = BTreeMap::from([(GridPos { col: 0, row: 0 }, Expr::ident("arg1"))]);

        let (expr, _) = compile_node_expr(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            &GridPos { col: 2, row: 0 },
            &overrides,
        )
        .expect("compile terminal");

        assert_eq!(JsBackend.render(&expr), "arg1.fast()");
    }

    #[test]
    fn compile_graph_without_overrides_matches_existing_behavior() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);

        let program =
            compile_graph(&graph, &registry, &sem, CompileMode::Preview).expect("compile graph");

        assert_eq!(program.terminals.len(), 1);
        assert_eq!(JsBackend.render(&program.terminals[0]), "'bd'.fast()");
    }

    #[test]
    fn compile_graph_dispatches_piece_compile_by_inferred_input_types() {
        let dispatch_count = Arc::new(AtomicUsize::new(0));
        let registry = inferred_dispatch_registry(dispatch_count.clone());

        let number_graph = inferred_dispatch_graph("test.number_source");
        let number_sem = semantic_pass(&number_graph, &registry);
        assert_eq!(
            number_sem.output_types.get(&GridPos { col: 1, row: 0 }),
            Some(&PortType::number())
        );
        let number_program =
            compile_graph(&number_graph, &registry, &number_sem, CompileMode::Preview)
                .expect("compile number dispatch graph");
        assert_eq!(
            JsBackend.render(&number_program.terminals[0]),
            "useNumber(7)"
        );

        let text_graph = inferred_dispatch_graph("test.source");
        let text_sem = semantic_pass(&text_graph, &registry);
        assert_eq!(
            text_sem.output_types.get(&GridPos { col: 1, row: 0 }),
            Some(&PortType::text())
        );
        let text_program = compile_graph(&text_graph, &registry, &text_sem, CompileMode::Preview)
            .expect("compile text dispatch graph");
        assert_eq!(
            JsBackend.render(&text_program.terminals[0]),
            "useText('bd')"
        );

        assert_eq!(dispatch_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn compile_inline_param_attaches_param_origin() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);

        let (expr, _) = compile_node_expr(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            &GridPos { col: 0, row: 0 },
            &BTreeMap::new(),
        )
        .expect("compile source");

        assert_eq!(
            expr.origin,
            Some(Origin {
                node: GridPos { col: 0, row: 0 },
                param: Some("value".into()),
            })
        );
    }

    struct ErrorSourcePiece {
        def: PieceDef,
    }

    impl ErrorSourcePiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.error_source".into(),
                    label: "broken".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for ErrorSourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::error("source missing")
        }
    }

    #[test]
    fn runtime_compile_reports_origin_of_nested_error() {
        let source_pos = GridPos { col: 0, row: 0 };
        let output_pos = GridPos { col: 1, row: 0 };
        let edge = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };

        let graph = Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.error_source".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "test.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge.id.clone(), edge)]),
            name: "runtime_error_origin".into(),
            cols: 2,
            rows: 1,
        };

        let mut registry = PieceRegistry::new();
        registry.register(ErrorSourcePiece::new());
        registry.register(TerminalPiece::new());

        let sem = semantic_pass(&graph, &registry);
        let err = compile_graph(&graph, &registry, &sem, CompileMode::Runtime)
            .expect_err("runtime error");

        assert_eq!(err.len(), 1);
        assert_eq!(err[0].site, Some(source_pos));
        match &err[0].kind {
            DiagnosticKind::InvalidOperation { reason } => {
                assert!(reason.contains("source missing"));
            }
            other => panic!("unexpected diagnostic: {other:?}"),
        }
    }

    #[test]
    fn compile_override_attaches_node_origin_when_missing() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);
        let overrides = BTreeMap::from([(GridPos { col: 0, row: 0 }, Expr::ident("arg1"))]);

        let (expr, _) = compile_node_expr(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            &GridPos { col: 2, row: 0 },
            &overrides,
        )
        .expect("compile terminal");

        match expr.kind {
            ExprKind::MethodCall { receiver, .. } => {
                assert_eq!(
                    receiver.origin,
                    Some(Origin {
                        node: GridPos { col: 0, row: 0 },
                        param: None,
                    })
                );
            }
            other => panic!("expected method call, got {other:?}"),
        }
    }

    fn text_param(
        id: &str,
        side: TileSide,
        can_inline: bool,
        required: bool,
        default: &str,
    ) -> ParamDef {
        ParamDef {
            id: id.into(),
            label: id.into(),
            side,
            schema: ParamSchema::Text {
                default: default.into(),
                can_inline,
            },
            text_semantics: Default::default(),
            variadic_group: None,
            required,
        }
    }

    fn any_param(id: &str, side: TileSide, can_inline: bool, required: bool) -> ParamDef {
        ParamDef {
            id: id.into(),
            label: id.into(),
            side,
            schema: ParamSchema::Custom {
                port_type: PortType::any(),
                value_kind: ParamValueKind::Json,
                default: None,
                can_inline,
                inline_mode: ParamInlineMode::Literal,
                min: None,
                max: None,
            },
            text_semantics: Default::default(),
            variadic_group: None,
            required,
        }
    }

    struct NumberSourcePiece {
        def: PieceDef,
    }

    impl NumberSourcePiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.number_source".into(),
                    label: "number source".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![],
                    output_type: Some(PortType::number()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for NumberSourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::int(7)
        }
    }

    struct GenericForwardPiece {
        def: PieceDef,
    }

    impl GenericForwardPiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.forward".into(),
                    label: "forward".into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![any_param("value", TileSide::LEFT, false, true)],
                    output_type: Some(PortType::any()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for GenericForwardPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            inputs
                .get("value")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing value"))
        }

        fn infer_output_type(
            &self,
            input_types: &BTreeMap<String, PortType>,
            _inline_params: &BTreeMap<String, Value>,
        ) -> Option<PortType> {
            input_types
                .get("value")
                .cloned()
                .or_else(|| Some(PortType::any()))
        }
    }

    struct AdHocDispatchPiece {
        def: PieceDef,
        compile_count: Arc<AtomicUsize>,
    }

    impl AdHocDispatchPiece {
        fn new(id: &str, compile_count: Arc<AtomicUsize>) -> Self {
            Self {
                def: PieceDef {
                    id: id.into(),
                    label: "dispatch".into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![any_param("value", TileSide::LEFT, false, true)],
                    output_type: Some(PortType::text()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
                compile_count,
            }
        }
    }

    impl Piece for AdHocDispatchPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::error("compile_with_types should dispatch this piece")
        }

        fn compile_with_types(
            &self,
            inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
            resolved_types: &ResolvedPieceTypes,
        ) -> Expr {
            self.compile_count.fetch_add(1, Ordering::SeqCst);

            let value = inputs
                .get("value")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing value"));
            match resolved_types.input_type("value").map(PortType::as_str) {
                Some("number") => Expr::call_named("useNumber", vec![value]),
                Some("text") => Expr::call_named("useText", vec![value]),
                Some(other) => Expr::call_named(format!("use_{other}"), vec![value]),
                None => Expr::call_named("useUnknown", vec![value]),
            }
        }
    }

    struct StableTypeSourcePiece {
        def: PieceDef,
        compile_count: Arc<AtomicUsize>,
    }

    impl StableTypeSourcePiece {
        fn new(compile_count: Arc<AtomicUsize>) -> Self {
            Self {
                def: PieceDef {
                    id: "test.stable_type_source".into(),
                    label: "stable type source".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![text_param("kind", TileSide::BOTTOM, true, false, "text")],
                    output_type: Some(PortType::any()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
                compile_count,
            }
        }
    }

    impl Piece for StableTypeSourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            self.compile_count.fetch_add(1, Ordering::SeqCst);
            Expr::ident("shared")
        }

        fn infer_output_type(
            &self,
            _input_types: &BTreeMap<String, PortType>,
            inline_params: &BTreeMap<String, Value>,
        ) -> Option<PortType> {
            match inline_params.get("kind").and_then(Value::as_str) {
                Some("number") => Some(PortType::number()),
                Some("text") | None => Some(PortType::text()),
                Some(other) => Some(PortType::from(other.to_string())),
            }
        }
    }

    struct StableDomainSourcePiece {
        def: PieceDef,
        compile_count: Arc<AtomicUsize>,
    }

    impl StableDomainSourcePiece {
        fn new(compile_count: Arc<AtomicUsize>) -> Self {
            Self {
                def: PieceDef {
                    id: "test.stable_domain_source".into(),
                    label: "stable domain source".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![text_param("domain", TileSide::BOTTOM, true, false, "control")],
                    output_type: Some(PortType::number()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
                compile_count,
            }
        }
    }

    impl Piece for StableDomainSourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            self.compile_count.fetch_add(1, Ordering::SeqCst);
            Expr::ident("shared")
        }

        fn infer_output_type(
            &self,
            _input_types: &BTreeMap<String, PortType>,
            inline_params: &BTreeMap<String, Value>,
        ) -> Option<PortType> {
            let domain = match inline_params.get("domain").and_then(Value::as_str) {
                Some("audio") => ExecutionDomain::Audio,
                Some("event") => ExecutionDomain::Event,
                Some("control") | None => ExecutionDomain::Control,
                Some(_) => ExecutionDomain::Control,
            };
            Some(PortType::number().with_domain(domain))
        }
    }

    struct BridgeDispatchPiece {
        def: PieceDef,
        compile_count: Arc<AtomicUsize>,
    }

    impl BridgeDispatchPiece {
        fn new(compile_count: Arc<AtomicUsize>) -> Self {
            Self {
                def: PieceDef {
                    id: "test.bridge_dispatch".into(),
                    label: "bridge dispatch".into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "value".into(),
                        label: "value".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Custom {
                            port_type: PortType::number().with_domain(ExecutionDomain::Audio),
                            value_kind: ParamValueKind::Json,
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
                    output_type: Some(PortType::text()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
                compile_count,
            }
        }
    }

    impl Piece for BridgeDispatchPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::error("compile_with_types should dispatch this bridge-aware piece")
        }

        fn compile_with_types(
            &self,
            inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
            resolved_types: &ResolvedPieceTypes,
        ) -> Expr {
            self.compile_count.fetch_add(1, Ordering::SeqCst);
            let value = inputs
                .get("value")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing value"));
            let callee = match resolved_types.input_type("value").and_then(PortType::domain) {
                Some(ExecutionDomain::Audio) => "useAudio",
                Some(ExecutionDomain::Control) => "useControl",
                Some(ExecutionDomain::Event) => "useEvent",
                None => "useUnknown",
            };
            Expr::call_named(callee, vec![value])
        }
    }

    struct BridgeMultiOutputPiece {
        def: PieceDef,
    }

    impl BridgeMultiOutputPiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.bridge_multi".into(),
                    label: "bridge multi".into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "value".into(),
                        label: "value".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Custom {
                            port_type: PortType::number().with_domain(ExecutionDomain::Audio),
                            value_kind: ParamValueKind::Json,
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
                    output_type: Some(PortType::text()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for BridgeMultiOutputPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::error("compile_multi_output_with_types should dispatch this bridge-aware piece")
        }

        fn compile_multi_output_with_types(
            &self,
            inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
            resolved_types: &ResolvedPieceTypes,
        ) -> Option<BTreeMap<TileSide, Expr>> {
            let value = inputs
                .get("value")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing value"));
            let callee = match resolved_types.input_type("value").and_then(PortType::domain) {
                Some(ExecutionDomain::Audio) => "useAudio",
                Some(ExecutionDomain::Control) => "useControl",
                Some(ExecutionDomain::Event) => "useEvent",
                None => "useUnknown",
            };
            Some(BTreeMap::from([(
                TileSide::RIGHT,
                Expr::call_named(callee, vec![value]),
            )]))
        }
    }

    struct BridgeStatefulPiece {
        def: PieceDef,
    }

    impl BridgeStatefulPiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.bridge_stateful".into(),
                    label: "bridge stateful".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "value".into(),
                        label: "value".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Custom {
                            port_type: PortType::number().with_domain(ExecutionDomain::Audio),
                            value_kind: ParamValueKind::Json,
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
                    output_type: Some(PortType::text()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for BridgeStatefulPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::error("compile_stateful_with_types should dispatch this bridge-aware piece")
        }

        fn initial_state(&self) -> Option<Value> {
            Some(json!(0))
        }

        fn compile_stateful_with_types(
            &self,
            inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
            state: &Value,
            resolved_types: &ResolvedPieceTypes,
        ) -> (Expr, Value) {
            let value = inputs
                .get("value")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing value"));
            let callee = match resolved_types.input_type("value").and_then(PortType::domain) {
                Some(ExecutionDomain::Audio) => "useAudio",
                Some(ExecutionDomain::Control) => "useControl",
                Some(ExecutionDomain::Event) => "useEvent",
                None => "useUnknown",
            };
            let tick = state.as_i64().unwrap_or(0);
            (Expr::call_named(callee, vec![value]), json!(tick + 1))
        }
    }

    struct CountedSourcePiece {
        def: PieceDef,
        compile_count: Arc<AtomicUsize>,
    }

    impl CountedSourcePiece {
        fn new(id: &str, compile_count: Arc<AtomicUsize>) -> Self {
            Self {
                def: PieceDef {
                    id: id.into(),
                    label: id.into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![text_param("value", TileSide::BOTTOM, true, false, "a")],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
                compile_count,
            }
        }
    }

    impl Piece for CountedSourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, inputs: &PieceInputs, inline_params: &BTreeMap<String, Value>) -> Expr {
            self.compile_count.fetch_add(1, Ordering::SeqCst);
            inputs
                .get("value")
                .cloned()
                .or_else(|| inline_params.get("value").map(Expr::from_json_value))
                .unwrap_or_else(|| Expr::str_lit("a"))
        }
    }

    struct CountedTransformPiece {
        def: PieceDef,
        compile_count: Arc<AtomicUsize>,
    }

    impl CountedTransformPiece {
        fn new(compile_count: Arc<AtomicUsize>) -> Self {
            Self {
                def: PieceDef {
                    id: "count.transform".into(),
                    label: "count.transform".into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![
                        text_param("pattern", TileSide::LEFT, false, true, ""),
                        text_param("suffix", TileSide::TOP, true, false, "fast"),
                    ],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
                compile_count,
            }
        }
    }

    impl Piece for CountedTransformPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, inputs: &PieceInputs, inline_params: &BTreeMap<String, Value>) -> Expr {
            self.compile_count.fetch_add(1, Ordering::SeqCst);
            let receiver = inputs
                .get("pattern")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing"));
            let suffix = inline_params
                .get("suffix")
                .and_then(Value::as_str)
                .unwrap_or("fast");
            Expr::method_call(receiver, suffix, Vec::new())
        }
    }

    struct CountedMergePiece {
        def: PieceDef,
        compile_count: Arc<AtomicUsize>,
    }

    impl CountedMergePiece {
        fn new(compile_count: Arc<AtomicUsize>) -> Self {
            Self {
                def: PieceDef {
                    id: "count.merge".into(),
                    label: "count.merge".into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![
                        text_param("a", TileSide::LEFT, false, true, ""),
                        ParamDef {
                            id: "b".into(),
                            label: "b".into(),
                            side: TileSide::BOTTOM,
                            schema: ParamSchema::Custom {
                                port_type: PortType::text(),
                                value_kind: ParamValueKind::Text,
                                default: None,
                                can_inline: false,
                                inline_mode: ParamInlineMode::Literal,
                                min: None,
                                max: None,
                            },
                            text_semantics: Default::default(),
                            variadic_group: None,
                            required: false,
                        },
                    ],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
                compile_count,
            }
        }
    }

    impl Piece for CountedMergePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            self.compile_count.fetch_add(1, Ordering::SeqCst);
            let base = inputs
                .get("a")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing a"));
            if let Some(extra) = inputs.get("b").cloned() {
                Expr::method_call(base, "layer", vec![extra])
            } else {
                base
            }
        }
    }

    struct CountedOutputPiece {
        def: PieceDef,
        compile_count: Arc<AtomicUsize>,
    }

    impl CountedOutputPiece {
        fn new(compile_count: Arc<AtomicUsize>) -> Self {
            Self {
                def: PieceDef {
                    id: "count.output".into(),
                    label: "count.output".into(),
                    category: PieceCategory::Output,
                    semantic_kind: PieceSemanticKind::Output,
                    namespace: "core".into(),
                    params: vec![text_param("pattern", TileSide::LEFT, false, true, "")],
                    output_type: None,
                    output_side: None,
                    description: None,
                    tags: vec![],
                },
                compile_count,
            }
        }
    }

    impl Piece for CountedOutputPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            self.compile_count.fetch_add(1, Ordering::SeqCst);
            inputs
                .get("pattern")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing output"))
        }
    }

    struct CountedMultiOutputPiece {
        def: PieceDef,
        compile_count: Arc<AtomicUsize>,
    }

    impl CountedMultiOutputPiece {
        fn new(compile_count: Arc<AtomicUsize>) -> Self {
            Self {
                def: PieceDef {
                    id: "count.multi".into(),
                    label: "count.multi".into(),
                    category: PieceCategory::Transform,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![text_param("emit", TileSide::TOP, true, false, "alpha")],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
                compile_count,
            }
        }
    }

    impl Piece for CountedMultiOutputPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            self.compile_count.fetch_add(1, Ordering::SeqCst);
            Expr::str_lit("primary")
        }

        fn compile_multi_output(
            &self,
            _inputs: &PieceInputs,
            inline_params: &BTreeMap<String, Value>,
        ) -> Option<BTreeMap<TileSide, Expr>> {
            let emit = inline_params
                .get("emit")
                .and_then(Value::as_str)
                .unwrap_or("alpha");
            Some(BTreeMap::from([(TileSide::RIGHT, Expr::str_lit(emit))]))
        }
    }

    struct CountedStatefulPiece {
        def: PieceDef,
        compile_count: Arc<AtomicUsize>,
    }

    impl CountedStatefulPiece {
        fn new(compile_count: Arc<AtomicUsize>) -> Self {
            Self {
                def: PieceDef {
                    id: "count.stateful".into(),
                    label: "count.stateful".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![],
                    output_type: Some("text".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
                compile_count,
            }
        }
    }

    impl Piece for CountedStatefulPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::str_lit("stateful")
        }

        fn initial_state(&self) -> Option<Value> {
            Some(json!(0))
        }

        fn compile_stateful(
            &self,
            _inputs: &PieceInputs,
            _inline_params: &BTreeMap<String, Value>,
            state: &Value,
        ) -> (Expr, Value) {
            self.compile_count.fetch_add(1, Ordering::SeqCst);
            let tick = state.as_i64().unwrap_or(0);
            (Expr::str_lit(format!("tick-{tick}")), json!(tick + 1))
        }
    }

    struct ArithmeticSourcePiece {
        def: PieceDef,
    }

    impl ArithmeticSourcePiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "opt.arithmetic".into(),
                    label: "opt.arithmetic".into(),
                    category: PieceCategory::Generator,
                    semantic_kind: PieceSemanticKind::Operator,
                    namespace: "core".into(),
                    params: vec![],
                    output_type: Some("number".into()),
                    output_side: Some(TileSide::RIGHT),
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for ArithmeticSourcePiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::bin_op(BinOp::Add, Expr::int(1), Expr::int(2))
        }
    }

    struct NumericOutputPiece {
        def: PieceDef,
    }

    impl NumericOutputPiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "opt.output".into(),
                    label: "opt.output".into(),
                    category: PieceCategory::Output,
                    semantic_kind: PieceSemanticKind::Output,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "value".into(),
                        label: "value".into(),
                        side: TileSide::LEFT,
                        schema: ParamSchema::Number {
                            default: 0.0,
                            min: None,
                            max: None,
                            can_inline: false,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: true,
                    }],
                    output_type: None,
                    output_side: None,
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for NumericOutputPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            inputs
                .get("value")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing output"))
        }
    }

    fn counted_registry(
        source_count: Arc<AtomicUsize>,
        transform_count: Arc<AtomicUsize>,
        output_count: Arc<AtomicUsize>,
    ) -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(CountedSourcePiece::new("count.source", source_count));
        registry.register(CountedTransformPiece::new(transform_count));
        registry.register(CountedOutputPiece::new(output_count));
        registry
    }

    fn merge_registry(
        source_a_count: Arc<AtomicUsize>,
        source_b_count: Arc<AtomicUsize>,
        merge_count: Arc<AtomicUsize>,
        output_count: Arc<AtomicUsize>,
    ) -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(CountedSourcePiece::new("count.source_a", source_a_count));
        registry.register(CountedSourcePiece::new("count.source_b", source_b_count));
        registry.register(CountedMergePiece::new(merge_count));
        registry.register(CountedOutputPiece::new(output_count));
        registry
    }

    fn multi_output_registry(
        multi_count: Arc<AtomicUsize>,
        output_count: Arc<AtomicUsize>,
    ) -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(CountedMultiOutputPiece::new(multi_count));
        registry.register(CountedOutputPiece::new(output_count));
        registry
    }

    fn stateful_registry(
        stateful_count: Arc<AtomicUsize>,
        output_count: Arc<AtomicUsize>,
    ) -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(CountedStatefulPiece::new(stateful_count));
        registry.register(CountedOutputPiece::new(output_count));
        registry
    }

    fn stable_type_registry(
        source_count: Arc<AtomicUsize>,
        dispatch_count: Arc<AtomicUsize>,
        output_count: Arc<AtomicUsize>,
    ) -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(StableTypeSourcePiece::new(source_count));
        registry.register(AdHocDispatchPiece::new("test.dispatch", dispatch_count));
        registry.register(CountedOutputPiece::new(output_count));
        registry
    }

    fn bridge_dispatch_registry(
        source_count: Arc<AtomicUsize>,
        dispatch_count: Arc<AtomicUsize>,
        output_count: Arc<AtomicUsize>,
    ) -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(StableDomainSourcePiece::new(source_count));
        registry.register(BridgeDispatchPiece::new(dispatch_count));
        registry.register(CountedOutputPiece::new(output_count));
        registry
    }

    fn bridge_multi_registry() -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(StableDomainSourcePiece::new(Arc::new(AtomicUsize::new(0))));
        registry.register(BridgeMultiOutputPiece::new());
        registry.register(CountedOutputPiece::new(Arc::new(AtomicUsize::new(0))));
        registry
    }

    fn bridge_stateful_registry() -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(StableDomainSourcePiece::new(Arc::new(AtomicUsize::new(0))));
        registry.register(BridgeStatefulPiece::new());
        registry.register(CountedOutputPiece::new(Arc::new(AtomicUsize::new(0))));
        registry
    }

    fn numeric_registry() -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(ArithmeticSourcePiece::new());
        registry.register(NumericOutputPiece::new());
        registry
    }

    fn counted_chain_graph(value: &str, suffix: &str) -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let transform_pos = GridPos { col: 1, row: 0 };
        let output_pos = GridPos { col: 2, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: transform_pos,
            to_param: "pattern".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: transform_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "count.source".into(),
                        inline_params: BTreeMap::from([(
                            "value".into(),
                            Value::String(value.into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    transform_pos,
                    Node {
                        piece_id: "count.transform".into(),
                        inline_params: BTreeMap::from([(
                            "suffix".into(),
                            Value::String(suffix.into()),
                        )]),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "count.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a), (edge_b.id.clone(), edge_b)]),
            name: "counted_chain".into(),
            cols: 4,
            rows: 2,
        }
    }

    fn stable_type_graph(kind: &str) -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let dispatch_pos = GridPos { col: 1, row: 0 };
        let output_pos = GridPos { col: 2, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: dispatch_pos,
            to_param: "value".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: dispatch_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.stable_type_source".into(),
                        inline_params: BTreeMap::from([(
                            "kind".into(),
                            Value::String(kind.into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    dispatch_pos,
                    Node {
                        piece_id: "test.dispatch".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "count.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a), (edge_b.id.clone(), edge_b)]),
            name: "stable_type".into(),
            cols: 3,
            rows: 1,
        }
    }

    fn bridge_dispatch_graph(source_domain: &str) -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let dispatch_pos = GridPos { col: 1, row: 0 };
        let output_pos = GridPos { col: 2, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: dispatch_pos,
            to_param: "value".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: dispatch_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.stable_domain_source".into(),
                        inline_params: BTreeMap::from([(
                            "domain".into(),
                            Value::String(source_domain.into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    dispatch_pos,
                    Node {
                        piece_id: "test.bridge_dispatch".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "count.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a), (edge_b.id.clone(), edge_b)]),
            name: "bridge_dispatch".into(),
            cols: 3,
            rows: 1,
        }
    }

    fn bridge_multi_graph(source_domain: &str) -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let multi_pos = GridPos { col: 1, row: 0 };
        let output_pos = GridPos { col: 2, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: multi_pos,
            to_param: "value".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: multi_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.stable_domain_source".into(),
                        inline_params: BTreeMap::from([(
                            "domain".into(),
                            Value::String(source_domain.into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    multi_pos,
                    Node {
                        piece_id: "test.bridge_multi".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "count.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a), (edge_b.id.clone(), edge_b)]),
            name: "bridge_multi".into(),
            cols: 3,
            rows: 1,
        }
    }

    fn bridge_stateful_graph(source_domain: &str) -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let stateful_pos = GridPos { col: 1, row: 0 };
        let output_pos = GridPos { col: 2, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: stateful_pos,
            to_param: "value".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: stateful_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.stable_domain_source".into(),
                        inline_params: BTreeMap::from([(
                            "domain".into(),
                            Value::String(source_domain.into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    stateful_pos,
                    Node {
                        piece_id: "test.bridge_stateful".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "count.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a), (edge_b.id.clone(), edge_b)]),
            name: "bridge_stateful".into(),
            cols: 3,
            rows: 1,
        }
    }

    fn merge_graph() -> (Graph, EdgeId) {
        let source_a_pos = GridPos { col: 0, row: 0 };
        let merge_pos = GridPos { col: 1, row: 0 };
        let output_pos = GridPos { col: 2, row: 0 };
        let source_b_pos = GridPos { col: 1, row: 1 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_a_pos,
            to_node: merge_pos,
            to_param: "a".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: source_b_pos,
            to_node: merge_pos,
            to_param: "b".into(),
        };
        let edge_out = Edge {
            id: EdgeId::new(),
            from: merge_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };

        let graph = Graph {
            nodes: BTreeMap::from([
                (
                    source_a_pos,
                    Node {
                        piece_id: "count.source_a".into(),
                        inline_params: BTreeMap::from([(
                            "value".into(),
                            Value::String("bd".into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    source_b_pos,
                    Node {
                        piece_id: "count.source_b".into(),
                        inline_params: BTreeMap::from([(
                            "value".into(),
                            Value::String("sd".into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: Some(TileSide::TOP),
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    merge_pos,
                    Node {
                        piece_id: "count.merge".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([
                            ("a".into(), TileSide::LEFT),
                            ("b".into(), TileSide::BOTTOM),
                        ]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "count.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([
                (edge_a.id.clone(), edge_a),
                (edge_b.id.clone(), edge_b.clone()),
                (edge_out.id.clone(), edge_out),
            ]),
            name: "merge".into(),
            cols: 4,
            rows: 2,
        };

        (graph, edge_b.id)
    }

    fn multi_output_graph(emit: &str) -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let output_pos = GridPos { col: 1, row: 0 };
        let edge = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "count.multi".into(),
                        inline_params: BTreeMap::from([(
                            "emit".into(),
                            Value::String(emit.into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "count.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge.id.clone(), edge)]),
            name: "multi_output".into(),
            cols: 3,
            rows: 1,
        }
    }

    fn stateful_graph() -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let output_pos = GridPos { col: 1, row: 0 };
        let edge = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "count.stateful".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "count.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge.id.clone(), edge)]),
            name: "stateful".into(),
            cols: 3,
            rows: 1,
        }
    }

    fn arithmetic_graph() -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let output_pos = GridPos { col: 1, row: 0 };
        let edge = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: output_pos,
            to_param: "value".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "opt.arithmetic".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "opt.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("value".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge.id.clone(), edge)]),
            name: "arithmetic".into(),
            cols: 3,
            rows: 1,
        }
    }

    fn duplicate_terminal_graph() -> Graph {
        let source_a_pos = GridPos { col: 0, row: 0 };
        let transform_a_pos = GridPos { col: 1, row: 0 };
        let output_a_pos = GridPos { col: 2, row: 0 };
        let source_b_pos = GridPos { col: 0, row: 1 };
        let transform_b_pos = GridPos { col: 1, row: 1 };
        let output_b_pos = GridPos { col: 2, row: 1 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_a_pos,
            to_node: transform_a_pos,
            to_param: "pattern".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: transform_a_pos,
            to_node: output_a_pos,
            to_param: "pattern".into(),
        };
        let edge_c = Edge {
            id: EdgeId::new(),
            from: source_b_pos,
            to_node: transform_b_pos,
            to_param: "pattern".into(),
        };
        let edge_d = Edge {
            id: EdgeId::new(),
            from: transform_b_pos,
            to_node: output_b_pos,
            to_param: "pattern".into(),
        };

        Graph {
            nodes: BTreeMap::from([
                (
                    source_a_pos,
                    Node {
                        piece_id: "count.source".into(),
                        inline_params: BTreeMap::from([(
                            "value".into(),
                            Value::String("bd".into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    transform_a_pos,
                    Node {
                        piece_id: "count.transform".into(),
                        inline_params: BTreeMap::from([(
                            "suffix".into(),
                            Value::String("fast".into()),
                        )]),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    source_b_pos,
                    Node {
                        piece_id: "count.source".into(),
                        inline_params: BTreeMap::from([(
                            "value".into(),
                            Value::String("bd".into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    transform_b_pos,
                    Node {
                        piece_id: "count.transform".into(),
                        inline_params: BTreeMap::from([(
                            "suffix".into(),
                            Value::String("fast".into()),
                        )]),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_a_pos,
                    Node {
                        piece_id: "count.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_b_pos,
                    Node {
                        piece_id: "count.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([
                (edge_a.id.clone(), edge_a),
                (edge_b.id.clone(), edge_b),
                (edge_c.id.clone(), edge_c),
                (edge_d.id.clone(), edge_d),
            ]),
            name: "duplicate_terminals".into(),
            cols: 4,
            rows: 2,
        }
    }

    fn fake_cached_expr(label: &str) -> CachedNodeExpr {
        CachedNodeExpr {
            expr: Expr::str_lit(label),
            inputs: PieceInputs::default(),
            inline_params: BTreeMap::new(),
            node_state: None,
            resolved_types: ResolvedPieceTypes::default(),
            side_outputs: BTreeMap::new(),
        }
    }

    #[test]
    fn compile_graph_cached_matches_from_scratch_on_cold_cache() {
        let graph = graph();
        let registry = registry();
        let sem = semantic_pass(&graph, &registry);

        let baseline =
            compile_graph(&graph, &registry, &sem, CompileMode::Preview).expect("baseline");
        let mut cache = CompileCache::new();
        let cached = compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("cached compile");

        assert_eq!(cached.terminals, baseline.terminals);
        assert!(cached.state_updates.is_empty());
        assert!(cache.semantic.is_some());
        assert!(cache.dirty.is_empty());
    }

    #[test]
    fn compile_graph_with_opts_none_preserves_unoptimized_ast() {
        let graph = arithmetic_graph();
        let registry = numeric_registry();
        let sem = semantic_pass(&graph, &registry);

        let raw = compile_graph_with_opts(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            OptLevel::None,
        )
        .expect("compile with no optimization");
        let basic = compile_graph_with_opts(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            OptLevel::Basic,
        )
        .expect("compile with basic optimization");

        assert_eq!(JsBackend.render(&raw.terminals[0]), "1 + 2");
        assert_eq!(JsBackend.render(&basic.terminals[0]), "3");
    }

    #[test]
    fn compile_graph_full_hoists_common_terminal_subexpressions() {
        let source_count = Arc::new(AtomicUsize::new(0));
        let transform_count = Arc::new(AtomicUsize::new(0));
        let output_count = Arc::new(AtomicUsize::new(0));
        let registry = counted_registry(source_count, transform_count, output_count);
        let graph = duplicate_terminal_graph();
        let sem = semantic_pass(&graph, &registry);

        let program = compile_graph_with_opts(
            &graph,
            &registry,
            &sem,
            CompileMode::Preview,
            OptLevel::Full,
        )
        .expect("compile with full optimization");

        assert_eq!(program.terminals.len(), 2);
        for terminal in &program.terminals {
            match &terminal.kind {
                ExprKind::Block { bindings, result } => {
                    assert_eq!(bindings.len(), 1);
                    assert_eq!(bindings[0].0, "_t0");
                    assert_eq!(JsBackend.render(result), "_t0");
                    assert_eq!(JsBackend.render(&bindings[0].1), "'bd'.fast()");
                }
                other => panic!("expected hoisted block terminal, got {other:?}"),
            }
        }
    }

    #[test]
    fn warm_cache_noop_reuses_nodes_without_recompile() {
        let source_count = Arc::new(AtomicUsize::new(0));
        let transform_count = Arc::new(AtomicUsize::new(0));
        let output_count = Arc::new(AtomicUsize::new(0));
        let registry = counted_registry(
            source_count.clone(),
            transform_count.clone(),
            output_count.clone(),
        );
        let graph = counted_chain_graph("bd", "fast");
        let mut cache = CompileCache::new();

        compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("first cached compile");
        assert_eq!(source_count.load(Ordering::SeqCst), 1);
        assert_eq!(transform_count.load(Ordering::SeqCst), 1);
        assert_eq!(output_count.load(Ordering::SeqCst), 1);

        compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("second cached compile");
        assert_eq!(source_count.load(Ordering::SeqCst), 1);
        assert_eq!(transform_count.load(Ordering::SeqCst), 1);
        assert_eq!(output_count.load(Ordering::SeqCst), 1);
        assert!(cache.dirty.is_empty());
    }

    #[test]
    fn cached_type_only_inference_changes_recompile_downstream_dispatch() {
        let source_count = Arc::new(AtomicUsize::new(0));
        let dispatch_count = Arc::new(AtomicUsize::new(0));
        let output_count = Arc::new(AtomicUsize::new(0));
        let registry = stable_type_registry(
            source_count.clone(),
            dispatch_count.clone(),
            output_count.clone(),
        );
        let mut graph = stable_type_graph("text");
        let mut cache = CompileCache::new();

        let initial = compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("initial cached compile");
        let source_pos = GridPos { col: 0, row: 0 };
        let source_expr_before = cache.node_exprs[&source_pos].expr.clone();

        assert_eq!(JsBackend.render(&initial.terminals[0]), "useText(shared)");
        assert_eq!(source_count.load(Ordering::SeqCst), 1);
        assert_eq!(dispatch_count.load(Ordering::SeqCst), 1);
        assert_eq!(output_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            cache.node_exprs[&source_pos].resolved_types.output_type,
            Some(PortType::text())
        );

        apply_ops_to_graph_cached(
            &mut graph,
            &registry,
            &[GraphOp::ParamSetInline {
                position: source_pos,
                param_id: "kind".into(),
                value: json!("number"),
            }],
            &mut cache,
        )
        .expect("change inferred source type");

        let recompiled = compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("recompile after type-only change");

        assert_eq!(
            JsBackend.render(&recompiled.terminals[0]),
            "useNumber(shared)"
        );
        assert_eq!(source_count.load(Ordering::SeqCst), 2);
        assert_eq!(dispatch_count.load(Ordering::SeqCst), 2);
        assert_eq!(output_count.load(Ordering::SeqCst), 2);
        assert_eq!(cache.node_exprs[&source_pos].expr, source_expr_before);
        assert_eq!(
            cache.node_exprs[&source_pos].resolved_types.output_type,
            Some(PortType::number())
        );
    }

    #[test]
    fn compile_graph_exposes_domain_bridges_and_normalized_typed_dispatch() {
        let registry = bridge_dispatch_registry(
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
        );
        let graph = bridge_dispatch_graph("control");
        let sem = semantic_pass(&graph, &registry);

        let program =
            compile_graph(&graph, &registry, &sem, CompileMode::Preview).expect("bridge compile");

        assert_eq!(
            JsBackend.render(&program.terminals[0]),
            "useAudio(__domain_convert('control_to_audio', shared))"
        );
        assert_eq!(program.domain_bridges.len(), 1);
        assert_eq!(
            program.domain_bridges[0].kind,
            DomainBridgeKind::ControlToAudio
        );
    }

    #[test]
    fn cached_bridge_presence_change_recompiles_downstream_dispatch() {
        let source_count = Arc::new(AtomicUsize::new(0));
        let dispatch_count = Arc::new(AtomicUsize::new(0));
        let output_count = Arc::new(AtomicUsize::new(0));
        let registry = bridge_dispatch_registry(
            source_count.clone(),
            dispatch_count.clone(),
            output_count.clone(),
        );
        let mut graph = bridge_dispatch_graph("control");
        let source_pos = GridPos { col: 0, row: 0 };
        let dispatch_pos = GridPos { col: 1, row: 0 };
        let mut cache = CompileCache::new();

        let initial = compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("initial cached bridge compile");
        assert_eq!(
            JsBackend.render(&initial.terminals[0]),
            "useAudio(__domain_convert('control_to_audio', shared))"
        );
        assert_eq!(source_count.load(Ordering::SeqCst), 1);
        assert_eq!(dispatch_count.load(Ordering::SeqCst), 1);
        assert_eq!(output_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            cache.node_exprs[&dispatch_pos].resolved_types.input_type("value"),
            Some(&PortType::number().with_domain(ExecutionDomain::Audio))
        );

        apply_ops_to_graph_cached(
            &mut graph,
            &registry,
            &[GraphOp::ParamSetInline {
                position: source_pos,
                param_id: "domain".into(),
                value: json!("audio"),
            }],
            &mut cache,
        )
        .expect("change source domain without changing effective target type");

        let recompiled = compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("recompile after bridge removal");

        assert_eq!(JsBackend.render(&recompiled.terminals[0]), "useAudio(shared)");
        assert!(recompiled.domain_bridges.is_empty());
        assert_eq!(source_count.load(Ordering::SeqCst), 2);
        assert_eq!(dispatch_count.load(Ordering::SeqCst), 2);
        assert_eq!(output_count.load(Ordering::SeqCst), 2);
        assert_eq!(
            cache.node_exprs[&dispatch_pos].resolved_types.input_type("value"),
            Some(&PortType::number().with_domain(ExecutionDomain::Audio))
        );
    }

    #[test]
    fn compile_multi_output_with_types_uses_normalized_bridge_domain() {
        let registry = bridge_multi_registry();
        let graph = bridge_multi_graph("control");
        let sem = semantic_pass(&graph, &registry);

        let program =
            compile_graph(&graph, &registry, &sem, CompileMode::Preview).expect("bridge multi");

        assert_eq!(
            JsBackend.render(&program.terminals[0]),
            "useAudio(__domain_convert('control_to_audio', shared))"
        );
    }

    #[test]
    fn compile_stateful_with_types_uses_normalized_bridge_domain() {
        let registry = bridge_stateful_registry();
        let graph = bridge_stateful_graph("control");
        let sem = semantic_pass(&graph, &registry);

        let program =
            compile_graph(&graph, &registry, &sem, CompileMode::Preview).expect("bridge stateful");

        assert_eq!(
            JsBackend.render(&program.terminals[0]),
            "useAudio(__domain_convert('control_to_audio', shared))"
        );
    }

    #[test]
    fn cached_compilation_invalidates_when_opt_level_changes() {
        let source_count = Arc::new(AtomicUsize::new(0));
        let transform_count = Arc::new(AtomicUsize::new(0));
        let output_count = Arc::new(AtomicUsize::new(0));
        let registry = counted_registry(
            source_count.clone(),
            transform_count.clone(),
            output_count.clone(),
        );
        let graph = counted_chain_graph("bd", "fast");
        let mut cache = CompileCache::new();

        compile_graph_cached_with_opts(
            &graph,
            &registry,
            CompileMode::Preview,
            OptLevel::None,
            &mut cache,
        )
        .expect("initial none compile");
        compile_graph_cached_with_opts(
            &graph,
            &registry,
            CompileMode::Preview,
            OptLevel::None,
            &mut cache,
        )
        .expect("repeat none compile");

        assert_eq!(source_count.load(Ordering::SeqCst), 1);
        assert_eq!(transform_count.load(Ordering::SeqCst), 1);
        assert_eq!(output_count.load(Ordering::SeqCst), 1);

        compile_graph_cached_with_opts(
            &graph,
            &registry,
            CompileMode::Preview,
            OptLevel::Full,
            &mut cache,
        )
        .expect("full compile after none");

        assert_eq!(source_count.load(Ordering::SeqCst), 2);
        assert_eq!(transform_count.load(Ordering::SeqCst), 2);
        assert_eq!(output_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn inline_param_change_recompiles_only_changed_node_and_downstream() {
        let source_count = Arc::new(AtomicUsize::new(0));
        let transform_count = Arc::new(AtomicUsize::new(0));
        let output_count = Arc::new(AtomicUsize::new(0));
        let registry = counted_registry(
            source_count.clone(),
            transform_count.clone(),
            output_count.clone(),
        );
        let mut graph = counted_chain_graph("bd", "fast");
        let mut cache = CompileCache::new();

        compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("initial compile");

        apply_ops_to_graph_cached(
            &mut graph,
            &registry,
            &[GraphOp::ParamSetInline {
                position: GridPos { col: 1, row: 0 },
                param_id: "suffix".into(),
                value: json!("slow"),
            }],
            &mut cache,
        )
        .expect("update inline param");

        let program = compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("recompile");

        assert_eq!(source_count.load(Ordering::SeqCst), 1);
        assert_eq!(transform_count.load(Ordering::SeqCst), 2);
        assert_eq!(output_count.load(Ordering::SeqCst), 2);
        assert_eq!(JsBackend.render(&program.terminals[0]), "'bd'.slow()");
    }

    #[test]
    fn edge_disconnect_recompiles_target_and_downstream_from_cached_ops() {
        let source_a_count = Arc::new(AtomicUsize::new(0));
        let source_b_count = Arc::new(AtomicUsize::new(0));
        let merge_count = Arc::new(AtomicUsize::new(0));
        let output_count = Arc::new(AtomicUsize::new(0));
        let registry = merge_registry(
            source_a_count.clone(),
            source_b_count.clone(),
            merge_count.clone(),
            output_count.clone(),
        );
        let (mut graph, extra_edge_id) = merge_graph();
        let mut cache = CompileCache::new();

        compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("initial compile");

        apply_ops_to_graph_cached(
            &mut graph,
            &registry,
            &[GraphOp::EdgeDisconnect {
                edge_id: extra_edge_id,
            }],
            &mut cache,
        )
        .expect("disconnect edge");

        let program = compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("recompile after disconnect");

        assert_eq!(source_a_count.load(Ordering::SeqCst), 1);
        assert_eq!(source_b_count.load(Ordering::SeqCst), 1);
        assert_eq!(merge_count.load(Ordering::SeqCst), 2);
        assert_eq!(output_count.load(Ordering::SeqCst), 2);
        assert_eq!(JsBackend.render(&program.terminals[0]), "'bd'");
    }

    #[test]
    fn multi_output_change_recompiles_downstream_even_when_primary_expr_is_stable() {
        let multi_count = Arc::new(AtomicUsize::new(0));
        let output_count = Arc::new(AtomicUsize::new(0));
        let registry = multi_output_registry(multi_count.clone(), output_count.clone());
        let mut graph = multi_output_graph("alpha");
        let mut cache = CompileCache::new();

        compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("initial compile");
        let primary_before = cache.node_exprs[&GridPos { col: 0, row: 0 }].expr.clone();

        apply_ops_to_graph_cached(
            &mut graph,
            &registry,
            &[GraphOp::ParamSetInline {
                position: GridPos { col: 0, row: 0 },
                param_id: "emit".into(),
                value: json!("beta"),
            }],
            &mut cache,
        )
        .expect("update multi-output param");

        let program = compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("recompile after multi-output change");

        assert_eq!(multi_count.load(Ordering::SeqCst), 2);
        assert_eq!(output_count.load(Ordering::SeqCst), 2);
        assert_eq!(
            cache.node_exprs[&GridPos { col: 0, row: 0 }].expr,
            primary_before
        );
        assert_eq!(JsBackend.render(&program.terminals[0]), "'beta'");
    }

    #[test]
    fn runtime_mode_switch_and_state_updates_force_safe_recompile() {
        let stateful_count = Arc::new(AtomicUsize::new(0));
        let output_count = Arc::new(AtomicUsize::new(0));
        let registry = stateful_registry(stateful_count.clone(), output_count.clone());
        let mut graph = stateful_graph();
        let mut cache = CompileCache::new();

        let preview = compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("preview compile");
        assert!(preview.state_updates.is_empty());
        assert_eq!(stateful_count.load(Ordering::SeqCst), 1);
        assert_eq!(output_count.load(Ordering::SeqCst), 1);

        let runtime = compile_graph_cached(&graph, &registry, CompileMode::Runtime, &mut cache)
            .expect("runtime compile");
        assert_eq!(stateful_count.load(Ordering::SeqCst), 2);
        assert_eq!(output_count.load(Ordering::SeqCst), 2);
        assert_eq!(runtime.state_updates.len(), 1);
        assert_eq!(runtime.state_updates[0].state, json!(1));

        apply_ops_to_graph_cached(
            &mut graph,
            &registry,
            &[GraphOp::NodeSetState {
                position: GridPos { col: 0, row: 0 },
                state: Some(runtime.state_updates[0].state.clone()),
            }],
            &mut cache,
        )
        .expect("persist state update");

        let runtime_again =
            compile_graph_cached(&graph, &registry, CompileMode::Runtime, &mut cache)
                .expect("runtime recompile with persisted state");
        assert_eq!(stateful_count.load(Ordering::SeqCst), 3);
        assert_eq!(output_count.load(Ordering::SeqCst), 3);
        assert_eq!(runtime_again.state_updates.len(), 1);
        assert_eq!(runtime_again.state_updates[0].state, json!(2));
        assert_eq!(JsBackend.render(&runtime_again.terminals[0]), "'tick-1'");
    }

    #[test]
    fn resize_structural_invalidation_preserves_cached_node_outputs() {
        let source_count = Arc::new(AtomicUsize::new(0));
        let transform_count = Arc::new(AtomicUsize::new(0));
        let output_count = Arc::new(AtomicUsize::new(0));
        let registry = counted_registry(
            source_count.clone(),
            transform_count.clone(),
            output_count.clone(),
        );
        let mut graph = counted_chain_graph("bd", "fast");
        let mut cache = CompileCache::new();

        compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("initial compile");

        apply_ops_to_graph_cached(
            &mut graph,
            &registry,
            &[GraphOp::ResizeGrid { cols: 8, rows: 2 }],
            &mut cache,
        )
        .expect("resize grid");

        assert!(cache.semantic.is_none());
        assert_eq!(cache.node_exprs.len(), 3);

        compile_graph_cached(&graph, &registry, CompileMode::Preview, &mut cache)
            .expect("compile after resize");

        assert_eq!(source_count.load(Ordering::SeqCst), 1);
        assert_eq!(transform_count.load(Ordering::SeqCst), 1);
        assert_eq!(output_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cache_invalidation_drops_stale_position_keys() {
        let a = GridPos { col: 0, row: 0 };
        let b = GridPos { col: 1, row: 0 };
        let c = GridPos { col: 2, row: 0 };

        let node = Node {
            piece_id: "count.source".into(),
            inline_params: BTreeMap::new(),
            input_sides: BTreeMap::new(),
            output_side: None,
            label: None,
            node_state: None,
        };

        let mut cache = CompileCache::new();
        cache.node_exprs.insert(a, fake_cached_expr("a"));
        cache.node_exprs.insert(b, fake_cached_expr("b"));
        cache.node_exprs.insert(c, fake_cached_expr("c"));

        let graph_after_move = Graph {
            nodes: BTreeMap::from([(b, node.clone()), (c, node.clone())]),
            edges: BTreeMap::new(),
            name: "move".into(),
            cols: 4,
            rows: 1,
        };
        cache.invalidate_from_apply_outcome(
            &graph_after_move,
            &[GraphOp::NodeMove { from: a, to: b }],
            &[],
            &BTreeMap::new(),
        );
        assert!(!cache.node_exprs.contains_key(&a));
        assert!(!cache.node_exprs.contains_key(&b));
        assert!(cache.node_exprs.contains_key(&c));

        cache.node_exprs.insert(a, fake_cached_expr("a"));
        cache.node_exprs.insert(b, fake_cached_expr("b"));
        let graph_after_swap = Graph {
            nodes: BTreeMap::from([(a, node.clone()), (b, node.clone()), (c, node.clone())]),
            edges: BTreeMap::new(),
            name: "swap".into(),
            cols: 4,
            rows: 1,
        };
        cache.invalidate_from_apply_outcome(
            &graph_after_swap,
            &[GraphOp::NodeSwap { a, b }],
            &[],
            &BTreeMap::new(),
        );
        assert!(!cache.node_exprs.contains_key(&a));
        assert!(!cache.node_exprs.contains_key(&b));
        assert!(cache.node_exprs.contains_key(&c));

        cache.node_exprs.insert(a, fake_cached_expr("a"));
        cache.node_exprs.insert(b, fake_cached_expr("b"));
        let graph_after_resize = Graph {
            nodes: BTreeMap::from([(a, node.clone()), (b, node)]),
            edges: BTreeMap::new(),
            name: "resize".into(),
            cols: 2,
            rows: 1,
        };
        cache.invalidate_from_apply_outcome(
            &graph_after_resize,
            &[GraphOp::ResizeGrid { cols: 2, rows: 1 }],
            &[],
            &BTreeMap::new(),
        );
        assert!(!cache.node_exprs.contains_key(&c));
    }

    // -- Partial compilation / best-effort error recovery tests --

    /// Build a graph where middle node has an unknown piece:
    /// source(0,0) → broken(1,0) → output(2,0)
    fn graph_with_unknown_piece() -> Graph {
        let source_pos = GridPos { col: 0, row: 0 };
        let broken_pos = GridPos { col: 1, row: 0 };
        let output_pos = GridPos { col: 2, row: 0 };
        let edge_a = Edge {
            id: EdgeId::new(),
            from: source_pos,
            to_node: broken_pos,
            to_param: "input".into(),
        };
        let edge_b = Edge {
            id: EdgeId::new(),
            from: broken_pos,
            to_node: output_pos,
            to_param: "pattern".into(),
        };
        Graph {
            nodes: BTreeMap::from([
                (
                    source_pos,
                    Node {
                        piece_id: "test.source".into(),
                        inline_params: BTreeMap::from([(
                            "value".into(),
                            Value::String("bd".into()),
                        )]),
                        input_sides: BTreeMap::new(),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    broken_pos,
                    Node {
                        piece_id: "nonexistent.piece".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("input".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
                (
                    output_pos,
                    Node {
                        piece_id: "test.output".into(),
                        inline_params: BTreeMap::new(),
                        input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                        output_side: None,
                        label: None,
                        node_state: None,
                    },
                ),
            ]),
            edges: BTreeMap::from([(edge_a.id.clone(), edge_a), (edge_b.id.clone(), edge_b)]),
            name: "broken_middle".into(),
            cols: 3,
            rows: 1,
        }
    }

    #[test]
    fn preview_with_unknown_piece_degrades_gracefully() {
        let graph = graph_with_unknown_piece();
        let reg = registry();
        let sem = semantic_pass(&graph, &reg);
        let result = compile_graph_with_opts(
            &graph,
            &reg,
            &sem,
            CompileMode::Preview,
            OptLevel::default(),
        );

        let program = result.expect("preview mode should succeed with degraded output");
        assert!(!program.terminals.is_empty());
        // The terminal expression should contain an error (propagated from the unknown piece).
        assert!(
            program.terminals[0].contains_error(),
            "terminal should contain propagated error from unknown piece"
        );
        // Diagnostics should mention the unknown piece.
        assert!(
            program.diagnostics.iter().any(|d| matches!(
                &d.kind,
                DiagnosticKind::UnknownPiece { piece_id } if piece_id == "nonexistent.piece"
            )),
            "diagnostics should contain UnknownPiece"
        );
    }

    #[test]
    fn runtime_mode_still_rejects_errors() {
        let graph = graph_with_unknown_piece();
        let reg = registry();
        let sem = semantic_pass(&graph, &reg);
        let result = compile_graph_with_opts(
            &graph,
            &reg,
            &sem,
            CompileMode::Runtime,
            OptLevel::default(),
        );

        assert!(
            result.is_err(),
            "runtime mode should fail when terminal contains errors"
        );
    }

    /// A terminal piece whose required param has no default (Custom schema with default: None).
    struct StrictTerminalPiece {
        def: PieceDef,
    }

    impl StrictTerminalPiece {
        fn new() -> Self {
            Self {
                def: PieceDef {
                    id: "test.strict_output".into(),
                    label: "strict output".into(),
                    category: PieceCategory::Output,
                    semantic_kind: PieceSemanticKind::Output,
                    namespace: "core".into(),
                    params: vec![ParamDef {
                        id: "signal".into(),
                        label: "signal".into(),
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
                        required: true,
                    }],
                    output_type: None,
                    output_side: None,
                    description: None,
                    tags: vec![],
                },
            }
        }
    }

    impl Piece for StrictTerminalPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            inputs
                .get("signal")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing signal"))
        }
    }

    #[test]
    fn preview_with_missing_required_param_inserts_error_expr() {
        // A terminal with a required param that has no default and no edge.
        let output_pos = GridPos { col: 0, row: 0 };
        let graph = Graph {
            nodes: BTreeMap::from([(
                output_pos,
                Node {
                    piece_id: "test.strict_output".into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::from([("signal".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            )]),
            edges: BTreeMap::new(),
            name: "missing_param".into(),
            cols: 1,
            rows: 1,
        };
        let mut reg = registry();
        reg.register(StrictTerminalPiece::new());
        let sem = semantic_pass(&graph, &reg);
        let result = compile_graph_with_opts(
            &graph,
            &reg,
            &sem,
            CompileMode::Preview,
            OptLevel::default(),
        );

        let program = result.expect("preview mode should succeed with error placeholder");
        assert!(
            program.terminals[0].contains_error(),
            "terminal should contain error for missing required param"
        );
        assert!(
            program.diagnostics.iter().any(|d| matches!(
                &d.kind,
                DiagnosticKind::MissingRequiredParam { param } if param == "signal"
            )),
            "diagnostics should contain MissingRequiredParam"
        );
    }

    #[test]
    fn error_propagates_through_downstream_nodes() {
        let graph = graph_with_unknown_piece();
        let reg = registry();
        let sem = semantic_pass(&graph, &reg);
        let result = compile_graph_with_opts(
            &graph,
            &reg,
            &sem,
            CompileMode::Preview,
            OptLevel::default(),
        );

        let program = result.expect("preview should succeed");
        // The output terminal's expression should contain a nested error from the unknown piece.
        let error_expr = program.terminals[0].first_error();
        assert!(
            error_expr.is_some(),
            "should find nested error from unknown piece"
        );
        if let Some(err) = error_expr {
            if let ExprKind::Error { message } = &err.kind {
                assert!(
                    message.contains("unknown piece"),
                    "error message should mention unknown piece, got: {message}"
                );
            }
        }
    }

    #[test]
    fn cached_partial_compilation_parity() {
        let graph = graph_with_unknown_piece();
        let reg = registry();
        let mut cache = CompileCache::default();

        let result = compile_graph_cached_with_opts(
            &graph,
            &reg,
            CompileMode::Preview,
            OptLevel::default(),
            &mut cache,
        );

        let program = result.expect("cached preview mode should succeed with degraded output");
        assert!(program.terminals[0].contains_error());
        assert!(program.diagnostics.iter().any(|d| matches!(
            &d.kind,
            DiagnosticKind::UnknownPiece { piece_id } if piece_id == "nonexistent.piece"
        )),);
    }

    #[test]
    fn diagnostics_carried_on_successful_program() {
        // A valid graph should have diagnostics (warnings) for unreachable nodes if any exist.
        // Use the standard graph which is fully connected — diagnostics should be empty.
        let g = graph();
        let reg = registry();
        let sem = semantic_pass(&g, &reg);
        let result =
            compile_graph_with_opts(&g, &reg, &sem, CompileMode::Preview, OptLevel::default());

        let program = result.expect("valid graph should compile");
        // A clean graph may still have semantic diagnostics (e.g., warnings).
        // The key assertion: diagnostics field exists and is populated from semantic pass.
        // For a fully clean graph, there should be no errors in diagnostics.
        let has_errors = program
            .diagnostics
            .iter()
            .any(|d| d.severity == crate::diagnostics::DiagnosticSeverity::Error);
        assert!(!has_errors, "clean graph should have no error diagnostics");
    }
}
