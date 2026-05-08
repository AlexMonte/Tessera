use crate::domain::{
    AtomOperatorToken, AtomTile, ContainerId, ContainerSurfaceTile, NoteAtom, ScalarAtom,
};

pub fn note(value: impl AsRef<str>) -> ContainerSurfaceTile {
    ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new(value.as_ref())))
}

pub fn notes<const N: usize>(values: [&str; N]) -> Vec<ContainerSurfaceTile> {
    values.into_iter().map(note).collect()
}

pub fn scalar(value: i64) -> ContainerSurfaceTile {
    ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(value)))
}

pub fn rest() -> ContainerSurfaceTile {
    ContainerSurfaceTile::Atom(AtomTile::Rest)
}

pub fn op(token: AtomOperatorToken) -> ContainerSurfaceTile {
    ContainerSurfaceTile::Atom(AtomTile::Operator(token))
}

pub fn nested(id: impl Into<String>) -> ContainerSurfaceTile {
    ContainerSurfaceTile::NestedContainer(ContainerId::new(id))
}

pub fn stack(items: Vec<ContainerSurfaceTile>) -> Vec<ContainerSurfaceTile> {
    items
}

#[derive(Debug, Clone, Default)]
pub struct StackBuilder {
    items: Vec<ContainerSurfaceTile>,
}

impl StackBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(mut self, item: ContainerSurfaceTile) -> Self {
        self.items.push(item);
        self
    }

    pub fn build(self) -> Vec<ContainerSurfaceTile> {
        self.items
    }
}
