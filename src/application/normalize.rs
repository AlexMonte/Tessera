use crate::domain::{
    AtomExpr, AtomExprKind, AtomModifier, AtomOperatorToken, AtomTile, ContainerId,
    ContainerSurfaceTile, Diagnostic, DiagnosticCategory, DiagnosticKind, DiagnosticLocation,
    MusicalValue, NormalizedContainer, NormalizedProgram, Rational, TesseraProgram,
};

pub fn normalize_container(
    program: &TesseraProgram,
    container_id: &ContainerId,
) -> Result<NormalizedContainer, Vec<Diagnostic>> {
    let Some(container) = program.containers.get(container_id) else {
        return Err(vec![Diagnostic::new(
            DiagnosticCategory::Placement,
            DiagnosticKind::MissingContainer,
            "Container is missing from the program container table.",
            Some(DiagnosticLocation::ContainerStack {
                container: container_id.clone(),
                index: 0,
            }),
        )]);
    };

    let mut exprs = Vec::new();
    let mut diagnostics = Vec::new();
    let mut index = 0usize;

    while index < container.stack.len() {
        let expr = parse_atom_expr(
            program,
            container_id,
            &container.stack,
            &mut index,
            &mut diagnostics,
        );
        if let Some(expr) = expr {
            exprs.push(expr);
        }
    }

    if diagnostics.is_empty() {
        Ok(NormalizedContainer::new(
            container_id.clone(),
            container.kind,
            exprs,
        ))
    } else {
        Err(diagnostics)
    }
}

fn parse_atom_expr(
    program: &TesseraProgram,
    container_id: &ContainerId,
    stack: &[ContainerSurfaceTile],
    index: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<AtomExpr> {
    let mut expr = parse_simple_expr(program, container_id, stack, index, diagnostics)?;
    loop {
        let Some(ContainerSurfaceTile::Atom(AtomTile::Operator(token))) = stack.get(*index) else {
            break;
        };
        let group_kind = match token {
            AtomOperatorToken::Choice => Some(AtomExprKind::Choice(Vec::new())),
            AtomOperatorToken::Parallel => Some(AtomExprKind::Parallel(Vec::new())),
            _ => None,
        };
        let Some(mut group_kind) = group_kind else {
            break;
        };
        *index += 1;
        let Some(rhs) = parse_simple_expr(program, container_id, stack, index, diagnostics) else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::LocalGrammar,
                DiagnosticKind::OperatorWithoutRightScalar,
                "Choice and parallel separators require an expression on their right.",
                Some(DiagnosticLocation::ContainerStack {
                    container: container_id.clone(),
                    index: index.saturating_sub(1),
                }),
            ));
            break;
        };
        let mut members = vec![expr, rhs];
        while matches!(stack.get(*index), Some(ContainerSurfaceTile::Atom(AtomTile::Operator(next))) if next == token)
        {
            *index += 1;
            if let Some(member) =
                parse_simple_expr(program, container_id, stack, index, diagnostics)
            {
                members.push(member);
            } else {
                break;
            }
        }
        group_kind = match group_kind {
            AtomExprKind::Choice(_) => AtomExprKind::Choice(members),
            AtomExprKind::Parallel(_) => AtomExprKind::Parallel(members),
            _ => unreachable!(),
        };
        expr = AtomExpr {
            kind: group_kind,
            modifiers: Vec::new(),
        };
    }
    Some(expr)
}

fn parse_simple_expr(
    _program: &TesseraProgram,
    container_id: &ContainerId,
    stack: &[ContainerSurfaceTile],
    index: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<AtomExpr> {
    let position = *index;
    let location = || DiagnosticLocation::ContainerStack {
        container: container_id.clone(),
        index: position,
    };

    let value = match stack.get(*index)? {
        ContainerSurfaceTile::Atom(AtomTile::Note(note)) => {
            let mut note = note.clone();
            if let Some(ContainerSurfaceTile::Atom(AtomTile::Scalar(octave))) =
                stack.get(*index + 1)
            {
                if matches!(
                    stack.get(*index + 2),
                    Some(ContainerSurfaceTile::Atom(AtomTile::Scalar(_)))
                ) {
                    diagnostics.push(Diagnostic::new(
                        DiagnosticCategory::LocalGrammar,
                        DiagnosticKind::AmbiguousOctaveBinding,
                        "More than one scalar directly after a note is ambiguous.",
                        Some(DiagnosticLocation::ContainerStack {
                            container: container_id.clone(),
                            index: *index + 1,
                        }),
                    ));
                    while matches!(
                        stack.get(*index + 1),
                        Some(ContainerSurfaceTile::Atom(AtomTile::Scalar(_)))
                    ) {
                        *index += 1;
                    }
                } else if octave.value.denominator == 1 {
                    note.octave = Some(octave.value.numerator);
                    *index += 1;
                } else {
                    diagnostics.push(Diagnostic::new(
                        DiagnosticCategory::LocalGrammar,
                        DiagnosticKind::AmbiguousOctaveBinding,
                        "Octave binding requires an integer scalar directly after a note.",
                        Some(DiagnosticLocation::ContainerStack {
                            container: container_id.clone(),
                            index: *index + 1,
                        }),
                    ));
                    *index += 1;
                }
            }
            MusicalValue::Note(note)
        }
        ContainerSurfaceTile::Atom(AtomTile::Rest) => MusicalValue::Rest,
        ContainerSurfaceTile::Atom(AtomTile::Scalar(scalar)) => {
            MusicalValue::Scalar(scalar.clone())
        }
        ContainerSurfaceTile::NestedContainer(nested) => {
            MusicalValue::NestedContainer(nested.clone())
        }
        ContainerSurfaceTile::Transform => {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::Placement,
                DiagnosticKind::TransformInsideContainer,
                "Transform tiles cannot appear inside a container stack.",
                Some(location()),
            ));
            *index += 1;
            return None;
        }
        ContainerSurfaceTile::Output => {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::Placement,
                DiagnosticKind::OutputInsideContainer,
                "Output tiles cannot appear inside a container stack.",
                Some(location()),
            ));
            *index += 1;
            return None;
        }
        ContainerSurfaceTile::Atom(AtomTile::Operator(_)) => {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::LocalGrammar,
                DiagnosticKind::OperatorWithoutLeftValue,
                "An operator requires a musical value on its left.",
                Some(location()),
            ));
            *index += 1;
            return None;
        }
    };

    *index += 1;
    let mut modifiers = Vec::new();
    while *index < stack.len() {
        let next_location = Some(DiagnosticLocation::ContainerStack {
            container: container_id.clone(),
            index: *index,
        });
        match &stack[*index] {
            ContainerSurfaceTile::Atom(AtomTile::Operator(
                AtomOperatorToken::Choice | AtomOperatorToken::Parallel,
            )) => {
                break;
            }
            ContainerSurfaceTile::Atom(AtomTile::Operator(token)) => match token {
                AtomOperatorToken::Degrade => {
                    if let Some(ContainerSurfaceTile::Atom(AtomTile::Scalar(scalar))) =
                        stack.get(*index + 1)
                    {
                        if scalar.value < Rational::zero() || scalar.value > Rational::one() {
                            diagnostics.push(Diagnostic::new(
                                DiagnosticCategory::LocalGrammar,
                                DiagnosticKind::InvalidModifierArgument,
                                "Degrade probability must be within 0..=1.",
                                next_location.clone(),
                            ));
                        }
                        modifiers.push(AtomModifier::Degrade(Some(scalar.value)));
                        *index += 2;
                    } else {
                        modifiers.push(AtomModifier::Degrade(None));
                        *index += 1;
                    }
                }
                AtomOperatorToken::Fast
                | AtomOperatorToken::Slow
                | AtomOperatorToken::Elongate
                | AtomOperatorToken::Replicate
                | AtomOperatorToken::Euclid
                | AtomOperatorToken::EuclidRot => {
                    let Some(ContainerSurfaceTile::Atom(AtomTile::Scalar(scalar))) =
                        stack.get(*index + 1)
                    else {
                        diagnostics.push(Diagnostic::new(
                            DiagnosticCategory::LocalGrammar,
                            DiagnosticKind::OperatorWithoutRightScalar,
                            "This modifier requires a scalar on its right.",
                            next_location,
                        ));
                        *index += 1;
                        break;
                    };
                    let modifier = build_modifier(
                        token,
                        scalar.value,
                        stack,
                        index,
                        container_id,
                        diagnostics,
                    );
                    modifiers.push(modifier);
                }
                AtomOperatorToken::Choice | AtomOperatorToken::Parallel => unreachable!(),
            },
            ContainerSurfaceTile::Atom(AtomTile::Scalar(_)) => break,
            _ => break,
        }
    }

    Some(AtomExpr {
        kind: AtomExprKind::Value(value),
        modifiers,
    })
}

fn build_modifier(
    token: &AtomOperatorToken,
    scalar: Rational,
    stack: &[ContainerSurfaceTile],
    index: &mut usize,
    container_id: &ContainerId,
    diagnostics: &mut Vec<Diagnostic>,
) -> AtomModifier {
    let next_location = Some(DiagnosticLocation::ContainerStack {
        container: container_id.clone(),
        index: *index,
    });
    let modifier = match token {
        AtomOperatorToken::Fast => {
            if scalar <= Rational::zero() {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::LocalGrammar,
                    DiagnosticKind::InvalidModifierArgument,
                    "Fast expects a positive non-zero rational factor.",
                    next_location.clone(),
                ));
            }
            *index += 2;
            AtomModifier::Fast(scalar)
        }
        AtomOperatorToken::Slow => {
            if scalar <= Rational::zero() {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::LocalGrammar,
                    DiagnosticKind::InvalidModifierArgument,
                    "Slow expects a positive non-zero rational factor.",
                    next_location.clone(),
                ));
            }
            *index += 2;
            AtomModifier::Slow(scalar)
        }
        AtomOperatorToken::Elongate => {
            if scalar <= Rational::zero() {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::LocalGrammar,
                    DiagnosticKind::InvalidModifierArgument,
                    "Elongate expects a positive non-zero rational weight.",
                    next_location.clone(),
                ));
            }
            *index += 2;
            AtomModifier::Elongate(scalar)
        }
        AtomOperatorToken::Replicate => {
            *index += 2;
            if scalar.denominator != 1 || scalar.numerator < 1 {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::LocalGrammar,
                    DiagnosticKind::InvalidModifierArgument,
                    "Replicate expects a positive integer scalar.",
                    next_location,
                ));
                AtomModifier::Replicate(1)
            } else {
                AtomModifier::Replicate(scalar.numerator as u32)
            }
        }
        AtomOperatorToken::Euclid => {
            let Some(ContainerSurfaceTile::Atom(AtomTile::Scalar(steps))) = stack.get(*index + 2)
            else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::LocalGrammar,
                    DiagnosticKind::OperatorWithoutRightScalar,
                    "Euclid expects pulses and steps scalar operands.",
                    next_location.clone(),
                ));
                *index += 2;
                return AtomModifier::Euclid {
                    pulses: 1,
                    steps: 1,
                };
            };
            if scalar.denominator != 1
                || steps.value.denominator != 1
                || scalar.numerator < 0
                || steps.value.numerator <= 0
                || scalar.numerator > steps.value.numerator
            {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::LocalGrammar,
                    DiagnosticKind::InvalidModifierArgument,
                    "Euclid expects integer pulses and steps with 0 <= pulses <= steps and steps > 0.",
                    next_location.clone(),
                ));
            }
            *index += 3;
            AtomModifier::Euclid {
                pulses: scalar.numerator.max(0) as u32,
                steps: steps.value.numerator.max(1) as u32,
            }
        }
        AtomOperatorToken::EuclidRot => {
            let Some(ContainerSurfaceTile::Atom(AtomTile::Scalar(steps))) = stack.get(*index + 2)
            else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::LocalGrammar,
                    DiagnosticKind::OperatorWithoutRightScalar,
                    "EuclidRot expects pulses, steps, and rotation scalar operands.",
                    next_location.clone(),
                ));
                *index += 2;
                return AtomModifier::EuclidRot {
                    pulses: 1,
                    steps: 1,
                    rotation: 0,
                };
            };
            let Some(ContainerSurfaceTile::Atom(AtomTile::Scalar(rotation))) =
                stack.get(*index + 3)
            else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::LocalGrammar,
                    DiagnosticKind::OperatorWithoutRightScalar,
                    "EuclidRot expects a rotation scalar operand.",
                    next_location.clone(),
                ));
                *index += 3;
                return AtomModifier::EuclidRot {
                    pulses: 1,
                    steps: 1,
                    rotation: 0,
                };
            };
            if scalar.denominator != 1
                || steps.value.denominator != 1
                || rotation.value.denominator != 1
                || scalar.numerator < 0
                || steps.value.numerator <= 0
                || scalar.numerator > steps.value.numerator
            {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::LocalGrammar,
                    DiagnosticKind::InvalidModifierArgument,
                    "EuclidRot expects integer pulses, steps, and rotation with 0 <= pulses <= steps and steps > 0.",
                    next_location.clone(),
                ));
            }
            *index += 4;
            AtomModifier::EuclidRot {
                pulses: scalar.numerator.max(0) as u32,
                steps: steps.value.numerator.max(1) as u32,
                rotation: rotation.value.numerator as i32,
            }
        }
        AtomOperatorToken::Degrade | AtomOperatorToken::Choice | AtomOperatorToken::Parallel => {
            unreachable!()
        }
    };
    modifier
}

pub fn normalize_program(program: &TesseraProgram) -> Result<NormalizedProgram, Vec<Diagnostic>> {
    let mut containers = std::collections::BTreeMap::new();
    let mut diagnostics = Vec::new();
    for container_id in program.containers.keys() {
        match normalize_container(program, container_id) {
            Ok(container) => {
                containers.insert(container_id.clone(), container);
            }
            Err(mut errs) => diagnostics.append(&mut errs),
        }
    }
    if diagnostics.is_empty() {
        Ok(NormalizedProgram {
            root_nodes: program.root_nodes.clone(),
            containers,
            relations: program.relations.clone(),
        })
    } else {
        Err(diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::{AtomExprKind, Container, ContainerKind, NoteAtom, ScalarAtom};

    use super::*;

    #[test]
    fn normalizes_broad_modifier_subset() {
        let container_id = ContainerId::new("phrase");
        let mut containers = BTreeMap::new();
        containers.insert(
            container_id.clone(),
            Container::new(
                ContainerKind::Sequence,
                vec![
                    ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("e"))),
                    ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Elongate)),
                    ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(4))),
                    ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Slow)),
                    ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(2))),
                    ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Replicate)),
                    ContainerSurfaceTile::Atom(AtomTile::Scalar(ScalarAtom::integer(3))),
                    ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Degrade)),
                    ContainerSurfaceTile::Atom(AtomTile::Operator(AtomOperatorToken::Choice)),
                    ContainerSurfaceTile::Atom(AtomTile::Note(NoteAtom::new("g"))),
                ],
            ),
        );

        let normalized = normalize_container(
            &TesseraProgram {
                root_nodes: BTreeMap::new(),
                containers,
                relations: vec![],
            },
            &container_id,
        )
        .expect("container should normalize");

        assert_eq!(normalized.exprs.len(), 1);
        match &normalized.exprs[0].kind {
            AtomExprKind::Choice(branches) => {
                assert_eq!(branches.len(), 2);
                assert_eq!(branches[0].modifiers.len(), 4);
            }
            other => panic!("expected choice expression, got {other:?}"),
        }
    }
}
