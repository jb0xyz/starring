use crate::turn::intent_detail_grammar::{
    valid_masked_direct_assignment_tail, DetailSlot, GroundedDetailAssignment,
};
use crate::turn::intent_detail_syntax::{
    grounded_detail_assignment_scope, grounded_static_detail_continuation,
};
use crate::turn::intent_interpretation::IntentLocaleHintV2;

use super::{syntax::*, AxisDirective};

pub(super) fn combine_locale_directive(
    directive: AxisDirective<IntentLocaleHintV2>,
    continued_locale: Option<IntentLocaleHintV2>,
) -> AxisDirective<IntentLocaleHintV2> {
    match (directive, continued_locale) {
        (AxisDirective::None, Some(locale)) => AxisDirective::Value(locale),
        (AxisDirective::Value(current), Some(continued)) if current != continued => {
            AxisDirective::Conflict
        }
        (directive, _) => directive,
    }
}

pub(super) fn bare_locale_directive(words: &[&str]) -> bool {
    matches!(words, ["use", "english" | "korean"])
}

pub(super) fn locale_branch_value(words: &[&str]) -> Option<IntentLocaleHintV2> {
    let words = match words {
        ["choose", "between", rest @ ..] | ["use", "either", rest @ ..] => rest,
        ["use", rest @ ..] => rest,
        _ => words,
    };
    let candidate = words.first().copied()?;
    locale_token_value(candidate)
}

fn locale_token_value(candidate: &str) -> Option<IntentLocaleHintV2> {
    let candidate = candidate
        .strip_suffix('로')
        .or_else(|| candidate.strip_suffix('를'))
        .unwrap_or(candidate);
    match candidate {
        "english" | "영어" => Some(IntentLocaleHintV2::En),
        "korean" | "한국어" => Some(IntentLocaleHintV2::Ko),
        _ => None,
    }
}

pub(super) fn locale_directive(
    words: &[&str],
    alternative_branch: bool,
) -> AxisDirective<IntentLocaleHintV2> {
    if contains_sequence(words, &["use", "korean", "rather", "than", "english"]) {
        return AxisDirective::Value(IntentLocaleHintV2::Ko);
    }
    if contains_sequence(words, &["use", "english", "rather", "than", "korean"]) {
        return AxisDirective::Value(IntentLocaleHintV2::En);
    }
    if let Some(locale) = korean_locale_correction(words) {
        return AxisDirective::Value(locale);
    }
    let english = english_locale_directive(words, "english", alternative_branch)
        || korean_language_directive(words, "영어");
    let korean = english_locale_directive(words, "korean", alternative_branch)
        || korean_language_directive(words, "한국어");
    match (english, korean) {
        (true, true) => AxisDirective::Conflict,
        (true, false) => AxisDirective::Value(IntentLocaleHintV2::En),
        (false, true) => AxisDirective::Value(IntentLocaleHintV2::Ko),
        (false, false) => AxisDirective::None,
    }
}

fn english_locale_directive(words: &[&str], locale: &str, alternative_branch: bool) -> bool {
    let direct_use = words.starts_with(&["use", locale])
        && words.get(2).is_none_or(|word| {
            matches!(
                *word,
                "copy"
                    | "default"
                    | "defaults"
                    | "interface"
                    | "label"
                    | "labels"
                    | "language"
                    | "naming"
                    | "responses"
                    | "ui"
            )
        })
        || (words.starts_with(&["use", locale, "for"])
            && words.iter().skip(3).any(|word| locale_surface_word(word)));
    let language_setting = words.starts_with(&["set", "language", "to", locale])
        || words.starts_with(&["set", "the", "language", "to", locale])
        || words.starts_with(&["set", "locale", "to", locale])
        || words.starts_with(&["set", "the", "locale", "to", locale])
        || words.starts_with(&["switch", "to", locale])
        || words.starts_with(&["set", "response", "language", "to", locale])
        || words.starts_with(&["set", "the", "response", "language", "to", locale])
        || words.starts_with(&["set", "interface", "to", locale])
        || words.starts_with(&["set", "the", "interface", "to", locale]);
    let direct_response = matches!(
        words,
        ["answer" | "reply" | "respond", "in" | "using", selected, ..] if *selected == locale
    ) || (words.starts_with(&["write"])
        && (contains_sequence(words, &["response", "in", locale])
            || contains_sequence(words, &["responses", "in", locale])
            || contains_sequence(words, &["copy", "in", locale])));
    let declarative_response = matches!(
        words,
        ["the", "response" | "responses", "should" | "must", "be", "in", selected, ..]
            if *selected == locale
    );
    let inherited_default = alternative_branch
        && matches!(words, [selected, "default" | "defaults", ..] if *selected == locale);
    let preserved_generated_names = generated_name_locale_default_candidate(words) == Some(locale)
        || kept_locale_candidate(words) == Some(locale);
    let interface_selection = words.starts_with(&["use", locale, "for", "the", "interface"])
        || words.starts_with(&["use", locale, "for", "interface"])
        || words.starts_with(&["all", "ui", "copy", "should", "be", locale])
        || words.starts_with(&["all", "interface", "copy", "should", "be", locale])
        || words.starts_with(&["all", "labels", "should", "be", locale])
        || words.starts_with(&["the", "interface", "language", "must", "be", locale])
        || words.starts_with(&["the", "interface", "language", "should", "be", locale])
        || matches!(words, ["use", selected, "throughout", ..] if *selected == locale);
    direct_use
        || language_setting
        || direct_response
        || declarative_response
        || interface_selection
        || preserved_generated_names
        || inherited_default
}

fn korean_language_directive(words: &[&str], language: &str) -> bool {
    if korean_negative_directive(words) || has_korean_semantic_analysis(words) {
        return false;
    }
    let language_indexes = words
        .iter()
        .enumerate()
        .filter_map(|(index, word)| word.starts_with(language).then_some(index))
        .collect::<Vec<_>>();
    let first_language = language_indexes.first() == Some(&0);
    let setting = language_indexes
        .iter()
        .any(|index| korean_language_selection_at(words, *index));
    let default_output = first_language
        && korean_locale_output_surface(words)
        && words
            .iter()
            .any(|word| ["사용", "작성"].iter().any(|marker| word.contains(marker)));
    let direct_response = first_language
        && words
            .iter()
            .any(|word| word.contains("답변") || word.contains("응답"))
        && words
            .iter()
            .any(|word| word.contains("해") || word.contains("작성"));
    setting || default_output || direct_response
}

fn korean_locale_correction(words: &[&str]) -> Option<IntentLocaleHintV2> {
    if contains_sequence(words, &["영어로", "하지", "말고", "한국어로"]) {
        return Some(IntentLocaleHintV2::Ko);
    }
    if contains_sequence(words, &["한국어로", "하지", "말고", "영어로"]) {
        return Some(IntentLocaleHintV2::En);
    }
    for (index, window) in words.windows(3).enumerate() {
        let excluded = if window[0].starts_with("한국어") {
            IntentLocaleHintV2::Ko
        } else if window[0].starts_with("영어") {
            IntentLocaleHintV2::En
        } else {
            continue;
        };
        if !matches!(window[1], "말고" | "대신") {
            continue;
        }
        let selected = if window[2].starts_with("한국어") {
            IntentLocaleHintV2::Ko
        } else if window[2].starts_with("영어") {
            IntentLocaleHintV2::En
        } else {
            continue;
        };
        if selected != excluded && korean_language_selection_at(words, index.saturating_add(2)) {
            return Some(selected);
        }
    }
    None
}

fn korean_language_selection_at(words: &[&str], index: usize) -> bool {
    let Some(language) = words.get(index) else {
        return false;
    };
    let tail = words.get(index.saturating_add(1)..).unwrap_or_default();
    let output_surface = korean_locale_output_surface(words);
    let action = words.iter().any(|word| {
        word.contains("답변")
            || word.contains("설정")
            || word.contains("응답")
            || word.contains("작성")
            || word.contains("써")
            || matches!(*word, "해" | "해줘" | "해주세요")
    });
    let direct_use = tail
        .first()
        .is_some_and(|word| word.contains("사용") || word.contains("설정"));
    (language.ends_with('로') && (output_surface || action))
        || (language.ends_with('를') && direct_use)
        || (output_surface
            && words
                .iter()
                .any(|word| word.contains("사용") || word.contains("작성") || action))
}

fn korean_locale_output_surface(words: &[&str]) -> bool {
    words.iter().any(|word| {
        [
            "ui",
            "기본",
            "답변",
            "로케일",
            "문구",
            "이름",
            "언어",
            "응답",
        ]
        .iter()
        .any(|marker| word.to_lowercase().contains(marker))
    })
}

pub(super) fn unsupported_locale_request(value: &str, words: &[&str]) -> bool {
    if korean_negative_directive(words) || has_korean_semantic_analysis(words) {
        return false;
    }
    let candidates = locale_candidate_tokens(words);
    let mentions_locale = !candidates.is_empty()
        || words.iter().any(|word| locale_language_token(word))
        || has_any(words, &["english", "korean"])
        || value.contains("한국어")
        || value.contains("영어");
    let direct_selection = words.first().is_some_and(|word| {
        matches!(
            *word,
            "answer" | "make" | "reply" | "respond" | "set" | "switch" | "use" | "write"
        )
    }) || locale_fragment_candidate(words).is_some()
        || generated_name_locale_default_candidate(words).is_some()
        || kept_locale_candidate(words).is_some()
        || words.iter().any(|word| {
            [
                "기본", "답변", "문구", "사용", "설정", "응답", "작성", "해줘",
            ]
            .iter()
            .any(|marker| word.contains(marker))
        });
    let output_surface =
        words.iter().any(|word| locale_surface_word(word)) || korean_locale_output_surface(words);
    mentions_locale
        && direct_selection
        && (output_surface || explicit_locale_selection(words))
        && (candidates.is_empty()
            || candidates
                .iter()
                .any(|candidate| !supported_locale_token(candidate)))
}

pub(super) fn unsupported_accumulated_locale_request(
    value: &str,
    words: &[&str],
    selected: IntentLocaleHintV2,
) -> bool {
    if selected == IntentLocaleHintV2::Unspecified {
        return false;
    }
    let negated = contains_sequence(words, &["do", "not"])
        || words
            .first()
            .is_some_and(|word| matches!(*word, "don't" | "don’t" | "dont" | "never" | "without"));
    let mentions_locale = words.iter().any(|word| locale_language_token(word))
        || value.contains("영어")
        || value.contains("한국어");
    let output_surface =
        words.iter().any(|word| locale_surface_word(word)) || korean_locale_output_surface(words);
    if negated {
        return output_surface
            && words
                .iter()
                .any(|word| locale_token_value(word) == Some(selected));
    }
    let distinct_locale = words
        .iter()
        .any(|word| locale_language_token(word) && locale_token_value(word) != Some(selected));
    mentions_locale
        && (output_surface || explicit_locale_selection(words))
        && (distinct_locale || unsupported_locale_modifier(value, words, Some(selected)))
}

pub(super) fn unsupported_locale_alternative_branch(_value: &str, words: &[&str]) -> bool {
    let candidates = locale_candidate_tokens(words);
    candidates
        .iter()
        .any(|candidate| !supported_locale_token(candidate))
        || words.first().is_some_and(|candidate| {
            locale_language_token(candidate)
                && !supported_locale_token(candidate)
                && words.get(1).is_some_and(|word| locale_surface_word(word))
        })
}

pub(super) fn unsupported_locale_modifier(
    value: &str,
    words: &[&str],
    selected: Option<IntentLocaleHintV2>,
) -> bool {
    let Some(selected) = selected else {
        return false;
    };
    let has_distinct_locale = words
        .iter()
        .any(|word| locale_language_token(word) && locale_token_value(word) != Some(selected));
    let recipe_detail_override = !has_distinct_locale
        && (value.contains("except for these exact overrides")
            || value.contains("except for the following exact overrides")
            || value.contains("except for generated names")
            || value.contains("except that the room help")
            || value.contains("except that the help")
            || value.contains("except that help")
            || value.contains("except that the ")
                && has_any(
                    words,
                    &[
                        "button", "channel", "content", "copy", "label", "name", "prefix",
                        "response", "suffix",
                    ],
                ));
    let exception = has_any(words, &["except", "excluding"]) || value.contains("제외");
    let conditional = has_any(
        words,
        &[
            "after", "before", "during", "if", "unless", "until", "when", "whenever", "while",
        ],
    );
    let restrictive = conditional || exception && !recipe_detail_override;
    let scoped_default = words
        .iter()
        .position(|word| matches!(*word, "default" | "defaults"))
        .is_some_and(|index| {
            words
                .get(index.saturating_add(1))
                .is_some_and(|word| matches!(*word, "on" | "during"))
        });
    let scoped_target = words.windows(2).any(|window| {
        matches!(window[0], "for" | "on")
            && matches!(
                window[1],
                "android"
                    | "desktop"
                    | "guests"
                    | "ios"
                    | "mobile"
                    | "weekdays"
                    | "weekends"
                    | "web"
            )
    });
    restrictive
        || scoped_default
        || scoped_target
        || has_distinct_locale && has_any(words, &["but"])
}

pub(super) fn unsupported_copy_carrier_locale_modifier(
    value: &str,
    selected: Option<IntentLocaleHintV2>,
) -> bool {
    if selected.is_none() {
        return false;
    }
    let words = words(value);
    !static_copy_carrier_tail(&words)
        || unsupported_copy_carrier_locale_context(value, &words, selected)
}

pub(super) fn legacy_copy_carrier_locale_scope(value: &str, carrier_index: usize) -> bool {
    let prefix = value.get(..carrier_index).unwrap_or_default().trim();
    if prefix.is_empty() {
        return true;
    }
    let prefix_words = words(prefix);
    prefix_words.first().is_some_and(|word| {
        matches!(
            *word,
            "a" | "an"
                | "change"
                | "customize"
                | "fallback"
                | "help"
                | "keep"
                | "override"
                | "panel"
                | "rename"
                | "room"
                | "set"
                | "the"
                | "use"
                | "변경"
                | "설정"
                | "재정의"
        )
    })
}

pub(super) fn unsupported_managed_detail_locale_grounding(
    assignment: Option<(GroundedDetailAssignment, DetailSlot)>,
    continuation: Option<&str>,
    continuation_link: Option<super::UnquotedGroundingLink>,
    selected: Option<IntentLocaleHintV2>,
    operative_consequent: bool,
) -> bool {
    if selected.is_none() {
        return false;
    }
    let Some((assignment, active_slot)) = assignment else {
        return false;
    };
    match assignment {
        GroundedDetailAssignment::Unsupported => true,
        GroundedDetailAssignment::Static if operative_consequent => true,
        GroundedDetailAssignment::Static => {
            continuation_link == Some(super::UnquotedGroundingLink::Alternative)
                || continuation.is_some_and(|value| {
                    unsupported_managed_detail_continuation(value, active_slot)
                })
        }
    }
}

pub(super) fn unsupported_copy_carrier_locale_continuation(
    value: &str,
    link: Option<super::UnquotedGroundingLink>,
    selected: Option<IntentLocaleHintV2>,
) -> bool {
    if selected.is_none() {
        return false;
    }
    let words = words(value);
    if !words.iter().any(|word| locale_language_token(word))
        && !value.contains("영어")
        && !value.contains("한국어")
    {
        return link != Some(super::UnquotedGroundingLink::Additive)
            || !supported_additive_copy_continuation(value, &words, None);
    }
    unsupported_copy_carrier_locale_context(value, &words, selected)
}

fn supported_additive_copy_continuation(
    value: &str,
    words: &[&str],
    active_slot: Option<DetailSlot>,
) -> bool {
    let static_detail =
        grounded_detail_assignment_scope(value) == Some(GroundedDetailAssignment::Static);
    let contextual_static =
        active_slot.is_some_and(|slot| grounded_static_detail_continuation(value, slot));
    let static_response = words
        .iter()
        .position(|word| matches!(*word, "response" | "responses"))
        .is_some_and(|index| valid_masked_direct_assignment_tail(&words[index + 1..]));
    let independent_post_panel = words.first() == Some(&"when")
        && words
            .iter()
            .position(|word| *word == "post")
            .is_some_and(|index| words[index.saturating_add(1)..].contains(&"panel"));
    let action_words = words.strip_prefix(&["also"]).unwrap_or(words);
    let independent_message_action = action_words
        .first()
        .is_some_and(|word| matches!(*word, "add" | "create" | "post" | "publish" | "send"))
        && action_words
            .iter()
            .any(|word| matches!(*word, "message" | "messages" | "panel"))
        && !action_words.iter().any(|word| locale_language_token(word))
        && !has_any(
            action_words,
            &["change", "customize", "override", "rename", "set", "update"],
        )
        && !unsupported_locale_condition_continuation_words(action_words);
    let independent_close_axis = !matches!(
        super::close::close_directive(value, words, false),
        AxisDirective::None
    );
    static_detail
        || contextual_static
        || static_response
        || independent_post_panel
        || independent_message_action
        || independent_close_axis
}

fn unsupported_managed_detail_continuation(value: &str, active_slot: DetailSlot) -> bool {
    let words = words(value);
    !supported_additive_copy_continuation(value, &words, Some(active_slot))
}

fn unsupported_copy_carrier_locale_context(
    value: &str,
    words: &[&str],
    selected: Option<IntentLocaleHintV2>,
) -> bool {
    let Some(selected) = selected else {
        return false;
    };
    let distinct_locale = words
        .iter()
        .any(|word| locale_language_token(word) && locale_token_value(word) != Some(selected));
    let alternative = has_any(
        words,
        &[
            "either",
            "else",
            "or",
            "otherwise",
            "versus",
            "vs",
            "또는",
            "아니면",
            "혹은",
        ],
    );
    distinct_locale || alternative || unsupported_locale_modifier(value, words, Some(selected))
}

fn static_copy_carrier_tail(words: &[&str]) -> bool {
    const CARRIERS: &[&[&str]] = &[
        &["button", "says"],
        &["button", "label"],
        &["button", "text"],
        &["displays", "the", "words"],
        &["display", "the", "words"],
        &["fallback", "panel", "title"],
        &["panel", "title"],
        &["caption", "is"],
        &["label", "is"],
        &["label", "to"],
        &["literal", "is"],
        &["named"],
        &["phrase", "is"],
        &["says"],
        &["text", "is"],
        &["text", "says"],
        &["text", "to"],
        &["under", "the", "label"],
        &["whose", "caption", "is"],
        &["with", "the", "label"],
        &["버튼", "라벨은"],
        &["버튼", "라벨을"],
        &["버튼", "글자"],
        &["패널", "제목"],
        &["라벨은"],
        &["문구는"],
        &["텍스트는"],
    ];
    CARRIERS.iter().any(|carrier| {
        words
            .strip_prefix(*carrier)
            .is_some_and(valid_masked_direct_assignment_tail)
    })
}

pub(super) fn connected_locale_modifier(
    value: &str,
    words: &[&str],
    previous: IntentLocaleHintV2,
) -> bool {
    let restrictive = has_any(words, &["but", "except", "excluding", "unless", "while"])
        || value.contains("제외");
    if !restrictive || !words.iter().any(|word| locale_surface_word(word)) {
        return false;
    }
    words.iter().any(|word| {
        locale_token_value(word).is_some_and(|locale| locale != previous)
            || plausible_locale_token(word) && !supported_locale_token(word)
    })
}

pub(super) fn exhaustive_locale_scope(value: &str, continuation: &str) -> bool {
    let continuation_words = words(continuation);
    value.contains("on desktop") && has_any(&continuation_words, &["mobile"])
        || value.contains("on mobile") && has_any(&continuation_words, &["desktop"])
}

pub(super) fn unsupported_locale_condition_continuation(value: &str) -> bool {
    let words = words(value);
    unsupported_locale_condition_continuation_words(&words)
}

fn unsupported_locale_condition_continuation_words(words: &[&str]) -> bool {
    !words.iter().any(|word| locale_language_token(word))
        && (words.first().is_some_and(|word| {
            matches!(
                *word,
                "after"
                    | "at"
                    | "before"
                    | "during"
                    | "on"
                    | "unless"
                    | "until"
                    | "when"
                    | "whenever"
                    | "while"
            )
        }) || matches!(
            words,
            ["only", "at" | "during" | "for" | "on" | "when", ..]
                | ["just", "at" | "during" | "on" | "when", ..]
        ) || has_any(
            words,
            &[
                "active",
                "approval",
                "archived",
                "desktop",
                "locked",
                "maintenance",
                "mobile",
                "scheduled",
                "weekdays",
                "weekends",
            ],
        ))
}

pub(super) fn locale_fragment_directive(words: &[&str]) -> bool {
    locale_fragment_candidate(words).is_some_and(supported_locale_token)
}

fn locale_fragment_candidate<'a>(words: &'a [&'a str]) -> Option<&'a str> {
    let candidate = words
        .first()
        .copied()
        .filter(|word| plausible_locale_token(word))?;
    if !words.get(1).is_some_and(|word| locale_surface_word(word))
        || has_any(
            words,
            &[
                "are",
                "called",
                "describe",
                "describes",
                "is",
                "means",
                "were",
            ],
        )
    {
        return None;
    }
    Some(candidate)
}

fn generated_name_locale_default_candidate<'a>(words: &'a [&'a str]) -> Option<&'a str> {
    match words {
        ["generated", "names", "at", "the", candidate, "default" | "defaults", ..]
        | ["keep", "its", "copy", "and", "generated", "names", "at", "the", candidate, "default" | "defaults", ..] => {
            Some(*candidate)
        }
        _ => None,
    }
}

fn kept_locale_candidate<'a>(words: &'a [&'a str]) -> Option<&'a str> {
    if words.first() != Some(&"keep") {
        return None;
    }
    let candidate_index = words
        .iter()
        .position(|word| matches!(*word, "default" | "defaults"))
        .and_then(|index| index.checked_sub(1))
        .or_else(|| {
            words
                .windows(2)
                .position(|window| {
                    matches!(window[0], "at" | "in") && plausible_locale_token(window[1])
                })
                .map(|index| index.saturating_add(1))
        })?;
    let candidate = *words.get(candidate_index)?;
    (plausible_locale_token(candidate)
        && words
            .iter()
            .enumerate()
            .any(|(index, word)| index != candidate_index && locale_surface_word(word)))
    .then_some(candidate)
}

fn explicit_locale_selection(words: &[&str]) -> bool {
    matches!(words, ["switch", "to", candidate, ..] if plausible_locale_token(candidate))
        || kept_locale_candidate(words).is_some()
}

pub(super) fn negated_locale_retraction(value: &str, words: &[&str]) -> bool {
    let mentions_supported = words.iter().any(|word| supported_locale_token(word));
    mentions_supported
        && (contains_sequence(words, &["do", "not", "use"])
            || matches!(words, ["don't" | "don’t" | "dont", "use", ..])
            || korean_negative_directive(words)
            || value.contains("사용하지 마")
            || value.contains("사용하지마"))
}

pub(super) fn split_locale_alternative_prefix(value: &str, words: &[&str]) -> bool {
    value.contains("between ")
        || words.starts_with(&["choose", "between"])
        || words.starts_with(&["use", "either"])
}

fn locale_candidate_tokens<'a>(words: &'a [&'a str]) -> Vec<&'a str> {
    let mut candidates = Vec::new();
    if words.first() == Some(&"use") {
        let has_surface = words.iter().skip(2).any(|word| locale_surface_word(word));
        let candidate_index = usize::from(words.get(1) == Some(&"either")).saturating_add(1);
        if let Some(candidate) = words.get(candidate_index).filter(|candidate| {
            plausible_locale_token(candidate)
                && (has_surface
                    || words.len() == candidate_index.saturating_add(1)
                        && supported_locale_token(candidate))
        }) {
            candidates.push(*candidate);
        }
        if words.len() > candidate_index.saturating_add(1) {
            for candidate in words
                .iter()
                .skip(candidate_index.saturating_add(1))
                .take(2)
                .filter(|candidate| plausible_locale_token(candidate))
            {
                push_locale_candidate(&mut candidates, candidate);
            }
        }
    }
    let setting_candidate = match words {
        ["set", "language" | "locale", "to", candidate, ..]
        | ["set", "the", "language" | "locale", "to", candidate, ..]
        | ["answer" | "reply" | "respond", "in", candidate, ..]
        | ["switch", "to", candidate, ..] => Some(*candidate),
        ["set", "response", "language", "to", candidate, ..]
        | ["set", "the", "response", "language", "to", candidate, ..]
        | ["set", "interface", "to", candidate, ..]
        | ["set", "the", "interface", "to", candidate, ..]
        | ["write", "response" | "responses", "in", candidate, ..]
        | ["the", "response" | "responses", "should" | "must", "be", "in", candidate, ..] => {
            Some(*candidate)
        }
        ["all", "ui" | "interface", "copy", "should" | "must", "be", candidate, ..]
        | ["the", "interface", "language", "should" | "must", "be", candidate, ..] => {
            Some(*candidate)
        }
        _ => None,
    };
    if let Some(candidate) = setting_candidate {
        push_locale_candidate(&mut candidates, candidate);
    }
    for (index, word) in words.iter().enumerate() {
        if alternative_word(word) {
            if let Some(candidate) = words
                .get(index.saturating_add(1))
                .filter(|candidate| plausible_locale_token(candidate))
            {
                push_locale_candidate(&mut candidates, candidate);
            }
        }
    }
    if korean_locale_output_surface(words)
        && words.iter().any(|word| {
            word.contains("사용")
                || word.contains("설정")
                || word.contains("작성")
                || matches!(*word, "해" | "해줘" | "해주세요")
        })
    {
        for candidate in words
            .iter()
            .copied()
            .filter(|word| locale_language_token(word))
        {
            push_locale_candidate(&mut candidates, candidate);
        }
    }
    if let Some(candidate) = locale_fragment_candidate(words) {
        push_locale_candidate(&mut candidates, candidate);
    }
    if let Some(candidate) = generated_name_locale_default_candidate(words) {
        push_locale_candidate(&mut candidates, candidate);
    }
    if let Some(candidate) = kept_locale_candidate(words) {
        push_locale_candidate(&mut candidates, candidate);
    }
    candidates
}

fn push_locale_candidate<'a>(candidates: &mut Vec<&'a str>, candidate: &'a str) {
    if candidates.len() < 3 && !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn locale_surface_word(word: &str) -> bool {
    matches!(
        word,
        "answer"
            | "copy"
            | "default"
            | "defaults"
            | "error"
            | "errors"
            | "interface"
            | "label"
            | "labels"
            | "language"
            | "locale"
            | "message"
            | "messages"
            | "naming"
            | "reply"
            | "respond"
            | "response"
            | "responses"
            | "throughout"
            | "ui"
            | "write"
    )
}

fn alternative_word(word: &str) -> bool {
    matches!(
        word,
        "and" | "between" | "either" | "else" | "or" | "versus" | "vs" | "또는" | "아니면" | "혹은"
    )
}

fn supported_locale_token(word: &str) -> bool {
    let normalized = word
        .strip_suffix('로')
        .or_else(|| word.strip_suffix('를'))
        .unwrap_or(word);
    matches!(normalized, "english" | "korean" | "영어" | "한국어")
}

fn plausible_locale_token(word: &str) -> bool {
    locale_language_token(word)
}

fn locale_language_token(word: &str) -> bool {
    let normalized = word
        .strip_suffix('로')
        .or_else(|| word.strip_suffix('를'))
        .unwrap_or(word);
    matches!(
        normalized,
        "arabic"
            | "chinese"
            | "dutch"
            | "french"
            | "german"
            | "italian"
            | "japanese"
            | "portuguese"
            | "polish"
            | "spanish"
            | "swedish"
            | "english"
            | "korean"
            | "독일어"
            | "스페인어"
            | "스웨덴어"
            | "아랍어"
            | "영어"
            | "이탈리아어"
            | "일본어"
            | "중국어"
            | "포르투갈어"
            | "폴란드어"
            | "프랑스어"
            | "한국어"
    )
}

fn korean_negative_directive(words: &[&str]) -> bool {
    words.iter().any(|word| {
        ["답변하지", "사용하지", "응답하지", "작성하지", "쓰지"]
            .iter()
            .any(|marker| word.contains(marker))
    }) || (words.contains(&"안")
        && words.iter().any(|word| {
            ["답변", "사용", "응답", "작성", "쓰기"]
                .iter()
                .any(|marker| word.contains(marker))
        }))
}

pub(super) fn korean_locale_default_fragment(words: &[&str]) -> Option<IntentLocaleHintV2> {
    if korean_negative_directive(words)
        || has_korean_semantic_analysis(words)
        || !has_korean_default_output(words)
    {
        return None;
    }
    let english = words.iter().any(|word| word.starts_with("영어"));
    let korean = words.iter().any(|word| word.starts_with("한국어"));
    match (english, korean) {
        (true, false) => Some(IntentLocaleHintV2::En),
        (false, true) => Some(IntentLocaleHintV2::Ko),
        _ => None,
    }
}

pub(super) fn korean_default_continuation(words: &[&str]) -> bool {
    !korean_negative_directive(words)
        && !has_korean_semantic_analysis(words)
        && words.first().is_some_and(|word| {
            ["기본", "문구", "이름", "응답", "컨트롤"]
                .iter()
                .any(|marker| word.contains(marker))
        })
        && words
            .iter()
            .any(|word| word.contains("사용") || word.contains("작성"))
}

fn has_korean_default_output(words: &[&str]) -> bool {
    words.iter().any(|word| word.contains("기본"))
        && words.iter().any(|word| {
            ["문구", "이름", "응답"]
                .iter()
                .any(|marker| word.contains(marker))
        })
}

fn has_korean_semantic_analysis(words: &[&str]) -> bool {
    words.iter().any(|word| {
        [
            "classifier",
            "classify",
            "classification",
            "detect",
            "detector",
            "detection",
            "감지",
            "검색",
            "분류",
            "분석",
            "인식",
            "탐지",
        ]
        .iter()
        .any(|marker| word.contains(marker))
    })
}

pub(super) fn inline_locale_alternative(value: &str, words: &[&str]) -> bool {
    let supported_korean_pair = value.contains("영어") && value.contains("한국어");
    let supported_ascii_pair = value.contains("english")
        && value.contains("korean")
        && (value.contains('/') || value.contains(" vs ") || value.contains(" & "));
    let candidates = locale_candidate_tokens(words);
    (has_alternative_connector(value, words) || slash_locale_alternative(value))
        && (candidates.len() > 1 || supported_korean_pair || supported_ascii_pair)
}

fn slash_locale_alternative(value: &str) -> bool {
    value.match_indices('/').any(|(index, _)| {
        let left = value
            .get(..index)
            .unwrap_or_default()
            .rsplit(|character: char| !character.is_alphanumeric())
            .next()
            .unwrap_or_default();
        let right = value
            .get(index.saturating_add(1)..)
            .unwrap_or_default()
            .split(|character: char| !character.is_alphanumeric())
            .next()
            .unwrap_or_default();
        locale_language_token(left) && locale_language_token(right)
    })
}
