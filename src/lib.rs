//! Tessera: a spatial music-pattern language.
//!
//! Public usage should go through `infrastructure`.
//!
//! Module boundaries:
//! - `domain`: language model and contracts
//! - `application`: internal use-case machinery
//! - `infrastructure`: user-facing API facade
//!
//! Default public authoring is spatial source mode through
//! `AuthoredTesseraProgram` and `TesseraCompiler`.
//! Explicit graph authoring remains available as an optional public surface
//! under the `graph` feature.

mod application;
pub mod domain;
pub mod infrastructure;

pub mod prelude {
    pub use crate::domain::*;
    pub use crate::infrastructure::*;
}

#[cfg(feature = "graph")]
pub mod graph_prelude {
    pub use crate::domain::{
        InputEndpoint, InputPort, OutputEndpoint, OutputPort, RootRelation, StreamSource,
        StreamTarget, TesseraProgram,
    };
    #[cfg(feature = "builders")]
    pub use crate::infrastructure::TesseraProgramBuilder;
    pub use crate::infrastructure::TesseraProgramExt;
}

pub use infrastructure::{
    CompileOptions, CompileReport, PreviewReport, TesseraCompiler, ValidationReport,
};

#[cfg(test)]
mod tests {
    use crate::prelude::{
        AtomOperatorToken, AtomTile, Container, ContainerId, ContainerKind, ContainerSurfaceTile,
        EventValue, InputEndpoint, InputPort, NodeId, NoteAtom, OutputNode, PortGroupId,
        PortMemberId, Rational, RootRelation, RootSurfaceNodeKind, ScalarAtom, StreamSource,
        StreamTarget, TesseraCompiler, TesseraProgram, TransformKind, TransformNode,
    };
    use std::collections::BTreeMap;

    #[test]
    fn crate_compiles_simple_program() {
        let mut root_nodes = BTreeMap::new();
        let mut containers = BTreeMap::new();
        containers.insert(
            ContainerId::new("phrase"),
            Container {
                kind: ContainerKind::Sequence,
                stack: vec![
                    ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("a"))),
                    ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Elongate)),
                    ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(3))),
                ],
            },
        );
        containers.insert(
            ContainerId::new("rate"),
            Container {
                kind: ContainerKind::Sequence,
                stack: vec![ContainerSurfaceTile::Atom(AtomTile::Scalar(
                    ScalarAtom::integer(2),
                ))],
            },
        );
        root_nodes.insert(
            NodeId::new("slow"),
            RootSurfaceNodeKind::Transform(TransformNode::new(TransformKind::Slow)),
        );
        root_nodes.insert(
            NodeId::new("out"),
            RootSurfaceNodeKind::Output(OutputNode::default()),
        );
        root_nodes.insert(
            NodeId::new("a"),
            RootSurfaceNodeKind::Container {
                container: ContainerId::new("phrase"),
            },
        );
        root_nodes.insert(
            NodeId::new("rate"),
            RootSurfaceNodeKind::Container {
                container: ContainerId::new("rate"),
            },
        );

        let program = TesseraProgram {
            root_nodes,
            containers,
            relations: vec![
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("a")),
                    to: StreamTarget::TransformInput {
                        node: NodeId::new("slow"),
                        endpoint: InputEndpoint::Socket(InputPort::new("main")),
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("rate")),
                    to: StreamTarget::TransformInput {
                        node: NodeId::new("slow"),
                        endpoint: InputEndpoint::Socket(InputPort::new("factor")),
                    },
                },
                RootRelation::FlowsTo {
                    from: StreamSource::node(NodeId::new("slow")),
                    to: StreamTarget::OutputInput {
                        node: NodeId::new("out"),
                        endpoint: InputEndpoint::GroupMember {
                            group: PortGroupId::new("inputs"),
                            member: PortMemberId::new("main"),
                        },
                    },
                },
            ],
        };

        let report = TesseraCompiler::new()
            .compile(&program)
            .expect("program should compile");
        assert_eq!(report.ir.outputs.len(), 1);
        assert_eq!(report.ir.outputs[0].id, NodeId::new("out"));
        assert_eq!(report.ir.outputs[0].events.len(), 1);
        match &report.ir.outputs[0].events[0].value {
            EventValue::Note { value, octave } => {
                assert_eq!(value, "a");
                assert_eq!(*octave, None);
            }
            value => panic!("unexpected event value: {value:?}"),
        }
        assert_eq!(
            report.ir.outputs[0].events[0].span.duration.0,
            Rational::from_integer(2)
        );
    }
}
