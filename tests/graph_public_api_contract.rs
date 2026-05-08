#![cfg(feature = "graph")]

use tessera::graph_prelude::*;
use tessera::prelude::{TesseraCompiler, TesseraProgramBuilder, notes};

#[test]
fn graph_mode_ext_compiles_simple_program() {
    let mut graph = TesseraProgram::empty();
    graph
        .add_sequence("phrase", notes(["a", "b", "c"]))
        .add_output("out")
        .connect("phrase", "out");

    let ir = TesseraCompiler::new()
        .compile_ir(&graph)
        .expect("graph program should compile");
    assert_eq!(ir.outputs.len(), 1);
}

#[test]
fn graph_mode_builder_compiles_simple_program() {
    let graph = TesseraProgramBuilder::new()
        .add_sequence("phrase", notes(["a", "b", "c"]))
        .add_output("out")
        .connect("phrase", "out")
        .build();

    let ir = TesseraCompiler::new()
        .compile_ir(&graph)
        .expect("builder graph program should compile");
    assert_eq!(ir.outputs.len(), 1);
}
