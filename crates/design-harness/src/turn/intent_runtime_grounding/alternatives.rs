use super::classification::{
    event_scoped_llm_action_required, event_time_llm_rejected, positive_runtime_axes,
    rejected_runtime_axes, runtime_axis_mentions, runtime_axis_order, unrejected_runtime_mentions,
};
use super::{RuntimeGroundingAmbiguity, RuntimeGroundingError, RuntimeRequirementAxis};

pub(super) fn validate_runtime_alternative(
    previous: &str,
    current: &str,
    inherited_event_scope: bool,
) -> Result<(), RuntimeGroundingError> {
    let mut previous_axes = positive_runtime_axes(previous);
    let mut current_axes = positive_runtime_axes(current);
    if inherited_event_scope
        && event_scoped_llm_action_required(previous)
        && !event_time_llm_rejected(previous, true)
    {
        previous_axes.push(RuntimeRequirementAxis::EventTimeLlm);
    }
    if inherited_event_scope
        && event_scoped_llm_action_required(current)
        && !event_time_llm_rejected(current, true)
    {
        current_axes.push(RuntimeRequirementAxis::EventTimeLlm);
    }
    if previous_axes.is_empty() {
        previous_axes = unrejected_runtime_mentions(previous);
    }
    if current_axes.is_empty() {
        current_axes = unrejected_runtime_mentions(current);
    }
    if previous_axes.is_empty() && current_axes.is_empty() {
        return Ok(());
    }
    let axis = previous_axes
        .iter()
        .chain(current_axes.iter())
        .copied()
        .min_by_key(runtime_axis_order);
    let Some(axis) = axis else {
        return Ok(());
    };
    Err(RuntimeGroundingError {
        axis,
        ambiguity: RuntimeGroundingAmbiguity::Alternative,
    })
}

pub(super) fn validate_inline_runtime_alternative(text: &str) -> Result<(), RuntimeGroundingError> {
    if let Some((requirement, _)) = text.split_once(" unless ") {
        return unresolved_alternative(positive_runtime_axes(requirement));
    }
    for marker in [
        "choose between ",
        "pick between ",
        "select one from ",
        "one of ",
    ] {
        if let Some((_, tail)) = text.split_once(marker) {
            if runtime_choice_starts(tail) {
                return unresolved_alternative(runtime_axis_mentions(tail));
            }
        }
    }
    if [
        " 중 하나",
        "중 하나",
        " 중 택일",
        "중 택일",
        "든 하나",
        "골라",
        "otherwise ",
    ]
    .iter()
    .any(|marker| text.contains(marker))
    {
        return unresolved_alternative(runtime_axis_mentions(text));
    }
    if let Some((previous, current)) = ["and/or", " xor ", " versus "]
        .iter()
        .find_map(|marker| text.split_once(marker))
    {
        return unresolved_alternative(
            runtime_axis_mentions(previous)
                .into_iter()
                .chain(runtime_axis_mentions(current))
                .collect(),
        );
    }
    if let Some((previous, current)) = [" 또는 ", " 혹은 ", " 아니면 ", "거나 "]
        .iter()
        .find_map(|marker| text.split_once(marker))
    {
        let axes = positive_runtime_axes(previous)
            .into_iter()
            .chain(positive_runtime_axes(current))
            .chain(positive_runtime_axes(text))
            .collect();
        return unresolved_alternative(axes);
    }
    if let Some((previous, current)) = ["이나 ", "나 "]
        .iter()
        .find_map(|marker| text.split_once(marker))
    {
        if runtime_particle_precedes(previous) {
            return unresolved_alternative(
                runtime_axis_mentions(previous)
                    .into_iter()
                    .chain(runtime_axis_mentions(current))
                    .chain(positive_runtime_axes(text))
                    .collect(),
            );
        }
    }
    Ok(())
}

pub(super) fn validate_inline_runtime_conflict(text: &str) -> Result<(), RuntimeGroundingError> {
    let rejected_axes = rejected_runtime_axes(text, false);
    for marker in [
        " without ",
        " but do not ",
        " but don't ",
        " but don’t ",
        " but never ",
    ] {
        let Some((positive, rejected)) = text.split_once(marker) else {
            continue;
        };
        if runtime_axis_mentions(rejected).is_empty() {
            continue;
        }
        if let Some(axis) = positive_runtime_axes(positive)
            .into_iter()
            .filter(|axis| rejected_axes.contains(axis))
            .min_by_key(runtime_axis_order)
        {
            return Err(RuntimeGroundingError {
                axis,
                ambiguity: RuntimeGroundingAmbiguity::Conflict,
            });
        }
    }
    Ok(())
}

fn runtime_choice_starts(tail: &str) -> bool {
    let tail = tail
        .trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, ':' | '-')
        })
        .strip_prefix("a ")
        .or_else(|| tail.trim_start().strip_prefix("an "))
        .or_else(|| tail.trim_start().strip_prefix("the "))
        .unwrap_or(tail.trim_start());
    [
        "durable scheduler",
        "durable timer",
        "persistent economy",
        "persistent scheduler",
        "persistent state",
        "persistent timer",
        "restart persistence",
        "내구성 타이머",
        "영속 경제",
        "영속 상태",
        "영속 타이머",
    ]
    .iter()
    .any(|marker| tail.starts_with(marker))
}

fn runtime_particle_precedes(previous: &str) -> bool {
    let previous = previous.trim_end();
    [
        "내구성 스케줄러",
        "내구성 타이머",
        "영속 경제",
        "영속 경험치",
        "영속 보상",
        "영속 상태",
        "영속 스케줄러",
        "영속 잔액",
        "영속 타이머",
        "재시작 영속성",
    ]
    .iter()
    .any(|marker| previous.ends_with(marker))
}

fn unresolved_alternative(axes: Vec<RuntimeRequirementAxis>) -> Result<(), RuntimeGroundingError> {
    let Some(axis) = axes.into_iter().min_by_key(runtime_axis_order) else {
        return Ok(());
    };
    Err(RuntimeGroundingError {
        axis,
        ambiguity: RuntimeGroundingAmbiguity::Alternative,
    })
}
