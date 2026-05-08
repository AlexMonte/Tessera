use crate::domain::{
    Container, ContainerId, ContainerKind, ContainerSurfaceTile, InputEndpoint, NodeId, OutputNode,
    RootRelation, RootSurfaceNodeKind, StreamSource, StreamTarget, TesseraProgram,
};

pub trait TesseraProgramExt {
    fn empty() -> Self;
    fn add_sequence(
        &mut self,
        id: impl Into<String>,
        stack: Vec<ContainerSurfaceTile>,
    ) -> &mut Self;
    fn add_output(&mut self, id: impl Into<String>) -> &mut Self;
    fn connect(&mut self, from: impl Into<String>, to: impl Into<String>) -> &mut Self;
}

impl TesseraProgramExt for TesseraProgram {
    fn empty() -> Self {
        Self::default()
    }

    fn add_sequence(
        &mut self,
        id: impl Into<String>,
        stack: Vec<ContainerSurfaceTile>,
    ) -> &mut Self {
        let id = id.into();
        let container_id = ContainerId::new(id.clone());
        self.containers.insert(
            container_id.clone(),
            Container {
                kind: ContainerKind::Sequence,
                stack,
            },
        );
        self.root_nodes.insert(
            NodeId::new(id),
            RootSurfaceNodeKind::Container {
                container: container_id,
            },
        );
        self
    }

    fn add_output(&mut self, id: impl Into<String>) -> &mut Self {
        self.root_nodes.insert(
            NodeId::new(id),
            RootSurfaceNodeKind::Output(OutputNode::default()),
        );
        self
    }

    fn connect(&mut self, from: impl Into<String>, to: impl Into<String>) -> &mut Self {
        self.relations.push(RootRelation::FlowsTo {
            from: StreamSource::node(NodeId::new(from)),
            to: StreamTarget::OutputInput {
                node: NodeId::new(to),
                endpoint: InputEndpoint::GroupMember {
                    group: crate::domain::PortGroupId::new("inputs"),
                    member: crate::domain::PortMemberId::new("main"),
                },
            },
        });
        self
    }
}
