# Tessera

Tessera is a Rust library for authoring, validating, and compiling spatial music-pattern programs into a typed `PatternIr`.

Tessera treats musical structure as authored tile structure. The default authoring mode is **spatial source mode**, where root tiles have logical board positions and directional input/output bindings. Touching tiles can resolve into stream relations when their sides and endpoint shapes are compatible.

Tessera also exposes an optional **explicit graph mode** for tools that want to construct endpoint relations directly.

## What Tessera provides

Tessera is not an audio engine. It does not synthesize sound directly.
## What Tessera Is

Tessera is a Rust crate for building musical patterns from tile structure.

Instead of treating music patterns as strings, Tessera treats them as authored structures: tiles placed on a surface, containers filled with musical atoms, and streams shaped through spatial relationships. This makes it useful for visual music tools, live-coding experiments, pattern editors, procedural composition systems, and any project that wants music to be represented as structured data instead of fragile text.

Tessera is built around one core idea:

> Musical patterns can be authored as tile arrangements, then compiled into precise rational-time events.

A Tessera program can describe:

- phrases made from notes, rests, numbers, operators, and nested containers
- musical timing created by container rules such as sequence, alternate, and layer
- spatial root-surface layouts where touching tiles can imply stream flow
- transforms such as slow, fast, reverse, gain, attack, transpose, and degrade
- flow-control structures such as layer, split, route, mask, switch, merge, mix, and choice
- final outputs compiled into `PatternIr`

The crate can be used in two ways:

1. **Spatial source mode**  
   The intended tile-authored mode. You place tiles on a logical board, assign endpoints to sides, and let Tessera resolve neighboring tiles into stream relations.

2. **Explicit graph mode**  
   An advanced mode for tools, importers, text frontends, procedural generation, and tests. You construct the stream graph directly.

Both modes compile into the same rational-time `PatternIr`.
## What the Crate Provides

Internally, Tessera provides:

- a domain model for tile-authored musical programs
- spatial root-surface authoring
- container-local pattern stacks
- endpoint and port-shape validation
- normalization from raw tile stacks into musical expressions
- graph resolution from spatial tile placement into explicit stream relations
- compilation into rational-time `PatternIr`
- ergonomic infrastructure helpers for common authoring workflows
- 
The normal pipeline is:

```text
authored spatial program
    -> resolved graph program
    -> validated program
    -> normalized containers
    -> compiled pattern streams
    -> PatternIr
```

## Authoring modes

Tessera supports two authoring modes.

### 1. Spatial source mode

Spatial source mode is the default public authoring surface.

Use this when you want Tessera’s tile-native behavior:

```text
placed tiles
+ directional endpoint bindings
+ container stacks
+ optional explicit relations
```

In this mode, root tiles live on a logical board. Each tile has spatial bindings that assign input and output endpoints to sides such as west, east, north, south, or off.

For example, if a container’s output faces east and an output tile’s input faces west, placing them next to each other resolves into a `FlowsTo` relation.

```text
[ phrase ][ out ]
```

resolves to:

```text
phrase.out -> out.inputs.main
```

### 2. Explicit graph mode

Explicit graph mode is the lower-level endpoint graph representation.

Use this for:

- generated programs
- text-language frontends
- tests
- import/export tools
- advanced graph construction

Graph mode is available behind the `graph` feature.

## Installation

Tessera is currently a local/private crate. Add it as a path dependency:

```toml
[dependencies]
tessera = { path = "../tessera" }
```

Default features include spatial authoring helpers:

```toml
[features]
default = ["spatial", "builders"]
```

To enable explicit graph authoring helpers:

```toml
[dependencies]
tessera = { path = "../tessera", features = ["graph"] }
```

## Quick start: spatial authoring

```rust
use tessera::prelude::*;

fn main() -> Result<(), Vec<Diagnostic>> {
    let mut program = AuthoredTesseraProgram::empty();

    program
        .place_sequence("phrase", slot(0, 0), notes(["a", "b", "c", "d"]))
        .place_output("out", slot(1, 0));

    let ir = TesseraCompiler::new().compile_authored_ir(&program)?;

    println!("{ir:#?}");

    Ok(())
}
```

This creates a spatial board equivalent to:

```text
[ phrase ][ out ]
```

The compiler resolves that adjacency into a stream relation, validates it, normalizes the container, and emits `PatternIr`.

## Containers

Containers build local musical phrases.

A container owns a stack of local tiles such as notes, rests, scalars, operators, and nested containers.

```rust
let stack = notes(["a", "b", "c"]);
```

```rust
program.place_sequence("phrase", slot(0, 0), stack);
```

The main container kinds are:

- `Sequence`: divides its span across expressions in order
- `Alternate`: chooses one expression per cycle
- `Layer`: places expressions over the same span

## Stack helpers

The infrastructure layer provides helpers for authoring container stacks:

```rust
use tessera::prelude::*;

let phrase = stack(vec![
    note("e"),
    op(AtomOperatorToken::Elongate),
    scalar(4),
    note("g"),
]);
```

You can also use the simple note-list helper:

```rust
let phrase = notes(["a", "b", "c", "d"]);
```

Available stack helpers include:

```rust
note("a")
rest()
scalar(2)
op(AtomOperatorToken::Slow)
nested("child_container")
notes(["a", "b", "c"])
```

## Spatial source concepts

Spatial source mode uses these core types:

```rust
AuthoredTesseraProgram
RootSurface
RootPlacement
BoardSlot
TileFootprint
SpatialSide
NodeSpatialBindings
```

An authored program contains:

```rust
pub struct AuthoredTesseraProgram {
    pub root_surface: RootSurface,
    pub containers: BTreeMap<ContainerId, Container>,
}
```

A root surface contains:

```rust
pub struct RootSurface {
    pub nodes: BTreeMap<NodeId, RootSurfaceNodeKind>,
    pub placements: BTreeMap<NodeId, RootPlacement>,
    pub bindings: BTreeMap<NodeId, NodeSpatialBindings>,
    pub explicit_relations: Vec<RootRelation>,
}
```

This means Tessera’s spatial source model is made from:

- root nodes
- logical board placement
- endpoint side bindings
- optional explicit endpoint relations
- container-local stacks

## Directional endpoint bindings

Each tile can bind its inputs and outputs to spatial sides.

```rust
SpatialSide::North
SpatialSide::South
SpatialSide::West
SpatialSide::East
SpatialSide::Off
```

For example, transforms commonly use:

```text
main input  -> West
aux input   -> North
out output  -> East
```

A simple transform chain can be written as:

```rust
use tessera::prelude::*;

fn main() -> Result<(), Vec<Diagnostic>> {
    let mut program = AuthoredTesseraProgram::empty();

    program
        .place_sequence("phrase", slot(0, 0), notes(["a", "b", "c", "d"]))
        .place_transform("slow", slot(1, 0), TransformKind::Slow)
        .place_output("out", slot(2, 0));

    let ir = TesseraCompiler::new().compile_authored_ir(&program)?;

    println!("{ir:#?}");

    Ok(())
}
```

This board:

```text
[ phrase ][ slow ][ out ]
```

resolves to:

```text
phrase.out -> slow.main
slow.out   -> out.inputs.main
```

## Binding control inputs

Some transforms and flow-control tiles use auxiliary or control inputs.

For example, `Slow` has a `factor` input. By default, that input faces north. If a scalar-producing container is placed above `slow`, its output must face south to feed that input.

```rust
use tessera::prelude::*;

fn main() -> Result<(), Vec<Diagnostic>> {
    let mut program = AuthoredTesseraProgram::empty();

    program
        .place_sequence("phrase", slot(0, 1), notes(["a", "b", "c", "d"]))
        .place_sequence("factor", slot(1, 0), vec![scalar(2)])
        .place_transform("slow", slot(1, 1), TransformKind::Slow)
        .place_output("out", slot(2, 1));

    program.bind_output_side(
        "factor",
        out_output(),
        SpatialSide::South,
    );

    let ir = TesseraCompiler::new().compile_authored_ir(&program)?;

    println!("{ir:#?}");

    Ok(())
}
```

The logical board is:

```text
          [ factor ]
[ phrase ][ slow   ][ out ]
```

The resolved graph is:

```text
phrase.out -> slow.main
factor.out -> slow.factor
slow.out   -> out.inputs.main
```

## Ports and endpoint helpers

Tessera exposes endpoint helper functions for common bindings.

```rust
input("main")
output("out")
input_member("inputs", "main")
output_member("branches", "even")
```

Common shortcuts include:

```rust
main_input()
factor_input()
amount_input()
mask_input()
control_input()
out_output()
inputs_member("main")
streams_member("a")
branches_member("even")
routes_member("drums")
```

These helpers return `InputEndpoint` or `OutputEndpoint` values used by side bindings and explicit graph relations.

## Explicit relations

Spatial adjacency is not the only way to author relationships.

An authored spatial program may also contain explicit endpoint relations. These are useful for long-distance connections, manual routing, generated structures, or cases where adjacency would be ambiguous.

```rust
program.add_explicit_relation(RootRelation::FlowsTo {
    from: StreamSource::node(NodeId::new("phrase")),
    to: StreamTarget::OutputInput {
        node: NodeId::new("out"),
        endpoint: inputs_member("main"),
    },
});
```

Explicit relations and spatially inferred relations both lower into the same resolved graph representation before validation and compilation.

## Flow-control tiles

Flow-control tiles shape stream topology.

Available flow-control kinds include:

- `Layer`
- `Merge`
- `Mix`
- `Split`
- `Mask`
- `Switch`
- `Route`
- `Choice`

Examples of flow-control behavior:

```text
Layer  -> overlays streams
Split  -> produces named output branches
Mask   -> gates or shapes a main stream with a control stream
Route  -> sends events into named output routes
Choice -> selects one stream from alternatives
```

Spatial source mode can place flow-control tiles:

```rust
program.place_layer("drums", slot(1, 0), ["kick", "snare"]);
```

For v1 spatial resolution, normal 1x1 side adjacency is supported. More complex flow-tile lane geometry can be layered on top of `TileFootprint` and endpoint bindings.

For precise flow-control routing, explicit relations remain available.

## Compiler API

The main compiler facade is `TesseraCompiler`.

### Compile a spatial authored program

```rust
let ir = TesseraCompiler::new().compile_authored_ir(&program)?;
```

### Resolve spatial source into explicit graph form

```rust
let graph = TesseraCompiler::new().resolve(&program)?;
```

### Validate a spatial authored program

```rust
let report = TesseraCompiler::new().validate_authored(&program);

if !report.valid {
    for diagnostic in report.diagnostics {
        eprintln!("{diagnostic:?}");
    }
}
```

### Compile and keep intermediate data

```rust
let report = TesseraCompiler::new().compile_authored(&program)?;

let normalized = report.normalized;
let ir = report.ir;
```

## Explicit graph mode

Explicit graph mode is available with the `graph` feature.

```toml
[dependencies]
tessera = { path = "../tessera", features = ["graph"] }
```

Graph mode uses the lower-level `TesseraProgram` representation:

```rust
pub struct TesseraProgram {
    pub root_nodes: BTreeMap<NodeId, RootSurfaceNodeKind>,
    pub containers: BTreeMap<ContainerId, Container>,
    pub relations: Vec<RootRelation>,
}
```

This is useful when building programs from a text parser, procedural generator, import tool, or low-level test.

```rust
use tessera::graph_prelude::*;
use tessera::prelude::*;

let mut graph = TesseraProgram::empty();

graph
    .add_sequence("phrase", notes(["a", "b", "c"]))
    .add_output("out")
    .connect("phrase", "out");

let ir = TesseraCompiler::new().compile_ir(&graph)?;
```

Graph mode is not the default public identity of Tessera. It is the explicit resolved representation that spatial source mode lowers into.

## Feature flags

```toml
[features]
default = ["spatial", "builders"]
spatial = []
builders = []
graph = []
serde = []
```

### `spatial`

Enables spatial source authoring types and helpers.

### `builders`

Enables builder-style infrastructure helpers.

### `graph`

Exposes explicit graph authoring helpers and graph prelude.

### `serde`

Reserved for serialization support.

## Diagnostics

Tessera reports validation and compilation failures through `Diagnostic` values.

Diagnostics include:

- placement errors
- unknown nodes
- invalid endpoint bindings
- invalid graph relations
- missing required inputs
- wrong stream shape
- container-local grammar errors
- invalid transform arguments
- flow-control topology errors

Use `validate_authored` to inspect errors before compiling:

```rust
let report = TesseraCompiler::new().validate_authored(&program);

for diagnostic in report.diagnostics {
    eprintln!("{diagnostic:?}");
}
```

## Mental model

Tessera has three layers:

```text
Spatial source mode:
  placed tiles + side bindings + container stacks

Resolved graph mode:
  explicit endpoint relations

Pattern IR:
  typed rational-time musical events
```

The spatial source is what authors write.
The resolved graph is what tools may generate.
The PatternIR is what runtimes consume.

## Current limitations

The current spatial resolver uses logical board slots and side bindings. It supports common 1x1 adjacency patterns such as:

```text
[ container ][ transform ][ output ]
```

and north/south auxiliary inputs such as:

```text
          [ scalar ]
[ phrase ][ slow   ][ output ]
```

More advanced flow-tile lane geometry can be added using `TileFootprint` and endpoint bindings without changing the compiler pipeline.

## License

MIT OR Apache-2.0
