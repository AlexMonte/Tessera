use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

pub const DELAY_PIECE_ID: &str = "tessera.delay";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rational {
    numerator: i64,
    denominator: i64,
}

impl Rational {
    pub const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };

    pub const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };

    pub fn new(numerator: i64, denominator: i64) -> Option<Self> {
        if denominator == 0 {
            return None;
        }

        let gcd = gcd(numerator.abs(), denominator.abs());
        let mut numerator = numerator / gcd;
        let mut denominator = denominator / gcd;
        if denominator < 0 {
            numerator *= -1;
            denominator *= -1;
        }

        Some(Self {
            numerator,
            denominator,
        })
    }

    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Some((numerator, denominator)) = trimmed.split_once('/') {
            let numerator = numerator.trim().parse::<i64>().ok()?;
            let denominator = denominator.trim().parse::<i64>().ok()?;
            return Self::new(numerator, denominator);
        }

        trimmed
            .parse::<i64>()
            .ok()
            .and_then(|value| Self::new(value, 1))
    }

    pub fn numerator(self) -> i64 {
        self.numerator
    }

    pub fn denominator(self) -> i64 {
        self.denominator
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.denominator == 1 {
            write!(f, "{}", self.numerator)
        } else {
            write!(f, "{}/{}", self.numerator, self.denominator)
        }
    }
}

impl Serialize for Rational {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Rational {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid rational literal '{value}'")))
    }
}

fn gcd(lhs: i64, rhs: i64) -> i64 {
    let mut lhs = lhs;
    let mut rhs = rhs;
    while rhs != 0 {
        let remainder = lhs % rhs;
        lhs = rhs;
        rhs = remainder;
    }
    lhs.max(1)
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

    pub fn rational() -> Self {
        Self::new("rational")
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
        self.is_any() || other.is_any() || self.kind == other.kind
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
            Plain(String),
            Full {
                kind: String,
                #[serde(default)]
                domain: Option<ExecutionDomain>,
            },
        }

        match PortTypeRepr::deserialize(deserializer)? {
            PortTypeRepr::Plain(kind) => Ok(Self {
                kind,
                domain: Some(ExecutionDomain::Control),
            }),
            PortTypeRepr::Full { kind, domain } => Ok(Self { kind, domain }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TileSide {
    #[serde(rename = "top", alias = "north")]
    TOP,
    #[serde(rename = "bottom", alias = "south")]
    BOTTOM,
    #[serde(rename = "right", alias = "east")]
    RIGHT,
    #[serde(rename = "left", alias = "west")]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PortRole {
    #[default]
    Value,
    Gate,
    Signal,
    Callback,
    Sequence,
    Field {
        name: String,
    },
}

impl PortRole {
    pub fn is_value(&self) -> bool {
        matches!(self, PortRole::Value)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DomainBridgeKind, ExecutionDomain, PieceCategory, PieceSemanticKind, PortType,
        PortTypeConnectionError, Rational, TileSide,
    };

    #[test]
    fn tile_side_deserializes_cardinal_aliases() {
        assert_eq!(
            serde_json::from_str::<TileSide>("\"north\"").unwrap(),
            TileSide::TOP
        );
        assert_eq!(
            serde_json::from_str::<TileSide>("\"south\"").unwrap(),
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
    fn rational_literals_parse_and_normalize() {
        assert_eq!(Rational::parse("1/4"), Some(Rational::new(1, 4).unwrap()));
        assert_eq!(Rational::parse("-2/4"), Some(Rational::new(-1, 2).unwrap()));
        assert_eq!(Rational::parse("3"), Some(Rational::new(3, 1).unwrap()));
    }

    #[test]
    fn rational_literals_reject_invalid_forms() {
        assert_eq!(Rational::parse("1/0"), None);
        assert_eq!(Rational::parse("abc"), None);
        assert_eq!(Rational::parse("1//4"), None);
        assert_eq!(Rational::parse("0.25"), None);
    }

    #[test]
    fn rational_serializes_canonically() {
        let value = Rational::new(-2, 4).unwrap();
        assert_eq!(value.to_string(), "-1/2");
        assert_eq!(serde_json::to_string(&value).unwrap(), "\"-1/2\"");
        assert_eq!(
            serde_json::from_str::<Rational>("\"-1/2\"").unwrap(),
            Rational::new(-1, 2).unwrap()
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
    fn port_type_common_type_returns_none_for_incompatible_types() {
        assert_eq!(PortType::number().common_type(&PortType::bool()), None);
    }

    #[test]
    fn port_type_resolve_connection_preserves_exact_matches() {
        let connection = PortType::text()
            .resolve_connection(&PortType::text())
            .expect("exact match");

        assert_eq!(connection.effective_type, PortType::text());
        assert_eq!(connection.bridge_kind, None);
    }

    #[test]
    fn port_type_resolve_connection_prefers_concrete_type_over_any() {
        let connection = PortType::any()
            .resolve_connection(&PortType::number())
            .expect("any should accept number");

        assert_eq!(connection.effective_type, PortType::number());
        assert_eq!(connection.bridge_kind, None);
    }

    #[test]
    fn port_type_resolve_connection_reports_supported_domain_bridges() {
        let connection = PortType::number()
            .resolve_connection(&PortType::number().with_domain(ExecutionDomain::Audio))
            .expect("audio -> control bridge");

        assert_eq!(connection.effective_type, PortType::number());
        assert_eq!(
            connection.bridge_kind,
            Some(DomainBridgeKind::AudioToControl)
        );
    }

    #[test]
    fn port_type_resolve_connection_rejects_unsupported_domain_bridges() {
        let err = PortType::number()
            .with_domain(ExecutionDomain::Event)
            .resolve_connection(&PortType::number().with_domain(ExecutionDomain::Audio));

        assert!(matches!(
            err,
            Err(PortTypeConnectionError::UnsupportedDomain { .. })
        ));
    }

    #[test]
    fn domain_bridge_kind_source_target_domain_round_trip() {
        let bridge =
            DomainBridgeKind::from_domains(ExecutionDomain::Control, ExecutionDomain::Audio)
                .expect("control -> audio");

        assert_eq!(bridge.source_domain(), ExecutionDomain::Control);
        assert_eq!(bridge.target_domain(), ExecutionDomain::Audio);
    }
}
