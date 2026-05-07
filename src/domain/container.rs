use serde::{Deserialize, Serialize};

use super::atom::{AtomExpr, AtomTile};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContainerId(pub String);

impl ContainerId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerKind {
    Sequence,
    Alternate,
    Layer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContainerSurfaceTile {
    Atom(AtomTile),
    NestedContainer(ContainerId),
    Transform,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    pub kind: ContainerKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<ContainerSurfaceTile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedContainer {
    pub id: ContainerId,
    pub kind: ContainerKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exprs: Vec<AtomExpr>,
}
