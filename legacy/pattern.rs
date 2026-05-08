use serde::{Deserialize, Serialize};

use crate::types::Rational;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PatternPos {
    pub col: i32,
    pub row: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PatternSurface {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<PatternRoot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternRoot {
    pub position: PatternPos,
    pub container: PatternContainer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternContainer {
    pub kind: PatternContainerKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<PatternItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternContainerKind {
    Basic,
    Subdivide,
    Alternate,
    Parallel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternItem {
    pub position: PatternPos,
    pub kind: PatternItemKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatternItemKind {
    Atom(PatternAtom),
    Container(Box<PatternContainer>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatternAtom {
    Note { value: String },
    Scalar { value: f64 },
    Rest,
    Operator { operator: PatternOperatorKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternOperatorKind {
    Elongation,
    PitchShift,
    Slow,
    Fast,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternSurfaceDiagnostic {
    pub code: PatternDiagnosticCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternDiagnosticCode {
    OperatorMissingTarget,
    OperatorMissingArgument,
    ConsecutiveOperators,
    TypeMismatch,
    InvalidPlacement,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyzedPatternSource {
    pub root_cycles: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<AnalyzedPatternRoot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PatternSurfaceDiagnostic>,
}

impl AnalyzedPatternSource {
    pub fn is_silent(&self) -> bool {
        self.roots
            .iter()
            .all(|root| root.spans.iter().all(|span| span.events.is_empty()))
    }

    pub fn is_layered(&self) -> bool {
        self.roots
            .iter()
            .any(|root| root.spans.iter().any(|span| span.events.len() > 1))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyzedPatternRoot {
    pub kind: PatternContainerKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<ResolvedPatternSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedPatternSpan {
    pub start: Rational,
    pub duration: Rational,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<ResolvedPatternEvent>,
    #[serde(default = "default_tempo_factor")]
    pub tempo_factor: f64,
}

fn default_tempo_factor() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedPatternEvent {
    pub value: PatternEventValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_octave: Option<i64>,
    #[serde(default)]
    pub octave_shift: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatternEventValue {
    Note { value: String },
    Scalar { value: f64 },
}

pub fn analyze_pattern_surface(surface: &PatternSurface) -> AnalyzedPatternSource {
    let mut diagnostics = Vec::new();
    let mut roots = surface.roots.clone();
    roots.sort_by_key(|root| (root.position.col, root.position.row));

    let roots = roots
        .iter()
        .enumerate()
        .map(|(cycle_index, root)| AnalyzedPatternRoot {
            kind: root.container.kind,
            spans: resolve_container(
                &root.container,
                Rational::new(cycle_index as i64, 1).expect("integral cycle offset"),
                Rational::ONE,
                cycle_index,
                &mut diagnostics,
                &[cycle_index],
            ),
        })
        .collect::<Vec<_>>();

    AnalyzedPatternSource {
        root_cycles: surface.roots.len(),
        roots,
        diagnostics,
    }
}

#[derive(Clone)]
struct BoundSlot {
    source: PatternItemKind,
    units: i64,
    absolute_octave: Option<i64>,
    octave_shift: i64,
    tempo_factor: f64,
}

fn resolve_container(
    container: &PatternContainer,
    start: Rational,
    duration: Rational,
    cycle_index: usize,
    diagnostics: &mut Vec<PatternSurfaceDiagnostic>,
    path: &[usize],
) -> Vec<ResolvedPatternSpan> {
    let slots = bind_slots(container, diagnostics, path);
    if slots.is_empty() {
        return vec![ResolvedPatternSpan {
            start,
            duration,
            events: Vec::new(),
            tempo_factor: 1.0,
        }];
    }

    match container.kind {
        PatternContainerKind::Basic => {
            resolve_weighted_sequence(&slots, start, duration, cycle_index, diagnostics, path)
        }
        PatternContainerKind::Subdivide => {
            let slot_duration = divide_rational(duration, slots.len() as i64);
            let mut spans = Vec::new();
            for (index, slot) in slots.iter().enumerate() {
                let slot_start =
                    add_rational(start, multiply_rational(slot_duration, index as i64));
                spans.extend(resolve_slot(
                    slot,
                    slot_start,
                    slot_duration,
                    cycle_index,
                    diagnostics,
                    &extend_path(path, index),
                ));
            }
            spans
        }
        PatternContainerKind::Alternate => {
            let slot_index = cycle_index % slots.len();
            resolve_slot(
                &slots[slot_index],
                start,
                duration,
                cycle_index,
                diagnostics,
                &extend_path(path, slot_index),
            )
        }
        PatternContainerKind::Parallel => {
            let mut spans = Vec::new();
            for (index, slot) in slots.iter().enumerate() {
                spans.extend(resolve_slot(
                    slot,
                    start,
                    duration,
                    cycle_index,
                    diagnostics,
                    &extend_path(path, index),
                ));
            }
            spans
        }
    }
}

fn resolve_weighted_sequence(
    slots: &[BoundSlot],
    start: Rational,
    duration: Rational,
    cycle_index: usize,
    diagnostics: &mut Vec<PatternSurfaceDiagnostic>,
    path: &[usize],
) -> Vec<ResolvedPatternSpan> {
    let total_units = slots
        .iter()
        .map(|slot| slot.units.max(1))
        .sum::<i64>()
        .max(1);
    let mut spans = Vec::new();
    let mut cursor = start;

    for (index, slot) in slots.iter().enumerate() {
        let slot_duration = multiply_rational(divide_rational(duration, total_units), slot.units);
        spans.extend(resolve_slot(
            slot,
            cursor,
            slot_duration,
            cycle_index,
            diagnostics,
            &extend_path(path, index),
        ));
        cursor = add_rational(cursor, slot_duration);
    }

    spans
}

fn resolve_slot(
    slot: &BoundSlot,
    start: Rational,
    duration: Rational,
    cycle_index: usize,
    diagnostics: &mut Vec<PatternSurfaceDiagnostic>,
    path: &[usize],
) -> Vec<ResolvedPatternSpan> {
    match &slot.source {
        PatternItemKind::Atom(atom) => match atom {
            PatternAtom::Note { value } => vec![ResolvedPatternSpan {
                start,
                duration,
                events: vec![ResolvedPatternEvent {
                    value: PatternEventValue::Note {
                        value: value.clone(),
                    },
                    absolute_octave: slot.absolute_octave,
                    octave_shift: slot.octave_shift,
                }],
                tempo_factor: slot.tempo_factor,
            }],
            PatternAtom::Scalar { value } => vec![ResolvedPatternSpan {
                start,
                duration,
                events: vec![ResolvedPatternEvent {
                    value: PatternEventValue::Scalar { value: *value },
                    absolute_octave: None,
                    octave_shift: 0,
                }],
                tempo_factor: slot.tempo_factor,
            }],
            PatternAtom::Rest => vec![ResolvedPatternSpan {
                start,
                duration,
                events: Vec::new(),
                tempo_factor: slot.tempo_factor,
            }],
            PatternAtom::Operator { .. } => {
                diagnostics.push(PatternSurfaceDiagnostic {
                    code: PatternDiagnosticCode::InvalidPlacement,
                    message: "unbound operator cannot resolve as a slot".into(),
                    path: path.to_vec(),
                });
                vec![ResolvedPatternSpan {
                    start,
                    duration,
                    events: Vec::new(),
                    tempo_factor: slot.tempo_factor,
                }]
            }
        },
        PatternItemKind::Container(container) => {
            let mut spans =
                resolve_container(container, start, duration, cycle_index, diagnostics, path);
            if slot.absolute_octave.is_some() || slot.octave_shift != 0 {
                for span in &mut spans {
                    for event in &mut span.events {
                        if matches!(event.value, PatternEventValue::Note { .. }) {
                            if slot.absolute_octave.is_some() {
                                event.absolute_octave = slot.absolute_octave;
                            }
                            event.octave_shift += slot.octave_shift;
                        }
                    }
                }
            }
            if (slot.tempo_factor - 1.0).abs() > f64::EPSILON {
                for span in &mut spans {
                    span.tempo_factor *= slot.tempo_factor;
                }
            }
            spans
        }
    }
}

fn bind_slots(
    container: &PatternContainer,
    diagnostics: &mut Vec<PatternSurfaceDiagnostic>,
    path: &[usize],
) -> Vec<BoundSlot> {
    let mut items = container.items.clone();
    items.sort_by_key(|item| (item.position.col, -item.position.row));

    let mut slots = Vec::<BoundSlot>::new();
    let mut pending_operator = None::<(PatternOperatorKind, Vec<usize>)>;

    for (index, item) in items.iter().enumerate() {
        match &item.kind {
            PatternItemKind::Atom(PatternAtom::Operator { operator }) => {
                if pending_operator.is_some() {
                    diagnostics.push(PatternSurfaceDiagnostic {
                        code: PatternDiagnosticCode::ConsecutiveOperators,
                        message: "consecutive operators require a value between them".into(),
                        path: extend_path(path, index),
                    });
                    pending_operator = Some((*operator, extend_path(path, index)));
                    continue;
                }
                if slots.is_empty() {
                    diagnostics.push(PatternSurfaceDiagnostic {
                        code: PatternDiagnosticCode::OperatorMissingTarget,
                        message: "operator requires a target on its left".into(),
                        path: extend_path(path, index),
                    });
                    continue;
                }
                pending_operator = Some((*operator, extend_path(path, index)));
            }
            _ => {
                if let Some((operator, op_path)) = pending_operator.take() {
                    if let Some(last) = slots.last_mut() {
                        apply_operator(last, &item.kind, operator, diagnostics, &op_path);
                    }
                } else if let PatternItemKind::Atom(PatternAtom::Scalar { value }) = &item.kind {
                    let can_bind_absolute_octave = value.fract() == 0.0
                        && slots
                            .last()
                            .is_some_and(|slot| matches!(slot.source, PatternItemKind::Atom(PatternAtom::Note { .. })));
                    if can_bind_absolute_octave {
                        if let Some(last) = slots.last_mut() {
                            last.absolute_octave = Some(*value as i64);
                        }
                    } else {
                        slots.push(BoundSlot {
                            source: item.kind.clone(),
                            units: 1,
                            absolute_octave: None,
                            octave_shift: 0,
                            tempo_factor: 1.0,
                        });
                    }
                } else {
                    slots.push(BoundSlot {
                        source: item.kind.clone(),
                        units: 1,
                        absolute_octave: None,
                        octave_shift: 0,
                        tempo_factor: 1.0,
                    });
                }
            }
        }
    }

    if let Some((_, op_path)) = pending_operator {
        diagnostics.push(PatternSurfaceDiagnostic {
            code: PatternDiagnosticCode::OperatorMissingArgument,
            message: "operator requires an argument on its right".into(),
            path: op_path,
        });
    }

    slots
}

fn apply_operator(
    target: &mut BoundSlot,
    argument: &PatternItemKind,
    operator: PatternOperatorKind,
    diagnostics: &mut Vec<PatternSurfaceDiagnostic>,
    path: &[usize],
) {
    let scalar = match argument {
        PatternItemKind::Atom(PatternAtom::Scalar { value }) => *value,
        _ => {
            diagnostics.push(PatternSurfaceDiagnostic {
                code: PatternDiagnosticCode::TypeMismatch,
                message: "operator argument must be a scalar atom".into(),
                path: path.to_vec(),
            });
            return;
        }
    };

    if scalar.fract() != 0.0 {
        diagnostics.push(PatternSurfaceDiagnostic {
            code: PatternDiagnosticCode::TypeMismatch,
            message: "operator argument must be an integer-valued scalar".into(),
            path: path.to_vec(),
        });
        return;
    }

    let scalar = scalar as i64;
    match operator {
        PatternOperatorKind::Elongation => target.units = scalar.max(1),
        PatternOperatorKind::PitchShift => target.octave_shift += scalar,
        PatternOperatorKind::Slow => {
            target.tempo_factor /= scalar as f64;
        }
        PatternOperatorKind::Fast => {
            target.tempo_factor *= scalar as f64;
        }
    }
}

fn extend_path(path: &[usize], next: usize) -> Vec<usize> {
    let mut extended = path.to_vec();
    extended.push(next);
    extended
}

fn add_rational(lhs: Rational, rhs: Rational) -> Rational {
    Rational::new(
        lhs.numerator() * rhs.denominator() + rhs.numerator() * lhs.denominator(),
        lhs.denominator() * rhs.denominator(),
    )
    .expect("valid rational addition")
}

fn multiply_rational(lhs: Rational, rhs: i64) -> Rational {
    Rational::new(lhs.numerator() * rhs, lhs.denominator()).expect("valid rational multiplication")
}

fn divide_rational(lhs: Rational, rhs: i64) -> Rational {
    Rational::new(lhs.numerator(), lhs.denominator() * rhs).expect("valid rational division")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(value: &str, col: i32) -> PatternItem {
        PatternItem {
            position: PatternPos { col, row: 0 },
            kind: PatternItemKind::Atom(PatternAtom::Note {
                value: value.into(),
            }),
        }
    }

    fn scalar(value: f64, col: i32) -> PatternItem {
        PatternItem {
            position: PatternPos { col, row: 0 },
            kind: PatternItemKind::Atom(PatternAtom::Scalar { value }),
        }
    }

    fn operator(operator: PatternOperatorKind, col: i32) -> PatternItem {
        PatternItem {
            position: PatternPos { col, row: 0 },
            kind: PatternItemKind::Atom(PatternAtom::Operator { operator }),
        }
    }

    #[test]
    fn subdivide_root_owns_one_cycle() {
        let analyzed = analyze_pattern_surface(&PatternSurface {
            roots: vec![PatternRoot {
                position: PatternPos { col: 0, row: 0 },
                container: PatternContainer {
                    kind: PatternContainerKind::Subdivide,
                    items: vec![note("b4", 0), note("d5", 1)],
                },
            }],
        });

        assert_eq!(analyzed.root_cycles, 1);
        assert_eq!(analyzed.roots[0].spans.len(), 2);
        assert_eq!(
            analyzed.roots[0].spans[0].duration,
            Rational::new(1, 2).unwrap()
        );
    }

    #[test]
    fn chained_roots_concatenate_cycles() {
        let analyzed = analyze_pattern_surface(&PatternSurface {
            roots: vec![
                PatternRoot {
                    position: PatternPos { col: 0, row: 0 },
                    container: PatternContainer {
                        kind: PatternContainerKind::Basic,
                        items: vec![note("b4", 0)],
                    },
                },
                PatternRoot {
                    position: PatternPos { col: 1, row: 0 },
                    container: PatternContainer {
                        kind: PatternContainerKind::Basic,
                        items: vec![note("d5", 0)],
                    },
                },
            ],
        });

        assert_eq!(analyzed.roots[0].spans[0].start, Rational::ZERO);
        assert_eq!(analyzed.roots[1].spans[0].start, Rational::ONE);
    }

    #[test]
    fn scalar_adjacent_to_note_binds_absolute_octave() {
        let analyzed = analyze_pattern_surface(&PatternSurface {
            roots: vec![PatternRoot {
                position: PatternPos { col: 0, row: 0 },
                container: PatternContainer {
                    kind: PatternContainerKind::Basic,
                    items: vec![note("e", 0), scalar(4.0, 1)],
                },
            }],
        });

        assert!(analyzed.diagnostics.is_empty());
        assert_eq!(analyzed.roots[0].spans[0].events[0].absolute_octave, Some(4));
        assert_eq!(analyzed.roots[0].spans[0].events[0].octave_shift, 0);
    }

    #[test]
    fn chained_operators_can_follow_note_octave_binding() {
        let analyzed = analyze_pattern_surface(&PatternSurface {
            roots: vec![PatternRoot {
                position: PatternPos { col: 0, row: 0 },
                container: PatternContainer {
                    kind: PatternContainerKind::Basic,
                    items: vec![
                        note("e", 0),
                        operator(PatternOperatorKind::Elongation, 1),
                        scalar(4.0, 2),
                        operator(PatternOperatorKind::PitchShift, 3),
                        scalar(7.0, 4),
                    ],
                },
            }],
        });

        assert!(analyzed.diagnostics.is_empty());
        assert_eq!(analyzed.roots[0].spans[0].events[0].absolute_octave, None);
        assert_eq!(analyzed.roots[0].spans[0].events[0].octave_shift, 7);
    }

    #[test]
    fn operator_binds_left_target_and_right_argument() {
        let analyzed = analyze_pattern_surface(&PatternSurface {
            roots: vec![PatternRoot {
                position: PatternPos { col: 0, row: 0 },
                container: PatternContainer {
                    kind: PatternContainerKind::Basic,
                    items: vec![
                        note("b4", 0),
                        operator(PatternOperatorKind::PitchShift, 1),
                        scalar(1.0, 2),
                    ],
                },
            }],
        });

        assert!(analyzed.diagnostics.is_empty());
        assert_eq!(analyzed.roots[0].spans[0].events[0].octave_shift, 1);
    }

    #[test]
    fn operator_missing_argument_produces_diagnostic() {
        let analyzed = analyze_pattern_surface(&PatternSurface {
            roots: vec![PatternRoot {
                position: PatternPos { col: 0, row: 0 },
                container: PatternContainer {
                    kind: PatternContainerKind::Basic,
                    items: vec![note("b4", 0), operator(PatternOperatorKind::Elongation, 1)],
                },
            }],
        });

        assert!(analyzed.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == PatternDiagnosticCode::OperatorMissingArgument
        }));
    }

    #[test]
    fn alternate_picks_one_slot_per_cycle() {
        let analyzed = analyze_pattern_surface(&PatternSurface {
            roots: vec![
                PatternRoot {
                    position: PatternPos { col: 0, row: 0 },
                    container: PatternContainer {
                        kind: PatternContainerKind::Alternate,
                        items: vec![note("a4", 0), note("b4", 1)],
                    },
                },
                PatternRoot {
                    position: PatternPos { col: 1, row: 0 },
                    container: PatternContainer {
                        kind: PatternContainerKind::Alternate,
                        items: vec![note("a4", 0), note("b4", 1)],
                    },
                },
            ],
        });

        match &analyzed.roots[0].spans[0].events[0].value {
            PatternEventValue::Note { value } => assert_eq!(value, "a4"),
            other => panic!("unexpected value {other:?}"),
        }
        match &analyzed.roots[1].spans[0].events[0].value {
            PatternEventValue::Note { value } => assert_eq!(value, "b4"),
            other => panic!("unexpected value {other:?}"),
        }
    }

    #[test]
    fn parallel_layers_child_events() {
        let analyzed = analyze_pattern_surface(&PatternSurface {
            roots: vec![PatternRoot {
                position: PatternPos { col: 0, row: 0 },
                container: PatternContainer {
                    kind: PatternContainerKind::Parallel,
                    items: vec![note("a4", 0), note("b4", 1)],
                },
            }],
        });

        assert_eq!(analyzed.roots[0].spans.len(), 2);
    }
}
