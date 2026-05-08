# Tessera Library Usage Guide

Tessera is a Rust library for compiling a tile-based spatial music language into rational-time `PatternIr`.

The public API is intentionally centered on the `infrastructure` facade. Most users should build a `TesseraProgram`, pass it to `TesseraCompiler`, and inspect either diagnostics, a normalized program, a preview stream, or compiled IR.

## Mental model

Tessera has three tile form factors:

| Form | Region | Purpose |
|---|---|---|
| Atomic tiles | Inside containers | Build local musical/control expressions |
| Normal tiles | Root graph | Container, transform, and output nodes |
| Flow-control tiles | Root graph | Spatial topology nodes such as layer, split, mask, route, switch, and choice |

The compilation pipeline is:

```text
authored TesseraProgram
  -> validation
  -> normalization
  -> graph-aware compilation
  -> PatternIr
```

The important design rule is:

```text
Authored tile structure is the concrete syntax.
Ports and relations are part of the syntax.
PatternIr is the compiled result, not the source model.
```

## Public entrypoint

Use `TesseraCompiler` from the prelude:

```rust
use tessera::prelude::*;

let compiler = TesseraCompiler::new();
```

The main methods are:

```rust
compiler.validate(&program);          // ValidationReport
compiler.normalize(&program);         // Result<NormalizedProgram, Vec<Diagnostic>>
compiler.compile(&program);           // Result<CompileReport, Vec<Diagnostic>>
compiler.compile_ir(&program);        // Result<PatternIr, Vec<Diagnostic>>
compiler.preview_container(...);      // Result<PreviewReport, Vec<Diagnostic>>
```

Use `compile_ir` when you only need the final IR. Use `compile` when you also want the normalized program that produced it.

## Minimal program: container to output

This creates one container with one note, connects it to the default output, and compiles the program.

```rust
use std::collections::BTreeMap;
use tessera::prelude::*;

let mut containers = BTreeMap::new();
let mut root_nodes = BTreeMap::new();

containers.insert(
    ContainerId::new("phrase"),
    Container::new(ContainerKind::Sequence, vec![ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("a")))])
);

root_nodes.insert(
    NodeId::new("phrase"),
    RootSurfaceNodeKind::Container {
        container: ContainerId::new("phrase"),
    },
);

root_nodes.insert(
    NodeId::new("out"),
    RootSurfaceNodeKind::Output(OutputNode::default()),
);

let program = TesseraProgram {
    root_nodes,
    containers,
    relations: vec![RootRelation::FlowsTo {
        from: StreamSource::node(NodeId::new("phrase")),
        to: StreamTarget::OutputInput {
            node: NodeId::new("out"),
            endpoint: InputEndpoint::GroupMember {
                group: PortGroupId::new("inputs"),
                member: PortMemberId::new("main"),
            },
        },
    }],
};

let ir = TesseraCompiler::new()
    .compile_ir(&program)
    .expect("program should compile");

assert_eq!(ir.outputs.len(), 1);
```

## Container-local syntax

Containers are local stack surfaces. Atomic tiles are read left-to-right and normalized into `AtomExpr` values before timing is assigned.

Example surface:

```text
[ e ][ @ ][ 3 ][ / ][ 2 ]
```

Normalizes conceptually as:

```text
[ e@3/2 ]
```

The timing pass sees one expression, not five raw tiles.

### Atomic tiles

```rust
AtomTile::Note(NoteAtom::new("e"))
AtomTile::Rest
AtomTile::Scalar(ScalarAtom::integer(2))
AtomTile::Operator(AtomOperatorToken::Fast)
AtomTile::Operator(AtomOperatorToken::Slow)
AtomTile::Operator(AtomOperatorToken::Elongate)
AtomTile::Operator(AtomOperatorToken::Replicate)
```

### Normalized expression forms

Normalization can produce:

```rust
AtomExprKind::Value(...)
AtomExprKind::Choice(Vec<AtomExpr>)
AtomExprKind::Parallel(Vec<AtomExpr>)
```

Local choice and parallel are container-local expression structures. They are distinct from root-level `FlowControlKind::Choice` and root-level `FlowControlKind::Layer`.

## Transform nodes

Transforms are root-surface nodes, but they are not root sources. They must receive an upstream stream through `FlowsTo`.

Example: pattern stream into `slow`, scalar control stream into `slow.factor`, then output.

```rust
let slow = RootSurfaceNodeKind::Transform(TransformNode::new(TransformKind::Slow));

RootRelation::FlowsTo {
    from: StreamSource::node(NodeId::new("phrase")),
    to: StreamTarget::TransformInput {
        node: NodeId::new("slow"),
        endpoint: InputEndpoint::Socket(InputPort::new("main")),
    },
};

RootRelation::FlowsTo {
    from: StreamSource::node(NodeId::new("factor")),
    to: StreamTarget::TransformInput {
        node: NodeId::new("slow"),
        endpoint: InputEndpoint::Socket(InputPort::new("factor")),
    },
};
```

Transform parameterization is stream-based. Static authored transform arguments are not part of the model. Optional transform sockets may define intrinsic default stream behavior, such as `slow.factor = 2`.

## Flow-control nodes

Flow-control nodes are first-class root graph topology nodes.

They use sockets and groups:

```text
socket = one fixed connection point
group  = variable family of related sockets
```

Examples:

| Flow node | Inputs | Outputs |
|---|---|---|
| Layer | `streams` input group | `out` socket |
| Merge | `streams` input group | `out` socket |
| Mix | `streams` input group + optional `amount` socket | `out` socket |
| Split | `main` socket | `branches` output group |
| Mask | `main` socket + `mask` socket | `out` socket |
| Switch | `candidates` input group + optional `control` socket | `out` socket |
| Route | `main` socket + optional `control` socket | `routes` output group |
| Choice | `options` input group + optional `control` socket | `out` socket |

### Layer example

```rust
root_nodes.insert(
    NodeId::new("layer"),
    RootSurfaceNodeKind::FlowControl(FlowControlNode::new(FlowControlKind::Layer)),
);

RootRelation::FlowsTo {
    from: StreamSource::node(NodeId::new("a")),
    to: StreamTarget::FlowControlInput {
        node: NodeId::new("layer"),
        endpoint: InputEndpoint::GroupMember {
            group: PortGroupId::new("streams"),
            member: PortMemberId::new("a"),
        },
    },
};

RootRelation::FlowsTo {
    from: StreamSource::node(NodeId::new("b")),
    to: StreamTarget::FlowControlInput {
        node: NodeId::new("layer"),
        endpoint: InputEndpoint::GroupMember {
            group: PortGroupId::new("streams"),
            member: PortMemberId::new("b"),
        },
    },
};

RootRelation::FlowsTo {
    from: StreamSource::node(NodeId::new("layer")),
    to: StreamTarget::OutputInput {
        node: NodeId::new("out"),
        endpoint: InputEndpoint::GroupMember {
            group: PortGroupId::new("inputs"),
            member: PortMemberId::new("main"),
        },
    },
};
```

The target group member matters. The compiler preserves authored member identity instead of relying on relation order.

## Endpoint-aware sources

Every produced stream is addressed by endpoint.

```rust
StreamSource {
    node: NodeId::new("split"),
    endpoint: OutputEndpoint::GroupMember {
        group: PortGroupId::new("branches"),
        member: PortMemberId::new("even"),
    },
}
```

For the common case of a node with an `out` socket, use:

```rust
StreamSource::node(NodeId::new("phrase"))
```

which is equivalent to:

```rust
StreamSource::socket(NodeId::new("phrase"), "out")
```

## ChainedTo

`ChainedTo` sequences the stream from a specific source endpoint into a target container node.

```rust
RootRelation::ChainedTo {
    from: StreamSource::node(NodeId::new("slow")),
    to: NodeId::new("next_container"),
};
```

If the source is a transform or flow-control node, the target container begins after the produced duration of the referenced source endpoint.

## Pattern IR

Compilation emits typed rational-time IR:

```rust
PatternIr {
    outputs: Vec<PatternOutput>
}

PatternOutput {
    id: NodeId,
    events: Vec<PatternEvent>
}

PatternEvent {
    span: CycleSpan,
    value: EventValue,
    fields: Vec<EventField>
}
```

Time is rational:

```rust
CycleSpan {
    start: CycleTime(Rational::zero()),
    duration: CycleDuration(Rational::one()),
}
```

Event values include:

```rust
EventValue::Note { value, octave }
EventValue::Rest
EventValue::Scalar { value }
```

Scalar streams are first-class and can feed transform aux/control sockets. Default outputs reject scalar-only streams unless an output signature explicitly accepts them.

## Validation and diagnostics

Use validation before building UI behavior around a graph.

```rust
let report = TesseraCompiler::new().validate(&program);

if report.is_invalid() {
    for diagnostic in report.diagnostics {
        eprintln!("{:?}: {}", diagnostic.kind, diagnostic.message);
    }
}
```

Diagnostics carry structured locations:

```rust
DiagnosticLocation::RootNode(...)
DiagnosticLocation::RootRelation { index }
DiagnosticLocation::ContainerStack { container, index }
DiagnosticLocation::InputEndpoint { node, endpoint }
DiagnosticLocation::OutputEndpoint { node, endpoint }
```

This makes diagnostics suitable for an editor or host integration.

## Previewing a container

Use `preview_container` to compile one container in program context.

```rust
let preview = TesseraCompiler::new().preview_container(
    &program,
    ContainerId::new("phrase"),
    CycleSpan {
        start: CycleTime(Rational::zero()),
        duration: CycleDuration(Rational::one()),
    },
)?;

let stream = preview.stream;
```

Preview uses the same container semantics as full compilation.

## Advanced domain usage

### Custom flow-control members

Flow-control groups have declared members. For custom names, construct `GroupMembers` and provide them on the node.

```rust
let mut members = GroupMembers::default();
members.outputs.insert(
    PortGroupId::new("branches"),
    vec![PortMemberId::new("high"), PortMemberId::new("low")],
);

let split = FlowControlNode {
    kind: FlowControlKind::Split,
    signature: FlowControlNode::new(FlowControlKind::Split).signature,
    policy: FlowControlPolicy::SplitByPitchRange { threshold_octave: 4 },
    members,
};
```

Then downstream relations can reference:

```rust
OutputEndpoint::GroupMember {
    group: PortGroupId::new("branches"),
    member: PortMemberId::new("high"),
}
```

### Custom node signatures

Use `NodeSignature::new` when defining a custom node shape. It validates duplicate socket IDs, duplicate group IDs, socket/group name collisions, and default stream shape compatibility.

```rust
let signature = NodeSignature::new(
    vec![InputSocketSpec {
        port: InputPort::new("main"),
        role: NodeInputRole::Main,
        shape: StreamShape::EventPattern,
        connection: ConnectionRule::Required,
        side: Some(Side::Left),
        default: None,
    }],
    vec![],
    vec![OutputSocketSpec {
        port: OutputPort::new("out"),
        shape: StreamShape::EventPattern,
        side: Some(Side::Right),
    }],
    vec![],
)?;
```

### Flow-control policies

Flow-control policies are deterministic compile-time musical policies. They do real event work, but they do not yet depend on a rich live runtime cycle/playhead context.

Examples:

```rust
FlowControlPolicy::MergeAppend
FlowControlPolicy::MergeInterleave
FlowControlPolicy::MixWeighted
FlowControlPolicy::SplitByIndexModulo
FlowControlPolicy::MaskScale
FlowControlPolicy::RouteByLabel
FlowControlPolicy::ChoiceSeededRandom
```

A future runtime evaluator can deepen policies such as `CycleIndex`, `Weighted`, and `SeededRandom` with live cycle/playhead context without changing the public graph shape.

## Production-readiness boundary

The library is production-ready for the current compiler scope:

```text
- authored structural source model
- validation and diagnostics
- normalization
- endpoint-aware graph compilation
- rational PatternIR
- first-class flow-control nodes
- deterministic compile-time musical policies
- infrastructure facade through TesseraCompiler
```

The next layer should be host/editor API ergonomics, not more core redesign.
