use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{
    CycleDuration, CycleSpan, CycleTime, Diagnostic, EventField, FieldValue, FlowControlKind,
    FlowControlNode, FlowControlPolicy, InputEndpoint, InputPort, OutputEndpoint, OutputPort,
    PatternEvent, PatternStream, PortGroupId, PortMemberId, Rational,
};

pub type NodeInputs = BTreeMap<InputEndpoint, Vec<PatternStream>>;
pub type NodeOutputs = BTreeMap<OutputEndpoint, PatternStream>;

#[allow(dead_code)]
pub fn flow_policy_contract_table() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "layer",
            "Deterministic compile-time layering of all input streams.",
        ),
        (
            "merge_append",
            "Deterministic compile-time append of input streams in member order.",
        ),
        (
            "merge_interleave",
            "Deterministic compile-time span-ordered interleave of input streams.",
        ),
        (
            "merge_priority",
            "Deterministic compile-time conflict resolution where earlier streams win overlaps.",
        ),
        (
            "merge_deduplicate",
            "Deterministic compile-time duplicate filtering by event fingerprint.",
        ),
        (
            "mix_field_blend",
            "Deterministic compile-time field blending across grouped inputs.",
        ),
        (
            "mix_weighted",
            "Deterministic compile-time weighted gain shaping using the control amount when present.",
        ),
        (
            "mix_gain_average",
            "Deterministic compile-time average gain shaping across grouped inputs.",
        ),
        (
            "split_copy_to_all",
            "Deterministic compile-time copy of each input event to every output member.",
        ),
        (
            "split_by_index_modulo",
            "Deterministic compile-time round-robin partition by event index.",
        ),
        (
            "split_by_event_field",
            "Deterministic compile-time partition by matching event fields to output member ids.",
        ),
        (
            "split_by_pitch_range",
            "Deterministic compile-time partition by note octave threshold.",
        ),
        (
            "mask_gate",
            "Deterministic compile-time gate using mask activity spans.",
        ),
        (
            "mask_scale",
            "Deterministic compile-time gain scaling using mask scalar values.",
        ),
        (
            "mask_clip",
            "Deterministic compile-time clipping to active mask spans.",
        ),
        (
            "mask_invert_gate",
            "Deterministic compile-time inverse gate using mask inactivity.",
        ),
        (
            "switch_cycle_index",
            "Deterministic compile-time candidate selection using the current cycle index fallback.",
        ),
        (
            "switch_control_value",
            "Deterministic compile-time candidate selection using the first control scalar.",
        ),
        (
            "switch_seeded_random",
            "Deterministic compile-time seeded selection, not runtime-live random.",
        ),
        (
            "route_by_event_field",
            "Deterministic compile-time routing by field label matching.",
        ),
        (
            "route_by_index_modulo",
            "Deterministic compile-time routing by event index modulo output count.",
        ),
        (
            "route_by_control_value",
            "Deterministic compile-time routing by the first control scalar.",
        ),
        (
            "route_by_label",
            "Deterministic compile-time routing by note label matching.",
        ),
        (
            "choice_cycle",
            "Deterministic compile-time option selection by cycle index fallback.",
        ),
        (
            "choice_seeded_random",
            "Deterministic compile-time seeded option selection.",
        ),
        (
            "choice_weighted",
            "Deterministic compile-time weighted option selection from control data.",
        ),
    ]
}

pub fn apply_flow_control_policy(
    control: &FlowControlNode,
    inputs: NodeInputs,
) -> Result<NodeOutputs, Vec<Diagnostic>> {
    let outputs = match control.kind {
        FlowControlKind::Layer => BTreeMap::from_iter([(
            OutputEndpoint::Socket(OutputPort::new("out")),
            PatternStream::layer(collect_group_inputs(&inputs, &PortGroupId::new("streams"))),
        )]),
        FlowControlKind::Merge => BTreeMap::from_iter([(
            OutputEndpoint::Socket(OutputPort::new("out")),
            apply_merge_policy(
                control,
                collect_group_inputs(&inputs, &PortGroupId::new("streams")),
            ),
        )]),
        FlowControlKind::Mix => BTreeMap::from_iter([(
            OutputEndpoint::Socket(OutputPort::new("out")),
            apply_mix_policy(
                control,
                collect_group_inputs(&inputs, &PortGroupId::new("streams")),
                first_socket_input(&inputs, "amount"),
            ),
        )]),
        FlowControlKind::Split => {
            let input = first_socket_input(&inputs, "main").unwrap_or_default();
            let mut outputs = BTreeMap::new();
            for member in control
                .members
                .outputs
                .get(&PortGroupId::new("branches"))
                .into_iter()
                .flatten()
            {
                outputs.insert(
                    OutputEndpoint::GroupMember {
                        group: PortGroupId::new("branches"),
                        member: member.clone(),
                    },
                    apply_split_policy(control, input.clone(), member),
                );
            }
            outputs
        }
        FlowControlKind::Mask => {
            let main = first_socket_input(&inputs, "main").unwrap_or_default();
            let mask = first_socket_input(&inputs, "mask").unwrap_or_default();
            BTreeMap::from_iter([(
                OutputEndpoint::Socket(OutputPort::new("out")),
                apply_mask_policy(control, main, mask),
            )])
        }
        FlowControlKind::Switch => {
            let candidates = collect_group_inputs_in_member_order(
                &inputs,
                &PortGroupId::new("candidates"),
                control.members.inputs.get(&PortGroupId::new("candidates")),
            );
            let stream =
                apply_switch_policy(control, candidates, first_socket_input(&inputs, "control"));
            BTreeMap::from_iter([(OutputEndpoint::Socket(OutputPort::new("out")), stream)])
        }
        FlowControlKind::Route => {
            let main = first_socket_input(&inputs, "main").unwrap_or_default();
            let mut outputs = BTreeMap::new();
            for member in control
                .members
                .outputs
                .get(&PortGroupId::new("routes"))
                .into_iter()
                .flatten()
            {
                outputs.insert(
                    OutputEndpoint::GroupMember {
                        group: PortGroupId::new("routes"),
                        member: member.clone(),
                    },
                    apply_route_policy(
                        control,
                        main.clone(),
                        first_socket_input(&inputs, "control"),
                        member,
                    ),
                );
            }
            outputs
        }
        FlowControlKind::Choice => {
            let options = collect_group_inputs_in_member_order(
                &inputs,
                &PortGroupId::new("options"),
                control.members.inputs.get(&PortGroupId::new("options")),
            );
            let stream =
                apply_choice_policy(control, options, first_socket_input(&inputs, "control"));
            BTreeMap::from_iter([(OutputEndpoint::Socket(OutputPort::new("out")), stream)])
        }
    };
    Ok(outputs)
}

fn collect_group_inputs(inputs: &NodeInputs, group: &PortGroupId) -> Vec<PatternStream> {
    let mut collected = Vec::new();
    for (endpoint, streams) in inputs {
        if matches!(endpoint, InputEndpoint::GroupMember { group: endpoint_group, .. } if endpoint_group == group)
        {
            collected.extend(streams.clone());
        }
    }
    collected
}

fn collect_group_inputs_in_member_order(
    inputs: &NodeInputs,
    group: &PortGroupId,
    members: Option<&Vec<PortMemberId>>,
) -> Vec<PatternStream> {
    let Some(members) = members else {
        return collect_group_inputs(inputs, group);
    };
    members
        .iter()
        .filter_map(|member| {
            inputs
                .get(&InputEndpoint::GroupMember {
                    group: group.clone(),
                    member: member.clone(),
                })
                .and_then(|streams| streams.first().cloned())
        })
        .collect()
}

fn first_socket_input(inputs: &NodeInputs, port: &str) -> Option<PatternStream> {
    inputs
        .get(&InputEndpoint::Socket(InputPort::new(port)))
        .and_then(|streams| streams.first().cloned())
}

fn select_indexed_stream(mut streams: Vec<PatternStream>, index: usize) -> PatternStream {
    if streams.is_empty() {
        PatternStream::default()
    } else {
        streams.swap_remove(index % streams.len())
    }
}

fn sort_stream_events(mut stream: PatternStream) -> PatternStream {
    stream.events.sort_by(|a, b| {
        a.span
            .start
            .0
            .cmp(&b.span.start.0)
            .then(a.span.duration.0.cmp(&b.span.duration.0))
    });
    stream
}

fn merge_priority_streams(streams: Vec<PatternStream>) -> PatternStream {
    let mut kept: Vec<PatternEvent> = Vec::new();
    for stream in streams {
        for event in stream.events {
            if !kept
                .iter()
                .any(|existing| spans_overlap(existing.span, event.span))
            {
                kept.push(event);
            }
        }
    }
    PatternStream { events: kept }
}

fn deduplicate_stream(stream: PatternStream) -> PatternStream {
    let mut seen = BTreeSet::new();
    PatternStream {
        events: stream
            .events
            .into_iter()
            .filter(|event| seen.insert(event_fingerprint(event)))
            .collect(),
    }
}

fn blend_fields(mut stream: PatternStream) -> PatternStream {
    let gain = average_gain_from_stream(&stream).unwrap_or_else(Rational::one);
    for event in &mut stream.events {
        if !event
            .fields
            .iter()
            .any(|field| matches!(field, EventField::Gain(_)))
        {
            event
                .fields
                .push(EventField::Gain(FieldValue::Rational { value: gain }));
        }
    }
    stream
}

fn add_gain_field(mut stream: PatternStream, value: Rational) -> PatternStream {
    for event in &mut stream.events {
        event
            .fields
            .push(EventField::Gain(FieldValue::Rational { value }));
    }
    stream
}

fn split_by_event_field(stream: PatternStream, label: &str) -> PatternStream {
    PatternStream {
        events: stream
            .events
            .into_iter()
            .filter(|event| matches_field_label(event, label))
            .collect(),
    }
}

fn split_by_pitch(stream: PatternStream, label: &str, threshold_octave: i64) -> PatternStream {
    let keep_high = matches!(label, "high" | "upper" | "even");
    PatternStream {
        events: stream
            .events
            .into_iter()
            .filter(|event| {
                let is_high = matches!(
                    event.value,
                    crate::domain::EventValue::Note {
                        octave: Some(octave),
                        ..
                    } if octave >= threshold_octave
                );
                if keep_high { is_high } else { !is_high }
            })
            .collect(),
    }
}

fn gate_by_mask(main: PatternStream, mask: &PatternStream, invert: bool) -> PatternStream {
    PatternStream {
        events: main
            .events
            .into_iter()
            .filter(|event| {
                let active = mask_active_at(mask, event.span.start.0);
                if invert { !active } else { active }
            })
            .collect(),
    }
}

fn scale_by_mask(mut main: PatternStream, mask: &PatternStream) -> PatternStream {
    for event in &mut main.events {
        if let Some(value) = mask_value_at(mask, event.span.start.0) {
            event
                .fields
                .push(EventField::Gain(FieldValue::Rational { value }));
        }
    }
    main
}

fn clip_by_mask(main: PatternStream, mask: &PatternStream) -> PatternStream {
    let mut clipped = Vec::new();
    for event in main.events {
        for mask_event in &mask.events {
            if !mask_event_active(mask_event) {
                continue;
            }
            let start = if event.span.start.0 > mask_event.span.start.0 {
                event.span.start.0
            } else {
                mask_event.span.start.0
            };
            let event_end = event.span.start.0 + event.span.duration.0;
            let mask_end = mask_event.span.start.0 + mask_event.span.duration.0;
            let end = if event_end < mask_end {
                event_end
            } else {
                mask_end
            };
            if end > start {
                let mut clipped_event = event.clone();
                clipped_event.span.start = CycleTime(start);
                clipped_event.span.duration = CycleDuration(end - start);
                clipped.push(clipped_event);
            }
        }
    }
    PatternStream { events: clipped }
}

fn route_by_control_value(
    main: PatternStream,
    control_stream: Option<&PatternStream>,
    label: &str,
) -> PatternStream {
    let selected_left = scalar_stream_value(control_stream)
        .map(rational_to_usize)
        .unwrap_or(0)
        % 2
        == 0;
    let keep = if matches!(label, "left" | "even") {
        selected_left
    } else {
        !selected_left
    };
    if keep { main } else { PatternStream::default() }
}

fn route_by_label(stream: PatternStream, label: &str) -> PatternStream {
    PatternStream {
        events: stream
            .events
            .into_iter()
            .filter(
                |event| matches!(&event.value, crate::domain::EventValue::Note { value, .. } if value == label),
            )
            .collect(),
    }
}

fn route_by_event_field(stream: PatternStream, label: &str) -> PatternStream {
    split_by_event_field(stream, label)
}

fn scalar_stream_value(stream: Option<&PatternStream>) -> Option<Rational> {
    stream
        .and_then(|stream| stream.events.first())
        .and_then(|event| match event.value {
            crate::domain::EventValue::Scalar { value } => Some(value),
            _ => None,
        })
}

fn rational_to_usize(value: Rational) -> usize {
    if value <= Rational::zero() {
        0
    } else {
        (value.numerator / value.denominator) as usize
    }
}

fn weighted_index(len: usize, scalar: Rational) -> usize {
    if len == 0 {
        return 0;
    }
    let total_weight: i64 = (1..=len as i64).sum();
    let normalized = ((scalar.numerator.abs() % (scalar.denominator * total_weight))
        / scalar.denominator)
        .max(0);
    let mut cursor = 0i64;
    for (index, weight) in (1..=len as i64).enumerate() {
        cursor += weight;
        if normalized < cursor {
            return index;
        }
    }
    len - 1
}

fn average_gain_from_stream(stream: &PatternStream) -> Option<Rational> {
    let mut values = Vec::new();
    for event in &stream.events {
        for field in &event.fields {
            if let EventField::Gain(FieldValue::Rational { value }) = field {
                values.push(*value);
            }
        }
    }
    if values.is_empty() {
        None
    } else {
        let sum = values
            .into_iter()
            .fold(Rational::zero(), |acc, value| acc + value);
        Some(sum / Rational::from_integer(stream.events.len() as i64))
    }
}

fn deterministic_event_seed(event: &PatternEvent) -> u64 {
    let base = (event.span.start.0.numerator as i128 * 31
        + event.span.start.0.denominator as i128 * 17
        + event.span.duration.0.numerator as i128 * 13
        + event.span.duration.0.denominator as i128 * 7) as u64;
    match &event.value {
        crate::domain::EventValue::Note { value, octave } => {
            base ^ value.bytes().fold(0u64, |acc, byte| {
                acc.wrapping_mul(33).wrapping_add(byte as u64)
            }) ^ octave.unwrap_or_default() as u64
        }
        crate::domain::EventValue::Rest => base ^ 0x9E37,
        crate::domain::EventValue::Scalar { value } => {
            base ^ value.numerator as u64 ^ ((value.denominator as u64) << 1)
        }
    }
}

fn mask_active_at(mask: &PatternStream, time: Rational) -> bool {
    mask.events.iter().any(|event| {
        mask_event_active(event)
            && time >= event.span.start.0
            && time < event.span.start.0 + event.span.duration.0
    })
}

fn mask_value_at(mask: &PatternStream, time: Rational) -> Option<Rational> {
    mask.events.iter().find_map(|event| {
        (mask_event_active(event)
            && time >= event.span.start.0
            && time < event.span.start.0 + event.span.duration.0)
            .then(|| match event.value {
                crate::domain::EventValue::Scalar { value } => value,
                _ => Rational::one(),
            })
    })
}

fn mask_event_active(event: &PatternEvent) -> bool {
    match event.value {
        crate::domain::EventValue::Scalar { value } => value > Rational::zero(),
        _ => true,
    }
}

fn spans_overlap(left: CycleSpan, right: CycleSpan) -> bool {
    let left_end = left.start.0 + left.duration.0;
    let right_end = right.start.0 + right.duration.0;
    left.start.0 < right_end && right.start.0 < left_end
}

fn matches_field_label(event: &PatternEvent, label: &str) -> bool {
    event.fields.iter().any(|field| {
        matches!(
            (field, label),
            (EventField::Gain(_), "gain")
                | (EventField::Attack(_), "attack")
                | (EventField::Transpose(_), "transpose")
                | (EventField::Degrade(_), "degrade")
        )
    }) || (label == "plain" && event.fields.is_empty())
}

fn event_fingerprint(event: &PatternEvent) -> String {
    let value = match &event.value {
        crate::domain::EventValue::Note { value, octave } => format!("note:{value}:{octave:?}"),
        crate::domain::EventValue::Rest => "rest".to_string(),
        crate::domain::EventValue::Scalar { value } => {
            format!("scalar:{}/{}", value.numerator, value.denominator)
        }
    };
    let fields = event
        .fields
        .iter()
        .map(|field| match field {
            EventField::Gain(FieldValue::Rational { value }) => {
                format!("gain:{}/{}", value.numerator, value.denominator)
            }
            EventField::Attack(FieldValue::Rational { value }) => {
                format!("attack:{}/{}", value.numerator, value.denominator)
            }
            EventField::Transpose(FieldValue::Rational { value }) => {
                format!("transpose:{}/{}", value.numerator, value.denominator)
            }
            EventField::Elongate(FieldValue::Rational { value }) => {
                format!("elongate:{}/{}", value.numerator, value.denominator)
            }
            EventField::Replicate(FieldValue::Rational { value }) => {
                format!("replicate:{}/{}", value.numerator, value.denominator)
            }
            EventField::Degrade(FieldValue::Rational { value }) => {
                format!("degrade:{}/{}", value.numerator, value.denominator)
            }
            EventField::RandomChoice => "random_choice".to_string(),
        })
        .collect::<Vec<_>>()
        .join("|");
    format!(
        "{}@{}/{}+{}/{}#{}",
        value,
        event.span.start.0.numerator,
        event.span.start.0.denominator,
        event.span.duration.0.numerator,
        event.span.duration.0.denominator,
        fields
    )
}

fn apply_merge_policy(control: &FlowControlNode, streams: Vec<PatternStream>) -> PatternStream {
    match control.policy {
        FlowControlPolicy::MergeAppend => streams
            .into_iter()
            .fold(PatternStream::default(), PatternStream::chain),
        FlowControlPolicy::MergeInterleave => sort_stream_events(PatternStream::layer(streams)),
        FlowControlPolicy::MergePriority => merge_priority_streams(streams),
        FlowControlPolicy::MergeDeduplicate => deduplicate_stream(PatternStream::layer(streams)),
        _ => PatternStream::layer(streams),
    }
}

fn apply_mix_policy(
    control: &FlowControlNode,
    streams: Vec<PatternStream>,
    amount: Option<PatternStream>,
) -> PatternStream {
    let amount_value = scalar_stream_value(amount.as_ref()).unwrap_or_else(Rational::one);
    match control.policy {
        FlowControlPolicy::MixFieldBlend => blend_fields(PatternStream::layer(streams)),
        FlowControlPolicy::MixWeighted => {
            add_gain_field(PatternStream::layer(streams), amount_value)
        }
        FlowControlPolicy::MixGainAverage => {
            let layered = PatternStream::layer(streams);
            add_gain_field(
                layered.clone(),
                average_gain_from_stream(&layered).unwrap_or(amount_value),
            )
        }
        _ => PatternStream::layer(streams),
    }
}

fn apply_split_policy(
    control: &FlowControlNode,
    stream: PatternStream,
    member: &PortMemberId,
) -> PatternStream {
    match &control.policy {
        FlowControlPolicy::SplitCopyToAll => stream,
        FlowControlPolicy::SplitByIndexModulo => {
            split_stream(stream, member.0 == "even" || member.0.ends_with('0'))
        }
        FlowControlPolicy::SplitByEventField => split_by_event_field(stream, &member.0),
        FlowControlPolicy::SplitByPitchRange { threshold_octave } => {
            split_by_pitch(stream, &member.0, *threshold_octave)
        }
        _ => split_stream(stream, member.0 == "even" || member.0.ends_with('0')),
    }
}

fn apply_mask_policy(
    control: &FlowControlNode,
    main: PatternStream,
    mask: PatternStream,
) -> PatternStream {
    match control.policy {
        FlowControlPolicy::MaskGate => gate_by_mask(main, &mask, false),
        FlowControlPolicy::MaskScale => scale_by_mask(main, &mask),
        FlowControlPolicy::MaskClip => clip_by_mask(main, &mask),
        FlowControlPolicy::MaskInvertGate => gate_by_mask(main, &mask, true),
        _ => gate_by_mask(main, &mask, false),
    }
}

fn apply_switch_policy(
    control: &FlowControlNode,
    candidates: Vec<PatternStream>,
    control_stream: Option<PatternStream>,
) -> PatternStream {
    match control.policy {
        FlowControlPolicy::SwitchCycleIndex => select_indexed_stream(candidates, 0),
        FlowControlPolicy::SwitchControlValue => {
            let index = scalar_stream_value(control_stream.as_ref())
                .map(rational_to_usize)
                .unwrap_or(0);
            select_indexed_stream(candidates, index)
        }
        FlowControlPolicy::SwitchSeededRandom => {
            let seed = candidates
                .iter()
                .flat_map(|stream| &stream.events)
                .fold(0u64, |acc, event| acc ^ deterministic_event_seed(event));
            select_indexed_stream(candidates, seed as usize)
        }
        _ => select_indexed_stream(candidates, 0),
    }
}

fn apply_route_policy(
    control: &FlowControlNode,
    main: PatternStream,
    control_stream: Option<PatternStream>,
    member: &PortMemberId,
) -> PatternStream {
    match control.policy {
        FlowControlPolicy::RouteByIndexModulo => {
            split_stream(main, member.0 == "left" || member.0 == "even")
        }
        FlowControlPolicy::RouteByControlValue => {
            route_by_control_value(main, control_stream.as_ref(), &member.0)
        }
        FlowControlPolicy::RouteByLabel => route_by_label(main, &member.0),
        FlowControlPolicy::RouteByEventField => route_by_event_field(main, &member.0),
        _ => split_stream(main, member.0 == "left" || member.0 == "even"),
    }
}

fn apply_choice_policy(
    control: &FlowControlNode,
    options: Vec<PatternStream>,
    control_stream: Option<PatternStream>,
) -> PatternStream {
    match control.policy {
        FlowControlPolicy::ChoiceCycle => select_indexed_stream(options, 0),
        FlowControlPolicy::ChoiceSeededRandom => {
            let seed = options
                .iter()
                .flat_map(|stream| &stream.events)
                .fold(0u64, |acc, event| acc ^ deterministic_event_seed(event));
            select_indexed_stream(options, seed as usize)
        }
        FlowControlPolicy::ChoiceWeighted => {
            let index = weighted_index(
                options.len(),
                scalar_stream_value(control_stream.as_ref()).unwrap_or_else(Rational::one),
            );
            select_indexed_stream(options, index)
        }
        _ => select_indexed_stream(options, 0),
    }
}

fn split_stream(stream: PatternStream, keep_even: bool) -> PatternStream {
    PatternStream {
        events: stream
            .events
            .into_iter()
            .enumerate()
            .filter_map(|(index, event)| {
                let keep = if keep_even {
                    index % 2 == 0
                } else {
                    index % 2 == 1
                };
                keep.then_some(event)
            })
            .collect(),
    }
}
