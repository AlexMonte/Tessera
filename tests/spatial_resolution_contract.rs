use tessera::prelude::*;

#[test]
fn adjacent_container_to_output_resolves_flow() {
    let mut program = AuthoredTesseraProgram::empty();
    program
        .place_sequence("phrase", slot(0, 0), notes(["a", "b", "c"]))
        .place_output("out", slot(1, 0));
    let resolved = TesseraCompiler::new()
        .resolve(&program)
        .expect("spatial program should resolve");
    assert_eq!(resolved.relations.len(), 1);
}

#[test]
fn adjacent_transform_chain_resolves() {
    let mut program = AuthoredTesseraProgram::empty();
    program
        .place_sequence("phrase", slot(0, 0), notes(["a", "b", "c"]))
        .place_transform("slow", slot(1, 0), TransformKind::Slow)
        .place_output("out", slot(2, 0));
    let resolved = TesseraCompiler::new()
        .resolve(&program)
        .expect("spatial program should resolve");
    assert_eq!(resolved.relations.len(), 2);
}

#[test]
fn factor_above_slow_resolves_factor_input() {
    let mut program = AuthoredTesseraProgram::empty();
    program
        .place_sequence("phrase", slot(0, 1), notes(["a", "b", "c"]))
        .place_sequence("factor", slot(1, 0), vec![scalar(2)])
        .place_transform("slow", slot(1, 1), TransformKind::Slow)
        .place_output("out", slot(2, 1));
    program.bind_output_side(
        "factor",
        OutputEndpoint::Socket(OutputPort::new("out")),
        SpatialSide::South,
    );
    let resolved = TesseraCompiler::new()
        .resolve(&program)
        .expect("spatial program should resolve");
    assert_eq!(resolved.relations.len(), 3);
}

#[test]
fn off_input_does_not_resolve() {
    let mut program = AuthoredTesseraProgram::empty();
    program
        .place_sequence("phrase", slot(0, 0), notes(["a"]))
        .place_transform("slow", slot(1, 0), TransformKind::Slow);
    program.bind_input_side(
        "slow",
        InputEndpoint::Socket(InputPort::new("main")),
        SpatialSide::Off,
    );
    let resolved = TesseraCompiler::new()
        .resolve(&program)
        .expect("off binding should not fail resolution");
    assert!(resolved.relations.is_empty());
}

#[test]
fn compile_authored_spatial_program() {
    let mut program = AuthoredTesseraProgram::empty();
    program
        .place_sequence("phrase", slot(0, 0), notes(["a", "b", "c"]))
        .place_output("out", slot(1, 0));
    let ir = TesseraCompiler::new()
        .compile_authored_ir(&program)
        .expect("authored spatial program should compile");
    assert_eq!(ir.outputs.len(), 1);
}
