use super::closed_axes::{closed_axis_semantic_authority, ClosedAxesAccumulator};
use super::directives::{request_directive, HoldDirective, RequestDirective};
use super::lexical::strip_repeated_prefixes;
use super::patterns::{
    ENGLISH_METALINGUISTIC_PREDICATES, ENGLISH_POLITE_BUILD_PREFIXES, ENGLISH_REQUEST_WRAPPERS,
};
use super::{
    unquoted_grounding_text, GroundedRequestControls, GroundedSemanticUnit, IntentRequestModeV2,
    UnquotedGroundingLink,
};
use crate::turn::intent_metalinguistic_scope::{
    analyzes_metalinguistic_copy, ends_metalinguistic_copy, english_ui_owner_before,
    first_copy_carrier_index, korean_ui_content_start, metalinguistic_carrier,
};

pub(super) fn grounded_request_controls(human: &str) -> GroundedRequestControls {
    let Some(grounding) = unquoted_grounding_text(human) else {
        return GroundedRequestControls {
            mode: None,
            preview: None,
            active_semantic_units: None,
            closed_axes: ClosedAxesAccumulator::default().finish(),
        };
    };
    let mut mode = None;
    let mut active_build_targets = Vec::new();
    let mut preview = None;
    let mut copied_block = false;
    let mut active_semantic_units = Vec::new();
    let mut closed_axes = ClosedAxesAccumulator::default();
    for sentence in &grounding.sentences {
        let mut question_build_scope = false;
        let mut non_authoritative_scope = false;
        let mut conditional_scope = false;
        let mut runtime_negation_scope = false;
        let mut ui_copy_scope = false;
        for (index, unit) in sentence.iter().enumerate() {
            if copied_block {
                closed_axes.break_ephemeral_scope();
                non_authoritative_scope = false;
                conditional_scope = false;
                runtime_negation_scope = false;
                question_build_scope = false;
                if analyzes_metalinguistic_copy(&unit.text) {
                    mode = Some(IntentRequestModeV2::Discussion);
                    active_build_targets.clear();
                    continue;
                }
                if ends_metalinguistic_copy(&unit.text) {
                    copied_block = false;
                }
                continue;
            }
            if metalinguistic_carrier(&unit.text) {
                closed_axes.break_ephemeral_scope();
                copied_block = true;
                non_authoritative_scope = false;
                conditional_scope = false;
                runtime_negation_scope = false;
                question_build_scope = false;
                continue;
            }
            if ui_copy_scope {
                closed_axes.break_ephemeral_scope();
                active_semantic_units.push(GroundedSemanticUnit {
                    text: unit.text.clone(),
                    authoritative: false,
                    link: unit.link,
                    operative_antecedent: false,
                });
                continue;
            }
            let continuation = sentence
                .get(index.saturating_add(1))
                .map(|unit| unit.text.as_str());
            let alternative_continuation = sentence
                .get(index.saturating_add(1))
                .filter(|unit| unit.link == UnquotedGroundingLink::Alternative)
                .map(|unit| unit.text.as_str());
            if unit.link == UnquotedGroundingLink::Detached {
                non_authoritative_scope = false;
                conditional_scope = false;
            }
            let operative_consequent = unit.operative_authority == Some(true);
            if operative_consequent {
                non_authoritative_scope = false;
                conditional_scope = false;
                question_build_scope = false;
            }
            let directive = request_directive(&unit.text, continuation);
            let directive_is_build = matches!(&directive, Some(RequestDirective::Build(_)));
            let directive_missing = directive.is_none();
            let scope_break = operative_consequent
                || has_authoritative_scope_wrapper(&unit.text)
                || (!conditional_scope
                    && (breaks_non_authoritative_scope(&unit.text, directive_is_build)
                        || starts_positive_runtime_scope(&unit.text)));
            let directive_authoritative = !non_authoritative_scope || scope_break;
            if let Some(directive) = directive.filter(|_| directive_authoritative) {
                match directive {
                    RequestDirective::Build(build) => {
                        mode = Some(IntentRequestModeV2::Build);
                        active_build_targets = build.targets;
                    }
                    RequestDirective::Discussion => {
                        mode = Some(IntentRequestModeV2::Discussion);
                        active_build_targets.clear();
                    }
                    RequestDirective::Hold(hold) => {
                        let withdraws_active_build = mode == Some(IntentRequestModeV2::Build)
                            && match hold {
                                HoldDirective::Global => true,
                                HoldDirective::Target(target) => {
                                    active_build_targets.contains(&target)
                                }
                                HoldDirective::Scoped => false,
                            };
                        if mode != Some(IntentRequestModeV2::Build) || withdraws_active_build {
                            mode = Some(IntentRequestModeV2::Discussion);
                            active_build_targets.clear();
                        }
                    }
                }
            }
            if operative_consequent && directive_missing {
                mode = Some(IntentRequestModeV2::Build);
                active_build_targets.clear();
            }
            if directive_authoritative {
                if let Some(preference) = preview_directive(&unit.text) {
                    preview = Some(preference);
                }
            }
            let unit_non_authoritative = non_authoritative_semantic_unit(&unit.text);
            if scope_break {
                non_authoritative_scope = false;
                conditional_scope = false;
            }
            if unit_non_authoritative {
                non_authoritative_scope = true;
                conditional_scope = conditional_non_authoritative_semantic_unit(&unit.text);
            }
            if unit.question && directive_is_build && directive_authoritative {
                question_build_scope = true;
            }
            let authoritative = unit.operative_authority.unwrap_or({
                !unit.question
                    || (directive_is_build && directive_authoritative)
                    || question_build_scope
            });
            let active = active_semantic_unit(&unit.text);
            let authoritative = authoritative && !non_authoritative_scope && active.is_some();
            let mut text = active.unwrap_or_else(|| unit.text.clone());
            if authoritative {
                closed_axes.observe(&text, unit.link, continuation, alternative_continuation);
                if unit.link == UnquotedGroundingLink::Detached
                    || starts_positive_runtime_scope(&text)
                {
                    runtime_negation_scope = false;
                }
                let distributes_negation = distributes_runtime_negation(&text);
                if distributes_negation {
                    runtime_negation_scope = true;
                }
                if runtime_negation_scope
                    && unit.link != UnquotedGroundingLink::Detached
                    && !distributes_negation
                {
                    text = format!("do not use {text}");
                }
            } else {
                closed_axes.break_ephemeral_scope();
                runtime_negation_scope = false;
                question_build_scope = false;
            }
            active_semantic_units.push(GroundedSemanticUnit {
                text,
                authoritative,
                link: unit.link,
                operative_antecedent: unit.operative_authority == Some(false),
            });
            if opens_ui_copy_scope(&unit.text) {
                ui_copy_scope = true;
            }
        }
        closed_axes.end_semantic_sentence();
    }
    GroundedRequestControls {
        mode,
        preview,
        active_semantic_units: Some(active_semantic_units),
        closed_axes: closed_axes.finish(),
    }
}

fn preview_directive(unit: &str) -> Option<bool> {
    let copy_boundary = first_copy_carrier_index(unit).unwrap_or(unit.len());
    let negative = [
        "do not prepare a preview",
        "do not preview",
        "don't prepare a preview",
        "don't preview",
        "don’t prepare a preview",
        "don’t preview",
        "without a preview",
        "without preview",
        "미리보기 없이",
        "미리보기하지 마",
        "미리보기하지마",
    ]
    .iter()
    .flat_map(|marker| {
        unit.match_indices(marker)
            .filter_map(|(position, matched)| {
                let end = position.saturating_add(matched.len());
                (!non_direct_preview_tail(&unit[end..])).then_some(position)
            })
    })
    .filter(|position| *position < copy_boundary)
    .max()
    .or_else(|| {
        let position = unit.len().saturating_sub("no preview".len());
        (position < copy_boundary && (unit == "no preview" || unit.ends_with(" no preview")))
            .then_some(position)
    });
    let positive = [
        "prepare a validated preview",
        "prepare its validated preview",
        "prepare the validated preview",
        "produce a validated preview",
        "produce its validated preview",
        "produce the validated preview",
        "show a validated preview",
        "show its validated preview",
        "show the validated preview",
        "validate and preview",
        "prepare a preview",
        "prepare its preview",
        "prepare the preview",
        "preview the draft",
        "preview the design",
        "검증된 미리보기를 생성",
        "검증된 미리보기를 준비",
        "검증된 미리보기를 보여",
        "검증된 미리보기까지 준비",
        "검증하고 미리보기",
        "미리보기를 준비",
        "미리보기까지 준비",
    ]
    .iter()
    .flat_map(|marker| {
        unit.match_indices(marker)
            .filter_map(|(position, matched)| {
                let end = position.saturating_add(matched.len());
                (!non_direct_preview_tail(&unit[end..])).then_some(position)
            })
    })
    .filter(|position| *position < copy_boundary)
    .max();
    match (negative, positive) {
        (Some(negative), Some(positive)) => Some(positive > negative),
        (Some(_), None) => Some(false),
        (None, Some(_)) => Some(true),
        (None, None) => None,
    }
}

fn non_direct_preview_tail(tail: &str) -> bool {
    let tail = tail.trim_start();
    [
        "as ",
        "capabilities",
        "capability",
        "called",
        "failure",
        "failures",
        "label",
        "mode",
        "modes",
        "named",
        "state",
        "states",
        "support",
        "systems",
        "기능",
        "라고",
        "라는",
        "라벨",
        "란",
        "모드",
        "문구",
        "로 부르",
        "로 표시",
        "상태",
        "실패",
        "오류",
        "작동",
        "지원",
    ]
    .iter()
    .any(|prefix| tail.starts_with(prefix))
}

fn active_semantic_unit(unit: &str) -> Option<String> {
    if non_authoritative_semantic_unit(unit) {
        return None;
    }
    if let Some(start) = korean_ui_content_start(unit) {
        let active = unit.get(start..)?.trim();
        return (!active.is_empty()).then(|| active.to_string());
    }
    let end = first_copy_carrier_index(unit).unwrap_or(unit.len());
    let end = first_non_executable_content_index(unit).map_or(end, |content| content.min(end));
    let active = unit.get(..end)?.trim();
    (!active.is_empty()).then(|| active.to_string())
}

fn first_non_executable_content_index(unit: &str) -> Option<usize> {
    [
        " to describe how to ",
        " to document how to ",
        " to explain how to ",
    ]
    .iter()
    .filter_map(|marker| unit.find(marker))
    .min()
}

fn opens_ui_copy_scope(unit: &str) -> bool {
    english_ui_owner_before(unit, unit.len())
        && ["ask", "asks", "explaining", "says"]
            .iter()
            .any(|suffix| unit.ends_with(suffix))
}

fn breaks_non_authoritative_scope(unit: &str, directive_is_build: bool) -> bool {
    let authoritative_wrapper = has_authoritative_scope_wrapper(unit);
    let unit = strip_repeated_prefixes(unit, ENGLISH_REQUEST_WRAPPERS);
    authoritative_wrapper
        || (directive_is_build
            && ENGLISH_POLITE_BUILD_PREFIXES
                .iter()
                .any(|prefix| unit.starts_with(prefix)))
}

fn has_authoritative_scope_wrapper(unit: &str) -> bool {
    let mut value = unit;
    let mut authoritative = false;
    loop {
        if let Some(tail) = [
            "actually, ",
            "actually ",
            "correction: ",
            "correction ",
            "definitely ",
            "instead ",
            "no, ",
            "no ",
            "now, ",
            "now ",
            "아니, ",
            "아니 ",
            "반드시 ",
            "이제 ",
            "정정: ",
            "정정 ",
        ]
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
        {
            authoritative = true;
            value = tail;
            continue;
        }
        if let Some(tail) = ["please, ", "please "]
            .iter()
            .find_map(|prefix| value.strip_prefix(prefix))
        {
            value = tail;
            continue;
        }
        return authoritative;
    }
}

fn non_authoritative_semantic_unit(unit: &str) -> bool {
    let semantic_unit = strip_repeated_prefixes(
        unit,
        &[
            "actually ",
            "actually, ",
            "correction ",
            "correction: ",
            "definitely ",
            "instead ",
            "no ",
            "no, ",
            "now ",
            "now, ",
            "please ",
            "please, ",
            "아니 ",
            "아니, ",
            "반드시 ",
            "이제 ",
            "정정 ",
            "정정: ",
        ],
    );
    let explicit_scope = ENGLISH_METALINGUISTIC_PREDICATES
        .iter()
        .any(|predicate| semantic_unit.contains(predicate))
        || [
            "if ",
            "if we built",
            "if we build",
            "if we were to",
            "imagine ",
            "suppose ",
            "hypothetically ",
            "maybe ",
            "perhaps ",
            "potentially ",
            "optionally ",
            "you may ",
            "when available",
            "what if ",
            "for example ",
            "are ",
            "is ",
            "does ",
            "can we ",
            "could we ",
            "we could ",
            "we may ",
            "we might ",
            "do we need ",
            "should we ",
            "would we ",
            "만약 ",
            "가정하면 ",
            "예를 들어 ",
            "선택적으로 ",
            "가능하면 ",
        ]
        .iter()
        .any(|prefix| semantic_unit.starts_with(prefix))
        || [
            "사용할까",
            "쓸까",
            "필요할까",
            "어떨까",
            "사용해도 돼",
            "사용해도 됩니다",
            "써도 돼",
            "써도 됩니다",
        ]
        .iter()
        .any(|suffix| semantic_unit.ends_with(suffix));
    if explicit_scope {
        return true;
    }
    let english_optional_expression = [
        " could be useful",
        " could be used",
        " may need ",
        " may use ",
        " may be useful",
        " may be used",
        " might need ",
        " might use ",
        " might be useful",
        " might be required",
        " can be used",
        " may be required",
    ]
    .iter()
    .any(|marker| semantic_unit.contains(marker));
    let korean_optional_expression = [
        "쓸 수도",
        "쓸 수도 있지만",
        "사용할 수도",
        "사용할 수도 있지만",
        "필요할 수도",
        "필요할 수도 있지만",
    ]
    .iter()
    .any(|marker| semantic_unit.contains(marker));
    korean_optional_expression
        || metalinguistic_runtime_comparison(semantic_unit)
        || (english_optional_expression && !closed_axis_semantic_authority(semantic_unit))
}

fn conditional_non_authoritative_semantic_unit(unit: &str) -> bool {
    let unit = strip_repeated_prefixes(
        unit,
        &[
            "actually ",
            "actually, ",
            "correction ",
            "correction: ",
            "definitely ",
            "instead ",
            "no ",
            "no, ",
            "now ",
            "now, ",
            "please ",
            "please, ",
            "아니 ",
            "아니, ",
            "반드시 ",
            "이제 ",
            "정정 ",
            "정정: ",
        ],
    );
    [
        "if ",
        "if we built",
        "if we build",
        "if we were to",
        "when available",
        "만약 ",
        "가능하면 ",
        "가정하면 ",
    ]
    .iter()
    .any(|prefix| unit.starts_with(prefix))
}

fn metalinguistic_runtime_comparison(unit: &str) -> bool {
    let english = [
        "compare ",
        "consider ",
        "discuss ",
        "brainstorm ",
        "explain ",
    ]
    .iter()
    .any(|prefix| unit.starts_with(prefix));
    let korean = [
        "고려",
        "고려해줘",
        "고려해 줘",
        "고려하자",
        "논의",
        "논의해줘",
        "논의해 줘",
        "논의하자",
        "비교",
        "비교해줘",
        "비교해 줘",
        "비교하자",
        "설명",
        "설명해줘",
        "설명해 줘",
        "설명하자",
    ]
    .iter()
    .any(|suffix| unit.ends_with(suffix));
    let runtime_subject = [
        "durable timer",
        "persistent timer",
        "persistent state",
        "restart persistence",
        "persistent economy",
        "event-time llm",
        "event time llm",
        "영속 타이머",
        "내구성 타이머",
        "영속 상태",
        "재시작 영속성",
        "영속 경제",
        "이벤트 시점 llm",
    ]
    .iter()
    .any(|subject| unit.contains(subject));
    (english || korean) && runtime_subject
}

fn distributes_runtime_negation(unit: &str) -> bool {
    let direct_unit = strip_repeated_prefixes(unit, ENGLISH_REQUEST_WRAPPERS);
    let direct = [
        "do not add ",
        "do not enable ",
        "do not include ",
        "do not require ",
        "do not use ",
        "don't add ",
        "don't enable ",
        "don't include ",
        "don't require ",
        "don't use ",
        "don’t add ",
        "don’t enable ",
        "don’t include ",
        "don’t require ",
        "don’t use ",
        "avoid ",
        "avoid using ",
        "disable ",
        "exclude ",
        "omit ",
        "remove ",
        "must not add ",
        "must not enable ",
        "must not include ",
        "must not require ",
        "must not use ",
        "cannot use ",
        "can't use ",
        "not use ",
        "without using ",
        "never add ",
        "never enable ",
        "never include ",
        "never require ",
        "never use ",
    ]
    .iter()
    .any(|marker| direct_unit.starts_with(marker));
    let targeted = [
        "no persistent state",
        "no restart persistence",
        "no durable timer",
        "no persistent timer",
        "no persistent economy",
        "no event-time llm",
        "without persistent state",
        "without restart persistence",
        "without durable timer",
        "without persistent timer",
        "without a persistent economy",
        "without persistent economy",
        "without an llm at event time",
        "without llm at event time",
        "영속 상태 없이",
        "재시작 영속성 없이",
        "영속 타이머 없이",
        "내구성 타이머 없이",
        "영속 경제 없이",
        "이벤트 시점 llm 없이",
        "이벤트 시점 언어 모델 없이",
        "이벤트 시점 인공지능 없이",
    ]
    .iter()
    .any(|marker| unit.contains(marker));
    let korean_targeted = [
        "영속 상태",
        "재시작 영속성",
        "영속 타이머",
        "내구성 타이머",
        "영속 경제",
        "이벤트 시점 llm",
        "이벤트 시점 언어 모델",
        "이벤트 시점 인공지능",
    ]
    .iter()
    .any(|target| unit.contains(target))
        && [
            "사용하지 마",
            "사용하지마",
            "쓰지 마",
            "쓰지마",
            "추가하지 마",
            "추가하지마",
            "포함하지 마",
            "포함하지마",
            "필요 없",
        ]
        .iter()
        .any(|marker| unit.contains(marker));
    direct || targeted || korean_targeted
}

fn starts_positive_runtime_scope(unit: &str) -> bool {
    let unit = strip_repeated_prefixes(
        unit,
        &[
            "actually ",
            "actually, ",
            "definitely ",
            "instead ",
            "now ",
            "now, ",
            "please ",
            "please, ",
            "반드시 ",
            "이제 ",
        ],
    );
    let action_first = [
        "add ",
        "enable ",
        "include ",
        "keep ",
        "make ",
        "persist ",
        "preserve ",
        "require ",
        "restore ",
        "retain ",
        "run ",
        "call ",
        "store ",
        "use ",
        "추가",
        "포함",
        "사용",
        "유지",
        "보존",
        "복구",
        "실행",
        "호출",
    ]
    .iter()
    .any(|prefix| unit.starts_with(prefix));
    let subject_first = [
        "timer must be durable",
        "timers must be durable",
        "timer must be persistent",
        "timers must be persistent",
        "timer must survive",
        "timers must survive",
        "state must persist",
        "state must survive",
        "economy must persist",
        "xp must persist",
        "durable timer is required",
        "durable timers are required",
        "persistent economy is required",
        "영속 타이머를 사용",
        "영속 타이머를 추가",
        "영속 경제를 사용",
        "영속 경제를 추가",
        "영속 상태를 사용",
    ]
    .iter()
    .any(|pattern| unit.starts_with(pattern));
    (action_first || subject_first) && !distributes_runtime_negation(unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_alternative_branches_remain_authoritative() {
        let controls = grounded_request_controls(
            "Build a managed private study-room automation. Use English or 한국어 기본 문구하고 이름을 사용해.",
        );
        let units = controls.active_semantic_units.unwrap();
        let locale_units = units
            .iter()
            .filter(|unit| unit.text.contains("english") || unit.text.contains("한국어"))
            .collect::<Vec<_>>();
        assert_eq!(locale_units.len(), 2);
        assert_eq!(locale_units[0].text, "use english");
        assert_eq!(locale_units[1].text, "한국어 기본 문구");
        assert!(locale_units.iter().all(|unit| unit.authoritative));
        assert!(matches!(
            locale_units[1].link,
            UnquotedGroundingLink::Alternative
        ));
        assert_eq!(
            controls.closed_axes.locale,
            Err(super::super::closed_axes::ClosedAxisGroundingError::AmbiguousLocale)
        );
    }
}
