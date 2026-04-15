# Tessera Functional Foundation

This document is the replacement charter for Tessera's next architecture.

It is authoritative during the ground-up rewrite. When existing code disagrees
with this document, the document wins.

## Purpose

Tessera is a graph-semantics kernel for tile-based editors.

Tessera does not own host lowering, execution policy, scheduler policy, or
domain-specific effect meaning. Tessera stops at deterministic analyzed graph
facts.

## Core Rule

The graph model is pure by default.

That means:

- ordinary pieces are pure transforms over explicit inputs
- state is only legal through declared state pieces
- outputs are explicit boundary nodes
- connectors are analysis-only routing aids
- subgraphs are reusable function-like boundaries

## Kernel Contract

Tessera owns only reusable graph facts:

- normalized resolved inputs
- deterministic evaluation order
- explicit output roots
- inferred output types
- domain bridge metadata
- state-edge and delay-edge classification
- subgraph signatures
- graph validation and diagnostics

Hosts own preview, probe, activity, telemetry, execution, and presentation
surfaces that may be derived from those facts.

Host crates must consume those facts directly. They should not reconstruct
meaning from raw graph shape when analyzed facts already exist.

## Execution Taxonomy

Tessera's generic execution taxonomy is intentionally small:

- `pure`
- `state`
- `connector`
- `boundary`

This taxonomy is kernel-level structure, not host meaning.

Examples:

- literals and transforms are usually `pure`
- delay/memory cells are `state`
- pass-through routing pieces are `connector`
- explicit outputs are `boundary`

Hosts may refine these classes, but Tessera should not absorb host-specific
semantic categories.

## Graph Legality

The legality rules for the replacement architecture are:

- pure cycles are invalid
- cycles that pass through declared state pieces are legal and classified
- connector traversal is transparent during analysis
- outputs are explicit and deterministic
- subgraphs expose signatures, not hidden lowering behavior

## Subgraph Rule

Subgraphs are function-like reusable boundaries.

Tessera may analyze:

- ordered inputs
- default values
- required inputs
- output boundary
- signature types

Tessera may not:

- lower subgraphs into host-specific representations
- compile host execution code
- hide host-specific call semantics inside generated convenience behavior

## Host Handoff

The stable handoff surface remains:

- `AnalyzedGraph`
- `AnalyzedNode`
- `ResolvedInput`
- `ResolvedInputSource`
- `SubgraphSignature`

If these types change, they should change to become clearer kernel facts, not
to absorb host meaning.

## Contract Gate

This boundary should be protected with fixed host-handoff fixtures.

Those fixtures should assert, at minimum:

- explicit output roots
- normalized resolved inputs
- connector traversal metadata
- inferred types
- delay and state-edge classification
- reusable subgraph signatures

If a future change weakens any of those facts, the fixtures should fail before
host crates have to rediscover the drift themselves.

## Non-Goals

Tessera is not:

- a runtime engine
- a host policy injection surface
- a text compiler
- a string renderer
- a framework for domain-specific lowering

## Pass 1 Slice

The first supported replacement slice is intentionally small:

- literals
- pure unary/binary transforms
- connector traversal
- one explicit state-cell family
- explicit outputs
- subgraph signatures
- minimal subgraph calls

Feature parity with the previous stack is not a goal for this pass.
