mod authored_program_ext;
#[cfg(feature = "builders")]
mod builder;
mod compiler;
mod ports;
#[cfg(feature = "graph")]
mod program_ext;
mod reports;
mod spatial;
mod stack;

pub use authored_program_ext::AuthoredTesseraProgramExt;
#[cfg(feature = "builders")]
pub use builder::TesseraProgramBuilder;
pub use compiler::{CompileOptions, TesseraCompiler};
pub use ports::*;
#[cfg(feature = "graph")]
pub use program_ext::TesseraProgramExt;
pub use reports::{CompileReport, PreviewReport, ValidationReport};
pub use spatial::{footprint, placement, placement_with_footprint, slot, unit_footprint};
pub use stack::{StackBuilder, nested, note, notes, op, rest, scalar, stack};
