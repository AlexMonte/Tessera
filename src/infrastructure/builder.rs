use crate::domain::TesseraProgram;

#[cfg(feature = "graph")]
use crate::domain::ContainerSurfaceTile;
#[cfg(feature = "graph")]
use crate::infrastructure::TesseraProgramExt;

#[derive(Debug, Clone, Default)]
pub struct TesseraProgramBuilder {
    program: TesseraProgram,
}

impl TesseraProgramBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build(self) -> TesseraProgram {
        self.program
    }

    #[cfg(feature = "graph")]
    pub fn add_sequence(mut self, id: impl Into<String>, stack: Vec<ContainerSurfaceTile>) -> Self {
        self.program.add_sequence(id, stack);
        self
    }

    #[cfg(feature = "graph")]
    pub fn add_output(mut self, id: impl Into<String>) -> Self {
        self.program.add_output(id);
        self
    }

    #[cfg(feature = "graph")]
    pub fn connect(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.program.connect(from, to);
        self
    }
}
