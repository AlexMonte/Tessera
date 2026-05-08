use serde::{Deserialize, Serialize};

use super::{InputPort, NodeId, OutputPort, PortGroupId, PortMemberId};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputEndpoint {
    Socket(InputPort),
    GroupMember {
        group: PortGroupId,
        member: PortMemberId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputEndpoint {
    Socket(OutputPort),
    GroupMember {
        group: PortGroupId,
        member: PortMemberId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StreamSource {
    pub node: NodeId,
    pub endpoint: OutputEndpoint,
}

impl StreamSource {
    pub fn socket(node: NodeId, port: impl Into<String>) -> Self {
        Self {
            node,
            endpoint: OutputEndpoint::Socket(OutputPort::new(port)),
        }
    }

    pub fn node(node: NodeId) -> Self {
        Self::socket(node, "out")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamTarget {
    OutputInput {
        node: NodeId,
        endpoint: InputEndpoint,
    },
    TransformInput {
        node: NodeId,
        endpoint: InputEndpoint,
    },
    FlowControlInput {
        node: NodeId,
        endpoint: InputEndpoint,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RootRelation {
    ChainedTo {
        from: StreamSource,
        to: NodeId,
    },
    FlowsTo {
        from: StreamSource,
        to: StreamTarget,
    },
}
