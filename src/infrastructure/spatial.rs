use crate::domain::{BoardSlot, RootPlacement, TileFootprint};

pub fn slot(x: i32, y: i32) -> BoardSlot {
    BoardSlot::new(x, y)
}

pub fn footprint(width: u32, height: u32) -> TileFootprint {
    TileFootprint::new(width, height)
}

pub fn unit_footprint() -> TileFootprint {
    TileFootprint::unit()
}

pub fn placement(x: i32, y: i32) -> RootPlacement {
    RootPlacement::unit(x, y)
}

pub fn placement_with_footprint(x: i32, y: i32, width: u32, height: u32) -> RootPlacement {
    RootPlacement::new(BoardSlot::new(x, y), TileFootprint::new(width, height))
}
