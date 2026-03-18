use std::cmp::Ordering;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

// Determinism contract: GridPos ordering is always col-first, then row.
// Keep Ord impl explicit so future field edits do not silently change topo tie-breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GridPos {
    pub col: i32,
    pub row: i32,
}

impl GridPos {
    pub fn adjacent_in_direction(&self, side: Option<TileSide>) -> Self {
        match side {
            Some(TileSide::TOP) => Self {
                col: self.col,
                row: self.row - 1,
            },
            Some(TileSide::BOTTOM) => Self {
                col: self.col,
                row: self.row + 1,
            },
            Some(TileSide::RIGHT) => Self {
                col: self.col + 1,
                row: self.row,
            },
            Some(TileSide::LEFT) => Self {
                col: self.col - 1,
                row: self.row,
            },
            None => *self,
        }
    }
}

pub fn adjacent_in_direction(pos: &GridPos, side: Option<TileSide>) -> GridPos {
    pos.adjacent_in_direction(side)
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

impl Default for EdgeId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionDomain {
    Audio,
    Control,
    Event,
}

impl Default for ExecutionDomain {
    fn default() -> Self {
        Self::Control
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainBridgeKind {
    ControlToAudio,
    AudioToControl,
    EventToControl,
}

impl DomainBridgeKind {
    pub fn from_domains(
        source: ExecutionDomain,
        target: ExecutionDomain,
    ) -> Option<DomainBridgeKind> {
        match (source, target) {
            (ExecutionDomain::Control, ExecutionDomain::Audio) => Some(Self::ControlToAudio),
            (ExecutionDomain::Audio, ExecutionDomain::Control) => Some(Self::AudioToControl),
            (ExecutionDomain::Event, ExecutionDomain::Control) => Some(Self::EventToControl),
            _ => None,
        }
    }

    pub fn source_domain(self) -> ExecutionDomain {
        match self {
            Self::ControlToAudio => ExecutionDomain::Control,
            Self::AudioToControl => ExecutionDomain::Audio,
            Self::EventToControl => ExecutionDomain::Event,
        }
    }

    pub fn target_domain(self) -> ExecutionDomain {
        match self {
            Self::ControlToAudio => ExecutionDomain::Audio,
            Self::AudioToControl | Self::EventToControl => ExecutionDomain::Control,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ControlToAudio => "control_to_audio",
            Self::AudioToControl => "audio_to_control",
            Self::EventToControl => "event_to_control",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainBridge {
    pub edge_id: EdgeId,
    pub source_pos: GridPos,
    pub target_pos: GridPos,
    pub param: String,
    pub kind: DomainBridgeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortType {
    kind: String,
    domain: Option<ExecutionDomain>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PortTypeConnection {
    pub effective_type: PortType,
    pub bridge_kind: Option<DomainBridgeKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PortTypeConnectionError {
    ValueMismatch { expected: PortType, got: PortType },
    UnsupportedDomain { expected: PortType, got: PortType },
}

impl PortType {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            kind: id.into(),
            domain: Some(ExecutionDomain::Control),
        }
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

    pub fn with_domain(mut self, domain: ExecutionDomain) -> Self {
        self.domain = Some(domain);
        self
    }

    pub fn with_unspecified_domain(mut self) -> Self {
        self.domain = None;
        self
    }

    pub fn as_str(&self) -> &str {
        &self.kind
    }

    pub fn domain(&self) -> Option<ExecutionDomain> {
        self.domain
    }

    pub fn is_any(&self) -> bool {
        self.as_str() == "any"
    }

    pub fn accepts(&self, other: &PortType) -> bool {
        self.accepts_value(other)
    }

    pub(crate) fn resolve_connection(
        &self,
        source: &PortType,
    ) -> Result<PortTypeConnection, PortTypeConnectionError> {
        let Some(mut effective_type) = self.common_value_type(source) else {
            return Err(PortTypeConnectionError::ValueMismatch {
                expected: self.clone(),
                got: source.clone(),
            });
        };

        let target_domain = self.domain.or(source.domain);
        let bridge_kind = match (source.domain, target_domain) {
            (Some(source_domain), Some(target_domain)) if source_domain != target_domain => {
                let Some(bridge_kind) =
                    DomainBridgeKind::from_domains(source_domain, target_domain)
                else {
                    return Err(PortTypeConnectionError::UnsupportedDomain {
                        expected: self.clone(),
                        got: source.clone(),
                    });
                };
                effective_type.domain = Some(target_domain);
                Some(bridge_kind)
            }
            (Some(source_domain), _) => {
                effective_type.domain = Some(source_domain);
                None
            }
            (None, Some(target_domain)) => {
                effective_type.domain = Some(target_domain);
                None
            }
            (None, None) => {
                effective_type.domain = None;
                None
            }
        };

        Ok(PortTypeConnection {
            effective_type,
            bridge_kind,
        })
    }

    fn accepts_value(&self, other: &PortType) -> bool {
        self.is_any()
            || other.is_any()
            || self == other
            // In mini-notation languages a text string is a valid pattern literal.
            || (self.as_str() == "pattern" && other.as_str() == "text")
    }

    fn common_value_type(&self, other: &PortType) -> Option<PortType> {
        if self.kind == other.kind {
            Some(Self {
                kind: self.kind.clone(),
                domain: None,
            })
        } else if self.is_any() {
            Some(Self {
                kind: other.kind.clone(),
                domain: None,
            })
        } else if other.is_any() || self.accepts_value(other) {
            Some(Self {
                kind: self.kind.clone(),
                domain: None,
            })
        } else if other.accepts_value(self) {
            Some(Self {
                kind: other.kind.clone(),
                domain: None,
            })
        } else {
            None
        }
    }

    pub fn common_type(&self, other: &PortType) -> Option<PortType> {
        let mut common = self.common_value_type(other)?;
        common.domain = match (self.domain, other.domain) {
            (Some(lhs), Some(rhs)) if lhs == rhs => Some(lhs),
            (Some(_), Some(_)) => return None,
            (Some(domain), None) | (None, Some(domain)) => Some(domain),
            (None, None) => None,
        };
        Some(common)
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

impl Serialize for PortType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct PortTypeRepr<'a> {
            kind: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            domain: Option<ExecutionDomain>,
        }

        match self.domain {
            Some(ExecutionDomain::Control) => serializer.serialize_str(self.as_str()),
            other => PortTypeRepr {
                kind: self.as_str(),
                domain: other,
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for PortType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum PortTypeRepr {
            Legacy(String),
            Rich {
                kind: String,
                #[serde(default)]
                domain: Option<ExecutionDomain>,
            },
        }

        match PortTypeRepr::deserialize(deserializer)? {
            PortTypeRepr::Legacy(kind) => Ok(Self {
                kind,
                domain: Some(ExecutionDomain::Control),
            }),
            PortTypeRepr::Rich { kind, domain } => Ok(Self { kind, domain }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TileSide {
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceSemanticKind {
    Literal,
    Operator,
    Construct,
    Intrinsic,
    Trick,
    Output,
    Connector,
}

#[cfg(test)]
mod tests {
    use super::{
        DomainBridgeKind, ExecutionDomain, PieceCategory, PieceSemanticKind, PortType, TileSide,
    };

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

    #[test]
    fn piece_category_and_semantic_kind_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&PieceCategory::Constant).unwrap(),
            "\"constant\""
        );
        assert_eq!(
            serde_json::to_string(&PieceSemanticKind::Construct).unwrap(),
            "\"construct\""
        );
    }

    #[test]
    fn port_type_common_type_prefers_specific_over_any() {
        assert_eq!(
            PortType::any().common_type(&PortType::number()),
            Some(PortType::number())
        );
        assert_eq!(
            PortType::number().common_type(&PortType::any()),
            Some(PortType::number())
        );
    }

    #[test]
    fn port_type_common_type_uses_compatible_pattern_supertype() {
        assert_eq!(
            PortType::new("pattern").common_type(&PortType::text()),
            Some(PortType::new("pattern"))
        );
    }

    #[test]
    fn port_type_common_type_returns_none_for_incompatible_types() {
        assert_eq!(PortType::number().common_type(&PortType::bool()), None);
    }

    #[test]
    fn port_type_deserializes_legacy_string_to_control_domain() {
        let port = serde_json::from_str::<PortType>("\"number\"").expect("deserialize port type");
        assert_eq!(port, PortType::number());
        assert_eq!(port.domain(), Some(ExecutionDomain::Control));
    }

    #[test]
    fn port_type_round_trips_audio_domain_as_object() {
        let port = PortType::number().with_domain(ExecutionDomain::Audio);
        let json = serde_json::to_value(&port).expect("serialize port type");
        assert_eq!(json, serde_json::json!({"kind": "number", "domain": "audio"}));
        let round_trip: PortType = serde_json::from_value(json).expect("deserialize port type");
        assert_eq!(round_trip, port);
    }

    #[test]
    fn port_type_round_trips_unspecified_domain_as_kind_only_object() {
        let port = PortType::new("pattern").with_unspecified_domain();
        let json = serde_json::to_value(&port).expect("serialize port type");
        assert_eq!(json, serde_json::json!({"kind": "pattern"}));
        let round_trip: PortType = serde_json::from_value(json).expect("deserialize port type");
        assert_eq!(round_trip, port);
        assert_eq!(round_trip.domain(), None);
    }

    #[test]
    fn resolve_connection_allows_supported_domain_bridge() {
        let expected = PortType::number().with_domain(ExecutionDomain::Audio);
        let source = PortType::number().with_domain(ExecutionDomain::Control);
        let resolution = expected
            .resolve_connection(&source)
            .expect("bridgeable connection");

        assert_eq!(
            resolution.bridge_kind,
            Some(DomainBridgeKind::ControlToAudio)
        );
        assert_eq!(resolution.effective_type, expected);
    }

    #[test]
    fn resolve_connection_rejects_unsupported_domain_bridge() {
        let expected = PortType::number().with_domain(ExecutionDomain::Event);
        let source = PortType::number().with_domain(ExecutionDomain::Audio);
        let err = expected
            .resolve_connection(&source)
            .expect_err("unsupported bridge");

        assert!(matches!(
            err,
            super::PortTypeConnectionError::UnsupportedDomain { .. }
        ));
    }
}
