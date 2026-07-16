use super::matching::{
    action_before_llm_near, has_any, has_runtime_llm_marker, llm_before_action_near,
    llm_before_setup_execution, ordered_near, requirement_action_owns,
};
use super::patterns::*;
use super::RuntimeRequirementAxis;

pub(super) fn bare_requirement_rejection(text: &str) -> bool {
    matches!(
        text.trim(),
        "not needed"
            | "not required"
            | "unnecessary"
            | "optional"
            | "they are not needed"
            | "they are not required"
            | "they are optional"
            | "필요 없어"
            | "필요 없습니다"
            | "선택 사항이야"
            | "선택 사항입니다"
    )
}

pub(super) fn positive_runtime_axes(text: &str) -> Vec<RuntimeRequirementAxis> {
    let mut axes = Vec::new();
    if persistence_required(text) && !persistence_rejected(text) {
        axes.push(RuntimeRequirementAxis::Persistence);
    }
    if durable_timer_required(text) && !durable_timer_rejected(text) {
        axes.push(RuntimeRequirementAxis::Timers);
    }
    if persistent_economy_required(text) && !persistent_economy_rejected(text) {
        axes.push(RuntimeRequirementAxis::Economy);
    }
    if event_time_llm_required(text, false) && !event_time_llm_rejected(text, false) {
        axes.push(RuntimeRequirementAxis::EventTimeLlm);
    }
    axes
}

pub(super) fn rejected_runtime_axes(
    text: &str,
    inherited_event_scope: bool,
) -> Vec<RuntimeRequirementAxis> {
    let mut axes = Vec::new();
    if persistence_rejected(text) {
        axes.push(RuntimeRequirementAxis::Persistence);
    }
    if durable_timer_rejected(text) {
        axes.push(RuntimeRequirementAxis::Timers);
    }
    if persistent_economy_rejected(text) {
        axes.push(RuntimeRequirementAxis::Economy);
    }
    if event_time_llm_rejected(text, inherited_event_scope) {
        axes.push(RuntimeRequirementAxis::EventTimeLlm);
    }
    axes
}

pub(super) fn runtime_axis_mentions(text: &str) -> Vec<RuntimeRequirementAxis> {
    let mut axes = Vec::new();
    if has_any(text, PERSISTENT_STATE_MARKERS) || has_any(text, PERSISTENCE_DIRECT) {
        axes.push(RuntimeRequirementAxis::Persistence);
    }
    if has_any(text, DURABLE_TIMER_PATTERNS) {
        axes.push(RuntimeRequirementAxis::Timers);
    }
    if has_any(text, ECONOMY_PERSISTENCE) {
        axes.push(RuntimeRequirementAxis::Economy);
    }
    if has_any(text, EVENT_TIME_MARKERS)
        && has_runtime_llm_marker(text)
        && llm_runtime_action_required(text)
    {
        axes.push(RuntimeRequirementAxis::EventTimeLlm);
    }
    axes
}

pub(super) fn unrejected_runtime_mentions(text: &str) -> Vec<RuntimeRequirementAxis> {
    runtime_axis_mentions(text)
        .into_iter()
        .filter(|axis| match axis {
            RuntimeRequirementAxis::Persistence => !persistence_rejected(text),
            RuntimeRequirementAxis::Timers => !durable_timer_rejected(text),
            RuntimeRequirementAxis::Economy => !persistent_economy_rejected(text),
            RuntimeRequirementAxis::EventTimeLlm => !event_time_llm_rejected(text, false),
        })
        .collect()
}

pub(super) fn runtime_axis_order(axis: &RuntimeRequirementAxis) -> u8 {
    match axis {
        RuntimeRequirementAxis::Persistence => 0,
        RuntimeRequirementAxis::Timers => 1,
        RuntimeRequirementAxis::Economy => 2,
        RuntimeRequirementAxis::EventTimeLlm => 3,
    }
}

pub(super) fn persistence_required(text: &str) -> bool {
    has_any(text, PERSISTENCE_DIRECT)
        || requirement_action_owns(text, PERSISTENT_STATE_MARKERS, 8)
        || ordered_near(
            text,
            PERSISTENT_STATE_MARKERS,
            POSITIVE_REQUIREMENT_SUFFIXES,
            8,
        )
        || (has_any(text, RESTART_MARKERS)
            && (ordered_near(text, POSITIVE_PERSISTENCE_ACTIONS, PERSISTENCE_SUBJECTS, 8)
                || ordered_near(text, PERSISTENCE_SUBJECTS, POSITIVE_PERSISTENCE_ACTIONS, 8)))
}

pub(super) fn durable_timer_required(text: &str) -> bool {
    has_any(text, DURABLE_TIMER_ASSERTIONS)
        || requirement_action_owns(text, DURABLE_TIMER_MARKERS, 8)
        || ordered_near(
            text,
            DURABLE_TIMER_MARKERS,
            POSITIVE_REQUIREMENT_SUFFIXES,
            8,
        )
}

pub(super) fn persistent_economy_required(text: &str) -> bool {
    has_any(text, ECONOMY_PERSISTENCE_ASSERTIONS)
        || requirement_action_owns(text, ECONOMY_PERSISTENCE_MARKERS, 8)
        || ordered_near(
            text,
            ECONOMY_PERSISTENCE_MARKERS,
            POSITIVE_REQUIREMENT_SUFFIXES,
            8,
        )
}

pub(super) fn event_time_llm_required(text: &str, inherited_event_scope: bool) -> bool {
    (has_any(text, EVENT_TIME_MARKERS) || (inherited_event_scope && !event_context_barrier(text)))
        && event_scoped_llm_action_required(text)
}

pub(super) fn event_scoped_llm_action_required(text: &str) -> bool {
    !setup_scoped_llm_action(text) && llm_runtime_action_required(text)
}

pub(super) fn event_context_barrier(text: &str) -> bool {
    setup_scoped_llm_action(text)
        || has_any(text, SETUP_CONTEXT_BARRIERS)
        || [
            "during setup ",
            "during setup only ",
            "only during setup ",
            "at setup time ",
            "during design ",
            "only during design ",
            "at design time ",
            "during compilation ",
            "during initialization ",
            "only during compilation ",
            "at compile time ",
            "at initialization time ",
            "before deployment ",
            "only before deployment ",
            "before launch ",
            "for setup ",
            "in the setup phase ",
            "once during initialization ",
            "while setting up ",
            "설정할 때 ",
            "설정 단계",
            "초기 설정 때",
            "설정 시점에 ",
            "설계할 때 ",
            "설계 시점에 ",
            "컴파일할 때 ",
            "컴파일 시점에 ",
            "배포 전에 ",
        ]
        .iter()
        .any(|prefix| text.starts_with(prefix))
}

fn setup_scoped_llm_action(text: &str) -> bool {
    has_any(text, SETUP_ONLY_LLM_CONTEXTS) || llm_before_setup_execution(text, 16)
}

fn llm_runtime_action_required(text: &str) -> bool {
    has_runtime_llm_marker(text)
        && (llm_before_action_near(text, LLM_ACTIONS, 8)
            || action_before_llm_near(text, LLM_PASSIVE_ACTIONS, 8)
            || has_any(
                text,
                &[
                    "call an llm",
                    "call the llm",
                    "calls an llm",
                    "calls the llm",
                    "run an llm",
                    "run the llm",
                    "runs an llm",
                    "runs the llm",
                    "invoke an llm",
                    "invoke the llm",
                    "invokes an llm",
                    "invokes the llm",
                    "llm gets called",
                    "llm is called",
                    "the llm gets called",
                    "the llm is called",
                    "use an llm to",
                    "use the llm to",
                    "uses an llm to",
                    "uses the llm to",
                    "ask an llm to",
                    "ask the llm to",
                    "asks an llm to",
                    "asks the llm to",
                    "llm을 호출",
                    "llm을 실행",
                    "언어 모델을 호출",
                    "언어 모델을 실행",
                    "ai를 호출",
                    "ai를 실행",
                ],
            ))
}

pub(super) fn persistence_rejected(text: &str) -> bool {
    has_any(text, PERSISTENCE_NEGATIONS)
        || (ordered_near(text, NEGATING_ACTIONS, PERSISTENCE_SUBJECTS, 8)
            && (has_any(text, RESTART_MARKERS) || has_any(text, SURVIVAL_MARKERS)))
        || (ordered_near(text, PERSISTENCE_SUBJECTS, NEGATED_PERSISTENCE_ACTIONS, 8)
            && has_any(text, RESTART_MARKERS))
        || (ordered_near(
            text,
            PERSISTENCE_SUBJECTS,
            NEGATED_REQUIREMENT_USE_ACTIONS,
            8,
        ) && (has_any(text, RESTART_MARKERS) || has_any(text, PERSISTENT_STATE_MARKERS)))
}

pub(super) fn durable_timer_rejected(text: &str) -> bool {
    let non_runtime_surface = has_any(text, NON_RUNTIME_TIMER_SURFACES);
    has_any(text, TIMER_NEGATIONS)
        || ordered_near(text, NEGATING_ACTIONS, DURABLE_TIMER_MARKERS, 8)
        || (!non_runtime_surface
            && (ordered_near(text, NEGATING_ACTIONS, TIMER_MARKERS, 8)
                || ordered_near(text, TIMER_MARKERS, NEGATED_REQUIREMENT_USE_ACTIONS, 8)
                || ordered_near(text, TIMER_MARKERS, NEGATED_PERSISTENCE_ACTIONS, 8)))
}

pub(super) fn persistent_economy_rejected(text: &str) -> bool {
    has_any(text, ECONOMY_NEGATIONS)
        || ordered_near(text, NEGATING_ACTIONS, ECONOMY_PERSISTENCE, 8)
        || ordered_near(
            text,
            ECONOMY_STORAGE_NEGATING_ACTIONS,
            ECONOMY_STORAGE_SUBJECTS,
            8,
        )
        || ordered_near(
            text,
            ECONOMY_PERSISTENCE,
            NEGATED_REQUIREMENT_USE_ACTIONS,
            8,
        )
        || ordered_near(
            text,
            ECONOMY_STORAGE_SUBJECTS,
            NEGATED_PERSISTENCE_ACTIONS,
            8,
        )
}

pub(super) fn event_time_llm_rejected(text: &str, inherited_event_scope: bool) -> bool {
    let event_scope = has_any(text, EVENT_TIME_MARKERS)
        || (inherited_event_scope && !event_context_barrier(text));
    let runtime_llm = has_runtime_llm_marker(text);
    (runtime_llm && has_any(text, DIRECT_EVENT_LLM_NEGATIONS))
        || (event_scope
            && ((runtime_llm && has_any(text, EVENT_LLM_NEGATIONS))
                || action_before_llm_near(text, NEGATING_ACTIONS, 8)
                || llm_before_action_near(text, NEGATED_LLM_ACTIONS, 8)))
}
