//! Live runtime feedback types for activity pulses, probes, and animation.
//!
//! This module defines the vocabulary for mapping runtime events back to graph
//! positions. Activity data is ephemeral — it describes what is currently
//! happening at runtime, not persisted state.
//!
//! Architecture rule: the core library defines types and traits. Hosts
//! implement the tracking and push events through [`HostAdapter`](crate::host::HostAdapter)
//! methods. [`ActivitySnapshot`] is owned by the host/UI layer, not by
//! [`GraphEngine`](crate::host::GraphEngine).
//!
//! ## Invariant enforcement
//!
//! Normalised fields (`phase`, `fraction`, `intensity`, `progress`, `start`,
//! `duration`, `subdivisions`, `elapsed`) are clamped to their documented
//! ranges at every construction and deserialisation boundary. Direct enum
//! variant construction (e.g. `ActivityKind::Trigger { intensity: 5.0 }`)
//! bypasses this — prefer the provided constructors and builder methods.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::GridPos;

// ---------------------------------------------------------------------------
// Clamping helpers
// ---------------------------------------------------------------------------

/// Clamp a fractional value to `[0.0, 1.0)`.
fn clamp_phase(v: f64) -> f64 {
    v.clamp(0.0, 1.0 - f64::EPSILON)
}

/// Clamp a normalised value to `[0.0, 1.0]`.
fn clamp_unit(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Host time
// ---------------------------------------------------------------------------

/// A host-specific point in time.
///
/// Different hosts measure time differently — cycle-based (Strudel), wall
/// clock, MIDI ticks, etc. `HostTime` abstracts over these without forcing
/// a single model on every host.
///
/// Constructors and deserialisation clamp values to the documented ranges.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", from = "RawHostTime")]
pub enum HostTime {
    /// Fractional position within a repeating cycle.
    Cycle {
        /// Position within the current cycle: `[0.0, 1.0)`.
        phase: f64,
        /// Monotonically increasing cycle number.
        cycle: u64,
    },
    /// Wall-clock seconds since an arbitrary epoch.
    Seconds {
        /// Elapsed seconds (non-negative).
        elapsed: f64,
    },
    /// Discrete tick with optional fractional sub-tick.
    Ticks {
        /// Whole tick number.
        tick: u64,
        /// Fractional position within the tick: `[0.0, 1.0)`.
        fraction: f64,
    },
}

/// Serde shadow type for [`HostTime`] — values are normalised via [`From`].
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RawHostTime {
    Cycle { phase: f64, cycle: u64 },
    Seconds { elapsed: f64 },
    Ticks { tick: u64, fraction: f64 },
}

impl From<RawHostTime> for HostTime {
    fn from(raw: RawHostTime) -> Self {
        match raw {
            RawHostTime::Cycle { phase, cycle } => Self::cycle(phase, cycle),
            RawHostTime::Seconds { elapsed } => Self::seconds(elapsed),
            RawHostTime::Ticks { tick, fraction } => Self::ticks(tick, fraction),
        }
    }
}

impl HostTime {
    /// Create a cycle-based time point. `phase` is clamped to `[0.0, 1.0)`.
    pub fn cycle(phase: f64, cycle: u64) -> Self {
        Self::Cycle {
            phase: clamp_phase(phase),
            cycle,
        }
    }

    /// Create a wall-clock time point. `elapsed` is clamped to `>= 0.0`.
    pub fn seconds(elapsed: f64) -> Self {
        Self::Seconds {
            elapsed: elapsed.max(0.0),
        }
    }

    /// Create a tick-based time point. `fraction` is clamped to `[0.0, 1.0)`.
    pub fn ticks(tick: u64, fraction: f64) -> Self {
        Self::Ticks {
            tick,
            fraction: clamp_phase(fraction),
        }
    }
}

// ---------------------------------------------------------------------------
// Activity event kinds
// ---------------------------------------------------------------------------

/// What kind of activity is happening at a graph node.
///
/// Constructors and deserialisation clamp normalised fields to their
/// documented ranges. Direct enum variant construction bypasses this —
/// prefer [`ActivityKind::trigger`] and [`ActivityKind::sustain`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", from = "RawActivityKind")]
pub enum ActivityKind {
    /// A runtime event triggered at this node (e.g. a note onset, a sample
    /// trigger, a value change).
    Trigger {
        /// Optional label for what was triggered (e.g. `"bd"`, `"sd"`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        /// Normalised intensity `[0.0, 1.0]` for UI brightness/size scaling.
        #[serde(default = "default_intensity")]
        intensity: f64,
    },
    /// The node is actively sustaining (e.g. a long note, an active filter).
    Sustain {
        /// How far through the sustain we are: `[0.0, 1.0]`.
        progress: f64,
    },
    /// A compilation or processing event.
    Processing {
        /// Human-readable status label.
        label: String,
    },
    /// An error occurred at runtime at this node.
    RuntimeError {
        /// Error description.
        message: String,
    },
}

fn default_intensity() -> f64 {
    1.0
}

/// Serde shadow type for [`ActivityKind`] — values are normalised via [`From`].
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RawActivityKind {
    Trigger {
        #[serde(default)]
        label: Option<String>,
        #[serde(default = "default_intensity")]
        intensity: f64,
    },
    Sustain {
        progress: f64,
    },
    Processing {
        label: String,
    },
    RuntimeError {
        message: String,
    },
}

impl From<RawActivityKind> for ActivityKind {
    fn from(raw: RawActivityKind) -> Self {
        match raw {
            RawActivityKind::Trigger { label, intensity } => Self::trigger(label, intensity),
            RawActivityKind::Sustain { progress } => Self::sustain(progress),
            RawActivityKind::Processing { label } => Self::Processing { label },
            RawActivityKind::RuntimeError { message } => Self::RuntimeError { message },
        }
    }
}

impl ActivityKind {
    /// Create a trigger event. `intensity` is clamped to `[0.0, 1.0]`.
    pub fn trigger(label: Option<String>, intensity: f64) -> Self {
        Self::Trigger {
            label,
            intensity: clamp_unit(intensity),
        }
    }

    /// Create a sustain event. `progress` is clamped to `[0.0, 1.0]`.
    pub fn sustain(progress: f64) -> Self {
        Self::Sustain {
            progress: clamp_unit(progress),
        }
    }
}

// ---------------------------------------------------------------------------
// Activity event
// ---------------------------------------------------------------------------

/// A single activity event at a specific graph position.
///
/// Follows the [`Diagnostic`](crate::diagnostics::Diagnostic) builder pattern
/// with [`Origin`](crate::ast::Origin)-based location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityEvent {
    /// Which node this event relates to.
    pub site: GridPos,
    /// Which parameter on the node, if the event is parameter-specific.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// What kind of activity.
    pub kind: ActivityKind,
    /// When this event occurs, in host-specific time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<HostTime>,
}

impl ActivityEvent {
    /// Create a trigger event at a grid position.
    pub fn trigger(site: GridPos) -> Self {
        Self {
            site,
            param: None,
            kind: ActivityKind::trigger(None, 1.0),
            at: None,
        }
    }

    /// Create a processing/compilation event.
    pub fn processing(site: GridPos, label: impl Into<String>) -> Self {
        Self {
            site,
            param: None,
            kind: ActivityKind::Processing {
                label: label.into(),
            },
            at: None,
        }
    }

    /// Create a runtime error event.
    pub fn runtime_error(site: GridPos, message: impl Into<String>) -> Self {
        Self {
            site,
            param: None,
            kind: ActivityKind::RuntimeError {
                message: message.into(),
            },
            at: None,
        }
    }

    /// Attach a parameter id.
    pub fn with_param(mut self, param: impl Into<String>) -> Self {
        self.param = Some(param.into());
        self
    }

    /// Set the trigger label (only affects `ActivityKind::Trigger`).
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        if let ActivityKind::Trigger {
            label: ref mut l, ..
        } = self.kind
        {
            *l = Some(label.into());
        }
        self
    }

    /// Set the trigger intensity, clamped to `[0.0, 1.0]`
    /// (only affects `ActivityKind::Trigger`).
    pub fn with_intensity(mut self, intensity: f64) -> Self {
        if let ActivityKind::Trigger {
            intensity: ref mut i,
            ..
        } = self.kind
        {
            *i = clamp_unit(intensity);
        }
        self
    }

    /// Set the time of this event.
    pub fn at(mut self, time: HostTime) -> Self {
        self.at = Some(time);
        self
    }
}

// ---------------------------------------------------------------------------
// Probe event
// ---------------------------------------------------------------------------

/// A single runtime value probe at a specific graph position.
///
/// Probes are node-scoped in v1 and carry an opaque JSON payload so hosts can
/// surface current values in the UI without the core engine imposing any
/// formatting or typing policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeEvent {
    /// Which node this probe relates to.
    pub site: GridPos,
    /// The current probed value for this node.
    pub value: Value,
}

impl ProbeEvent {
    /// Create a probe event at a grid position.
    pub fn new(site: GridPos, value: impl Into<Value>) -> Self {
        Self {
            site,
            value: value.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Static timeline hints (compile-time)
// ---------------------------------------------------------------------------

/// A single step in a piece's declared animation timeline.
///
/// Pieces that know their temporal structure at compile time can declare a
/// preview timeline so the UI can animate nodes without a running runtime.
/// All positions are normalised over one abstract preview window `[0.0, 1.0)`.
///
/// Constructors and deserialisation clamp `start` to `[0.0, 1.0)` and
/// `duration` to `[ε, 1.0]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawTimelineStep")]
pub struct TimelineStep {
    /// Where in the preview window this step starts: `[0.0, 1.0)`.
    pub start: f64,
    /// Duration as a fraction of the window: `(0.0, 1.0]`.
    pub duration: f64,
    /// Optional display label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Which parameter this step relates to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
}

/// Serde shadow type for [`TimelineStep`] — values are normalised via [`From`].
#[derive(Deserialize)]
struct RawTimelineStep {
    start: f64,
    duration: f64,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    param: Option<String>,
}

impl From<RawTimelineStep> for TimelineStep {
    fn from(raw: RawTimelineStep) -> Self {
        TimelineStep::new(raw.start, raw.duration)
            .with_label_opt(raw.label)
            .with_param_opt(raw.param)
    }
}

/// Smallest positive duration for timeline steps.
const MIN_DURATION: f64 = f64::EPSILON;

impl TimelineStep {
    /// Create a step. `start` is clamped to `[0.0, 1.0)`, `duration` to
    /// `[ε, 1.0]`.
    pub fn new(start: f64, duration: f64) -> Self {
        Self {
            start: clamp_phase(start),
            duration: duration.clamp(MIN_DURATION, 1.0),
            label: None,
            param: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_param(mut self, param: impl Into<String>) -> Self {
        self.param = Some(param.into());
        self
    }

    /// Set label from an `Option`, leaving it `None` when the option is `None`.
    fn with_label_opt(mut self, label: Option<String>) -> Self {
        self.label = label;
        self
    }

    /// Set param from an `Option`, leaving it `None` when the option is `None`.
    fn with_param_opt(mut self, param: Option<String>) -> Self {
        self.param = param;
        self
    }
}

/// A complete preview timeline for a piece, usable without a running runtime.
///
/// This is returned by [`Piece::preview_timeline`](crate::piece::Piece::preview_timeline)
/// to let the UI animate nodes based purely on compile-time information. All
/// step positions are normalised over one abstract preview window `[0.0, 1.0)`,
/// independent of host runtime time.
///
/// `subdivisions` is clamped to `>= 1` at construction and deserialisation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawPreviewTimeline")]
pub struct PreviewTimeline {
    /// Ordered list of steps within one preview window.
    pub steps: Vec<TimelineStep>,
    /// How many sub-divisions per window (for nested patterns).
    /// Minimum: 1 (no nesting).
    #[serde(default = "default_subdivisions")]
    pub subdivisions: u32,
}

fn default_subdivisions() -> u32 {
    1
}

/// Serde shadow type for [`PreviewTimeline`] — values are normalised via [`From`].
#[derive(Deserialize)]
struct RawPreviewTimeline {
    steps: Vec<TimelineStep>,
    #[serde(default = "default_subdivisions")]
    subdivisions: u32,
}

impl From<RawPreviewTimeline> for PreviewTimeline {
    fn from(raw: RawPreviewTimeline) -> Self {
        PreviewTimeline {
            steps: raw.steps,
            subdivisions: raw.subdivisions.max(1),
        }
    }
}

impl PreviewTimeline {
    pub fn new(steps: Vec<TimelineStep>) -> Self {
        Self {
            steps,
            subdivisions: 1,
        }
    }

    /// Set subdivisions, clamped to `>= 1`.
    pub fn with_subdivisions(mut self, subdivisions: u32) -> Self {
        self.subdivisions = subdivisions.max(1);
        self
    }

    /// Create a uniform timeline splitting the window evenly among labels.
    ///
    /// This is the common case for evenly-spaced sequential events.
    pub fn uniform(labels: &[&str]) -> Self {
        let n = labels.len();
        if n == 0 {
            return Self::new(vec![]);
        }
        let duration = 1.0 / n as f64;
        let steps = labels
            .iter()
            .enumerate()
            .map(|(i, label)| TimelineStep::new(i as f64 * duration, duration).with_label(*label))
            .collect();
        Self::new(steps)
    }
}

// ---------------------------------------------------------------------------
// Activity snapshot
// ---------------------------------------------------------------------------

/// A snapshot of all active events across the graph.
///
/// Owned by the host/UI layer, not by [`GraphEngine`](crate::host::GraphEngine).
/// This is a value type that gets replaced wholesale each time the host pushes
/// a new frame of activity data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivitySnapshot {
    /// Current playhead position in host-specific time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<HostTime>,
    /// Active events indexed by grid position for O(1) lookup.
    ///
    /// Serialised as a list of `{ "pos": GridPos, "events": [...] }` objects
    /// because JSON keys must be strings.
    #[serde(default, with = "grid_pos_events_map")]
    pub events: BTreeMap<GridPos, Vec<ActivityEvent>>,
}

/// Serde helper: serialise `BTreeMap<GridPos, Vec<ActivityEvent>>` as a JSON
/// array of `{ pos, events }` objects (JSON keys must be strings).
mod grid_pos_events_map {
    use super::*;
    use serde::ser::SerializeSeq;

    #[derive(Serialize, Deserialize)]
    struct Entry {
        pos: GridPos,
        events: Vec<ActivityEvent>,
    }

    pub fn serialize<S: serde::Serializer>(
        map: &BTreeMap<GridPos, Vec<ActivityEvent>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(map.len()))?;
        for (pos, events) in map {
            seq.serialize_element(&Entry {
                pos: *pos,
                events: events.clone(),
            })?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<GridPos, Vec<ActivityEvent>>, D::Error> {
        let entries: Vec<Entry> = Vec::deserialize(deserializer)?;
        Ok(entries.into_iter().map(|e| (e.pos, e.events)).collect())
    }
}

impl ActivitySnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all events at a grid position.
    pub fn events_at(&self, pos: &GridPos) -> &[ActivityEvent] {
        self.events.get(pos).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Check if any node has activity.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Return all positions that have at least one event.
    pub fn active_positions(&self) -> Vec<GridPos> {
        self.events.keys().copied().collect()
    }
}

// ---------------------------------------------------------------------------
// Probe snapshot
// ---------------------------------------------------------------------------

/// A snapshot of all current probed values across the graph.
///
/// Owned by the host/UI layer, not by [`GraphEngine`](crate::host::GraphEngine).
/// Unlike [`ActivitySnapshot`], this stores at most one current value per node
/// for the frame.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProbeSnapshot {
    /// Current playhead position in host-specific time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<HostTime>,
    /// Current probed values indexed by grid position for O(1) lookup.
    ///
    /// Serialised as a list of `{ "pos": GridPos, "value": ... }` objects
    /// because JSON keys must be strings.
    #[serde(default, with = "grid_pos_values_map")]
    pub values: BTreeMap<GridPos, Value>,
}

/// Serde helper: serialise `BTreeMap<GridPos, Value>` as a JSON array of
/// `{ pos, value }` objects (JSON keys must be strings).
mod grid_pos_values_map {
    use super::*;
    use serde::ser::SerializeSeq;

    #[derive(Serialize, Deserialize)]
    struct Entry {
        pos: GridPos,
        value: Value,
    }

    pub fn serialize<S: serde::Serializer>(
        map: &BTreeMap<GridPos, Value>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(map.len()))?;
        for (pos, value) in map {
            seq.serialize_element(&Entry {
                pos: *pos,
                value: value.clone(),
            })?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<GridPos, Value>, D::Error> {
        let entries: Vec<Entry> = Vec::deserialize(deserializer)?;
        Ok(entries
            .into_iter()
            .map(|entry| (entry.pos, entry.value))
            .collect())
    }
}

impl ProbeSnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current probed value at a grid position.
    pub fn value_at(&self, pos: &GridPos) -> Option<&Value> {
        self.values.get(pos)
    }

    /// Check if any node currently has a probed value.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Return all positions that currently have a probed value.
    pub fn probed_positions(&self) -> Vec<GridPos> {
        self.values.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- HostTime --

    #[test]
    fn host_time_cycle_constructor() {
        let t = HostTime::cycle(0.25, 3);
        assert_eq!(
            t,
            HostTime::Cycle {
                phase: 0.25,
                cycle: 3
            }
        );
    }

    #[test]
    fn host_time_seconds_constructor() {
        let t = HostTime::seconds(1.5);
        assert_eq!(t, HostTime::Seconds { elapsed: 1.5 });
    }

    #[test]
    fn host_time_ticks_constructor() {
        let t = HostTime::ticks(4, 0.75);
        assert_eq!(
            t,
            HostTime::Ticks {
                tick: 4,
                fraction: 0.75
            }
        );
    }

    #[test]
    fn host_time_cycle_clamps_phase() {
        let over = HostTime::cycle(1.5, 0);
        if let HostTime::Cycle { phase, .. } = over {
            assert!(phase < 1.0, "phase {phase} should be < 1.0");
            assert!(phase >= 0.0);
        }

        let neg = HostTime::cycle(-0.3, 0);
        if let HostTime::Cycle { phase, .. } = neg {
            assert_eq!(phase, 0.0);
        }
    }

    #[test]
    fn host_time_seconds_clamps_negative() {
        let neg = HostTime::seconds(-5.0);
        assert_eq!(neg, HostTime::Seconds { elapsed: 0.0 });
    }

    #[test]
    fn host_time_ticks_clamps_fraction() {
        let over = HostTime::ticks(1, 2.0);
        if let HostTime::Ticks { fraction, .. } = over {
            assert!(fraction < 1.0, "fraction {fraction} should be < 1.0");
        }

        let neg = HostTime::ticks(1, -0.1);
        if let HostTime::Ticks { fraction, .. } = neg {
            assert_eq!(fraction, 0.0);
        }
    }

    #[test]
    fn host_time_serde_cycle_round_trip() {
        let t = HostTime::cycle(0.5, 7);
        let json = serde_json::to_string(&t).unwrap();
        let back: HostTime = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
        assert!(json.contains("\"kind\":\"cycle\""));
    }

    #[test]
    fn host_time_serde_seconds_round_trip() {
        let t = HostTime::seconds(42.0);
        let json = serde_json::to_string(&t).unwrap();
        let back: HostTime = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
        assert!(json.contains("\"kind\":\"seconds\""));
    }

    #[test]
    fn host_time_serde_ticks_round_trip() {
        let t = HostTime::ticks(10, 0.5);
        let json = serde_json::to_string(&t).unwrap();
        let back: HostTime = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
        assert!(json.contains("\"kind\":\"ticks\""));
    }

    #[test]
    fn host_time_serde_clamps_on_deserialise() {
        // phase out of range
        let json = r#"{"kind":"cycle","phase":2.5,"cycle":0}"#;
        let t: HostTime = serde_json::from_str(json).unwrap();
        if let HostTime::Cycle { phase, .. } = t {
            assert!(phase < 1.0, "deserialised phase {phase} should be < 1.0");
        } else {
            panic!("expected Cycle");
        }

        // negative elapsed
        let json = r#"{"kind":"seconds","elapsed":-10.0}"#;
        let t: HostTime = serde_json::from_str(json).unwrap();
        assert_eq!(t, HostTime::Seconds { elapsed: 0.0 });

        // fraction out of range
        let json = r#"{"kind":"ticks","tick":0,"fraction":1.0}"#;
        let t: HostTime = serde_json::from_str(json).unwrap();
        if let HostTime::Ticks { fraction, .. } = t {
            assert!(
                fraction < 1.0,
                "deserialised fraction {fraction} should be < 1.0"
            );
        } else {
            panic!("expected Ticks");
        }
    }

    // -- ActivityKind --

    #[test]
    fn activity_kind_trigger_constructor_clamps() {
        let kind = ActivityKind::trigger(Some("bd".into()), 5.0);
        if let ActivityKind::Trigger { intensity, .. } = kind {
            assert_eq!(intensity, 1.0);
        } else {
            panic!("expected Trigger");
        }

        let kind = ActivityKind::trigger(None, -2.0);
        if let ActivityKind::Trigger { intensity, .. } = kind {
            assert_eq!(intensity, 0.0);
        } else {
            panic!("expected Trigger");
        }
    }

    #[test]
    fn activity_kind_sustain_constructor_clamps() {
        let kind = ActivityKind::sustain(1.5);
        if let ActivityKind::Sustain { progress } = kind {
            assert_eq!(progress, 1.0);
        } else {
            panic!("expected Sustain");
        }

        let kind = ActivityKind::sustain(-0.5);
        if let ActivityKind::Sustain { progress } = kind {
            assert_eq!(progress, 0.0);
        } else {
            panic!("expected Sustain");
        }
    }

    #[test]
    fn activity_kind_trigger_serde_round_trip() {
        let kind = ActivityKind::trigger(Some("bd".into()), 0.8);
        let json = serde_json::to_string(&kind).unwrap();
        let back: ActivityKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, back);
        assert!(json.contains("\"kind\":\"trigger\""));
    }

    #[test]
    fn activity_kind_trigger_intensity_defaults_to_one() {
        let json = r#"{"kind":"trigger"}"#;
        let kind: ActivityKind = serde_json::from_str(json).unwrap();
        assert_eq!(
            kind,
            ActivityKind::Trigger {
                label: None,
                intensity: 1.0
            }
        );
    }

    #[test]
    fn activity_kind_serde_clamps_on_deserialise() {
        // Trigger with out-of-range intensity
        let json = r#"{"kind":"trigger","intensity":99.0}"#;
        let kind: ActivityKind = serde_json::from_str(json).unwrap();
        if let ActivityKind::Trigger { intensity, .. } = kind {
            assert_eq!(intensity, 1.0);
        } else {
            panic!("expected Trigger");
        }

        // Sustain with negative progress
        let json = r#"{"kind":"sustain","progress":-3.0}"#;
        let kind: ActivityKind = serde_json::from_str(json).unwrap();
        if let ActivityKind::Sustain { progress } = kind {
            assert_eq!(progress, 0.0);
        } else {
            panic!("expected Sustain");
        }
    }

    #[test]
    fn activity_kind_sustain_serde_round_trip() {
        let kind = ActivityKind::sustain(0.5);
        let json = serde_json::to_string(&kind).unwrap();
        let back: ActivityKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, back);
    }

    #[test]
    fn activity_kind_processing_serde_round_trip() {
        let kind = ActivityKind::Processing {
            label: "compiled".into(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        let back: ActivityKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, back);
    }

    #[test]
    fn activity_kind_runtime_error_serde_round_trip() {
        let kind = ActivityKind::RuntimeError {
            message: "division by zero".into(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        let back: ActivityKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, back);
    }

    // -- ActivityEvent --

    #[test]
    fn activity_event_trigger_builder() {
        let pos = GridPos { col: 1, row: 2 };
        let event = ActivityEvent::trigger(pos)
            .with_label("sd")
            .with_intensity(0.7)
            .with_param("value")
            .at(HostTime::cycle(0.25, 5));

        assert_eq!(event.site, pos);
        assert_eq!(event.param, Some("value".into()));
        assert_eq!(
            event.kind,
            ActivityKind::Trigger {
                label: Some("sd".into()),
                intensity: 0.7,
            }
        );
        assert_eq!(
            event.at,
            Some(HostTime::Cycle {
                phase: 0.25,
                cycle: 5
            })
        );
    }

    #[test]
    fn activity_event_processing_factory() {
        let pos = GridPos { col: 0, row: 0 };
        let event = ActivityEvent::processing(pos, "evaluating");
        assert_eq!(
            event.kind,
            ActivityKind::Processing {
                label: "evaluating".into()
            }
        );
    }

    #[test]
    fn activity_event_runtime_error_factory() {
        let pos = GridPos { col: 3, row: 1 };
        let event = ActivityEvent::runtime_error(pos, "oops");
        assert_eq!(
            event.kind,
            ActivityKind::RuntimeError {
                message: "oops".into()
            }
        );
    }

    #[test]
    fn activity_event_intensity_clamped() {
        let event = ActivityEvent::trigger(GridPos { col: 0, row: 0 }).with_intensity(2.0);
        if let ActivityKind::Trigger { intensity, .. } = event.kind {
            assert_eq!(intensity, 1.0);
        } else {
            panic!("expected Trigger");
        }

        let event = ActivityEvent::trigger(GridPos { col: 0, row: 0 }).with_intensity(-0.5);
        if let ActivityKind::Trigger { intensity, .. } = event.kind {
            assert_eq!(intensity, 0.0);
        } else {
            panic!("expected Trigger");
        }
    }

    #[test]
    fn activity_event_serde_round_trip() {
        let event = ActivityEvent::trigger(GridPos { col: 1, row: 0 })
            .with_label("hh")
            .at(HostTime::seconds(1.0));
        let json = serde_json::to_string(&event).unwrap();
        let back: ActivityEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn activity_event_serde_omits_none_fields() {
        let event = ActivityEvent::trigger(GridPos { col: 0, row: 0 });
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("\"param\""));
        assert!(!json.contains("\"at\""));
    }

    // -- ProbeEvent --

    #[test]
    fn probe_event_constructor() {
        let pos = GridPos { col: 2, row: 1 };
        let event = ProbeEvent::new(pos, json!({"level": 0.75}));
        assert_eq!(event.site, pos);
        assert_eq!(event.value, json!({"level": 0.75}));
    }

    #[test]
    fn probe_event_serde_round_trip() {
        let event = ProbeEvent::new(GridPos { col: 1, row: 0 }, json!(["bd", 0.5]));
        let json = serde_json::to_string(&event).unwrap();
        let back: ProbeEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    // -- TimelineStep --

    #[test]
    fn timeline_step_builder() {
        let step = TimelineStep::new(0.0, 0.5)
            .with_label("bd")
            .with_param("pattern");
        assert_eq!(step.start, 0.0);
        assert_eq!(step.duration, 0.5);
        assert_eq!(step.label.as_deref(), Some("bd"));
        assert_eq!(step.param.as_deref(), Some("pattern"));
    }

    #[test]
    fn timeline_step_clamps_start_and_duration() {
        let step = TimelineStep::new(-0.5, -1.0);
        assert_eq!(step.start, 0.0);
        assert!(step.duration > 0.0, "duration must be positive");
        assert!(step.duration <= 1.0);

        let step = TimelineStep::new(2.0, 3.0);
        assert!(
            step.start < 1.0,
            "start {s} should be < 1.0",
            s = step.start
        );
        assert_eq!(step.duration, 1.0);

        let step = TimelineStep::new(0.5, 0.0);
        assert!(step.duration > 0.0, "zero duration should become positive");
    }

    #[test]
    fn timeline_step_serde_clamps_on_deserialise() {
        let json = r#"{"start":-1.0,"duration":5.0}"#;
        let step: TimelineStep = serde_json::from_str(json).unwrap();
        assert_eq!(step.start, 0.0);
        assert_eq!(step.duration, 1.0);
    }

    // -- PreviewTimeline --

    #[test]
    fn preview_timeline_uniform_three_labels() {
        let tl = PreviewTimeline::uniform(&["bd", "sd", "hh"]);
        assert_eq!(tl.steps.len(), 3);
        assert_eq!(tl.subdivisions, 1);

        let eps = 1e-10;
        assert!((tl.steps[0].start - 0.0).abs() < eps);
        assert!((tl.steps[1].start - 1.0 / 3.0).abs() < eps);
        assert!((tl.steps[2].start - 2.0 / 3.0).abs() < eps);

        for step in &tl.steps {
            assert!((step.duration - 1.0 / 3.0).abs() < eps);
        }

        assert_eq!(tl.steps[0].label.as_deref(), Some("bd"));
        assert_eq!(tl.steps[1].label.as_deref(), Some("sd"));
        assert_eq!(tl.steps[2].label.as_deref(), Some("hh"));
    }

    #[test]
    fn preview_timeline_uniform_empty() {
        let tl = PreviewTimeline::uniform(&[]);
        assert!(tl.steps.is_empty());
    }

    #[test]
    fn preview_timeline_with_subdivisions() {
        let tl = PreviewTimeline::uniform(&["a", "b"]).with_subdivisions(4);
        assert_eq!(tl.subdivisions, 4);
    }

    #[test]
    fn preview_timeline_subdivisions_clamped_to_one() {
        let tl = PreviewTimeline::new(vec![]).with_subdivisions(0);
        assert_eq!(tl.subdivisions, 1);
    }

    #[test]
    fn preview_timeline_serde_clamps_subdivisions() {
        let json = r#"{"steps":[],"subdivisions":0}"#;
        let tl: PreviewTimeline = serde_json::from_str(json).unwrap();
        assert_eq!(tl.subdivisions, 1);
    }

    #[test]
    fn preview_timeline_serde_round_trip() {
        let tl = PreviewTimeline::uniform(&["x", "y"]).with_subdivisions(2);
        let json = serde_json::to_string(&tl).unwrap();
        let back: PreviewTimeline = serde_json::from_str(&json).unwrap();
        assert_eq!(tl, back);
    }

    #[test]
    fn preview_timeline_serde_subdivisions_defaults() {
        let json = r#"{"steps":[]}"#;
        let tl: PreviewTimeline = serde_json::from_str(json).unwrap();
        assert_eq!(tl.subdivisions, 1);
    }

    // -- ActivitySnapshot --

    #[test]
    fn snapshot_events_at_returns_empty_for_missing() {
        let snap = ActivitySnapshot::new();
        let events = snap.events_at(&GridPos { col: 5, row: 5 });
        assert!(events.is_empty());
    }

    #[test]
    fn snapshot_events_at_returns_events() {
        let pos = GridPos { col: 0, row: 0 };
        let mut snap = ActivitySnapshot::new();
        snap.events
            .entry(pos)
            .or_default()
            .push(ActivityEvent::trigger(pos).with_label("bd"));

        let events = snap.events_at(&pos);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn snapshot_is_empty() {
        let snap = ActivitySnapshot::new();
        assert!(snap.is_empty());

        let mut snap2 = ActivitySnapshot::new();
        let pos = GridPos { col: 0, row: 0 };
        snap2
            .events
            .entry(pos)
            .or_default()
            .push(ActivityEvent::trigger(pos));
        assert!(!snap2.is_empty());
    }

    #[test]
    fn snapshot_active_positions() {
        let mut snap = ActivitySnapshot::new();
        let p1 = GridPos { col: 0, row: 0 };
        let p2 = GridPos { col: 2, row: 1 };
        snap.events
            .entry(p1)
            .or_default()
            .push(ActivityEvent::trigger(p1));
        snap.events
            .entry(p2)
            .or_default()
            .push(ActivityEvent::processing(p2, "ok"));
        let positions = snap.active_positions();
        assert_eq!(positions, vec![p1, p2]);
    }

    #[test]
    fn snapshot_serde_round_trip() {
        let mut snap = ActivitySnapshot::new();
        snap.position = Some(HostTime::cycle(0.5, 1));
        let pos = GridPos { col: 1, row: 0 };
        snap.events
            .entry(pos)
            .or_default()
            .push(ActivityEvent::trigger(pos).with_label("bd"));

        let json = serde_json::to_string(&snap).unwrap();
        let back: ActivitySnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.position, back.position);
        assert_eq!(snap.events.len(), back.events.len());
    }

    #[test]
    fn snapshot_serde_defaults() {
        let json = "{}";
        let snap: ActivitySnapshot = serde_json::from_str(json).unwrap();
        assert!(snap.position.is_none());
        assert!(snap.events.is_empty());
    }

    // -- ProbeSnapshot --

    #[test]
    fn probe_snapshot_value_at_returns_none_for_missing() {
        let snap = ProbeSnapshot::new();
        assert_eq!(snap.value_at(&GridPos { col: 5, row: 5 }), None);
    }

    #[test]
    fn probe_snapshot_value_at_returns_value() {
        let pos = GridPos { col: 0, row: 0 };
        let expected = json!(0.75);
        let mut snap = ProbeSnapshot::new();
        snap.values.insert(pos, expected.clone());

        assert_eq!(snap.value_at(&pos), Some(&expected));
    }

    #[test]
    fn probe_snapshot_is_empty() {
        let snap = ProbeSnapshot::new();
        assert!(snap.is_empty());

        let mut snap2 = ProbeSnapshot::new();
        snap2.values.insert(GridPos { col: 0, row: 0 }, json!("bd"));
        assert!(!snap2.is_empty());
    }

    #[test]
    fn probe_snapshot_probed_positions() {
        let mut snap = ProbeSnapshot::new();
        let p1 = GridPos { col: 0, row: 0 };
        let p2 = GridPos { col: 2, row: 1 };
        snap.values.insert(p1, json!(0.25));
        snap.values.insert(p2, json!("sd"));

        assert_eq!(snap.probed_positions(), vec![p1, p2]);
    }

    #[test]
    fn probe_snapshot_serde_round_trip() {
        let mut snap = ProbeSnapshot::new();
        snap.position = Some(HostTime::cycle(0.5, 1));
        let pos = GridPos { col: 1, row: 0 };
        snap.values.insert(pos, json!({"level": 0.75}));

        let json = serde_json::to_string(&snap).unwrap();
        let back: ProbeSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap.position, back.position);
        assert_eq!(snap.values, back.values);
    }

    #[test]
    fn probe_snapshot_serde_defaults() {
        let json = "{}";
        let snap: ProbeSnapshot = serde_json::from_str(json).unwrap();
        assert!(snap.position.is_none());
        assert!(snap.values.is_empty());
    }
}
