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
    let Some(words) = source
        .unique_complete_asserted_sentence_tokens(value)
        .or_else(|| source.unique_complete_asserted_clause_tokens(value))
    else {
        return false;
    };
    let words = words.iter().map(String::as_str).collect::<Vec<_>>();
    metalinguistic_non_instruction(&words)
        || design_interaction_completion(&words)
        || managed_base_and_outcome(&words, context)
        || pending_hub(&words, context.grounded_channel)
        || selected_hub_and_disabled_close(&words, context)
        || closed_axis_restatement(value, &words, context)
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

fn managed_base_and_outcome(words: &[&str], context: &ManagedRecipeCoreContext<'_>) -> bool {
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
            | ["한국어" | "영어", "기본", "문구와", "이름을", "사용해"]
    )
}

fn fully_owned_close_sentence(words: &[&str]) -> bool {
    matches!(
        words,
        ["leave" | "keep", "room", "closing", "disabled"]
            | ["leave" | "keep", "closing", "disabled"]
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
