use std::collections::BTreeMap;

use crate::domain::{
    Container, ContainerId, InputEndpoint, NodeId, OutputEndpoint, RootRelation,
    RootSurfaceNodeKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredTesseraProgram {
    pub root_surface: RootSurface,
    pub containers: BTreeMap<ContainerId, Container>,
}

impl AuthoredTesseraProgram {
    pub fn empty() -> Self {
        Self {
            root_surface: RootSurface::default(),
            containers: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RootSurface {
    pub nodes: BTreeMap<NodeId, RootSurfaceNodeKind>,
    pub placements: BTreeMap<NodeId, RootPlacement>,
    pub bindings: BTreeMap<NodeId, NodeSpatialBindings>,
    pub explicit_relations: Vec<RootRelation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoardSlot {
    pub x: i32,
    pub y: i32,
}

impl BoardSlot {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn offset(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileFootprint {
    pub width: u32,
    pub height: u32,
}

impl TileFootprint {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub fn unit() -> Self {
        Self {
            width: 1,
            height: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootPlacement {
    pub slot: BoardSlot,
    pub footprint: TileFootprint,
}

impl RootPlacement {
    pub fn new(slot: BoardSlot, footprint: TileFootprint) -> Self {
        Self { slot, footprint }
    }

    pub fn unit(x: i32, y: i32) -> Self {
        Self {
            slot: BoardSlot::new(x, y),
            footprint: TileFootprint::unit(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpatialSide {
    Off,
    North,
    South,
    West,
    East,
}

impl SpatialSide {
    pub fn is_enabled(self) -> bool {
        !matches!(self, SpatialSide::Off)
    }

    pub fn offset(self) -> Option<(i32, i32)> {
        match self {
            SpatialSide::Off => None,
            SpatialSide::North => Some((0, -1)),
            SpatialSide::South => Some((0, 1)),
            SpatialSide::West => Some((-1, 0)),
            SpatialSide::East => Some((1, 0)),
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            SpatialSide::Off => SpatialSide::Off,
            SpatialSide::North => SpatialSide::South,
            SpatialSide::South => SpatialSide::North,
            SpatialSide::West => SpatialSide::East,
            SpatialSide::East => SpatialSide::West,
        }
    }

    pub fn rotate_cw(self) -> Self {
        match self {
            SpatialSide::Off => SpatialSide::Off,
            SpatialSide::North => SpatialSide::East,
            SpatialSide::East => SpatialSide::South,
            SpatialSide::South => SpatialSide::West,
            SpatialSide::West => SpatialSide::North,
        }
    }

    pub fn rotate_ccw(self) -> Self {
        match self {
            SpatialSide::Off => SpatialSide::Off,
            SpatialSide::North => SpatialSide::West,
            SpatialSide::West => SpatialSide::South,
            SpatialSide::South => SpatialSide::East,
            SpatialSide::East => SpatialSide::North,
        }
    }

    pub fn mirror_vertical(self) -> Self {
        match self {
            SpatialSide::North => SpatialSide::South,
            SpatialSide::South => SpatialSide::North,
            other => other,
        }
    }

    pub fn mirror_horizontal(self) -> Self {
        match self {
            SpatialSide::West => SpatialSide::East,
            SpatialSide::East => SpatialSide::West,
            other => other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NodeSpatialBindings {
    pub inputs: BTreeMap<InputEndpoint, SpatialSide>,
    pub outputs: BTreeMap<OutputEndpoint, SpatialSide>,
}

impl NodeSpatialBindings {
    pub fn bind_input(mut self, endpoint: InputEndpoint, side: SpatialSide) -> Self {
        self.inputs.insert(endpoint, side);
        self
    }

    pub fn bind_output(mut self, endpoint: OutputEndpoint, side: SpatialSide) -> Self {
        self.outputs.insert(endpoint, side);
        self
    }

    pub fn rotate_cw(&mut self) {
        for side in self.inputs.values_mut() {
            *side = side.rotate_cw();
        }
        for side in self.outputs.values_mut() {
            *side = side.rotate_cw();
        }
    }

    pub fn rotate_ccw(&mut self) {
        for side in self.inputs.values_mut() {
            *side = side.rotate_ccw();
        }
        for side in self.outputs.values_mut() {
            *side = side.rotate_ccw();
        }
    }

    pub fn mirror_vertical(&mut self) {
        for side in self.inputs.values_mut() {
            *side = side.mirror_vertical();
        }
        for side in self.outputs.values_mut() {
            *side = side.mirror_vertical();
        }
    }

    pub fn mirror_horizontal(&mut self) {
        for side in self.inputs.values_mut() {
            *side = side.mirror_horizontal();
        }
        for side in self.outputs.values_mut() {
            *side = side.mirror_horizontal();
        }
    }
}
