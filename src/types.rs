use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Determinism contract: GridPos ordering is always col-first, then row.
// Keep Ord impl explicit so future field edits do not silently change topo tie-breaking.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GridPos {
    pub col: i32,
    pub row: i32,
}

impl GridPos {
    pub fn adjacent_in_direction(&self, side: TileSide) -> Self {
        match side {
            TileSide::TOP => Self {
                col: self.col,
                row: self.row - 1,
            },
            TileSide::BOTTOM => Self {
                col: self.col,
                row: self.row + 1,
            },
            TileSide::RIGHT => Self {
                col: self.col + 1,
                row: self.row,
            },
            TileSide::LEFT => Self {
                col: self.col - 1,
                row: self.row,
            },
            TileSide::NONE => self.clone(),
        }
    }
}

pub fn adjacent_in_direction(pos: &GridPos, side: &TileSide) -> GridPos {
    pos.adjacent_in_direction(*side)
}

impl PartialOrd for GridPos {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GridPos {
    fn cmp(&self, other: &Self) -> Ordering {
        self.col.cmp(&other.col).then(self.row.cmp(&other.row))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EdgeId(pub Uuid);

impl EdgeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PortType(String);

impl PortType {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn number() -> Self {
        Self::new("number")
    }

    pub fn text() -> Self {
        Self::new("text")
    }

    pub fn bool() -> Self {
        Self::new("bool")
    }

    pub fn any() -> Self {
        Self::new("any")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_any(&self) -> bool {
        self.as_str() == "any"
    }

    pub fn accepts(&self, other: &PortType) -> bool {
        self.is_any()
            || other.is_any()
            || self == other
            // In mini-notation languages a text string is a valid pattern literal.
            || (self.as_str() == "pattern" && other.as_str() == "text")
    }
}

impl From<&str> for PortType {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for PortType {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum TileSide {
    #[default]
    #[serde(rename = "none", alias = "n_o_n_e")]
    NONE,
    #[serde(rename = "top", alias = "north", alias = "t_o_p")]
    TOP,
    #[serde(
        rename = "bottom",
        alias = "south",
        alias = "buttom",
        alias = "b_o_t_t_o_m"
    )]
    BOTTOM,
    #[serde(rename = "right", alias = "east", alias = "r_i_g_h_t")]
    RIGHT,
    #[serde(rename = "left", alias = "west", alias = "l_e_f_t")]
    LEFT,
}

impl TileSide {
    pub fn faces(self, other: TileSide) -> bool {
        matches!(
            (self, other),
            (TileSide::RIGHT, TileSide::LEFT)
                | (TileSide::LEFT, TileSide::RIGHT)
                | (TileSide::TOP, TileSide::BOTTOM)
                | (TileSide::BOTTOM, TileSide::TOP)
        )
    }

    pub fn opposite(self) -> TileSide {
        match self {
            TileSide::TOP => TileSide::BOTTOM,
            TileSide::BOTTOM => TileSide::TOP,
            TileSide::LEFT => TileSide::RIGHT,
            TileSide::RIGHT => TileSide::LEFT,
            TileSide::NONE => TileSide::NONE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceCategory {
    Generator,
    Transform,
    Trick,
    Constant,
    Output,
    Control,
    Connector,
}

#[cfg(test)]
mod tests {
    use super::TileSide;

    #[test]
    fn tile_side_deserializes_legacy_and_typo_variants() {
        assert_eq!(
            serde_json::from_str::<TileSide>("\"north\"").unwrap(),
            TileSide::TOP
        );
        assert_eq!(
            serde_json::from_str::<TileSide>("\"south\"").unwrap(),
            TileSide::BOTTOM
        );
        assert_eq!(
            serde_json::from_str::<TileSide>("\"buttom\"").unwrap(),
            TileSide::BOTTOM
        );
        assert_eq!(
            serde_json::from_str::<TileSide>("\"east\"").unwrap(),
            TileSide::RIGHT
        );
        assert_eq!(
            serde_json::from_str::<TileSide>("\"west\"").unwrap(),
            TileSide::LEFT
        );
        assert_eq!(
            serde_json::from_str::<TileSide>("\"t_o_p\"").unwrap(),
            TileSide::TOP
        );
    }

    #[test]
    fn tile_side_serializes_canonical_variants() {
        assert_eq!(serde_json::to_string(&TileSide::TOP).unwrap(), "\"top\"");
        assert_eq!(
            serde_json::to_string(&TileSide::BOTTOM).unwrap(),
            "\"bottom\""
        );
        assert_eq!(
            serde_json::to_string(&TileSide::RIGHT).unwrap(),
            "\"right\""
        );
        assert_eq!(serde_json::to_string(&TileSide::LEFT).unwrap(), "\"left\"");
    }
}
