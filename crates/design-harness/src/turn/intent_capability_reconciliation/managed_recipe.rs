use crate::intent::{ExistingChannelKey, IntentRequestedOutcome};

use super::super::intent_interpretation::{CloseAuthorizationV2, IntentLocaleHintV2};
use super::super::intent_request_mode_grounding::grounded_closed_axis_restatement;
use super::syntax::SourceText;

pub(in crate::turn) struct ManagedRecipeCoreContext<'a> {
    pub(in crate::turn) requested_outcome: IntentRequestedOutcome,
    pub(in crate::turn) grounded_channel: Option<&'a ExistingChannelKey>,
    pub(in crate::turn) locale: IntentLocaleHintV2,
    pub(in crate::turn) close_authorization: CloseAuthorizationV2,
}

pub(super) fn managed_recipe_restatement_owns(
    source: &SourceText<'_>,
    context: &ManagedRecipeCoreContext<'_>,
    value: &str,
) -> bool {
    let complete_sentence = source.unique_complete_asserted_sentence_tokens(value);
    let standalone = complete_sentence.is_some();
    let complete_words =
        complete_sentence.or_else(|| source.unique_complete_asserted_clause_tokens(value));
    if let Some(words) = complete_words {
        let words = words.iter().map(String::as_str).collect::<Vec<_>>();
        return standalone
            && (metalinguistic_non_instruction(&words)
                || design_interaction_completion(&words)
                || managed_base_and_outcome(&words, context)
                || pending_hub(&words, context.grounded_channel)
                || selected_hub_and_disabled_close(&words, context)
                || short_design_interaction_completion(&words)
                || default_copy_preservation(&words, context)
                || closed_axis_restatement(value, &words, context))
            || framed_locale_restatement(source, value, &words, context)
            || framed_closed_axis_restatement(source, value, &words, context)
            || framed_design_interaction_completion(source, value, &words, context)
            || framed_default_copy_preservation(source, value, &words, context);
    }
    let Some(words) = candidate_sentence_tokens(value) else {
        return false;
    };
    let words = words.iter().map(String::as_str).collect::<Vec<_>>();
    framed_locale_restatement(source, value, &words, context)
        || framed_closed_axis_restatement(source, value, &words, context)
        || framed_design_interaction_completion(source, value, &words, context)
        || framed_default_copy_preservation(source, value, &words, context)
}

fn candidate_sentence_tokens(value: &str) -> Option<Vec<String>> {
    let candidate = SourceText::analyze(value).ok()?;
    candidate.unique_complete_asserted_sentence_tokens(value)
}

fn framed_locale_restatement(
    source: &SourceText<'_>,
    value: &str,
    words: &[&str],
    context: &ManagedRecipeCoreContext<'_>,
) -> bool {
    let (locale, _) = grounded_closed_axis_restatement(value);
    if locale != Some(context.locale) || !fully_owned_locale_sentence(words) {
        return false;
    }
    let mut frames = vec![
        format!("{value} and leave room closing disabled"),
        format!("{value}, and leave room closing disabled"),
        format!(
            "{value} except that the room help button label is exactly 'Guide' and its ephemeral response is exactly 'Read the guide'"
        ),
        format!(
            "{value}, with exactly one copy override: set the launcher create-button label to 'Begin deep work'"
        ),
    ];
    if context.close_authorization == CloseAuthorizationV2::Disabled {
        frames.push(format!("{value}, 방 닫기 기능은 넣지 마"));
    }
    if let Some(key) = context.grounded_channel {
        frames.extend([
            format!(
                "{value}, use the existing channel binding {} as the discovery hub, and leave room closing disabled",
                key.as_str()
            ),
            format!(
                "{value}, use the exact existing channel binding {} as the discovery hub, and leave room closing disabled",
                key.as_str()
            ),
            format!(
                "{value}, {} as the existing discovery hub, and leave room closing disabled",
                key.as_str()
            ),
            format!(
                "{value}, place discovery in the existing {} channel binding, and keep room closing turned off",
                key.as_str()
            ),
        ]);
    }
    asserted_sentence_frame(source, &frames)
}

fn framed_closed_axis_restatement(
    source: &SourceText<'_>,
    value: &str,
    words: &[&str],
    context: &ManagedRecipeCoreContext<'_>,
) -> bool {
    let (_, close_authorization) = grounded_closed_axis_restatement(value);
    if close_authorization != Some(context.close_authorization)
        || !fully_owned_close_sentence(words)
    {
        return false;
    }
    let mut frames = vec![
        format!("use english default copy and naming and {value}"),
        format!("use english defaults and {value}"),
        format!("use english defaults, and {value}"),
        format!("{value} and do not ask a follow-up question"),
        format!("{value}, and do not ask a follow-up question"),
        format!(
            "leave all copy and controls at their defaults, {value}, and do not ask a follow-up question"
        ),
        format!(
            "keep default copy and naming, {value}, and do not ask a follow-up question"
        ),
    ];
    match context.locale {
        IntentLocaleHintV2::En => {
            frames.push(format!("영어 기본 문구와 이름을 사용해, {value}"));
        }
        IntentLocaleHintV2::Ko => {
            frames.push(format!("한국어 기본 문구와 이름을 사용해, {value}"));
        }
        IntentLocaleHintV2::Unspecified => {}
    }
    if let Some(key) = context.grounded_channel {
        frames.extend([
            format!(
                "use english default copy and naming, use the existing channel binding {} as the discovery hub, and {value}",
                key.as_str()
            ),
            format!(
                "use english default copy and naming, use the exact existing channel binding {} as the discovery hub, and {value}",
                key.as_str()
            ),
            format!(
                "use english default copy and naming, {} as the existing discovery hub, and {value}",
                key.as_str()
            ),
            format!(
                "keep its copy and generated names at the english defaults, place discovery in the existing {} channel binding, and {value}",
                key.as_str()
            ),
            format!(
                "build a managed private study room in {}, but {value}",
                key.as_str()
            ),
            format!(
                "build a managed private study-room in {}, but {value}",
                key.as_str()
            ),
            format!(
                "build a managed private study-room automation in {}, but {value}",
                key.as_str()
            ),
            format!("기존 채널 바인딩 {}를 안내 허브로 쓰고 {value}", key.as_str()),
        ]);
    }
    asserted_sentence_frame(source, &frames)
}

fn framed_design_interaction_completion(
    source: &SourceText<'_>,
    value: &str,
    words: &[&str],
    context: &ManagedRecipeCoreContext<'_>,
) -> bool {
    if context.close_authorization != CloseAuthorizationV2::Disabled
        || !short_design_interaction_completion(words)
    {
        return false;
    }
    let frames = [
        format!("leave room closing disabled and {value}"),
        format!("leave room closing disabled, and {value}"),
        format!("leave closing disabled, and {value}"),
        format!("keep room closing disabled, and {value}"),
        format!("keep closing disabled, and {value}"),
        format!("keep room closing turned off, and {value}"),
        format!("keep closing turned off, and {value}"),
        format!(
            "leave all copy and controls at their defaults, keep closing disabled, and {value}"
        ),
        format!("keep default copy and naming, leave closing disabled, and {value}"),
        format!("keep validation and preview, and {value}"),
        format!("all material choices are provided, so {value}"),
        format!("all choices are provided, so {value}"),
        format!("nothing material is left undecided, so {value}"),
        format!("필요한 선택은 전부 줬으니 {value}"),
    ];
    asserted_sentence_frame(source, &frames)
}

fn framed_default_copy_preservation(
    source: &SourceText<'_>,
    value: &str,
    words: &[&str],
    context: &ManagedRecipeCoreContext<'_>,
) -> bool {
    if !default_copy_preservation(words, context) {
        return false;
    }
    asserted_sentence_frame(
        source,
        &[
            format!("{value}, leave closing disabled, and do not ask a follow-up question"),
            format!("{value}, leave closing disabled, and proceed without asking me anything"),
        ],
    )
}

fn asserted_sentence_frame(source: &SourceText<'_>, frames: &[String]) -> bool {
    frames
        .iter()
        .any(|frame| source.has_unique_complete_asserted_sentence_frame(frame))
}

fn metalinguistic_non_instruction(words: &[&str]) -> bool {
    let suffix = [
        "is",
        "mentioned",
        "only",
        "as",
        "an",
        "example",
        "not",
        "as",
        "an",
        "instruction",
    ];
    words.starts_with(&["the", "literal"])
        && words.len() > suffix.len().saturating_add(2)
        && words.ends_with(&suffix)
}

fn design_interaction_completion(words: &[&str]) -> bool {
    matches!(
        words,
        [
            "all",
            "material",
            "choices",
            "are",
            "provided",
            "so",
            "do",
            "not",
            "ask",
            "a",
            "follow-up",
            "question"
        ] | [
            "nothing",
            "material",
            "is",
            "left",
            "undecided",
            "so",
            "proceed",
            "without",
            "asking",
            "me",
            "anything"
        ] | [
            "필요한",
            "선택은",
            "전부",
            "줬으니",
            "추가",
            "질문은",
            "하지",
            "마"
        ]
    )
}

fn short_design_interaction_completion(words: &[&str]) -> bool {
    matches!(
        words,
        ["do", "not", "ask", "a", "follow-up", "question"]
            | ["proceed", "without", "asking", "me", "anything"]
            | ["추가", "질문은", "하지", "마"]
    )
}

fn managed_base_and_outcome(words: &[&str], context: &ManagedRecipeCoreContext<'_>) -> bool {
    if context.requested_outcome == IntentRequestedOutcome::ValidatedPreview
        && matches!(
            words,
            [
                "please",
                "prepare",
                "a",
                "validated",
                "preview",
                "of",
                "the",
                "managed",
                "private",
                "study-room",
                "automation"
            ]
        )
    {
        return true;
    }
    if context.requested_outcome == IntentRequestedOutcome::ValidatedPreview
        && matches!(
            words,
            [
                "관리형",
                "비공개",
                "스터디룸",
                "자동화를",
                "만들고",
                "검증된",
                "미리보기까지",
                "준비해줘"
            ]
        )
    {
        return true;
    }
    if context.requested_outcome == IntentRequestedOutcome::WorkingDraft
        && matches!(
            words,
            ["관리형", "비공개", "스터디룸", "자동화를", "만들어줘"]
        )
    {
        return true;
    }
    let Some(mut tail) = english_managed_base_tail(words) else {
        return false;
    };
    if tail.starts_with(&["in"]) {
        let Some(key) = context.grounded_channel else {
            return false;
        };
        if tail.get(1) != Some(&key.as_str()) {
            return false;
        }
        tail = tail.get(2..).unwrap_or_default();
    }
    match context.requested_outcome {
        IntentRequestedOutcome::WorkingDraft => tail.is_empty(),
        IntentRequestedOutcome::ValidatedPreview => {
            tail == ["and", "prepare", "its", "validated", "preview"]
        }
        IntentRequestedOutcome::Discussion => false,
    }
}

fn english_managed_base_tail<'a>(words: &'a [&str]) -> Option<&'a [&'a str]> {
    let prefixes: &[&[&str]] = &[
        &[
            "build",
            "a",
            "managed",
            "private",
            "study-room",
            "automation",
        ],
        &[
            "build",
            "a",
            "managed",
            "private",
            "study",
            "room",
            "automation",
        ],
        &["build", "a", "managed", "private", "study-room"],
        &["build", "a", "managed", "private", "study", "room"],
    ];
    prefixes
        .iter()
        .find_map(|prefix| words.strip_prefix(*prefix))
}

fn pending_hub(words: &[&str], grounded_channel: Option<&ExistingChannelKey>) -> bool {
    grounded_channel.is_none()
        && matches!(
            words,
            [
                "i",
                "have",
                "not",
                "selected",
                "which",
                "existing",
                "channel",
                "should",
                "be",
                "the",
                "discovery",
                "hub",
                "yet"
            ]
        )
}

fn selected_hub_and_disabled_close(words: &[&str], context: &ManagedRecipeCoreContext<'_>) -> bool {
    let Some(key) = context.grounded_channel else {
        return false;
    };
    context.close_authorization == CloseAuthorizationV2::Disabled
        && words.len() == 12
        && words[..3] == ["기존", "채널", "바인딩"]
        && words[3]
            .strip_suffix('를')
            .is_some_and(|candidate| candidate == key.as_str())
        && words[4..]
            == [
                "안내",
                "허브로",
                "쓰고",
                "방",
                "닫기",
                "기능은",
                "넣지",
                "마",
            ]
}

fn closed_axis_restatement(
    value: &str,
    words: &[&str],
    context: &ManagedRecipeCoreContext<'_>,
) -> bool {
    let (locale, close_authorization) = grounded_closed_axis_restatement(value);
    let locale_owned = locale == Some(context.locale) && fully_owned_locale_sentence(words);
    let close_owned = close_authorization == Some(context.close_authorization)
        && fully_owned_close_sentence(words);
    locale_owned || close_owned
}

fn fully_owned_locale_sentence(words: &[&str]) -> bool {
    matches!(
        words,
        [
            "use",
            "english" | "korean",
            "default",
            "copy",
            "and",
            "naming"
        ] | ["use", "english" | "korean", "defaults"]
            | [
                "use",
                "english" | "korean",
                "defaults",
                "for",
                "every",
                "name",
                "and",
                "room",
                "control"
            ]
            | [
                "keep",
                "its",
                "copy",
                "and",
                "generated",
                "names",
                "at",
                "the",
                "english" | "korean",
                "defaults"
            ]
            | ["한국어" | "영어", "기본", "문구와", "이름을", "사용해"]
    )
}

fn default_copy_preservation(words: &[&str], context: &ManagedRecipeCoreContext<'_>) -> bool {
    context.locale != IntentLocaleHintV2::Unspecified
        && matches!(words, ["keep", "default", "copy", "and", "naming"])
}

fn fully_owned_close_sentence(words: &[&str]) -> bool {
    matches!(
        words,
        ["leave" | "keep", "room", "closing", "disabled"]
            | ["leave" | "keep", "closing", "disabled"]
            | ["leave" | "keep", "room", "closing", "turned", "off"]
            | ["leave" | "keep", "closing", "turned", "off"]
            | ["enable", "the", "close", "button", "for", "any", "room", "member"]
            | [
                "enable", "the", "close", "button", "for", "any", "room", "member", "using", "the",
                "recipe's", "default", "close", "label", "and", "closed", "response"
            ]
            | [
                "the", "close", "button", "must", "work", "only", "for", "the", "person", "who",
                "created", "that", "room"
            ]
            | ["only", "the", "room", "creator", "may" | "can", "close"]
            | ["방", "닫기", "기능은", "넣지", "마"]
    )
}
