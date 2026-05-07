use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Sub};

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldValue {
    Rational { value: Rational },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventField {
    Gain(FieldValue),
    Attack(FieldValue),
    Transpose(FieldValue),
    Elongate(FieldValue),
    Replicate(FieldValue),
    Degrade(FieldValue),
    RandomChoice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternEvent {
    pub span: CycleSpan,
    pub value: EventValue,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<EventField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PatternStream {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<PatternEvent>,
}

impl PatternStream {
    pub fn bounds(&self) -> Option<(CycleTime, CycleTime)> {
        let first = self.events.first()?;
        let mut min_start = first.span.start.0;
        let mut max_end = first.span.start.0 + first.span.duration.0;
        for event in &self.events {
            if event.span.start.0 < min_start {
                min_start = event.span.start.0;
            }
            let end = event.span.start.0 + event.span.duration.0;
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
                .map(|mut event| {
                    event.span.start = CycleTime(event.span.start.0 + offset.0);
                    event
                })
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
                .map(|mut event| {
                    let event_end = event.span.start.0 + event.span.duration.0;
                    event.span.start = CycleTime(end.0 - (event_end - origin.0));
                    event
                })
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
                .map(|mut event| {
                    let relative = event.span.start.0 - origin.0;
                    event.span.start = CycleTime(origin.0 + relative * factor);
                    event.span.duration = CycleDuration(event.span.duration.0 * factor);
                    event
                })
                .collect(),
        }
    }

    pub fn normalize_to_origin(self) -> Self {
        let Some((origin, _)) = self.bounds() else {
            return self;
        };
        self.shift_by(CycleDuration(Rational::zero() - origin.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternOutput {
    pub id: super::program::NodeId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<PatternEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PatternIr {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<PatternOutput>,
}
