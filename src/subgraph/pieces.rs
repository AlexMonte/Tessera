use std::collections::BTreeMap;

use serde_json::Value;

use crate::piece::{ParamDef, ParamInlineMode, ParamSchema, ParamValueKind, Piece, PieceDef};
use crate::types::{PieceCategory, PieceSemanticKind, PortType, TileSide};

use super::helpers::{can_inline_for_port, schema_for_port, subgraph_boundary_port_type};
use super::types::{
    SUBGRAPH_INPUT_1_ID, SUBGRAPH_INPUT_2_ID, SUBGRAPH_INPUT_3_ID, SUBGRAPH_OUTPUT_ID,
    SubgraphInput, SubgraphSignature,
};

pub struct SubgraphInputPiece {
    def: PieceDef,
    slot: u8,
}

impl SubgraphInputPiece {
    pub fn new(slot: u8) -> Self {
        let id = match slot {
            1 => SUBGRAPH_INPUT_1_ID,
            2 => SUBGRAPH_INPUT_2_ID,
            3 => SUBGRAPH_INPUT_3_ID,
            _ => SUBGRAPH_INPUT_1_ID,
        };
        Self {
            def: PieceDef {
                id: id.into(),
                label: format!("arg{slot}"),
                category: PieceCategory::Trick,
                semantic_kind: PieceSemanticKind::Trick,
                namespace: "core".into(),
                params: vec![
                    ParamDef {
                        id: "label".into(),
                        label: "label".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Text {
                            default: format!("input {slot}"),
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                        role: Default::default(),
                    },
                    ParamDef {
                        id: "port_type".into(),
                        label: "type".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Text {
                            default: "any".into(),
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                        role: Default::default(),
                    },
                    ParamDef {
                        id: "domain".into(),
                        label: "domain".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Enum {
                            options: vec![
                                "control".into(),
                                "audio".into(),
                                "event".into(),
                                "unspecified".into(),
                            ],
                            default: "control".into(),
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                        role: Default::default(),
                    },
                    ParamDef {
                        id: "required".into(),
                        label: "required".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Bool {
                            default: true,
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                        role: Default::default(),
                    },
                    ParamDef {
                        id: "is_receiver".into(),
                        label: "receiver".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Bool {
                            default: false,
                            can_inline: true,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                        role: Default::default(),
                    },
                    ParamDef {
                        id: "default_value".into(),
                        label: "default".into(),
                        side: TileSide::BOTTOM,
                        schema: ParamSchema::Custom {
                            port_type: PortType::any().with_unspecified_domain(),
                            value_kind: ParamValueKind::Json,
                            default: None,
                            can_inline: true,
                            inline_mode: ParamInlineMode::Literal,
                            min: None,
                            max: None,
                        },
                        text_semantics: Default::default(),
                        variadic_group: None,
                        required: false,
                        role: Default::default(),
                    },
                ],
                output_type: Some(PortType::any().with_unspecified_domain()),
                output_side: Some(TileSide::RIGHT),
                output_role: Default::default(),
                description: Some(
                    "Subgraph boundary input. Configure its metadata via inline params.".into(),
                ),
                tags: vec!["subgraph".into(), "boundary".into()],
            },
            slot,
        }
    }
}

impl Piece for SubgraphInputPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }

    fn infer_output_type(
        &self,
        _input_types: &BTreeMap<String, PortType>,
        inline_params: &BTreeMap<String, Value>,
    ) -> Option<PortType> {
        let _ = self.slot;
        Some(subgraph_boundary_port_type(inline_params))
    }
}

pub struct SubgraphOutputPiece {
    def: PieceDef,
}

impl Default for SubgraphOutputPiece {
    fn default() -> Self {
        Self::new()
    }
}

impl SubgraphOutputPiece {
    pub fn new() -> Self {
        Self {
            def: PieceDef {
                id: SUBGRAPH_OUTPUT_ID.into(),
                label: "return".into(),
                category: PieceCategory::Output,
                semantic_kind: PieceSemanticKind::Output,
                namespace: "core".into(),
                params: vec![ParamDef {
                    id: "input".into(),
                    label: "input".into(),
                    side: TileSide::LEFT,
                    schema: ParamSchema::Custom {
                        port_type: PortType::any().with_unspecified_domain(),
                        value_kind: ParamValueKind::None,
                        default: None,
                        can_inline: false,
                        inline_mode: ParamInlineMode::Literal,
                        min: None,
                        max: None,
                    },
                    text_semantics: Default::default(),
                    variadic_group: None,
                    required: true,
                    role: Default::default(),
                }],
                output_type: None,
                output_side: None,
                output_role: Default::default(),
                description: Some(
                    "Subgraph boundary output. Connect the value this subgraph should emit.".into(),
                ),
                tags: vec!["subgraph".into(), "boundary".into()],
            },
        }
    }
}

impl Piece for SubgraphOutputPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }
}

#[derive(Clone)]
pub struct GeneratedSubgraphPiece {
    def: PieceDef,
    ordered_inputs: Vec<SubgraphInput>,
}

impl GeneratedSubgraphPiece {
    pub fn new(
        subgraph_id: &str,
        label: &str,
        ordered_inputs: &[SubgraphInput],
        output_type: Option<PortType>,
    ) -> Self {
        let params = ordered_inputs
            .iter()
            .map(|input| ParamDef {
                id: format!("arg{}", input.slot),
                label: input.label.clone(),
                side: TileSide::LEFT,
                schema: schema_for_port(
                    &input.port_type,
                    input.default_value.clone(),
                    can_inline_for_port(&input.port_type),
                ),
                text_semantics: Default::default(),
                variadic_group: None,
                required: input.required,
                role: Default::default(),
            })
            .collect::<Vec<_>>();

        Self {
            def: PieceDef {
                id: format!("tessera.subgraph.{subgraph_id}"),
                label: label.into(),
                category: PieceCategory::Trick,
                semantic_kind: PieceSemanticKind::Trick,
                namespace: "user".into(),
                params,
                output_type: output_type
                    .clone()
                    .or_else(|| Some(PortType::any().with_unspecified_domain())),
                output_side: Some(TileSide::RIGHT),
                output_role: Default::default(),
                description: Some("User-defined subgraph signature.".into()),
                tags: vec!["subgraph".into()],
            },
            ordered_inputs: ordered_inputs.to_vec(),
        }
    }

    pub fn from_signature(subgraph_id: &str, label: &str, signature: &SubgraphSignature) -> Self {
        Self::new(
            subgraph_id,
            label,
            &signature.inputs,
            signature.output_type.clone(),
        )
    }

    pub fn ordered_inputs(&self) -> &[SubgraphInput] {
        &self.ordered_inputs
    }
}

impl Piece for GeneratedSubgraphPiece {
    fn def(&self) -> &PieceDef {
        &self.def
    }
}

pub fn subgraph_pieces(defs: &[(&str, &str, SubgraphSignature)]) -> Vec<GeneratedSubgraphPiece> {
    defs.iter()
        .map(|(subgraph_id, label, signature)| {
            GeneratedSubgraphPiece::from_signature(subgraph_id, label, signature)
        })
        .collect()
}

pub fn subgraph_editor_pieces() -> Vec<Box<dyn Piece>> {
    vec![
        Box::new(SubgraphInputPiece::new(1)),
        Box::new(SubgraphInputPiece::new(2)),
        Box::new(SubgraphInputPiece::new(3)),
        Box::new(SubgraphOutputPiece::new()),
    ]
}
