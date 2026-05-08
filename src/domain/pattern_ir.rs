use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Sub};

use serde::{Deserialize, Serialize};

use super::program::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rational {
    pub numerator: i64,
    pub denominator: i64,
}

impl Rational {
    pub fn new(numerator: i64, denominator: i64) -> Self {
        assert!(denominator != 0, "denominator cannot be zero");
        let sign = if denominator < 0 { -1 } else { 1 };
        let mut numerator = numerator * sign;
        let mut denominator = denominator.abs();
        let gcd = gcd_i64(numerator.abs(), denominator);
        numerator /= gcd;
        denominator /= gcd;
        Self {
            numerator,
            denominator,
        }
    }

    pub fn from_integer(value: i64) -> Self {
        Self::new(value, 1)
    }

    pub fn zero() -> Self {
        Self::from_integer(0)
    }

    pub fn one() -> Self {
        Self::from_integer(1)
    }

    pub fn is_zero(self) -> bool {
        self.numerator == 0
    }

    pub fn is_positive(self) -> bool {
        self > Self::zero()
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.numerator as i128 * other.denominator as i128)
            .cmp(&(other.numerator as i128 * self.denominator as i128))
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Add for Rational {
    type Output = Rational;
    fn add(self, rhs: Self) -> Self::Output {
        Rational::new(
            self.numerator * rhs.denominator + rhs.numerator * self.denominator,
            self.denominator * rhs.denominator,
        )
    }
}

impl Sub for Rational {
    type Output = Rational;
    fn sub(self, rhs: Self) -> Self::Output {
        Rational::new(
            self.numerator * rhs.denominator - rhs.numerator * self.denominator,
            self.denominator * rhs.denominator,
        )
    }
}

impl Mul for Rational {
    type Output = Rational;
    fn mul(self, rhs: Self) -> Self::Output {
        Rational::new(
            self.numerator * rhs.numerator,
            self.denominator * rhs.denominator,
        )
    }
}

impl Div for Rational {
    type Output = Rational;
    fn div(self, rhs: Self) -> Self::Output {
        assert!(!rhs.is_zero(), "cannot divide by zero rational");
        Rational::new(
            self.numerator * rhs.denominator,
            self.denominator * rhs.numerator,
        )
    }
}

fn gcd_i64(a: i64, b: i64) -> i64 {
    if b == 0 { a.max(1) } else { gcd_i64(b, a % b) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CycleTime(pub Rational);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CycleDuration(pub Rational);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleSpan {
    pub start: CycleTime,
    pub duration: CycleDuration,
}

impl CycleSpan {
    pub fn new(start: CycleTime, duration: CycleDuration) -> Self {
        Self { start, duration }
    }

    pub fn end(self) -> CycleTime {
        CycleTime(self.start.0 + self.duration.0)
    }

    pub fn shift_by(self, offset: CycleDuration) -> Self {
        Self {
            start: CycleTime(self.start.0 + offset.0),
            duration: self.duration,
        }
    }

    pub fn scale_relative_to(self, origin: CycleTime, factor: Rational) -> Self {
        let relative_start = self.start.0 - origin.0;
        Self {
            start: CycleTime(origin.0 + relative_start * factor),
            duration: CycleDuration(self.duration.0 * factor),
        }
    }

    pub fn reverse_within(self, origin: CycleTime, end: CycleTime) -> Self {
        let event_end = self.end();
        Self {
            start: CycleTime(end.0 - (event_end.0 - origin.0)),
            duration: self.duration,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventValue {
    Note {
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        octave: Option<i64>,
    },
    Rest,
    Scalar {
        value: Rational,
    },
}

impl EventValue {
    pub fn is_rest(&self) -> bool {
        matches!(self, Self::Rest)
    }

    pub fn is_scalar(&self) -> bool {
        matches!(self, Self::Scalar { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldValue {
    Rational { value: Rational },
    Bool { value: bool },
    Symbol { value: String },
}

impl FieldValue {
    pub fn rational(value: Rational) -> Self {
        Self::Rational { value }
    }

    pub fn bool(value: bool) -> Self {
        Self::Bool { value }
    }

    pub fn symbol(value: impl Into<String>) -> Self {
        Self::Symbol {
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventField {
    Gain(FieldValue),
    PostGain(FieldValue),
    Pan(FieldValue),
    Pitch(FieldValue),
    PitchBend(FieldValue),
    PlaybackRate(FieldValue),
    PlaybackStart(FieldValue),
    PlaybackEnd(FieldValue),
    Reverse(FieldValue),
    Attack(FieldValue),
    Decay(FieldValue),
    Sustain(FieldValue),
    Release(FieldValue),
    LowPassCutoff(FieldValue),
    LowPassResonance(FieldValue),
    HighPassCutoff(FieldValue),
    HighPassResonance(FieldValue),
    ReverbSend(FieldValue),
    DelaySend(FieldValue),
    Select(FieldValue),
    Custom { key: String, value: FieldValue },
    Elongate(FieldValue),
    Replicate(FieldValue),
    Degrade(FieldValue),
    RandomChoice,
    Transpose(FieldValue),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternEvent {
    pub span: CycleSpan,
    pub value: EventValue,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<EventField>,
}

impl PatternEvent {
    pub fn new(span: CycleSpan, value: EventValue) -> Self {
        Self {
            span,
            value,
            fields: Vec::new(),
        }
    }

    pub fn with_fields(mut self, fields: Vec<EventField>) -> Self {
        self.fields = fields;
        self
    }

    pub fn shift_by(mut self, offset: CycleDuration) -> Self {
        self.span = self.span.shift_by(offset);
        self
    }

    pub fn scale_relative_to(mut self, origin: CycleTime, factor: Rational) -> Self {
        self.span = self.span.scale_relative_to(origin, factor);
        self
    }

    pub fn reverse_within(mut self, origin: CycleTime, end: CycleTime) -> Self {
        self.span = self.span.reverse_within(origin, end);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternStreamShape {
    Event,
    Control,
    Scalar,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PatternStream {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<PatternEvent>,
}

impl PatternStream {
    pub fn new(events: Vec<PatternEvent>) -> Self {
        Self { events }
    }

    pub fn bounds(&self) -> Option<(CycleTime, CycleTime)> {
        let first = self.events.first()?;
        let mut min_start = first.span.start.0;
        let mut max_end = first.span.end().0;
        for event in &self.events {
            if event.span.start.0 < min_start {
                min_start = event.span.start.0;
            }
            let end = event.span.end().0;
            if end > max_end {
                max_end = end;
            }
        }
        Some((CycleTime(min_start), CycleTime(max_end)))
    }

    pub fn duration(&self) -> CycleDuration {
        self.bounds()
            .map(|(start, end)| CycleDuration(end.0 - start.0))
            .unwrap_or(CycleDuration(Rational::zero()))
    }

    pub fn shift_by(&self, offset: CycleDuration) -> Self {
        Self {
            events: self
                .events
                .iter()
                .cloned()
                .map(|event| event.shift_by(offset))
                .collect(),
        }
    }

    pub fn layer(streams: Vec<PatternStream>) -> Self {
        let mut events = Vec::new();
        for stream in streams {
            events.extend(stream.events);
        }
        Self { events }
    }

    pub fn chain(left: PatternStream, right: PatternStream) -> Self {
        let offset = left.duration();
        let mut events = left.normalize_to_origin().events;
        events.extend(right.normalize_to_origin().shift_by(offset).events);
        Self { events }
    }

    pub fn reverse(self) -> Self {
        let Some((origin, end)) = self.bounds() else {
            return self;
        };
        Self {
            events: self
                .events
                .into_iter()
                .map(|event| event.reverse_within(origin, end))
                .collect(),
        }
    }

    pub fn slow(self, factor: Rational) -> Self {
        self.scale_relative(factor)
    }

    pub fn fast(self, factor: Rational) -> Self {
        if factor <= Rational::zero() {
            return self;
        }
        self.scale_relative(Rational::one() / factor)
    }

    fn scale_relative(self, factor: Rational) -> Self {
        let Some((origin, _)) = self.bounds() else {
            return self;
        };
        Self {
            events: self
                .events
                .into_iter()
                .map(|event| event.scale_relative_to(origin, factor))
                .collect(),
        }
    }

    pub fn normalize_to_origin(self) -> Self {
        let Some((origin, _)) = self.bounds() else {
            return self;
        };
        self.shift_by(CycleDuration(Rational::zero() - origin.0))
    }

    pub fn without_rests(self) -> Self {
        Self {
            events: self
                .events
                .into_iter()
                .filter(|event| !event.value.is_rest())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ControlStream {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<ControlEvent>,
}

impl ControlStream {
    pub fn new(controls: Vec<ControlEvent>) -> Self {
        Self { controls }
    }

    pub fn to_pattern_stream(&self) -> PatternStream {
        PatternStream {
            events: self
                .controls
                .iter()
                .cloned()
                .map(PatternEvent::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlEvent {
    pub span: CycleSpan,
    pub key: ControlKeyIr,
    pub value: ControlValueIr,
}

impl ControlEvent {
    pub fn new(span: CycleSpan, key: ControlKeyIr, value: ControlValueIr) -> Self {
        Self { span, key, value }
    }
}

impl From<ControlEvent> for PatternEvent {
    fn from(control: ControlEvent) -> Self {
        PatternEvent {
            span: control.span,
            value: EventValue::Scalar {
                value: control.value.as_rational_fallback(),
            },
            fields: vec![EventField::Custom {
                key: control.key.as_str().to_string(),
                value: control.value.into_field_value(),
            }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKeyIr {
    Gate,
    Gain,
    PostGain,
    Pan,
    Pitch,
    PitchBend,
    PlaybackRate,
    PlaybackStart,
    PlaybackEnd,
    Reverse,
    Attack,
    Decay,
    Sustain,
    Release,
    LowPassCutoff,
    LowPassResonance,
    HighPassCutoff,
    HighPassResonance,
    ReverbSend,
    DelaySend,
    Select(String),
    Custom(String),
}

impl ControlKeyIr {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Gate => "gate",
            Self::Gain => "gain",
            Self::PostGain => "post_gain",
            Self::Pan => "pan",
            Self::Pitch => "pitch",
            Self::PitchBend => "pitch_bend",
            Self::PlaybackRate => "playback_rate",
            Self::PlaybackStart => "playback_start",
            Self::PlaybackEnd => "playback_end",
            Self::Reverse => "reverse",
            Self::Attack => "attack",
            Self::Decay => "decay",
            Self::Sustain => "sustain",
            Self::Release => "release",
            Self::LowPassCutoff => "low_pass_cutoff",
            Self::LowPassResonance => "low_pass_resonance",
            Self::HighPassCutoff => "high_pass_cutoff",
            Self::HighPassResonance => "high_pass_resonance",
            Self::ReverbSend => "reverb_send",
            Self::DelaySend => "delay_send",
            Self::Select(_) => "select",
            Self::Custom(_) => "custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlValueIr {
    Rational { value: Rational },
    Bool { value: bool },
    Symbol { value: String },
}

impl ControlValueIr {
    pub fn rational(value: Rational) -> Self {
        Self::Rational { value }
    }

    pub fn bool(value: bool) -> Self {
        Self::Bool { value }
    }

    pub fn symbol(value: impl Into<String>) -> Self {
        Self::Symbol {
            value: value.into(),
        }
    }

    pub fn as_rational_fallback(&self) -> Rational {
        match self {
            Self::Rational { value } => *value,
            Self::Bool { value } => {
                if *value {
                    Rational::one()
                } else {
                    Rational::zero()
                }
            }
            Self::Symbol { .. } => Rational::zero(),
        }
    }

    pub fn into_field_value(self) -> FieldValue {
        match self {
            Self::Rational { value } => FieldValue::rational(value),
            Self::Bool { value } => FieldValue::bool(value),
            Self::Symbol { value } => FieldValue::symbol(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScalarStream {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<ScalarEvent>,
}

impl ScalarStream {
    pub fn new(values: Vec<ScalarEvent>) -> Self {
        Self { values }
    }

    pub fn to_pattern_stream(&self) -> PatternStream {
        PatternStream {
            events: self
                .values
                .iter()
                .cloned()
                .map(PatternEvent::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScalarEvent {
    pub span: CycleSpan,
    pub value: Rational,
}

impl ScalarEvent {
    pub fn new(span: CycleSpan, value: Rational) -> Self {
        Self { span, value }
    }
}

impl From<ScalarEvent> for PatternEvent {
    fn from(value: ScalarEvent) -> Self {
        PatternEvent {
            span: value.span,
            value: EventValue::Scalar { value: value.value },
            fields: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeightedPatternIr {
    pub weight: Rational,
    pub node: Box<PatternNodeIr>,
}

impl WeightedPatternIr {
    pub fn new(weight: Rational, node: PatternNodeIr) -> Self {
        assert!(
            weight > Rational::zero(),
            "weighted pattern weight must be positive"
        );
        Self {
            weight,
            node: Box::new(node),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeduplicateKeyIr {
    Lifecycle,
    WholeSpanAndValue,
    StartAndValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeduplicateWinnerIr {
    First,
    Last,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeduplicatePolicyIr {
    pub key: DeduplicateKeyIr,
    pub winner: DeduplicateWinnerIr,
}

impl DeduplicatePolicyIr {
    pub fn whole_span_and_value() -> Self {
        Self {
            key: DeduplicateKeyIr::WholeSpanAndValue,
            winner: DeduplicateWinnerIr::First,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriorityConflictIr {
    SameWholeStartAndValue,
    SameWholeSpanAndValue,
    WholeSpanOverlap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorityMergePolicyIr {
    pub conflict: PriorityConflictIr,
}

impl PriorityMergePolicyIr {
    pub fn whole_span_overlap() -> Self {
        Self {
            conflict: PriorityConflictIr::WholeSpanOverlap,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatternNodeIr {
    EventStream(EventStreamNodeIr),
    ControlStream(ControlStreamNodeIr),
    ScalarStream(ScalarStreamNodeIr),
    Merge {
        children: Vec<PatternNodeIr>,
    },
    CycleRoute {
        children: Vec<PatternNodeIr>,
    },
    CycleSlots {
        children: Vec<PatternNodeIr>,
    },
    TimeScale {
        inner: Box<PatternNodeIr>,
        factor: Rational,
    },
    Shift {
        inner: Box<PatternNodeIr>,
        offset: CycleDuration,
    },
    ReflectCycle {
        inner: Box<PatternNodeIr>,
    },
    Degrade {
        inner: Box<PatternNodeIr>,
        keep_probability: Rational,
        seed: u64,
    },
    Deduplicate {
        inner: Box<PatternNodeIr>,
        policy: DeduplicatePolicyIr,
    },
    PriorityMerge {
        children: Vec<PatternNodeIr>,
        policy: PriorityMergePolicyIr,
    },
    WeightedChoice {
        options: Vec<WeightedPatternIr>,
        seed: u64,
    },
    MaskClip {
        source: Box<PatternNodeIr>,
        mask: Box<PatternNodeIr>,
    },
}

impl PatternNodeIr {
    pub fn event_stream(stream: PatternStream) -> Self {
        Self::EventStream(EventStreamNodeIr { stream })
    }

    pub fn control_stream(stream: ControlStream) -> Self {
        Self::ControlStream(ControlStreamNodeIr { stream })
    }

    pub fn scalar_stream(stream: ScalarStream) -> Self {
        Self::ScalarStream(ScalarStreamNodeIr { stream })
    }

    pub fn merge(children: Vec<PatternNodeIr>) -> Self {
        Self::Merge { children }
    }

    pub fn cycle_route(children: Vec<PatternNodeIr>) -> Self {
        Self::CycleRoute { children }
    }

    pub fn cycle_slots(children: Vec<PatternNodeIr>) -> Self {
        Self::CycleSlots { children }
    }

    pub fn time_scale(inner: PatternNodeIr, factor: Rational) -> Self {
        Self::TimeScale {
            inner: Box::new(inner),
            factor,
        }
    }

    pub fn shift(inner: PatternNodeIr, offset: CycleDuration) -> Self {
        Self::Shift {
            inner: Box::new(inner),
            offset,
        }
    }

    pub fn reflect_cycle(inner: PatternNodeIr) -> Self {
        Self::ReflectCycle {
            inner: Box::new(inner),
        }
    }

    pub fn degrade(inner: PatternNodeIr, keep_probability: Rational, seed: u64) -> Self {
        Self::Degrade {
            inner: Box::new(inner),
            keep_probability,
            seed,
        }
    }

    pub fn deduplicate(inner: PatternNodeIr, policy: DeduplicatePolicyIr) -> Self {
        Self::Deduplicate {
            inner: Box::new(inner),
            policy,
        }
    }

    pub fn priority_merge(children: Vec<PatternNodeIr>, policy: PriorityMergePolicyIr) -> Self {
        Self::PriorityMerge { children, policy }
    }

    pub fn weighted_choice(options: Vec<WeightedPatternIr>, seed: u64) -> Self {
        Self::WeightedChoice { options, seed }
    }

    pub fn mask_clip(source: PatternNodeIr, mask: PatternNodeIr) -> Self {
        Self::MaskClip {
            source: Box::new(source),
            mask: Box::new(mask),
        }
    }

    pub fn shape(&self) -> PatternStreamShape {
        match self {
            Self::EventStream(_) => PatternStreamShape::Event,
            Self::ControlStream(_) => PatternStreamShape::Control,
            Self::ScalarStream(_) => PatternStreamShape::Scalar,
            Self::Merge { .. }
            | Self::CycleRoute { .. }
            | Self::CycleSlots { .. }
            | Self::TimeScale { .. }
            | Self::Shift { .. }
            | Self::ReflectCycle { .. }
            | Self::Degrade { .. }
            | Self::Deduplicate { .. }
            | Self::PriorityMerge { .. }
            | Self::WeightedChoice { .. }
            | Self::MaskClip { .. } => PatternStreamShape::Event,
        }
    }

    pub fn flatten(&self) -> PatternStream {
        match self {
            Self::EventStream(node) => node.stream.clone(),
            Self::ControlStream(node) => node.stream.to_pattern_stream(),
            Self::ScalarStream(node) => node.stream.to_pattern_stream(),
            Self::Merge { children } => {
                PatternStream::layer(children.iter().map(Self::flatten).collect())
            }
            Self::CycleRoute { children } => {
                PatternStream::layer(children.iter().map(Self::flatten).collect())
            }
            Self::CycleSlots { children } => {
                PatternStream::layer(children.iter().map(Self::flatten).collect())
            }
            Self::TimeScale { inner, factor } => {
                if *factor <= Rational::zero() {
                    inner.flatten()
                } else {
                    inner.flatten().slow(*factor)
                }
            }
            Self::Shift { inner, offset } => inner.flatten().shift_by(*offset),
            Self::ReflectCycle { inner } => inner.flatten().reverse(),
            Self::Degrade { inner, .. } => inner.flatten(),
            Self::Deduplicate { inner, .. } => inner.flatten(),
            Self::PriorityMerge { children, .. } => {
                PatternStream::layer(children.iter().map(Self::flatten).collect())
            }
            Self::WeightedChoice { options, .. } => {
                PatternStream::layer(options.iter().map(|option| option.node.flatten()).collect())
            }
            Self::MaskClip { source, .. } => source.flatten(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventStreamNodeIr {
    pub stream: PatternStream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlStreamNodeIr {
    pub stream: ControlStream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScalarStreamNodeIr {
    pub stream: ScalarStream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternOutput {
    pub id: NodeId,
    pub root: PatternNodeIr,
}

impl PatternOutput {
    pub fn new(id: NodeId, root: PatternNodeIr) -> Self {
        Self { id, root }
    }

    pub fn shape(&self) -> PatternStreamShape {
        self.root.shape()
    }

    pub fn events(&self) -> Vec<PatternEvent> {
        self.root.flatten().events
    }

    pub fn stream(&self) -> PatternStream {
        self.root.flatten()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PatternIr {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<PatternOutput>,
}

impl PatternIr {
    pub fn new(outputs: Vec<PatternOutput>) -> Self {
        Self { outputs }
    }

    pub fn flat_outputs(&self) -> Vec<FlatPatternOutput> {
        self.outputs
            .iter()
            .map(|output| FlatPatternOutput {
                id: output.id.clone(),
                events: output.events(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatPatternOutput {
    pub id: NodeId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<PatternEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: i64, duration: i64) -> CycleSpan {
        CycleSpan {
            start: CycleTime(Rational::from_integer(start)),
            duration: CycleDuration(Rational::from_integer(duration)),
        }
    }

    fn note(start: i64, duration: i64, value: &str) -> PatternEvent {
        PatternEvent::new(
            span(start, duration),
            EventValue::Note {
                value: value.into(),
                octave: None,
            },
        )
    }

    #[test]
    fn pattern_output_exposes_flat_events_from_structured_root() {
        let left = PatternNodeIr::event_stream(PatternStream::new(vec![note(0, 1, "a")]));
        let right = PatternNodeIr::event_stream(PatternStream::new(vec![note(1, 1, "b")]));
        let output =
            PatternOutput::new(NodeId::new("out"), PatternNodeIr::merge(vec![left, right]));
        let events = output.events();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn time_scale_node_flattens_by_scaling_stream_time() {
        let node = PatternNodeIr::time_scale(
            PatternNodeIr::event_stream(PatternStream::new(vec![note(0, 1, "a")])),
            Rational::from_integer(2),
        );
        let stream = node.flatten();
        assert_eq!(
            stream.events[0].span.duration,
            CycleDuration(Rational::from_integer(2))
        );
    }

    #[test]
    fn control_stream_has_control_shape_and_flat_scalar_view() {
        let control = ControlEvent::new(
            span(0, 1),
            ControlKeyIr::Gain,
            ControlValueIr::rational(Rational::new(1, 2)),
        );
        let node = PatternNodeIr::control_stream(ControlStream::new(vec![control]));
        assert_eq!(node.shape(), PatternStreamShape::Control);
        assert_eq!(node.flatten().events.len(), 1);
    }

    #[test]
    fn structural_degrade_flattens_to_inner_for_preview_only() {
        let node = PatternNodeIr::degrade(
            PatternNodeIr::event_stream(PatternStream::new(vec![note(0, 1, "a")])),
            Rational::new(1, 2),
            42,
        );
        assert_eq!(node.flatten().events.len(), 1);
    }
}
