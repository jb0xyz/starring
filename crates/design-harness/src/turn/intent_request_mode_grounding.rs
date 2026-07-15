use super::intent_boundary_grounding::unquoted_grounding_text;
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
}

pub(super) fn grounded_request_controls(human: &str) -> GroundedRequestControls {
    let Some(grounding) = unquoted_grounding_text(human) else {
        return GroundedRequestControls {
            mode: None,
            preview: None,
        };
    };
    let mut mode = None;
    let mut active_build_targets = Vec::new();
    let mut preview = None;
    let mut copied_block = false;
    for sentence in &grounding.sentences {
        for (index, unit) in sentence.iter().enumerate() {
            if copied_block {
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
                continue;
            }
            let continuation = sentence
                .get(index.saturating_add(1))
                .map(|unit| unit.text.as_str());
            if let Some(directive) = request_directive(&unit.text, continuation) {
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
            if let Some(preference) = preview_directive(&unit.text) {
                preview = Some(preference);
            }
        }
    }
    GroundedRequestControls { mode, preview }
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
        "called ",
        "failure",
        "failures",
        "label",
        "mode",
        "modes",
        "named ",
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
    [
        "button says",
        "button label",
        "button text",
        "displays the words",
        "display the words",
        "fallback panel title",
        "label",
        "literal",
        "panel title",
        "phrase",
        "text is",
        "text to",
        "라벨",
        "문구",
        "버튼 글자",
        "제목",
        "텍스트는",
    ]
    .iter()
    .filter_map(|marker| unit.find(marker))
    .min()
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
