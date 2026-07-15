use super::intent_boundary_grounding::{unquoted_grounding_text, UnquotedGroundingLink};
use super::intent_interpretation::IntentRequestModeV2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestTarget {
    Automation,
    Bot,
    Button,
    Channel,
    Feature,
    Flow,
    Game,
    Modal,
    Panel,
    Role,
    Room,
    Rule,
    System,
    Workflow,
}

struct BuildDirective {
    targets: Vec<RequestTarget>,
}

enum HoldDirective {
    Global,
    Target(RequestTarget),
    Scoped,
}

enum RequestDirective {
    Build(BuildDirective),
    Discussion,
    Hold(HoldDirective),
}

const ENGLISH_BUILD_TARGETS: &[(&str, RequestTarget)] = &[
    ("automation", RequestTarget::Automation),
    ("bot", RequestTarget::Bot),
    ("button", RequestTarget::Button),
    ("channel", RequestTarget::Channel),
    ("feature", RequestTarget::Feature),
    ("flow", RequestTarget::Flow),
    ("game", RequestTarget::Game),
    ("modal", RequestTarget::Modal),
    ("panel", RequestTarget::Panel),
    ("role", RequestTarget::Role),
    ("room", RequestTarget::Room),
    ("rule", RequestTarget::Rule),
    ("rule set", RequestTarget::Rule),
    ("ruleset", RequestTarget::Rule),
    ("system", RequestTarget::System),
    ("workflow", RequestTarget::Workflow),
];

const ENGLISH_BUILD_PREFIXES: &[&str] = &[
    "add ",
    "build ",
    "configure ",
    "create ",
    "design ",
    "implement ",
    "make ",
    "set up ",
];

const ENGLISH_OBJECT_BARRIERS: &[&str] = &[
    " about ",
    " between ",
    " comparing ",
    " for ",
    " in ",
    " of ",
    " on ",
    " that ",
    " to ",
    " versus ",
    " vs ",
    " where ",
    " which ",
    " whose ",
    " with ",
    " using ",
];

const ENGLISH_POLITE_BUILD_PREFIXES: &[&str] =
    &["can you ", "could you ", "will you ", "would you "];

const ENGLISH_REQUEST_WRAPPERS: &[&str] = &[
    "actually ",
    "actually, ",
    "instead ",
    "now ",
    "now, ",
    "please ",
    "please, ",
];

const ENGLISH_METALINGUISTIC_PREDICATES: &[&str] = &[
    " is a hypothetical request",
    " is an example prompt",
    " is an imperative example",
    " is an imperative phrase",
    " is only a hypothetical request",
    " is a hypothetical prompt",
    " would be a hypothetical request",
    " would only be a hypothetical request",
];

const ENGLISH_DISCUSSION_DIRECTIVES: &[&str] = &[
    "brainstorming only",
    "brainstorming only for now",
    "discussion only",
    "discussion only for now",
    "just brainstorm",
    "just brainstorm for now",
    "let's only brainstorm",
    "let's only brainstorm for now",
    "let us only brainstorm",
    "let us only brainstorm for now",
    "only brainstorm",
    "only brainstorm for now",
];

const ENGLISH_DRAFT_HOLD_DIRECTIVES: &[&str] = &[
    "do not change the draft",
    "do not change the draft for now",
    "do not change the draft yet",
    "don't change the draft",
    "don't change the draft for now",
    "don't change the draft yet",
    "don’t change the draft",
    "don’t change the draft for now",
    "don’t change the draft yet",
];

const ENGLISH_BUILD_HOLD_PREFIXES: &[&str] = &[
    "do not add",
    "do not build",
    "do not configure",
    "do not create",
    "do not design",
    "do not implement",
    "do not make",
    "do not set up",
    "don't add",
    "don't build",
    "don't configure",
    "don't create",
    "don't design",
    "don't implement",
    "don't make",
    "don't set up",
    "don’t add",
    "don’t build",
    "don’t configure",
    "don’t create",
    "don’t design",
    "don’t implement",
    "don’t make",
    "don’t set up",
];

const KOREAN_BUILD_SUFFIXES: &[&str] = &[
    "구축해줘",
    "구축해 줘",
    "구축해주세요",
    "구축해 주세요",
    "구축해 줄래",
    "구축해 줄래요",
    "구축해줄래",
    "구축해줄래요",
    "만들어줘",
    "만들어 줘",
    "만들어주세요",
    "만들어 주세요",
    "만들어 줄래",
    "만들어 줄래요",
    "만들어줄래",
    "만들어줄래요",
    "설계해줘",
    "설계해 줘",
    "설계해주세요",
    "설계해 주세요",
    "설계해 줄래",
    "설계해 줄래요",
    "설계해줄래",
    "설계해줄래요",
    "추가해줘",
    "추가해 줘",
    "추가해주세요",
    "추가해 주세요",
    "추가해 줄래",
    "추가해 줄래요",
    "추가해줄래",
    "추가해줄래요",
];

const KOREAN_COMPOUND_BUILD_MARKERS: &[&str] = &["구축하고", "만들고", "설계하고", "추가하고"];

const KOREAN_SPLIT_COMPOUND_BUILD_SUFFIXES: &[&str] = &["구축", "설계", "추가"];

const KOREAN_COMPOUND_CONTINUATION_PREFIXES: &[&str] = &[
    "검증",
    "게시",
    "미리보기",
    "보여",
    "설정",
    "연결",
    "준비",
    "테스트",
    "확인",
];

const KOREAN_BUILD_TARGETS: &[(&str, RequestTarget)] = &[
    ("자동화", RequestTarget::Automation),
    ("봇", RequestTarget::Bot),
    ("기능", RequestTarget::Feature),
    ("게임", RequestTarget::Game),
    ("규칙", RequestTarget::Rule),
    ("시스템", RequestTarget::System),
    ("워크플로", RequestTarget::Workflow),
    ("스터디룸", RequestTarget::Room),
    ("룸", RequestTarget::Room),
    ("패널", RequestTarget::Panel),
    ("역할", RequestTarget::Role),
    ("채널", RequestTarget::Channel),
    ("버튼", RequestTarget::Button),
    ("모달", RequestTarget::Modal),
    ("방", RequestTarget::Room),
];

const KOREAN_TARGET_PARTICLES: &[&str] = &[
    "으로", "에서", "에게", "까지", "부터", "처럼", "보다", "조차", "마저", "은", "는", "이", "가",
    "을", "를", "에", "로", "과", "와", "도", "만", "의",
];

const KOREAN_DISCUSSION_SUFFIXES: &[&str] = &[
    "논의만 하자",
    "논의만 해줘",
    "논의만 해주세요",
    "브레인스토밍만 하자",
    "브레인스토밍만 해줘",
    "브레인스토밍만 해주세요",
    "말지 논의하자",
    "말지를 논의하자",
    "여부를 논의하자",
];

const KOREAN_NO_BUILD_SUFFIXES: &[&str] = &[
    "구축하지 말아줘",
    "구축하지 말아 줘",
    "구축하지 말아주세요",
    "구축하지 말아 주세요",
    "구축하지 말자",
    "구축하지 마",
    "구축하지마",
    "구축하지 마세요",
    "만들지는 말아줘",
    "만들지 말아줘",
    "만들지 말아 줘",
    "만들지 말아주세요",
    "만들지 말아 주세요",
    "만들지 말자",
    "만들지 마",
    "만들지마",
    "만들지 마세요",
    "설계하지 말아줘",
    "설계하지 말아 줘",
    "설계하지 말아주세요",
    "설계하지 말아 주세요",
    "설계하지 말자",
    "설계하지 마",
    "설계하지마",
    "설계하지 마세요",
    "추가하지 말아줘",
    "추가하지 말아 줘",
    "추가하지 말아주세요",
    "추가하지 말아 주세요",
    "추가하지 말자",
    "추가하지 마",
    "추가하지마",
    "추가하지 마세요",
];

const KOREAN_BUILD_HOLD_MARKERS: &[&str] = &["당장은", "아직", "우선은", "일단은", "지금은"];

const KOREAN_NEGATIVE_BUILD_VERBS: &[&str] =
    &["구축하지", "만들지는", "만들지", "설계하지", "추가하지"];

const KOREAN_DRAFT_HOLD_DIRECTIVES: &[&str] = &["초안을 변경하지 마", "초안을 변경하지마"];

pub(super) struct GroundedRequestControls {
    pub(super) mode: Option<IntentRequestModeV2>,
    pub(super) preview: Option<bool>,
    pub(super) active_semantic_units: Option<Vec<GroundedSemanticUnit>>,
}

pub(super) struct GroundedSemanticUnit {
    pub(super) text: String,
    pub(super) authoritative: bool,
    pub(super) link: UnquotedGroundingLink,
}

pub(super) fn grounded_request_controls(human: &str) -> GroundedRequestControls {
    let Some(grounding) = unquoted_grounding_text(human) else {
        return GroundedRequestControls {
            mode: None,
            preview: None,
            active_semantic_units: None,
        };
    };
    let mut mode = None;
    let mut active_build_targets = Vec::new();
    let mut preview = None;
    let mut copied_block = false;
    let mut active_semantic_units = Vec::new();
    for sentence in &grounding.sentences {
        let mut question_build_scope = false;
        let mut non_authoritative_scope = false;
        let mut conditional_scope = false;
        let mut runtime_negation_scope = false;
        let mut ui_copy_scope = false;
        for (index, unit) in sentence.iter().enumerate() {
            if copied_block {
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
                copied_block = true;
                non_authoritative_scope = false;
                conditional_scope = false;
                runtime_negation_scope = false;
                question_build_scope = false;
                continue;
            }
            if ui_copy_scope {
                active_semantic_units.push(GroundedSemanticUnit {
                    text: unit.text.clone(),
                    authoritative: false,
                    link: unit.link,
                });
                continue;
            }
            let continuation = sentence
                .get(index.saturating_add(1))
                .map(|unit| unit.text.as_str());
            if unit.link == UnquotedGroundingLink::Detached {
                non_authoritative_scope = false;
                conditional_scope = false;
            }
            let directive = request_directive(&unit.text, continuation);
            let directive_is_build = matches!(&directive, Some(RequestDirective::Build(_)));
            let scope_break = breaks_non_authoritative_scope(&unit.text, directive_is_build)
                || (!conditional_scope && starts_positive_runtime_scope(&unit.text));
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
            let authoritative = !unit.question
                || (directive_is_build && directive_authoritative)
                || question_build_scope;
            let active = active_semantic_unit(&unit.text);
            let authoritative = authoritative && !non_authoritative_scope && active.is_some();
            let mut text = active.unwrap_or_else(|| unit.text.clone());
            if authoritative {
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
                runtime_negation_scope = false;
                question_build_scope = false;
            }
            active_semantic_units.push(GroundedSemanticUnit {
                text,
                authoritative,
                link: unit.link,
            });
            if opens_ui_copy_scope(&unit.text) {
                ui_copy_scope = true;
            }
        }
    }
    GroundedRequestControls {
        mode,
        preview,
        active_semantic_units: Some(active_semantic_units),
    }
}

#[cfg(test)]
pub(super) fn grounded_request_mode(human: &str) -> Option<IntentRequestModeV2> {
    grounded_request_controls(human).mode
}

#[cfg(test)]
pub(super) fn grounded_preview_preference(human: &str) -> Option<bool> {
    grounded_request_controls(human).preview
}

fn explicit_build(unit: &str, continuation: Option<&str>) -> Option<BuildDirective> {
    explicit_english_build(unit).or_else(|| explicit_korean_build(unit, continuation))
}

fn request_directive(unit: &str, continuation: Option<&str>) -> Option<RequestDirective> {
    if explicit_discussion(unit) {
        Some(RequestDirective::Discussion)
    } else if let Some(directive) = english_build_hold_directive(unit) {
        Some(directive)
    } else if let Some(directive) = korean_build_hold_directive(unit) {
        Some(directive)
    } else {
        explicit_build(unit, continuation).map(RequestDirective::Build)
    }
}

fn explicit_english_build(unit: &str) -> Option<BuildDirective> {
    if ENGLISH_METALINGUISTIC_PREDICATES
        .iter()
        .any(|predicate| unit.contains(predicate))
    {
        return None;
    }
    let mut value = strip_repeated_prefixes(unit, ENGLISH_REQUEST_WRAPPERS);
    if let Some(tail) = ENGLISH_POLITE_BUILD_PREFIXES
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
    {
        value = strip_repeated_prefixes(tail, ENGLISH_REQUEST_WRAPPERS);
    }
    if let Some(targets) = ENGLISH_BUILD_PREFIXES.iter().find_map(|prefix| {
        value
            .strip_prefix(prefix)
            .map(direct_english_build_targets)
            .filter(|targets| !targets.is_empty())
    }) {
        return Some(BuildDirective { targets });
    }
    (value.starts_with("i want this designed now") || value.starts_with("i want this built now"))
        .then_some(BuildDirective {
            targets: Vec::new(),
        })
}

fn explicit_korean_build(unit: &str, continuation: Option<&str>) -> Option<BuildDirective> {
    let verb = KOREAN_BUILD_SUFFIXES
        .iter()
        .find_map(|suffix| {
            unit.ends_with(suffix)
                .then_some(unit.len().saturating_sub(suffix.len()))
        })
        .or_else(|| {
            KOREAN_COMPOUND_BUILD_MARKERS.iter().find_map(|marker| {
                unit.find(marker).and_then(|position| {
                    let tail = unit[position.saturating_add(marker.len())..].trim_start();
                    KOREAN_COMPOUND_CONTINUATION_PREFIXES
                        .iter()
                        .any(|prefix| tail.starts_with(prefix))
                        .then_some(position)
                })
            })
        })
        .or_else(|| {
            continuation
                .filter(|tail| {
                    KOREAN_COMPOUND_CONTINUATION_PREFIXES
                        .iter()
                        .any(|prefix| tail.starts_with(prefix))
                })
                .and_then(|_| {
                    KOREAN_SPLIT_COMPOUND_BUILD_SUFFIXES
                        .iter()
                        .find_map(|suffix| unit.strip_suffix(suffix).map(|head| head.len()))
                })
        })?;
    let targets = direct_korean_build_targets(unit.get(..verb)?.trim());
    (!targets.is_empty()).then_some(BuildDirective { targets })
}

fn explicit_discussion(unit: &str) -> bool {
    let english = strip_repeated_prefixes(unit, ENGLISH_REQUEST_WRAPPERS);
    let english = english
        .strip_prefix("this is ")
        .or_else(|| english.strip_prefix("for now "))
        .or_else(|| english.strip_prefix("for now, "))
        .unwrap_or(english);
    let english_discussion = ENGLISH_DISCUSSION_DIRECTIVES.contains(&english)
        || ENGLISH_DRAFT_HOLD_DIRECTIVES.contains(&english);
    let korean_discussion = KOREAN_DISCUSSION_SUFFIXES
        .iter()
        .any(|suffix| unit.ends_with(suffix))
        || KOREAN_DRAFT_HOLD_DIRECTIVES.contains(&unit);
    english_discussion || korean_discussion
}

fn english_build_hold_directive(unit: &str) -> Option<RequestDirective> {
    let english = strip_repeated_prefixes(unit, ENGLISH_REQUEST_WRAPPERS);
    let tail = ENGLISH_BUILD_HOLD_PREFIXES
        .iter()
        .find_map(|prefix| english.strip_prefix(prefix))?
        .trim();
    let subject = tail
        .strip_suffix(" for now")
        .or_else(|| tail.strip_suffix(" yet"))
        .or_else(|| (tail == "for now" || tail == "yet").then_some(""))
        .unwrap_or(tail);
    let subject = strip_english_article(subject.trim());
    if subject.is_empty() || matches!(subject, "anything" | "it" | "that" | "this") {
        return Some(RequestDirective::Hold(HoldDirective::Global));
    }
    if let Some(target) = terminal_english_target(subject) {
        return Some(RequestDirective::Hold(HoldDirective::Target(target)));
    }
    Some(RequestDirective::Hold(HoldDirective::Scoped))
}

fn korean_build_hold_directive(unit: &str) -> Option<RequestDirective> {
    if !KOREAN_NO_BUILD_SUFFIXES
        .iter()
        .any(|suffix| unit.ends_with(suffix))
    {
        return None;
    }
    let verb = KOREAN_NEGATIVE_BUILD_VERBS
        .iter()
        .filter_map(|verb| unit.rfind(verb))
        .max()?;
    let raw_subject =
        strip_repeated_prefixes(unit.get(..verb)?.trim(), &["아니 ", "아니요 ", "대신 "]);
    let subject = strip_korean_hold_markers(raw_subject);
    if subject.is_empty() || matches!(subject, "그건" | "그것은" | "그걸" | "그것을") {
        return Some(RequestDirective::Hold(HoldDirective::Global));
    }
    terminal_korean_target(subject)
        .map(HoldDirective::Target)
        .or(Some(HoldDirective::Scoped))
        .map(RequestDirective::Hold)
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

fn first_copy_carrier_index(unit: &str) -> Option<usize> {
    let structural_english = [
        "button says",
        "button label",
        "button text",
        "displays the words",
        "display the words",
        "fallback panel title",
        "panel title",
    ]
    .iter()
    .filter_map(|marker| first_ascii_word_index(unit, marker));
    let owned_english = [
        "about",
        "ask if",
        "ask whether",
        "asking if",
        "asking whether",
        "asking users if",
        "asking users whether",
        "asks if",
        "asks whether",
        "called",
        "caption is",
        "describing",
        "explaining",
        "explaining how",
        "explaining when",
        "explaining that",
        "explaining whether",
        "explaining why",
        "explains how",
        "explains that",
        "explains whether",
        "explains why",
        "label is",
        "label to",
        "literal is",
        "named",
        "phrase is",
        "prompting users if",
        "prompting users whether",
        "prompts the user if",
        "prompts the user whether",
        "posing if",
        "posing whether",
        "says",
        "text is",
        "text says",
        "text to",
        "under the label",
        "whose caption is",
        "with the label",
    ]
    .iter()
    .filter_map(|marker| first_ascii_word_index(unit, marker))
    .filter(|position| english_ui_owner_before(unit, *position));
    let korean = [
        "라벨은",
        "버튼 라벨",
        "버튼 글자",
        "패널 제목",
        "문구는",
        "텍스트는",
    ]
    .iter()
    .filter_map(|marker| unit.find(marker))
    .min();
    structural_english
        .chain(owned_english)
        .chain(korean)
        .chain(korean_ui_content_start(unit).map(|_| 0))
        .min()
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

fn english_ui_owner_before(unit: &str, boundary: usize) -> bool {
    let ui = [
        "button",
        "caption",
        "copy",
        "help panel",
        "label",
        "message",
        "modal",
        "panel",
        "response",
        "text",
        "title",
    ]
    .iter()
    .filter_map(|owner| last_ascii_word_index_before(unit, owner, boundary))
    .map(|position| (position, true));
    let non_ui = [
        "automation",
        "channel",
        "game",
        "llm",
        "role",
        "room",
        "user",
        "workflow",
    ]
    .iter()
    .filter_map(|owner| last_ascii_word_index_before(unit, owner, boundary))
    .map(|position| (position, false));
    ui.chain(non_ui)
        .max_by_key(|(position, _)| *position)
        .is_some_and(|(_, is_ui)| is_ui)
}

fn opens_ui_copy_scope(unit: &str) -> bool {
    english_ui_owner_before(unit, unit.len())
        && ["ask", "asks", "explaining", "says"]
            .iter()
            .any(|suffix| unit.ends_with(suffix))
}

fn korean_ui_content_start(unit: &str) -> Option<usize> {
    let carrier = ["묻는", "질문하는", "안내하는", "확인하는"]
        .iter()
        .filter_map(|marker| unit.find(marker))
        .min()?;
    ["패널", "모달", "버튼", "메시지", "문구"]
        .iter()
        .filter_map(|owner| {
            unit[carrier..]
                .find(owner)
                .map(|position| carrier + position)
        })
        .min()
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
            "definitely ",
            "instead ",
            "now, ",
            "now ",
            "반드시 ",
            "이제 ",
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
    ENGLISH_METALINGUISTIC_PREDICATES
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
        .any(|suffix| semantic_unit.ends_with(suffix))
        || [
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
            "쓸 수도",
            "쓸 수도 있지만",
            "사용할 수도",
            "사용할 수도 있지만",
            " may be required",
            "필요할 수도",
            "필요할 수도 있지만",
        ]
        .iter()
        .any(|marker| semantic_unit.contains(marker))
        || metalinguistic_runtime_comparison(semantic_unit)
}

fn conditional_non_authoritative_semantic_unit(unit: &str) -> bool {
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

fn metalinguistic_carrier(unit: &str) -> bool {
    let unit = unit.trim();
    [
        "example:",
        "example prompt:",
        "here is an example prompt:",
        "payload says:",
        "prompt:",
        "sample prompt:",
        "the payload says:",
        "the user said:",
        "user said:",
        "사용자 발화:",
        "사용자가 말함:",
        "예시 프롬프트:",
        "예시:",
        "예를 들어:",
        "프롬프트:",
    ]
    .iter()
    .any(|carrier| unit.starts_with(carrier))
}

fn strip_repeated_prefixes<'a>(mut value: &'a str, prefixes: &[&str]) -> &'a str {
    loop {
        let Some(tail) = prefixes
            .iter()
            .find_map(|prefix| value.strip_prefix(prefix))
        else {
            return value;
        };
        value = tail;
    }
}

fn direct_english_build_targets(value: &str) -> Vec<RequestTarget> {
    let barrier = ENGLISH_OBJECT_BARRIERS
        .iter()
        .filter_map(|barrier| value.find(barrier))
        .min();
    let mut targets = ENGLISH_BUILD_TARGETS
        .iter()
        .filter_map(|(word, target)| {
            first_ascii_inflected_word_index(value, word)
                .filter(|position| barrier.is_none_or(|barrier| *position < barrier))
                .map(|position| (position, *target))
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|(position, _)| *position);
    targets
        .into_iter()
        .map(|(_, target)| target)
        .fold(Vec::new(), |mut unique, target| {
            if !unique.contains(&target) {
                unique.push(target);
            }
            unique
        })
}

fn first_ascii_inflected_word_index(value: &str, expected: &str) -> Option<usize> {
    first_ascii_word_index(value, expected).or_else(|| {
        let plural = format!("{expected}s");
        first_ascii_word_index(value, &plural)
    })
}

fn terminal_english_target(subject: &str) -> Option<RequestTarget> {
    ENGLISH_BUILD_TARGETS
        .iter()
        .filter_map(|(word, target)| {
            first_ascii_inflected_word_index(subject, word).and_then(|index| {
                let suffix = &subject[index..];
                (suffix == *word || suffix == format!("{word}s")).then_some((*word, *target))
            })
        })
        .max_by_key(|(word, _)| word.len())
        .map(|(_, target)| target)
}

fn strip_english_article(value: &str) -> &str {
    ["a ", "an ", "the "]
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
        .unwrap_or(value)
}

fn strip_korean_hold_markers(mut value: &str) -> &str {
    loop {
        let leading = KOREAN_BUILD_HOLD_MARKERS
            .iter()
            .find_map(|marker| value.strip_prefix(marker));
        if let Some(tail) = leading {
            value = tail.trim_start();
            continue;
        }
        let trailing = KOREAN_BUILD_HOLD_MARKERS
            .iter()
            .find_map(|marker| value.strip_suffix(marker));
        if let Some(head) = trailing {
            value = head.trim_end();
            continue;
        }
        return value;
    }
}

fn terminal_korean_target(subject: &str) -> Option<RequestTarget> {
    let subject = ["으로", "에서", "에게", "은", "는", "이", "가", "을", "를"]
        .iter()
        .find_map(|particle| subject.strip_suffix(particle))
        .unwrap_or(subject)
        .trim_end();
    KOREAN_BUILD_TARGETS
        .iter()
        .filter(|(word, _)| subject.ends_with(word))
        .max_by_key(|(word, _)| word.len())
        .map(|(_, target)| *target)
}

fn direct_korean_build_targets(subject: &str) -> Vec<RequestTarget> {
    KOREAN_BUILD_TARGETS
        .iter()
        .filter(|(word, _)| contains_korean_build_target(subject, word))
        .map(|(_, target)| *target)
        .fold(Vec::new(), |mut unique, target| {
            if !unique.contains(&target) {
                unique.push(target);
            }
            unique
        })
}

fn contains_korean_build_target(subject: &str, expected: &str) -> bool {
    subject.match_indices(expected).any(|(start, _)| {
        if expected != "방" {
            return true;
        }
        let suffix = &subject[start.saturating_add(expected.len())..];
        suffix.is_empty()
            || suffix
                .chars()
                .next()
                .is_some_and(|character| !character.is_alphanumeric())
            || KOREAN_TARGET_PARTICLES
                .iter()
                .any(|particle| suffix.starts_with(particle))
    })
}

fn ends_metalinguistic_copy(unit: &str) -> bool {
    matches!(
        unit,
        "end of example"
            | "end of payload"
            | "end of prompt"
            | "붙여넣기 끝"
            | "예시 끝"
            | "프롬프트 끝"
    )
}

fn analyzes_metalinguistic_copy(unit: &str) -> bool {
    matches!(
        unit,
        "analyze the payload"
            | "analyze this payload"
            | "explain what the payload does"
            | "explain what this payload does"
            | "이 페이로드를 분석해"
            | "이 페이로드가 무엇을 하는지 설명해"
            | "페이로드를 분석해"
    )
}

fn first_ascii_word_index(value: &str, expected: &str) -> Option<usize> {
    value.match_indices(expected).find_map(|(start, _)| {
        let end = start.saturating_add(expected.len());
        (value
            .get(..start)
            .and_then(|prefix| prefix.chars().next_back())
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
            && value
                .get(end..)
                .and_then(|suffix| suffix.chars().next())
                .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_'))
        .then_some(start)
    })
}

fn last_ascii_word_index_before(value: &str, expected: &str, boundary: usize) -> Option<usize> {
    value
        .match_indices(expected)
        .filter_map(|(start, _)| {
            let end = start.saturating_add(expected.len());
            (start < boundary
                && value
                    .get(..start)
                    .and_then(|prefix| prefix.chars().next_back())
                    .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
                && value
                    .get(end..)
                    .and_then(|suffix| suffix.chars().next())
                    .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_'))
            .then_some(start)
        })
        .max()
}
