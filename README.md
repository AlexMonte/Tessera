# Tessera

Tessera is a host-agnostic typed graph engine for visual music and visual programming editors. It gives a host application the core pieces needed to describe a grid-based graph, validate it, compile it into a language-neutral expression AST, and then render that AST into a target language such as JavaScript or Lua.

This repository currently contains the engine crate, not a standalone editor application. The crate is also not published to crates.io yet (`publish = false` in `Cargo.toml`), so the normal way to adopt it today is as a workspace member or a local path dependency.

## What Tessera Is

Tessera is designed for hosts that want a reusable graph core without baking target-language rules, UI policies, or domain-specific pieces into the engine itself.

The main pipeline looks like this:

1. A host defines a set of `Piece`s and registers them in a `PieceRegistry`.
2. The host or editor stores user work as a `Graph` made of typed nodes and edges.
3. Tessera runs semantic analysis to detect structural, typing, wiring, and reachability problems.
4. Tessera compiles the graph into a neutral `Expr` AST with graph-origin metadata.
5. A backend renders those expressions into target output such as JavaScript or Lua.
6. The host decides how to run, persist, display, animate, and react to the compiled result.

That separation is the key design rule in this crate: Tessera owns the graph engine and shared IR, while host code owns domain behavior.

## Getting Started

Because the crate is not published yet, use it from the local workspace or via a path dependency:

```toml
[dependencies]
tessera = { path = "../tessera" }
serde_json = "1"
```

If you are building a host application, you will typically:

- Define one or more `Piece` implementations for your domain.
- Register those pieces in a `PieceRegistry`.
- Implement `HostAdapter` to provide the registry and, optionally, a backend override.
- Store editor state as `Graph` or `ProjectDocument`.
- Use `GraphEngine` for analysis, compilation, graph ops, and runtime snapshot shaping.

## Current Features

- Typed grid graphs with `Graph`, `Node`, `Edge`, bounds (`cols` and `rows`), labels, and optional per-node opaque state.
- A flexible piece model with `Piece`, `PieceDef`, `ParamDef`, and `ParamSchema`.
- Inline parameter values, compile-time defaults, per-param side assignment, output-side overrides, and variadic fan-in groups.
- Preview timelines and stateful compilation hooks for pieces that need animation hints or persisted state.
- Semantic diagnostics for unknown pieces and params, missing required params, duplicate connections, type mismatches, side mismatches, non-adjacent wiring, cycles, missing terminals, multiple terminals, and unreachable nodes.
- Two compile modes: `Preview` and `Runtime`.
- A language-neutral `Expr` AST with origin tracking back to graph nodes and params.
- Built-in backends for JavaScript (`JsBackend`) and Lua 5.3+ (`LuaBackend`).
- Graph mutation helpers for placement, movement, swapping, removal, resizing, inline param changes, side changes, labels, node state, and explicit edge operations.
- Atomic batch placement with optional explicit edges and adjacency-based auto-wiring.
- Edge probing helpers that return machine-readable rejection reasons plus repair suggestions.
- Subgraph boundary pieces, subgraph analysis, compiled subgraph metadata, and generated subgraph pieces.
- Runtime activity and probe snapshot types for UI feedback surfaces.
- Small optimization helpers including constant folding, dead-code elimination, and backward reachability analysis.
- Serde support across the core graph, diagnostics, AST, activity, and project document types.

## Core Concepts

| Type | What it means |
| --- | --- |
| `Graph` | A rectangular workspace containing nodes and edges. |
| `Node` | One placed piece instance on the grid, including inline params, side assignments, label, and optional node state. |
| `Edge` | A directed connection from one node output to a specific target parameter on another node. |
| `Piece` | The trait every placeable node type implements. It defines metadata plus compile behavior. |
| `PieceDef` | Static description of a piece: id, label, category, params, output type, namespace, tags, and more. |
| `PieceRegistry` | Lookup table for all pieces available to a host. |
| `HostAdapter` | Host-specific integration point for registry creation, backend selection, terminal rendering, state persistence, and runtime snapshot shaping. |
| `GraphEngine` | Convenience wrapper around analysis, compilation, graph ops, preview timelines, and runtime snapshots. |
| `CompileProgram` | Output of a successful compile: terminal expressions, node state updates, and activity events. |
| `GraphOp` | Canonical mutation language understood by the engine and op-application layer. |
| `Expr` | The neutral AST that pieces compile into before rendering. |
| `ProjectDocument` | Legacy schema-v2 wrapper for persisting a named graph. |

Some design details are especially important when embedding Tessera:

- Graph connectivity is side-aware. Param sides and output sides matter during validation and auto-wiring.
- Parameters can be satisfied by upstream edges, inline values, or schema defaults.
- Terminal pieces are pieces whose `output_type` is `None` or whose semantic kind is `Output`.
- Runtime compilation is stricter than preview compilation: unresolved `Expr::Error` placeholders are rejected in runtime mode.

## How To Use Tessera

### 1. Minimal host integration

The smallest useful integration is: define pieces, register them, build a graph, compile it, and render the terminal expressions.

```rust
use std::collections::BTreeMap;

use serde_json::Value;
use tessera::{
    CompileMode, Edge, EdgeId, Expr, Graph, GraphEngine, GridPos, HostAdapter, Node, ParamDef,
    ParamSchema, Piece, PieceCategory, PieceDef, PieceInputs, PieceRegistry,
    PieceSemanticKind, TileSide,
};

struct SourcePiece {
    def: PieceDef,
}

impl SourcePiece {
    fn new() -> Self {
        Self {
            def: PieceDef {
                id: "demo.source".into(),
                label: "source".into(),
                category: PieceCategory::Generator,
                semantic_kind: PieceSemanticKind::Operator,
                namespace: "demo".into(),
                params: vec![ParamDef {
                    id: "value".into(),
                    label: "value".into(),
                    side: TileSide::BOTTOM,
                    schema: ParamSchema::Text {
                        default: "bd".into(),
                        can_inline: true,
                    },
                    text_semantics: Default::default(),
                    variadic_group: None,
                    required: false,
                }],
                output_type: Some("text".into()),
                output_side: Some(TileSide::RIGHT),
                description: Some("Emit a text literal".into()),
                tags: vec!["demo".into()],
            },
        }
    }
}

impl Piece for SourcePiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn compile(
        &self,
        inputs: &PieceInputs,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Expr {
        inputs
            .get("value")
            .cloned()
            .unwrap_or_else(|| Expr::str_lit("bd"))
    }
}

struct OutputPiece {
    def: PieceDef,
}

impl OutputPiece {
    fn new() -> Self {
        Self {
            def: PieceDef {
                id: "demo.output".into(),
                label: "output".into(),
                category: PieceCategory::Output,
                semantic_kind: PieceSemanticKind::Output,
                namespace: "demo".into(),
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
                description: Some("Terminal sink".into()),
                tags: vec!["demo".into()],
            },
        }
    }
}

impl Piece for OutputPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn compile(
        &self,
        inputs: &PieceInputs,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Expr {
        inputs
            .get("pattern")
            .cloned()
            .unwrap_or_else(|| Expr::error("missing pattern"))
    }
}

struct DemoHost;

impl HostAdapter for DemoHost {
    fn create_registry(&self) -> PieceRegistry {
        let mut registry = PieceRegistry::new();
        registry.register(SourcePiece::new());
        registry.register(OutputPiece::new());
        registry
    }
}

fn main() {
    let engine = GraphEngine::new(DemoHost);
    let source_pos = GridPos { col: 0, row: 0 };
    let output_pos = GridPos { col: 1, row: 0 };
    let edge_id = EdgeId::new();

    let graph = Graph {
        nodes: BTreeMap::from([
            (
                source_pos,
                Node {
                    piece_id: "demo.source".into(),
                    inline_params: BTreeMap::from([(
                        "value".into(),
                        Value::String("bd".into()),
                    )]),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: Some("kick".into()),
                    node_state: None,
                },
            ),
            (
                output_pos,
                Node {
                    piece_id: "demo.output".into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: Some("main out".into()),
                    node_state: None,
                },
            ),
        ]),
        edges: BTreeMap::from([(
            edge_id.clone(),
            Edge {
                id: edge_id,
                from: source_pos,
                to_node: output_pos,
                to_param: "pattern".into(),
            },
        )]),
        name: "demo".into(),
        cols: 4,
        rows: 1,
    };

    let sem = engine.analyze(&graph);
    assert!(sem.is_valid());

    let program = engine
        .compile(&graph, CompileMode::Preview)
        .expect("graph should compile");
    let rendered = engine.render_terminals(&program.terminals);

    assert_eq!(rendered, vec!["'bd'"]);
}
```

If your host wants Lua output instead of JavaScript, override `HostAdapter::backend()` and return `&LuaBackend`.

### 2. Lower-level analysis, compilation, backends, and optimization

`GraphEngine` is the easiest way to work with Tessera, but the lower-level functions are public too. That is useful if you want finer control over the pipeline or want to run custom passes in between.

```rust
use tessera::{
    compile_graph, eliminate_dead_code, fold_constants, reachable_nodes, semantic_pass,
    Backend, BinOp, CompileMode, Expr, JsBackend, LuaBackend,
};

let registry = DemoHost.create_registry();
let sem = semantic_pass(&graph, &registry);
let program = compile_graph(&graph, &registry, &sem, CompileMode::Preview)
    .expect("graph should compile");

let live_nodes = reachable_nodes(&graph, &sem.terminals);
assert!(live_nodes.contains(&GridPos { col: 0, row: 0 }));

let expr = fold_constants(&Expr::bin_op(BinOp::Add, Expr::int(1), Expr::int(2)));
let expr = eliminate_dead_code(&expr);
assert_eq!(JsBackend.render(&expr), "3");

let call = Expr::method_call(Expr::str_lit("bd"), "fast", vec![]);
assert_eq!(JsBackend.render(&call), "'bd'.fast()");
assert_eq!(LuaBackend.render(&call), "'bd':fast()");
```

### 3. Serialized graph and project data

`Graph` is the main persisted workspace type. `ProjectDocument` is a legacy wrapper retained for schema-v2 migration and still useful if you want a named top-level document shape.

This is the serialized shape you should expect for a small project:

```json
{
  "schema_version": 2,
  "name": "Demo Project",
  "graph": {
    "nodes": [
      {
        "position": { "col": 0, "row": 0 },
        "node": {
          "piece_id": "demo.source",
          "inline_params": { "value": "bd" },
          "input_sides": {},
          "output_side": null,
          "label": "kick"
        }
      },
      {
        "position": { "col": 1, "row": 0 },
        "node": {
          "piece_id": "demo.output",
          "inline_params": {},
          "input_sides": { "pattern": "left" },
          "output_side": null,
          "label": "main out"
        }
      }
    ],
    "edges": {
      "11111111-1111-4111-8111-111111111111": {
        "id": "11111111-1111-4111-8111-111111111111",
        "from": { "col": 0, "row": 0 },
        "to_node": { "col": 1, "row": 0 },
        "to_param": "pattern"
      }
    },
    "name": "demo",
    "cols": 4,
    "rows": 1
  }
}
```

Two serialization details are easy to miss:

- `nodes` serialize as an array of `{ position, node }` entries rather than a JSON object keyed by coordinates.
- `input_sides` serialize as plain string enums such as `"left"` or `"bottom"`.

### 4. Applying graph operations

Hosts can mutate graphs directly, but `GraphOp` plus `GraphEngine::apply_ops()` is the intended engine-level mutation path because it validates, canonicalizes, and returns undo information.

```rust
use tessera::{BatchPlaceEdge, BatchPlaceEntry, GraphOp};

let engine = GraphEngine::new(DemoHost);
let mut graph = Graph {
    nodes: BTreeMap::new(),
    edges: BTreeMap::new(),
    name: "ops".into(),
    cols: 4,
    rows: 1,
};

let outcome = engine
    .apply_ops(
        &mut graph,
        &[GraphOp::NodeBatchPlace {
            nodes: vec![
                BatchPlaceEntry {
                    position: GridPos { col: 0, row: 0 },
                    piece_id: "demo.source".into(),
                    inline_params: BTreeMap::from([(
                        "value".into(),
                        Value::String("bd".into()),
                    )]),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: Some("src".into()),
                },
                BatchPlaceEntry {
                    position: GridPos { col: 1, row: 0 },
                    piece_id: "demo.output".into(),
                    inline_params: BTreeMap::new(),
                    input_sides: BTreeMap::from([("pattern".into(), TileSide::LEFT)]),
                    output_side: None,
                    label: Some("out".into()),
                },
            ],
            edges: vec![BatchPlaceEdge {
                from: GridPos { col: 0, row: 0 },
                to_node: GridPos { col: 1, row: 0 },
                to_param: "pattern".into(),
            }],
            auto_wire: false,
        }],
    )
    .expect("batch place should succeed");

assert_eq!(graph.nodes.len(), 2);
assert_eq!(graph.edges.len(), 1);
assert!(!outcome.undo_ops.is_empty());

let removed = engine
    .apply_ops(
        &mut graph,
        &[GraphOp::NodeRemove {
            position: GridPos { col: 0, row: 0 },
        }],
    )
    .expect("remove should succeed");

assert_eq!(graph.nodes.len(), 1);
assert_eq!(removed.removed_edges.len(), 1);
```

For editor-driven wiring, the lower-level edge helpers are often useful too:

- `probe_edge_connect()` tries a connection and returns structured accept/reject information.
- `pick_target_param_for_edge()` can auto-select the best target param based on adjacency, type, and side.
- `validate_edge_connect()` checks an explicit connection.
- `RepairSuggestion` values can be turned back into `GraphOp`s with `to_ops()`.

### 5. Registering the built-in core expression pieces

The engine ships a small target-agnostic core piece set. Today that includes:

- `core.if_expr`
- `core.not`
- `core.and`
- `core.or`
- `core.eq`
- `core.gt`
- `core.lt`

You can add them to a registry like this:

```rust
use std::sync::Arc;

use tessera::{core_expression_pieces, PieceRegistry};

let mut registry = PieceRegistry::new();

for piece in core_expression_pieces() {
    let id = piece.def().id.clone();
    registry.register_arc(id, Arc::from(piece));
}

let logic_pieces = registry.search_by_tag("logic");
assert!(logic_pieces.iter().any(|def| def.id == "core.not"));
```

These pieces lower into the shared `Expr` AST. They do not contain host-specific runtime logic.

### 6. Defining and compiling subgraphs

Subgraphs let a host treat one graph as a reusable macro-like piece. Tessera provides the boundary pieces plus compilation helpers; the host still decides how compiled subgraphs become part of the runtime registry.

This example creates a one-input subgraph that compiles to `src.xform()`:

```rust
use std::sync::Arc;

use tessera::{
    compile_subgraph, subgraph_editor_pieces, subgraph_pieces, ParamInlineMode, ParamValueKind,
    PortType, SubgraphDef, SUBGRAPH_INPUT_1_ID, SUBGRAPH_OUTPUT_ID,
};

struct TransformPiece {
    def: PieceDef,
}

impl TransformPiece {
    fn new() -> Self {
        Self {
            def: PieceDef {
                id: "demo.transform".into(),
                label: "xform".into(),
                category: PieceCategory::Transform,
                semantic_kind: PieceSemanticKind::Operator,
                namespace: "demo".into(),
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
                description: Some("Apply a method call".into()),
                tags: vec!["demo".into()],
            },
        }
    }
}

impl Piece for TransformPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn compile(
        &self,
        inputs: &PieceInputs,
        _inline_params: &BTreeMap<String, Value>,
    ) -> Expr {
        Expr::method_call(
            inputs
                .get("input")
                .cloned()
                .unwrap_or_else(|| Expr::error("missing input")),
            "xform",
            vec![],
        )
    }
}

let mut editor_registry = PieceRegistry::new();
for piece in subgraph_editor_pieces() {
    let id = piece.def().id.clone();
    editor_registry.register_arc(id, Arc::from(piece));
}
editor_registry.register(TransformPiece::new());

let input_pos = GridPos { col: 0, row: 0 };
let transform_pos = GridPos { col: 1, row: 0 };
let output_pos = GridPos { col: 2, row: 0 };
let edge_a = EdgeId::new();
let edge_b = EdgeId::new();

let subgraph = SubgraphDef {
    id: "demo_xform".into(),
    name: "demo xform".into(),
    graph: Graph {
        nodes: BTreeMap::from([
            (
                input_pos,
                Node {
                    piece_id: SUBGRAPH_INPUT_1_ID.into(),
                    inline_params: BTreeMap::from([("label".into(), Value::String("src".into()))]),
                    input_sides: BTreeMap::new(),
                    output_side: None,
                    label: None,
                    node_state: None,
                },
            ),
            (
                transform_pos,
                Node {
                    piece_id: "demo.transform".into(),
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
                edge_a.clone(),
                Edge {
                    id: edge_a,
                    from: input_pos,
                    to_node: transform_pos,
                    to_param: "input".into(),
                },
            ),
            (
                edge_b.clone(),
                Edge {
                    id: edge_b,
                    from: transform_pos,
                    to_node: output_pos,
                    to_param: "input".into(),
                },
            ),
        ]),
        name: "demo subgraph".into(),
        cols: 4,
        rows: 1,
    },
};

let compiled = compile_subgraph(&subgraph, &editor_registry)
    .expect("subgraph should compile");
assert_eq!(compiled.signature.inputs.len(), 1);

let generated = subgraph_pieces(std::slice::from_ref(&compiled));
let mut runtime_registry = PieceRegistry::new();
runtime_registry.register(generated[0].clone());
```

Important current subgraph rules:

- A subgraph can declare at most three inputs.
- It must contain exactly one subgraph output piece.
- It may declare at most one receiver input.
- Generated pieces are regular `Piece`s that you can register into a host registry.

### 7. Activity and probe snapshots

Tessera also defines host-facing runtime feedback types for pulsing nodes, current probe values, and preview animation timelines. The core engine does not own runtime data; the host produces events and shapes them into snapshots.

```rust
use serde_json::json;
use tessera::{ActivityEvent, HostTime, ProbeEvent};

let engine = GraphEngine::new(DemoHost);

let activity = engine.build_activity_snapshot(
    &[ActivityEvent::trigger(GridPos { col: 0, row: 0 })
        .with_label("kick")
        .at(HostTime::seconds(0.0))],
    Some(HostTime::seconds(0.0)),
);

let probes = engine.build_probe_snapshot(
    &[ProbeEvent::new(
        GridPos { col: 1, row: 0 },
        json!({ "rendered": "'bd'" }),
    )],
    Some(HostTime::seconds(0.0)),
);

assert!(!activity.is_empty());
assert_eq!(
    probes.value_at(&GridPos { col: 1, row: 0 }),
    Some(&json!({ "rendered": "'bd'" }))
);
```

If your host wants custom decay, coalescing, filtering, or value formatting, override `HostAdapter::build_activity_snapshot()` or `HostAdapter::build_probe_snapshot()`.

## Built-ins vs Host Responsibilities

Tessera intentionally keeps the boundary between engine behavior and host behavior clear.

Tessera currently ships:

- The graph, AST, diagnostic, backend, and op systems.
- The `GraphEngine` integration wrapper.
- Core expression pieces via `core_expression_pieces()`.
- Subgraph boundary and generated-piece helpers via `subgraph_editor_pieces()`, `compile_subgraph()`, `compile_subgraphs()`, and `subgraph_pieces()`.
- Runtime feedback value types such as `ActivityEvent`, `ActivitySnapshot`, `ProbeEvent`, `ProbeSnapshot`, and `PreviewTimeline`.

The host is responsible for:

- Domain-specific pieces and registries.
- UI/editor behavior, layout, palette organization, and persistence choices.
- Target-specific adaptation beyond the shared AST and backend rules.
- Runtime execution, scheduling, transport, playback, and side effects.
- How compiled terminal expressions are combined, interpreted, or sent downstream.

## Current Limitations And Non-Goals

- This crate is a library core, not a standalone visual editor application.
- It is not published to crates.io yet, so integration currently assumes a path or workspace dependency.
- The built-in reusable piece set is intentionally small: core expression pieces plus subgraph helpers.
- Domain registries, runtime systems, and editor UX are expected to live in the host.
- The examples in this README show embedding Tessera into another Rust application, not launching a finished end-user product.

## Public API Checklist

If you are scanning the crate surface before adopting it, these are the main entry points to know:

- Graph model: `Graph`, `Node`, `Edge`, `ProjectDocument`
- Mutation model: `GraphOp`, `BatchPlaceEntry`, `BatchPlaceEdge`, `apply_ops_to_graph()`
- Piece model: `Piece`, `PieceDef`, `ParamDef`, `ParamSchema`, `PieceInputs`, `PieceRegistry`
- Engine integration: `HostAdapter`, `GraphEngine`
- Compilation: `CompileMode`, `CompileProgram`, `compile_graph()`, `semantic_pass()`
- Rendering: `Backend`, `JsBackend`, `LuaBackend`
- Subgraphs: `SubgraphDef`, `compile_subgraph()`, `compile_subgraphs()`, `subgraph_pieces()`, `subgraph_editor_pieces()`
- Runtime feedback: `ActivityEvent`, `ActivitySnapshot`, `ProbeEvent`, `ProbeSnapshot`, `PreviewTimeline`
- Optimization helpers: `fold_constants()`, `eliminate_dead_code()`, `reachable_nodes()`

## Development Note

The examples in this README are derived from the current public API and the existing test suite in this repository. If you extend the engine surface, the README should be updated alongside the tests so the crate stays understandable to new host integrators.
