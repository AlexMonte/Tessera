# Tessera

Tessera is a graph-analysis kernel for tile-based editors.

A host application defines pieces, users arrange them on a grid, and Tessera
turns that editable graph into stable semantic facts:

- which nodes are explicit output roots
- which source feeds each input
- which effective types were inferred
- where domain bridges are needed
- which edges behave like delay feedback
- what subgraph boundaries expose to the host

The host crate then walks those facts and lowers them into its own
representation.

## What Tessera Owns

- Graph structure and mutation ops
- Piece catalogs and graph typing rules
- Validation and deterministic semantic analysis
- Connection probing and repair hints
- Subgraph boundary analysis

## What The Host Owns

- Project documents and persistence
- Domain-specific piece meaning
- Lowering from `AnalyzedGraph` into the app's own representation
- Execution policy and scheduling
- Preview, probe, and activity/telemetry representations
- External resources and side effects

## Quick Start

Add Tessera as a dependency:

```toml
[dependencies]
tessera = { path = "../tessera" }
serde_json = "1"
```

Define a piece catalog:

```rust
use tessera::*;

struct NotePiece {
    def: PieceDef,
}

impl NotePiece {
    fn new() -> Self {
        Self {
            def: PieceDef {
                id: "demo.text_literal".into(),
                label: "text".into(),
                category: PieceCategory::Generator,
                semantic_kind: PieceSemanticKind::Literal,
                namespace: "demo".into(),
                params: vec![],
                output_type: Some(PortType::text()),
                output_side: Some(TileSide::RIGHT),
                output_role: Default::default(),
                description: Some("A text value.".into()),
                tags: vec!["demo".into()],
            },
        }
    }
}

impl Piece for NotePiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }
}
```

Build a registry and analyze a graph directly:

```rust
use tessera::*;

let mut registry = PieceRegistry::new();
registry.register(NotePiece::new());

let analyzed = semantic_pass(&graph, &registry);

if analyzed.is_valid() {
    for (output_pos, output_node) in analyzed.output_nodes() {
        let _site = output_pos;
        let _piece = &output_node.piece_id;

        // Walk the output roots and lower from resolved inputs into your host.
    }
}
```

## The Main Contract

The stable host-facing contract is:

- `AnalyzedGraph`
- `AnalyzedNode`
- `ResolvedInput`
- `ResolvedInputSource`
- `AnalysisCache`
- `SubgraphInput`
- `SubgraphSignature`

That surface is meant to be enough for a host crate to do its own lowering
without Tessera knowing the target host representation.

## What Tessera Gives You

- A typed tile grid with adjacency and side rules
- Deterministic output roots and evaluation order
- Per-input source resolution for edge, inline, default, and missing values
- Effective type inference and domain bridge metadata
- Native port matching based on exact type identity, `any`, and supported domain bridges
- Delay-edge classification for feedback flows
- Connector traversal that normalizes pass-through routing into real upstream sources
- Piece-local semantic diagnostics over normalized analyzed nodes
- Connection probing with human-readable repair suggestions
- Subgraph signatures that describe reusable boundaries
- Incremental analysis caching across graph edits
- Full serde support for public graph and analysis types

Placed nodes must keep each input side unique. Tessera rejects edits that
assign multiple params on the same node to the same side, and persisted graphs
that violate that rule analyze with explicit diagnostics.

Piece implementations can also add read-only semantic diagnostics during
analysis through `Piece::validate_analysis`. That hook receives the final
normalized `AnalyzedNode`, so host crates can express piece-specific invariants
without pulling host-specific concerns into Tessera itself.

Connector pieces are also first-class in analysis. When a piece is marked as a
connector, Tessera treats single-input connector chains as transparent during
source resolution, type checks, bridge detection, and connection probing. Edge
sources can therefore report the real upstream producer plus any traversed
connector positions in `ResolvedInputSource::Edge.via`.

## Subgraphs

Use `analyze_subgraph` to turn a subgraph definition into a stable signature.
Use `subgraph_editor_pieces` for boundary marker pieces in an editor, and
`subgraph_pieces` when you want reusable piece definitions from saved
signatures.

## Design Note

Tessera may study external systems such as Psi for design inspiration, but the
crate stays clean-room at the code and API level. Reuse ideas and behaviors,
not source code, assets, or copied surface design.

## License

MIT OR Apache-2.0
