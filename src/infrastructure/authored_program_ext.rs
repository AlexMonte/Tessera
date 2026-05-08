use crate::domain::{
    AuthoredTesseraProgram, BoardSlot, Container, ContainerId, ContainerKind, ContainerSurfaceTile,
    FlowControlKind, FlowControlNode, InputEndpoint, NodeId, OutputEndpoint, OutputNode,
    RootPlacement, RootRelation, RootSurfaceNodeKind, SpatialSide, TileFootprint, TransformKind,
    TransformNode, apply_flow_member_default_bindings, default_spatial_bindings,
};

pub trait AuthoredTesseraProgramExt {
    fn place_container(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        kind: ContainerKind,
        stack: Vec<ContainerSurfaceTile>,
    ) -> &mut Self;
    fn place_sequence(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        stack: Vec<ContainerSurfaceTile>,
    ) -> &mut Self;
    fn place_alternate(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        stack: Vec<ContainerSurfaceTile>,
    ) -> &mut Self;
    fn place_layer_container(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        stack: Vec<ContainerSurfaceTile>,
    ) -> &mut Self;
    fn place_transform(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        kind: TransformKind,
    ) -> &mut Self;
    fn place_output(&mut self, id: impl Into<String>, slot: BoardSlot) -> &mut Self;
    fn place_flow_control(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        kind: FlowControlKind,
    ) -> &mut Self;
    fn place_layer(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        members: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self;
    fn place_split(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        members: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self;
    fn place_route(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        members: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self;
    fn bind_input_side(
        &mut self,
        node: impl Into<String>,
        endpoint: InputEndpoint,
        side: SpatialSide,
    ) -> &mut Self;
    fn bind_output_side(
        &mut self,
        node: impl Into<String>,
        endpoint: OutputEndpoint,
        side: SpatialSide,
    ) -> &mut Self;
    fn add_explicit_relation(&mut self, relation: RootRelation) -> &mut Self;
    fn rotate_node_cw(&mut self, node: impl Into<String>) -> &mut Self;
    fn rotate_node_ccw(&mut self, node: impl Into<String>) -> &mut Self;
}

impl AuthoredTesseraProgramExt for AuthoredTesseraProgram {
    fn place_container(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        kind: ContainerKind,
        stack: Vec<ContainerSurfaceTile>,
    ) -> &mut Self {
        let id = id.into();
        let container_id = ContainerId::new(id.clone());
        let node_id = NodeId::new(id);
        self.containers
            .insert(container_id.clone(), Container { kind, stack });
        let node = RootSurfaceNodeKind::Container {
            container: container_id,
        };
        insert_placed_node(self, node_id, slot, node);
        self
    }

    fn place_sequence(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        stack: Vec<ContainerSurfaceTile>,
    ) -> &mut Self {
        self.place_container(id, slot, ContainerKind::Sequence, stack)
    }

    fn place_alternate(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        stack: Vec<ContainerSurfaceTile>,
    ) -> &mut Self {
        self.place_container(id, slot, ContainerKind::Alternate, stack)
    }

    fn place_layer_container(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        stack: Vec<ContainerSurfaceTile>,
    ) -> &mut Self {
        self.place_container(id, slot, ContainerKind::Layer, stack)
    }

    fn place_transform(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        kind: TransformKind,
    ) -> &mut Self {
        let node_id = NodeId::new(id);
        let node = RootSurfaceNodeKind::Transform(TransformNode::new(kind));
        insert_placed_node(self, node_id, slot, node);
        self
    }

    fn place_output(&mut self, id: impl Into<String>, slot: BoardSlot) -> &mut Self {
        let node_id = NodeId::new(id);
        let node = RootSurfaceNodeKind::Output(OutputNode::default());
        insert_placed_node(self, node_id, slot, node);
        self
    }

    fn place_flow_control(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        kind: FlowControlKind,
    ) -> &mut Self {
        let node_id = NodeId::new(id);
        let node = RootSurfaceNodeKind::FlowControl(FlowControlNode::new(kind));
        insert_placed_node(self, node_id, slot, node);
        self
    }

    fn place_layer(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        members: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self {
        let mut node = FlowControlNode::new(FlowControlKind::Layer);
        node.members.inputs.insert(
            crate::domain::PortGroupId::new("streams"),
            members
                .into_iter()
                .map(crate::domain::PortMemberId::new)
                .collect(),
        );
        let node_id = NodeId::new(id);
        let node = RootSurfaceNodeKind::FlowControl(node);
        insert_placed_node(self, node_id, slot, node);
        self
    }

    fn place_split(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        members: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self {
        let mut node = FlowControlNode::new(FlowControlKind::Split);
        node.members.outputs.insert(
            crate::domain::PortGroupId::new("branches"),
            members
                .into_iter()
                .map(crate::domain::PortMemberId::new)
                .collect(),
        );
        let node_id = NodeId::new(id);
        let node = RootSurfaceNodeKind::FlowControl(node);
        insert_placed_node(self, node_id, slot, node);
        self
    }

    fn place_route(
        &mut self,
        id: impl Into<String>,
        slot: BoardSlot,
        members: impl IntoIterator<Item = impl Into<String>>,
    ) -> &mut Self {
        let mut node = FlowControlNode::new(FlowControlKind::Route);
        node.members.outputs.insert(
            crate::domain::PortGroupId::new("routes"),
            members
                .into_iter()
                .map(crate::domain::PortMemberId::new)
                .collect(),
        );
        let node_id = NodeId::new(id);
        let node = RootSurfaceNodeKind::FlowControl(node);
        insert_placed_node(self, node_id, slot, node);
        self
    }

    fn bind_input_side(
        &mut self,
        node: impl Into<String>,
        endpoint: InputEndpoint,
        side: SpatialSide,
    ) -> &mut Self {
        let node = NodeId::new(node);
        self.root_surface
            .bindings
            .entry(node)
            .or_default()
            .inputs
            .insert(endpoint, side);
        self
    }

    fn bind_output_side(
        &mut self,
        node: impl Into<String>,
        endpoint: OutputEndpoint,
        side: SpatialSide,
    ) -> &mut Self {
        let node = NodeId::new(node);
        self.root_surface
            .bindings
            .entry(node)
            .or_default()
            .outputs
            .insert(endpoint, side);
        self
    }

    fn add_explicit_relation(&mut self, relation: RootRelation) -> &mut Self {
        self.root_surface.explicit_relations.push(relation);
        self
    }

    fn rotate_node_cw(&mut self, node: impl Into<String>) -> &mut Self {
        let node = NodeId::new(node);
        if let Some(bindings) = self.root_surface.bindings.get_mut(&node) {
            bindings.rotate_cw();
        }
        self
    }

    fn rotate_node_ccw(&mut self, node: impl Into<String>) -> &mut Self {
        let node = NodeId::new(node);
        if let Some(bindings) = self.root_surface.bindings.get_mut(&node) {
            bindings.rotate_ccw();
        }
        self
    }
}

fn insert_placed_node(
    program: &mut AuthoredTesseraProgram,
    node_id: NodeId,
    slot: BoardSlot,
    node: RootSurfaceNodeKind,
) {
    let mut bindings = default_spatial_bindings(&node);
    if let RootSurfaceNodeKind::FlowControl(flow) = &node {
        apply_flow_member_default_bindings(flow, &mut bindings);
    }
    program.root_surface.nodes.insert(node_id.clone(), node);
    program.root_surface.placements.insert(
        node_id.clone(),
        RootPlacement {
            slot,
            footprint: TileFootprint::unit(),
        },
    );
    program.root_surface.bindings.insert(node_id, bindings);
}
