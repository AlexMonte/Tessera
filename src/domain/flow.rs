use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::pattern_ir::Rational;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformKind {
    Slow,
    Fast,
    Rev,
    Transpose,
    Gain,
    Attack,
    Degrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowControlKind {
    Layer,
    Merge,
    Mix,
    Split,
    Mask,
    Switch,
    Route,
    Choice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeInputRole {
    Main,
    Aux,
    Control,
    Mask,
    Mix,
    Choice,
    Route,
    Sidechain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionRule {
    Required,
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortCountRule {
    ZeroOrMore,
    OneOrMore,
    Exactly(u32),
    Range { min: u32, max: u32 },
}

impl PortCountRule {
    pub fn range(min: u32, max: u32) -> Result<Self, String> {
        if min > max {
            Err("port count range requires min <= max".into())
        } else {
            Ok(Self::Range { min, max })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamShape {
    Any,
    NotePattern,
    ScalarPattern,
    ControlPattern,
    EventPattern,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InputPort(pub String);

impl InputPort {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OutputPort(pub String);

impl OutputPort {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PortGroupId(pub String);

impl PortGroupId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PortMemberId(pub String);

impl PortMemberId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DefaultStreamBehavior {
    ConstantScalar { value: Rational },
}

impl DefaultStreamBehavior {
    pub fn shape(&self) -> StreamShape {
        match self {
            Self::ConstantScalar { .. } => StreamShape::ScalarPattern,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputSocketSpec {
    pub port: InputPort,
    pub role: NodeInputRole,
    pub shape: StreamShape,
    pub connection: ConnectionRule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<DefaultStreamBehavior>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputGroupSpec {
    pub group: PortGroupId,
    pub role: NodeInputRole,
    pub shape: StreamShape,
    pub count: PortCountRule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputSocketSpec {
    pub port: OutputPort,
    pub shape: StreamShape,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputGroupSpec {
    pub group: PortGroupId,
    pub shape: StreamShape,
    pub count: PortCountRule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NodeSignature {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_sockets: Vec<InputSocketSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_groups: Vec<InputGroupSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_sockets: Vec<OutputSocketSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_groups: Vec<OutputGroupSpec>,
}

impl NodeSignature {
    pub fn new(
        input_sockets: Vec<InputSocketSpec>,
        input_groups: Vec<InputGroupSpec>,
        output_sockets: Vec<OutputSocketSpec>,
        output_groups: Vec<OutputGroupSpec>,
    ) -> Result<Self, String> {
        validate_socket_defaults(&input_sockets)?;
        validate_unique_ports(
            input_sockets.iter().map(|spec| spec.port.0.as_str()),
            "duplicate input socket id",
        )?;
        validate_unique_ports(
            input_groups.iter().map(|spec| spec.group.0.as_str()),
            "duplicate input group id",
        )?;
        validate_unique_ports(
            output_sockets.iter().map(|spec| spec.port.0.as_str()),
            "duplicate output socket id",
        )?;
        validate_unique_ports(
            output_groups.iter().map(|spec| spec.group.0.as_str()),
            "duplicate output group id",
        )?;
        validate_no_socket_group_collision(
            input_sockets.iter().map(|spec| spec.port.0.as_str()),
            input_groups.iter().map(|spec| spec.group.0.as_str()),
            "input socket id collides with input group id",
        )?;
        validate_no_socket_group_collision(
            output_sockets.iter().map(|spec| spec.port.0.as_str()),
            output_groups.iter().map(|spec| spec.group.0.as_str()),
            "output socket id collides with output group id",
        )?;

        Ok(Self {
            input_sockets,
            input_groups,
            output_sockets,
            output_groups,
        })
    }

    pub fn input_socket(&self, port: &InputPort) -> Option<&InputSocketSpec> {
        self.input_sockets.iter().find(|spec| &spec.port == port)
    }

    pub fn input_group(&self, group: &PortGroupId) -> Option<&InputGroupSpec> {
        self.input_groups.iter().find(|spec| &spec.group == group)
    }

    pub fn output_socket(&self, port: &OutputPort) -> Option<&OutputSocketSpec> {
        self.output_sockets.iter().find(|spec| &spec.port == port)
    }

    pub fn output_group(&self, group: &PortGroupId) -> Option<&OutputGroupSpec> {
        self.output_groups.iter().find(|spec| &spec.group == group)
    }
}

fn validate_unique_ports<'a>(
    ids: impl Iterator<Item = &'a str>,
    message: &str,
) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        if !seen.insert(id.to_string()) {
            return Err(message.into());
        }
    }
    Ok(())
}

fn validate_no_socket_group_collision<'a>(
    sockets: impl Iterator<Item = &'a str>,
    groups: impl Iterator<Item = &'a str>,
    message: &str,
) -> Result<(), String> {
    let socket_ids = sockets.collect::<std::collections::BTreeSet<_>>();
    for group in groups {
        if socket_ids.contains(group) {
            return Err(message.into());
        }
    }
    Ok(())
}

fn validate_socket_defaults(sockets: &[InputSocketSpec]) -> Result<(), String> {
    for socket in sockets {
        if let Some(default) = &socket.default && default.shape() != socket.shape {
            return Err(format!(
                "default stream for socket '{}' does not match declared shape",
                socket.port.0
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlowControlPolicy {
    Layer,
    MergeAppend,
    MergeInterleave,
    MergePriority,
    MergeDeduplicate,
    MixFieldBlend,
    MixWeighted,
    MixGainAverage,
    SplitCopyToAll,
    SplitByIndexModulo,
    SplitByEventField,
    SplitByPitchRange { threshold_octave: i64 },
    MaskGate,
    MaskScale,
    MaskClip,
    MaskInvertGate,
    SwitchCycleIndex,
    SwitchControlValue,
    SwitchSeededRandom,
    RouteByEventField,
    RouteByIndexModulo,
    RouteByControlValue,
    RouteByLabel,
    ChoiceCycle,
    ChoiceSeededRandom,
    ChoiceWeighted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GroupMembers {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<PortGroupId, Vec<PortMemberId>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<PortGroupId, Vec<PortMemberId>>,
}

impl GroupMembers {
    pub fn validate(&self) -> Result<(), String> {
        for members in self.inputs.values().chain(self.outputs.values()) {
            let mut seen = std::collections::BTreeSet::new();
            for member in members {
                if !seen.insert(member.clone()) {
                    return Err("duplicate declared group member".into());
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformNode {
    pub kind: TransformKind,
    pub signature: NodeSignature,
}

impl TransformNode {
    pub fn new(kind: TransformKind) -> Self {
        Self {
            kind,
            signature: default_transform_signature(kind),
        }
    }
}

fn default_transform_signature(kind: TransformKind) -> NodeSignature {
    match kind {
        TransformKind::Slow => NodeSignature {
            input_sockets: vec![
                InputSocketSpec {
                    port: InputPort::new("main"),
                    role: NodeInputRole::Main,
                    shape: StreamShape::EventPattern,
                    connection: ConnectionRule::Required,
                    side: Some(Side::Left),
                    default: None,
                },
                InputSocketSpec {
                    port: InputPort::new("factor"),
                    role: NodeInputRole::Aux,
                    shape: StreamShape::ScalarPattern,
                    connection: ConnectionRule::Optional,
                    side: Some(Side::Top),
                    default: Some(DefaultStreamBehavior::ConstantScalar {
                        value: Rational::from_integer(2),
                    }),
                },
            ],
            output_sockets: vec![OutputSocketSpec {
                port: OutputPort::new("out"),
                shape: StreamShape::EventPattern,
                side: Some(Side::Right),
            }],
            ..NodeSignature::default()
        },
        TransformKind::Fast => NodeSignature {
            input_sockets: vec![
                InputSocketSpec {
                    port: InputPort::new("main"),
                    role: NodeInputRole::Main,
                    shape: StreamShape::EventPattern,
                    connection: ConnectionRule::Required,
                    side: Some(Side::Left),
                    default: None,
                },
                InputSocketSpec {
                    port: InputPort::new("factor"),
                    role: NodeInputRole::Aux,
                    shape: StreamShape::ScalarPattern,
                    connection: ConnectionRule::Optional,
                    side: Some(Side::Top),
                    default: Some(DefaultStreamBehavior::ConstantScalar {
                        value: Rational::from_integer(2),
                    }),
                },
            ],
            output_sockets: vec![OutputSocketSpec {
                port: OutputPort::new("out"),
                shape: StreamShape::EventPattern,
                side: Some(Side::Right),
            }],
            ..NodeSignature::default()
        },
        TransformKind::Rev => NodeSignature {
            input_sockets: vec![InputSocketSpec {
                port: InputPort::new("main"),
                role: NodeInputRole::Main,
                shape: StreamShape::EventPattern,
                connection: ConnectionRule::Required,
                side: Some(Side::Left),
                default: None,
            }],
            output_sockets: vec![OutputSocketSpec {
                port: OutputPort::new("out"),
                shape: StreamShape::EventPattern,
                side: Some(Side::Right),
            }],
            ..NodeSignature::default()
        },
        TransformKind::Transpose | TransformKind::Gain | TransformKind::Attack | TransformKind::Degrade => {
            NodeSignature {
                input_sockets: vec![
                    InputSocketSpec {
                        port: InputPort::new("main"),
                        role: NodeInputRole::Main,
                        shape: StreamShape::EventPattern,
                        connection: ConnectionRule::Required,
                        side: Some(Side::Left),
                        default: None,
                    },
                    InputSocketSpec {
                        port: InputPort::new("amount"),
                        role: NodeInputRole::Aux,
                        shape: StreamShape::ScalarPattern,
                        connection: ConnectionRule::Optional,
                        side: Some(Side::Top),
                        default: Some(DefaultStreamBehavior::ConstantScalar {
                            value: Rational::from_integer(1),
                        }),
                    },
                ],
                output_sockets: vec![OutputSocketSpec {
                    port: OutputPort::new("out"),
                    shape: StreamShape::EventPattern,
                    side: Some(Side::Right),
                }],
                ..NodeSignature::default()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowControlNode {
    pub kind: FlowControlKind,
    pub signature: NodeSignature,
    pub policy: FlowControlPolicy,
    #[serde(default)]
    pub members: GroupMembers,
}

impl FlowControlNode {
    pub fn new(kind: FlowControlKind) -> Self {
        let signature = default_flow_control_signature(kind);
        let members = default_flow_control_members(kind);
        members
            .validate()
            .expect("default flow-control members should be valid");
        let policy = default_flow_control_policy(kind);
        Self {
            kind,
            signature,
            policy,
            members,
        }
    }

    pub fn with_policy(mut self, policy: FlowControlPolicy) -> Self {
        self.policy = policy;
        self
    }
}

fn default_flow_control_policy(kind: FlowControlKind) -> FlowControlPolicy {
    match kind {
        FlowControlKind::Layer => FlowControlPolicy::Layer,
        FlowControlKind::Merge => FlowControlPolicy::MergeAppend,
        FlowControlKind::Mix => FlowControlPolicy::MixFieldBlend,
        FlowControlKind::Split => FlowControlPolicy::SplitByIndexModulo,
        FlowControlKind::Mask => FlowControlPolicy::MaskGate,
        FlowControlKind::Switch => FlowControlPolicy::SwitchCycleIndex,
        FlowControlKind::Route => FlowControlPolicy::RouteByIndexModulo,
        FlowControlKind::Choice => FlowControlPolicy::ChoiceCycle,
    }
}

fn default_flow_control_members(kind: FlowControlKind) -> GroupMembers {
    let mut members = GroupMembers::default();
    match kind {
        FlowControlKind::Layer | FlowControlKind::Merge | FlowControlKind::Mix => {
            members.inputs.insert(
                PortGroupId::new("streams"),
                vec![PortMemberId::new("a"), PortMemberId::new("b")],
            );
        }
        FlowControlKind::Split => {
            members.outputs.insert(
                PortGroupId::new("branches"),
                vec![PortMemberId::new("even"), PortMemberId::new("odd")],
            );
        }
        FlowControlKind::Switch => {
            members.inputs.insert(
                PortGroupId::new("candidates"),
                vec![PortMemberId::new("a"), PortMemberId::new("b")],
            );
        }
        FlowControlKind::Route => {
            members.outputs.insert(
                PortGroupId::new("routes"),
                vec![PortMemberId::new("left"), PortMemberId::new("right")],
            );
        }
        FlowControlKind::Choice => {
            members.inputs.insert(
                PortGroupId::new("options"),
                vec![PortMemberId::new("a"), PortMemberId::new("b")],
            );
        }
        FlowControlKind::Mask => {}
    }
    members
}

fn default_flow_control_signature(kind: FlowControlKind) -> NodeSignature {
    match kind {
        FlowControlKind::Layer => NodeSignature {
            input_groups: vec![InputGroupSpec {
                group: PortGroupId::new("streams"),
                role: NodeInputRole::Main,
                shape: StreamShape::EventPattern,
                count: PortCountRule::OneOrMore,
            }],
            output_sockets: vec![OutputSocketSpec {
                port: OutputPort::new("out"),
                shape: StreamShape::EventPattern,
                side: Some(Side::Right),
            }],
            ..NodeSignature::default()
        },
        FlowControlKind::Merge => NodeSignature {
            input_groups: vec![InputGroupSpec {
                group: PortGroupId::new("streams"),
                role: NodeInputRole::Main,
                shape: StreamShape::EventPattern,
                count: PortCountRule::OneOrMore,
            }],
            output_sockets: vec![OutputSocketSpec {
                port: OutputPort::new("out"),
                shape: StreamShape::EventPattern,
                side: Some(Side::Right),
            }],
            ..NodeSignature::default()
        },
        FlowControlKind::Mix => NodeSignature {
            input_groups: vec![InputGroupSpec {
                group: PortGroupId::new("streams"),
                role: NodeInputRole::Mix,
                shape: StreamShape::EventPattern,
                count: PortCountRule::OneOrMore,
            }],
            input_sockets: vec![InputSocketSpec {
                port: InputPort::new("amount"),
                role: NodeInputRole::Control,
                shape: StreamShape::ScalarPattern,
                connection: ConnectionRule::Optional,
                side: Some(Side::Top),
                default: None,
            }],
            output_sockets: vec![OutputSocketSpec {
                port: OutputPort::new("out"),
                shape: StreamShape::EventPattern,
                side: Some(Side::Right),
            }],
            ..NodeSignature::default()
        },
        FlowControlKind::Split => NodeSignature {
            input_sockets: vec![InputSocketSpec {
                port: InputPort::new("main"),
                role: NodeInputRole::Main,
                shape: StreamShape::EventPattern,
                connection: ConnectionRule::Required,
                side: Some(Side::Left),
                default: None,
            }],
            output_groups: vec![OutputGroupSpec {
                group: PortGroupId::new("branches"),
                shape: StreamShape::EventPattern,
                count: PortCountRule::OneOrMore,
            }],
            ..NodeSignature::default()
        },
        FlowControlKind::Mask => NodeSignature {
            input_sockets: vec![
                InputSocketSpec {
                    port: InputPort::new("main"),
                    role: NodeInputRole::Main,
                    shape: StreamShape::EventPattern,
                    connection: ConnectionRule::Required,
                    side: Some(Side::Left),
                    default: None,
                },
                InputSocketSpec {
                    port: InputPort::new("mask"),
                    role: NodeInputRole::Mask,
                    shape: StreamShape::ControlPattern,
                    connection: ConnectionRule::Required,
                    side: Some(Side::Top),
                    default: None,
                },
            ],
            output_sockets: vec![OutputSocketSpec {
                port: OutputPort::new("out"),
                shape: StreamShape::EventPattern,
                side: Some(Side::Right),
            }],
            ..NodeSignature::default()
        },
        FlowControlKind::Switch => NodeSignature {
            input_groups: vec![InputGroupSpec {
                group: PortGroupId::new("candidates"),
                role: NodeInputRole::Choice,
                shape: StreamShape::EventPattern,
                count: PortCountRule::OneOrMore,
            }],
            input_sockets: vec![InputSocketSpec {
                port: InputPort::new("control"),
                role: NodeInputRole::Control,
                shape: StreamShape::ScalarPattern,
                connection: ConnectionRule::Optional,
                side: Some(Side::Top),
                default: None,
            }],
            output_sockets: vec![OutputSocketSpec {
                port: OutputPort::new("out"),
                shape: StreamShape::EventPattern,
                side: Some(Side::Right),
            }],
            ..NodeSignature::default()
        },
        FlowControlKind::Route => NodeSignature {
            input_sockets: vec![
                InputSocketSpec {
                    port: InputPort::new("main"),
                    role: NodeInputRole::Main,
                    shape: StreamShape::EventPattern,
                    connection: ConnectionRule::Required,
                    side: Some(Side::Left),
                    default: None,
                },
                InputSocketSpec {
                    port: InputPort::new("control"),
                    role: NodeInputRole::Control,
                    shape: StreamShape::ControlPattern,
                    connection: ConnectionRule::Optional,
                    side: Some(Side::Top),
                    default: None,
                },
            ],
            output_groups: vec![OutputGroupSpec {
                group: PortGroupId::new("routes"),
                shape: StreamShape::EventPattern,
                count: PortCountRule::OneOrMore,
            }],
            ..NodeSignature::default()
        },
        FlowControlKind::Choice => NodeSignature {
            input_groups: vec![InputGroupSpec {
                group: PortGroupId::new("options"),
                role: NodeInputRole::Choice,
                shape: StreamShape::EventPattern,
                count: PortCountRule::OneOrMore,
            }],
            input_sockets: vec![InputSocketSpec {
                port: InputPort::new("control"),
                role: NodeInputRole::Control,
                shape: StreamShape::ScalarPattern,
                connection: ConnectionRule::Optional,
                side: Some(Side::Top),
                default: None,
            }],
            output_sockets: vec![OutputSocketSpec {
                port: OutputPort::new("out"),
                shape: StreamShape::EventPattern,
                side: Some(Side::Right),
            }],
            ..NodeSignature::default()
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub signature: NodeSignature,
}

impl Default for OutputNode {
    fn default() -> Self {
        let signature = NodeSignature::new(
            Vec::new(),
            vec![InputGroupSpec {
                group: PortGroupId::new("inputs"),
                role: NodeInputRole::Main,
                shape: StreamShape::EventPattern,
                count: PortCountRule::OneOrMore,
            }],
            Vec::new(),
            Vec::new(),
        )
        .expect("default output signature should be valid");
        Self {
            label: None,
            signature,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RootSurfaceNodeKind {
    Container { container: super::ContainerId },
    Transform(TransformNode),
    FlowControl(FlowControlNode),
    Output(OutputNode),
}
