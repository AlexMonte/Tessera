use serde::{Deserialize, Serialize};

use super::{ContainerId, InputEndpoint, NodeId, OutputEndpoint, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    Placement,
    LocalGrammar,
    RootRelation,
    TransformTopology,
    TransformArgument,
    FlowControlTopology,
    StreamShape,
    Cycle,
    Compile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiagnosticKind {
    AtomAtRoot,
    MissingContainer,
    TransformInsideContainer,
    OutputInsideContainer,
    DanglingScalar,
    AmbiguousOctaveBinding,
    OperatorWithoutLeftValue,
    OperatorWithoutRightScalar,
    InvalidModifierArgument,
    InvalidRandomChoicePlacement,
    InvalidParallelPlacement,
    InvalidFlowSource,
    InvalidFlowTarget,
    UnknownInputSocket,
    UnknownInputGroup,
    UnknownInputGroupMember,
    UnknownOutputSocket,
    UnknownOutputGroup,
    UnknownOutputGroupMember,
    EndpointShapeMismatch,
    InvalidChainSource,
    InvalidChainTarget,
    TransformMissingMainInput,
    TransformMissingRequiredAuxInput,
    TransformSideAlreadyBound,
    NodeInputArityMismatch,
    RequiredSocketMissing,
    OptionalSocketMultiplyBound,
    PortCountViolation,
    InvalidStreamShape,
    OutputMissingInput,
    OutputCannotProduceStream,
    RootCycle,
    InvalidTransformArgument,
    FlowControlCannotStartComposition,
    CompileFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiagnosticLocation {
    RootNode(NodeId),
    RootRelation { index: usize },
    ContainerStack { container: ContainerId, index: usize },
    TransformSide { node: NodeId, side: Side },
    InputEndpoint { node: NodeId, endpoint: InputEndpoint },
    OutputEndpoint { node: NodeId, endpoint: OutputEndpoint },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub category: DiagnosticCategory,
    pub kind: DiagnosticKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<DiagnosticLocation>,
}

impl Diagnostic {
    pub fn new(
        category: DiagnosticCategory,
        kind: DiagnosticKind,
        message: impl Into<String>,
        location: Option<DiagnosticLocation>,
    ) -> Self {
        Self {
            category,
            kind,
            message: message.into(),
            location,
        }
    }
}
