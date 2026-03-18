# Tessera — Submodule Refactoring Plan

This document outlines how each source file exceeding 1,000 lines can be decomposed
into focused submodules. The goal is to improve navigability, reduce cognitive load
per file, and make the dependency graph between concerns explicit.

> **Files in scope** (sorted by size):
>
> | File | Lines | Proposed modules |
> |------|------:|-----------------|
> | `compiler.rs` | 3,998 | 7 + tests |
> | `ops.rs` | 3,006 | 5 + tests |
> | `semantic.rs` | 1,660 | 4 + tests |
> | `subgraph.rs` | 1,287 | 5 + tests |
> | `activity.rs` | 1,224 | 6 + tests |

---

## 1. `compiler.rs` → `compiler/`

The compiler is the largest file. It owns the graph compilation pipeline:
data structures, caching, parameter resolution, node compilation, program
assembly, and the public API. Tests alone account for ~2,700 lines.

### Proposed layout

```
compiler/
├── mod.rs            # re-exports, CompileMode enum
├── types.rs          # data structures (~80 lines)
├── cache.rs          # incremental compilation cache (~110 lines)
├── diagnostics.rs    # error/origin helpers (~50 lines)
├── resolution.rs     # param & type resolution (~130 lines)
├── compilation.rs    # core node compilation loop (~200 lines)
├── program.rs        # CompileProgram assembly (~100 lines)
├── api.rs            # public entry points (~240 lines)
└── tests.rs          # all test infrastructure & cases (~2,700 lines)
```

### Module responsibilities

| Module | Key items | Lines (approx) |
|--------|-----------|------:|
| **types.rs** | `CompileMode`, `NodeStateUpdate`, `DelaySlot`, `CompileProgram`, `CompiledNodes`, `CachedNodeExpr` | 80 |
| **cache.rs** | `CompileCache` struct + `invalidate_from_apply_outcome()` — handles dirty-tracking for node ops, edge ops, param changes, and state updates | 110 |
| **diagnostics.rs** | `error()`, `node_origin()`, `param_origin()`, `unresolved_error_diagnostic()` | 50 |
| **resolution.rs** | `direction_from_to()`, `resolve_param_inputs()`, `resolved_piece_types()` — resolves all node parameters from edges, inline values, or defaults | 130 |
| **compilation.rs** | `compile_piece_expr()`, `compile_node_outputs()`, `compile_nodes()`, `delay_slot_name()`, `stamp_delay_slot()`, `cache_signature_matches()`, `hydrate_cached_outputs()`, `mark_direct_downstream_dirty()` | 200 |
| **program.rs** | `build_compile_program()` — collects terminals, delay slots, domain bridges, diagnostics; optionally hoists common subexpressions | 100 |
| **api.rs** | `compile_graph()`, `compile_graph_with_opts()`, `compile_graph_cached()`, `compile_graph_cached_with_opts()`, `compile_node_expr()`, `compile_node_expr_with_opts()` | 240 |
| **tests.rs** | 18 test piece structs, 11 registry builders, 12 graph builders, 67 test cases | 2,700 |

### Dependency flow

```
api.rs
 ├── cache.rs
 ├── compilation.rs
 │    ├── resolution.rs
 │    │    └── types.rs
 │    └── diagnostics.rs
 └── program.rs
      └── types.rs
```

---

## 2. `ops.rs` → `ops/`

Graph mutation operations: validation, auto-wiring, edge pruning, and the
central `apply_ops_to_graph()` dispatcher (a ~1,050-line match over 15
`GraphOp` variants).

### Proposed layout

```
ops/
├── mod.rs            # re-exports
├── types.rs          # enums & structs (~180 lines)
├── validation.rs     # edge connection probing & validation (~340 lines)
├── pruning.rs        # edge validity checks & pruning (~110 lines)
├── auto_wire.rs      # auto-wiring logic (~100 lines)
├── apply.rs          # apply_ops_to_graph + cached wrapper + helpers (~1,080 lines)
└── tests.rs          # all test infrastructure & cases (~1,180 lines)
```

### Module responsibilities

| Module | Key items | Lines (approx) |
|--------|-----------|------:|
| **types.rs** | `RepairSuggestion`, `ApplyOpsOutcome`, `EdgeConnectProbeReason`, `EdgeTargetParamProbe`, `EdgeConnectBase` | 180 |
| **validation.rs** | `resolve_edge_connect_base()`, `source_output_type_for_target_side()`, `side_from_to_node()`, `pick_target_param_for_edge()`, `validate_edge_connect()`, `probe_edge_connect()` | 340 |
| **pruning.rs** | `edge_is_still_adjacent()`, `node_param_side()`, `node_output_side()`, `edge_is_still_valid()`, `prune_invalid_edges_for_node()`, `prune_invalid_touching_edges()` | 110 |
| **auto_wire.rs** | `auto_wire_node()`, `edge_connect_from()` — removes invalid edges, discovers and connects adjacent params/outputs | 100 |
| **apply.rs** | `apply_ops_to_graph()` (15-arm match: NodePlace, NodeBatchPlace, NodeMove, NodeSwap, NodeRemove, EdgeConnect, EdgeDisconnect, ParamSetInline, ParamClearInline, ParamSetSide, ParamClearSide, OutputSetSide, OutputClearSide, NodeAutoWire, ResizeGrid), `apply_ops_to_graph_cached()`, `swap_rewrite_pos()`, `invalid_op()`, `in_bounds()`, `ensure_in_bounds()` | 1,080 |
| **tests.rs** | `TestPiece`, `GenericProbePiece`, fixture builders, 16 test functions | 1,180 |

### Dependency flow

```
apply.rs
 ├── auto_wire.rs
 │    ├── pruning.rs
 │    └── validation.rs
 │         └── types.rs
 └── types.rs
```

### Note on `apply_ops_to_graph`

The large match statement in `apply.rs` (~1,050 lines) could be further
decomposed by extracting each arm into a dedicated helper function
(e.g., `handle_node_place()`, `handle_node_batch_place()`, etc.) within
the same file or a sub-file like `ops/handlers.rs`. This would bring each
handler to a manageable 30–80 lines while keeping the match statement as a
clean dispatcher.

---

## 3. `semantic.rs` → `semantic/`

The semantic analysis pass: validates graph structure, infers output types,
resolves domain bridges, and performs topological sorting.

### Proposed layout

```
semantic/
├── mod.rs                # re-exports, PendingEdgeTypeCheck, semantic_pass() orchestrator
├── input_resolution.rs   # parameter/type resolution helpers (~80 lines)
├── type_inference.rs     # output type inference + delay reconciliation (~110 lines)
├── topo_sort.rs          # Kahn's algorithm with delay-edge support (~80 lines)
├── validation.rs         # structural validation phases (~150 lines)
└── tests.rs              # 18 test piece structs, 12 test functions (~1,040 lines)
```

### Module responsibilities

| Module | Key items | Lines (approx) |
|--------|-----------|------:|
| **input_resolution.rs** | `incoming_edge_for_param()`, `node_param_side()`, `node_output_side()`, `resolved_input_connection_for_param()`, `resolved_input_type_for_param()`, `resolved_input_types_for_piece()` | 80 |
| **type_inference.rs** | `infer_output_types()` — forward inference in eval order; `reconcile_delay_output_types()` — reconciles default vs feedback types for delay nodes | 110 |
| **topo_sort.rs** | Kahn's algorithm implementation, delay edge identification (`DELAY_PIECE_ID` + `"value"` param), cycle detection | 80 |
| **validation.rs** | Extractable validation phases from `semantic_pass()`: node/piece validation, edge validation, inline param validation, terminal validation, reachability analysis | 150 |
| **mod.rs** | `semantic_pass()` orchestrator that calls the submodules in sequence: validate → topo-sort → infer types → check edge types → detect domain bridges → validate terminals → check reachability | ~100 |
| **tests.rs** | `NumberSourcePiece`, `PatternSourcePiece`, `GenericForwardPiece`, `SinkPiece`, `DomainSourcePiece`, `DomainForwardPiece`, fixture builders, 12 tests | 1,040 |

### Dependency flow

```
mod.rs (semantic_pass orchestrator)
 ├── validation.rs
 ├── topo_sort.rs
 ├── type_inference.rs
 │    └── input_resolution.rs
 └── input_resolution.rs
```

### Validation phases within `semantic_pass()`

The `semantic_pass()` function (lines 216–619) runs a multi-phase pipeline.
Each phase can be extracted into a function in `validation.rs`:

1. **Node validation** — unknown piece checks
2. **Edge validation** — existence, duplicates, param checks, adjacency, side-facing
3. **Inline param validation** — unknown params, inline-allowed, type match, required params
4. **Topological sort** — delay edge handling, Kahn's algorithm, cycle detection
5. **Type inference** — forward inference, delay reconciliation
6. **Edge type checking** — type mismatch, domain bridge detection
7. **Terminal validation** — exactly one terminal
8. **Reachability** — backward reachability from terminals

---

## 4. `subgraph.rs` → `subgraph/`

Subgraph definition, analysis, compilation, and the boundary/generated
piece implementations.

### Proposed layout

```
subgraph/
├── mod.rs            # re-exports, public API surface
├── types.rs          # core data structures + constants (~65 lines)
├── pieces.rs         # boundary & generated piece impls (~300 lines)
├── analysis.rs       # analyze_subgraph() validation (~180 lines)
├── compiler.rs       # compile_subgraph(), compile_subgraphs(), public helpers (~140 lines)
├── helpers.rs        # param building, port type mapping, naming (~170 lines)
└── tests.rs          # test infrastructure & cases (~330 lines)
```

### Module responsibilities

| Module | Key items | Lines (approx) |
|--------|-----------|------:|
| **types.rs** | `SUBGRAPH_INPUT_*_ID`, `SUBGRAPH_OUTPUT_ID`, `MAX_SUBGRAPH_INPUTS`, `is_subgraph_input_id()`, `SubgraphDef`, `SubgraphInput`, `SubgraphSignature`, `CompiledSubgraph` | 65 |
| **pieces.rs** | `SubgraphInputPiece` (boundary input, slots 1–3), `SubgraphOutputPiece` (boundary output), `GeneratedSubgraphPiece` (compiled subgraph as a piece in the main graph) — all with `Piece` trait impls | 300 |
| **analysis.rs** | `analyze_subgraph()` — validates max inputs, duplicate slots, output count, receiver count, input edge types; returns `SubgraphSignature` | 180 |
| **compiler.rs** | `compile_subgraph()`, `compile_subgraphs()`, `subgraph_pieces()`, `subgraph_editor_pieces()` — orchestration, binding name deduplication | 140 |
| **helpers.rs** | `build_generated_params()`, `schema_for_port()`, `can_inline_for_port()`, `value_kind_for_port()`, `subgraph_boundary_port_type()`, `subgraph_boundary_domain()`, `default_expr_for_input()`, `ResolvedInput`, `resolve_input()`, `strip_trailing_none()`, `slot_from_piece_id()`, `unique_binding_name()`, `sanitize_identifier()` | 170 |
| **tests.rs** | `make_input()`, `make_typed_input()`, `editor_registry()`, `SimpleTransform`, `simple_subgraph()`, test suites for pieces, analysis, and compilation | 330 |

### Dependency flow

```
compiler.rs (orchestration)
 ├── analysis.rs
 │    └── types.rs
 ├── helpers.rs
 │    └── types.rs
 └── pieces.rs
      ├── helpers.rs
      └── types.rs
```

---

## 5. `activity.rs` → `activity/`

Runtime feedback vocabulary: activity pulses, probe values, timeline
hints, and frame snapshots. Cleanly domain-separated.

### Proposed layout

```
activity/
├── mod.rs              # re-exports, clamp helpers
├── host_time.rs        # HostTime enum + RawHostTime serde shadow (~80 lines)
├── event.rs            # ActivityKind + ActivityEvent + builders (~190 lines)
├── probe.rs            # ProbeEvent + ProbeSnapshot + serde helper (~120 lines)
├── timeline.rs         # TimelineStep + PreviewTimeline (~155 lines)
├── snapshot.rs         # ActivitySnapshot + serde helper (~120 lines)
└── tests.rs            # 49 test functions across all components (~580 lines)
```

### Module responsibilities

| Module | Key items | Lines (approx) |
|--------|-----------|------:|
| **mod.rs** | `clamp_phase()`, `clamp_unit()`, re-exports for all public types | 20 |
| **host_time.rs** | `HostTime` (Cycle / Seconds / Ticks variants), `RawHostTime` serde shadow, constructors with clamping | 80 |
| **event.rs** | `ActivityKind` (Trigger / Sustain / Processing / RuntimeError), `RawActivityKind`, `ActivityEvent` with builder pattern (`with_param()`, `with_label()`, `with_intensity()`, `at()`) | 190 |
| **probe.rs** | `ProbeEvent`, `ProbeSnapshot`, `grid_pos_values_map` serde helper | 120 |
| **timeline.rs** | `TimelineStep`, `RawTimelineStep`, `PreviewTimeline`, `RawPreviewTimeline`, `uniform()` factory | 155 |
| **snapshot.rs** | `ActivitySnapshot`, `grid_pos_events_map` serde helper, query methods (`events_at()`, `is_empty()`, `active_positions()`) | 120 |
| **tests.rs** | Organized by component: HostTime (8), ActivityKind (7), ActivityEvent (5), ProbeEvent (2), TimelineStep (4), PreviewTimeline (7), ActivitySnapshot (8), ProbeSnapshot (8) | 580 |

### Dependency flow

```
snapshot.rs ──→ event.rs ──→ host_time.rs
                  │              ↑
                  └──────────────┘
probe.rs (independent)
timeline.rs (independent, uses clamp helpers only)
```

---

## General Migration Strategy

1. **Create the directory** — e.g., `mkdir src/compiler`
2. **Move the file** — `mv src/compiler.rs src/compiler/mod.rs`
3. **Extract one module at a time** — start with the leaf modules (fewest
   dependencies), then work inward. For each extraction:
   - Move the items into the new file
   - Add `mod` and `pub use` in `mod.rs`
   - Run `cargo check` to verify
4. **Extract tests last** — move `#[cfg(test)] mod tests` into `tests.rs`
   and add `#[cfg(test)] mod tests;` in `mod.rs`
5. **Keep public API unchanged** — use `pub use` re-exports in `mod.rs` so
   that callers (`use crate::compiler::compile_graph`) continue to work
   without changes.

### Recommended extraction order

Extract files roughly in dependency order — leaves first:

| Order | Module | Rationale |
|:-----:|--------|-----------|
| 1 | `activity/` | Fewest inbound dependents; cleanly domain-separated |
| 2 | `subgraph/` | Self-contained subsystem with clear boundaries |
| 3 | `semantic/` | Depended upon by compiler, so stabilize before touching compiler |
| 4 | `ops/` | Central but independent of compiler internals |
| 5 | `compiler/` | Largest file; depends on semantic, ops; do last for stability |
