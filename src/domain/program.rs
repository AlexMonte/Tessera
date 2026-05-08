use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Container, ContainerId, RootRelation, RootSurfaceNodeKind};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TesseraProgram {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub root_nodes: BTreeMap<NodeId, RootSurfaceNodeKind>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub containers: BTreeMap<ContainerId, Container>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<RootRelation>,
}

impl TesseraProgram {
    pub fn empty() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NormalizedProgram {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub root_nodes: BTreeMap<NodeId, RootSurfaceNodeKind>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub containers: BTreeMap<ContainerId, super::NormalizedContainer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<RootRelation>,
}
