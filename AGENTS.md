# Tessera — Technical Reference for AI Agents

This document describes the live Tessera crate as it exists after the
independence pass.

For the human-facing overview, see `README.md`.

## What Tessera Is

Tessera is a Rust crate that owns graph structure, graph edits, validation,
type inference, subgraph boundary analysis, and host-facing semantic facts for
tile-based editors.

Tessera does not know the host representation. It stops at analyzed graph
facts.

## Ownership Boundaries

### Tessera owns

- editable graph structure
- graph mutation operations and undo data
- piece catalogs and port typing rules
- validation, deterministic semantic analysis, and diagnostics
- explicit output roots and analyzed node/input facts
- domain bridge detection
- delay-edge classification
- subgraph signatures

### Host crates own

- project documents and persistence
- domain-specific piece meaning
- lowering from `AnalyzedGraph` into the host's own representation
- project-level orchestration
- execution policy and scheduling
- preview, probe, and activity/telemetry representations
- filesystem and media resources

### Host Integration Contract

- Tessera remains independent of any host runtime crate
- hosts must walk Tessera analysis results instead of reconstructing meaning
  from the raw graph
- hosts own all lowering, execution, scheduling, and effect semantics
- Tessera should document the handoff surface clearly enough that host crates
  can implement their own lowering without needing Tessera-side glue

## Module Map

```text
src/
  lib.rs              — crate root, re-exports, prelude
  analysis.rs         — AnalyzedGraph, AnalyzedNode, ResolvedInput, AnalysisCache
  diagnostics.rs      — Diagnostic, DiagnosticKind, severity
  graph.rs            — Graph, Node, Edge, GraphOp, undo-facing records
  piece.rs            — Piece trait, PieceDef, ParamDef, ParamSchema
  piece_registry.rs   — PieceRegistry
  types.rs            — GridPos, EdgeId, PortType, Rational, domains, roles, sides

  semantic/
    mod.rs            — semantic_pass orchestration
    input_resolution.rs
    topo_sort.rs
    type_inference.rs
    validation.rs
    tests.rs

  ops/
    mod.rs            — public ops surface
    apply.rs          — graph mutation application
    auto_wire.rs      — auto-wiring helpers
    pruning.rs        — edge pruning on node removal
    types.rs          — ApplyOpsOutcome and probing result types
    validation.rs     — connection probing/validation helpers
    tests.rs

  subgraph/
    mod.rs            — public subgraph surface
    analysis.rs       — analyze_subgraph
    helpers.rs        — boundary type helpers
    pieces.rs         — boundary marker pieces and generated signature pieces
    types.rs          — SubgraphDef, SubgraphInput, SubgraphSignature
    tests.rs
```

## Main Flow

```text
Pieces (host-defined)
     |
     v
Graph (nodes + edges on a grid)
     |
     v
semantic_pass() -> AnalyzedGraph
     |
     +-- diagnostics
     +-- eval_order
     +-- outputs
     +-- per-node resolved inputs
     +-- inferred output types
     +-- domain bridges
     +-- delay edges
     |
     v
Host walks output roots and lowers into its own IR
```

## Core Types

### GridPos

Position on the tile grid.

- fields: `col: i32`, `row: i32`
- ordering: col-first, then row
- helper: `adjacent_in_direction`

### TileSide

`TOP`, `BOTTOM`, `RIGHT`, `LEFT`

- used for parameter input sides and node output side
- `faces(other)` checks whether two sides oppose each other

### PortType

Typed value port with an optional execution domain.

- built-in helpers: `number`, `text`, `bool`, `rational`, `any`
- domains: `Audio`, `Control`, `Event`
- host crates can use custom type ids as strings

### PortRole

Extra semantic shape for ports:

- `Value`
- `Gate`
- `Signal`
- `Callback`
- `Sequence`
- `Field { name }`

Roles do not change graph structure, but they help hosts interpret edges more
accurately.

## Defining Pieces

Implement the `Piece` trait:

```rust
trait Piece {
    fn def(&self) -> &PieceDef;

    fn infer_output_type(
        &self,
        input_types: &BTreeMap<String, PortType>,
        inline_params: &BTreeMap<String, Value>,
    ) -> Option<PortType>;

    fn validate_analysis(
        &self,
        position: GridPos,
        node: &AnalyzedNode,
    ) -> Vec<Diagnostic> {
        Vec::new()
    }
}
```

Important rule: the piece trait only describes graph semantics. It does not
produce host-specific representations.

### PieceDef

Important fields:

- `id`, `label`, `namespace`
- `category`, `semantic_kind`
- `params`
- `output_type`
- `output_side`
- `output_role`
- `description`, `tags`

`semantic_kind == Output` marks an explicit output root for host walking.

### ParamDef

Important fields:

- `id`, `label`
- `side`
- `schema`
- `text_semantics`
- `variadic_group`
- `required`
- `role`

### ParamSchema

Variants:

- `Number`
- `Text`
- `Enum`
- `Bool`
- `Rational`
- `Custom`

`Custom` is the escape hatch for host-defined types and JSON-shaped defaults.

## Analysis Contract

These types are the stable handoff surface for host crates:

- `AnalyzedGraph`
- `AnalyzedNode`
- `ResolvedInput`
- `ResolvedInputSource`
- `AnalysisCache`
- `SubgraphInput`
- `SubgraphSignature`

### AnalyzedGraph

Key fields:

- `diagnostics`
- `eval_order`
- `outputs`
- `nodes`
- `output_types`
- `domain_bridges`
- `delay_edges`

Useful helpers:

- `is_valid()`
- `node(pos)`
- `output_nodes()`

### AnalyzedNode

Key fields:

- `piece_id`
- `inline_params`
- `scalar_inputs`
- `variadic_inputs`
- `input_types`
- `output_type`
- `input_roles`
- `output_role`
- `output_side`
- `node_state`

Useful helpers:

- `input(param_id)`
- `variadic_group(group_id)`

### ResolvedInput

Key fields:

- `source`
- `effective_type`
- `bridge_kind`

Useful helper:

- `is_missing()`

### ResolvedInputSource

Variants:

- `Edge { edge_id, from, exit_side, via }`
- `Inline { value }`
- `Default { value }`
- `Missing`

That split is the core host-lowering contract. Hosts should not try to
re-derive this information from the raw graph.

Connector rule:

- connector pieces are transparent during analysis when they declare exactly
  one input param
- `from` identifies the real upstream producer
- `via` lists traversed connector positions in source-to-target order
- connector-backed inline/default inputs normalize to `Inline` or `Default`
  instead of forcing hosts to inspect connector nodes manually

## Semantic Pass

`semantic_pass(graph, registry) -> AnalyzedGraph`

High-level steps:

1. validate known piece ids
2. validate edge structure, adjacency, side rules, and duplicate occupancy
3. validate inline values and required inputs
4. produce deterministic topological order with delay-edge separation
5. infer output types bottom-up
6. reconcile delay-node feedback types
7. finalize edge type checks and bridge metadata
8. collect explicit output roots
9. warn on unreachable nodes

## Direct Integration

Tessera exposes direct functions instead of an engine wrapper.

Main APIs:

- `semantic_pass`
- `analyze_cached`
- `apply_ops_to_graph`
- `apply_ops_to_graph_cached`
- `probe_edge_connect`

Hosts construct their own `PieceRegistry` and keep any higher-level
orchestration in the host crate. Tessera's type matching stays native to the
crate: exact type matches, `any`, and supported domain bridges.

## Subgraphs

Subgraphs are analyzed as reusable boundaries, not lowered by Tessera.

Main pieces:

- `SubgraphInputPiece`
- `SubgraphOutputPiece`
- `GeneratedSubgraphPiece`

Main APIs:

- `analyze_subgraph`
- `subgraph_editor_pieces`
- `subgraph_pieces`

### SubgraphSignature

Contains:

- ordered `inputs`
- `output_pos`
- `output_type`

Each `SubgraphInput` carries:

- `slot`
- `pos`
- `label`
- `port_type`
- `required`
- `is_receiver`
- `default_value`

## Diagnostics

`DiagnosticKind` covers the semantic failures hosts are expected to surface:

- unknown piece/node/param
- invalid operation
- duplicate connection
- cycle
- no output node
- unreachable node
- type mismatch
- unsupported domain crossing
- delay type mismatch
- side mismatch
- non-adjacent edge
- output from terminal
- missing required param
- inline not allowed
- inline type mismatch
- role mismatch

## Key Invariants

1. Tessera remains independent of any host runtime crate.
2. Hosts lower analyzed graphs into their own IR.
3. Output roots are explicit and deterministic.
4. Input sources are normalized before the host sees them.
5. Delay edges are identified during analysis, not guessed by the host.
6. Domain bridges are surfaced as metadata, not hidden side effects.
7. Grid ordering stays deterministic through `GridPos`.
8. Public graph and analysis types remain serde-friendly.
9. A placed node may not assign the same input side to multiple params.
10. Piece-local semantic diagnostics may inspect analyzed nodes, but they do not
    mutate graph facts or host concerns.
11. Connector traversal is an analysis rule, not a host concern; hosts should
    use normalized resolved inputs rather than reconstructing connector chains.

## Clean-Room Note

External systems such as Psi can inform Tessera's design, but live Tessera code
and API choices should remain clean-room implementations rather than copied
source, assets, or prose.
