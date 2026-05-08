pub mod atom;
pub mod container;
pub mod diagnostics;
pub mod flow;
pub mod pattern_ir;
pub mod program;
pub mod relations;
pub mod spatial_defaults;
pub mod surface;

pub use atom::{
    AtomExpr, AtomExprKind, AtomModifier, AtomOperatorToken, AtomTile, MusicalValue, NoteAtom,
    ScalarAtom,
};
pub use container::{
    Container, ContainerId, ContainerKind, ContainerSurfaceTile, NormalizedContainer,
};
pub use diagnostics::{Diagnostic, DiagnosticCategory, DiagnosticKind, DiagnosticLocation};
pub use flow::{
    ConnectionRule, DefaultStreamBehavior, FlowControlKind, FlowControlNode, FlowControlPolicy,
    GroupMembers, InputGroupSpec, InputPort, InputSocketSpec, NodeInputRole, NodeSignature,
    OutputGroupSpec, OutputNode, OutputPort, OutputSocketSpec, PortCountRule, PortGroupId,
    PortMemberId, RootSurfaceNodeKind, Side, StreamShape, TransformKind, TransformNode,
};
pub use pattern_ir::{
    ControlEvent, ControlKeyIr, ControlStream, ControlStreamNodeIr, ControlValueIr, CycleDuration,
    CycleSpan, CycleTime, DeduplicateKeyIr, DeduplicatePolicyIr, DeduplicateWinnerIr, EventField,
    EventStreamNodeIr, EventValue, FieldValue, FlatPatternOutput, PatternEvent, PatternIr,
    PatternNodeIr, PatternOutput, PatternStream, PatternStreamShape, PriorityConflictIr,
    PriorityMergePolicyIr, Rational, ScalarEvent, ScalarStream, ScalarStreamNodeIr,
    WeightedPatternIr,
};
pub use program::{NodeId, NormalizedProgram, TesseraProgram};
pub use relations::{InputEndpoint, OutputEndpoint, RootRelation, StreamSource, StreamTarget};
pub use spatial_defaults::{apply_flow_member_default_bindings, default_spatial_bindings};
pub use surface::{
    AuthoredTesseraProgram, BoardSlot, NodeSpatialBindings, RootPlacement, RootSurface,
    SpatialSide, TileFootprint,
};
