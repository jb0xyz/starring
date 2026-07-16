#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static CLOSED_AXIS_WORK: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(super) fn reset_closed_axis_work() {
    CLOSED_AXIS_WORK.with(|work| work.set(0));
}

#[cfg(test)]
pub(super) fn closed_axis_work() -> usize {
    CLOSED_AXIS_WORK.with(Cell::get)
}

pub(super) fn has_alternative_connector(value: &str, words: &[&str]) -> bool {
    [" or ", " versus ", " vs "]
        .iter()
        .any(|marker| value.contains(marker))
        || words
            .first()
            .is_some_and(|word| matches!(*word, "or" | "versus" | "vs"))
        || value.contains("and/or")
        || value.contains(" & ")
        || (value.contains("between ") && has_any(words, &["and"]))
        || value.contains("english/korean")
        || value.contains("korean/english")
        || [" 또는 ", " 혹은 ", " 아니면 ", " 중 하나"]
            .iter()
            .any(|marker| value.contains(marker))
}

pub(super) fn starts_alternative_prefix(value: &str) -> bool {
    value.starts_with("or ")
        || value.starts_with("or else ")
        || value.starts_with("versus ")
        || value.starts_with("vs ")
        || value.starts_with("또는 ")
        || value.starts_with("아니면 ")
        || value.starts_with("혹은 ")
}

pub(super) fn closed_axis_detector_context(value: &str) -> bool {
    let lexical_context = [
        " as detector input",
        " as classifier input",
        " as an example",
        " for classification",
        " language detection",
        " condition to detect",
        " phrase to detect",
        " policy description",
        "detector where ",
        "detects whether ",
        "detects if ",
        "달라는 요청",
        "달라고 요청",
        "요청을 감지",
        "요청을 기록",
    ]
    .iter()
    .any(|marker| value.contains(marker));
    let user_interface_context = ["customer-facing", "end-user", "user-facing"]
        .iter()
        .any(|marker| value.contains(marker))
        && [
            "copy",
            "interface",
            "label",
            "labels",
            "panel",
            "screen",
            "settings",
            "ui",
        ]
        .iter()
        .any(|marker| value.contains(marker));
    let training_context = !user_interface_context
        && (value.contains("classifier") || value.contains("detector"))
        && (value.contains(" to train ")
            || value.contains(" training")
            || value.contains(" in the classifier")
            || value.contains(" in the detector"));
    lexical_context
        || training_context
        || value.starts_with("detect ")
        || value.starts_with("classify ")
        || value.starts_with("record the phrase ")
}

pub(super) fn opens_closed_axis_detector_scope(value: &str) -> bool {
    [
        "audit automation that records",
        "audit automation that detects",
        "automation that records",
        "automation that detects",
        "detector that records",
        "detector that detects",
    ]
    .iter()
    .any(|marker| value.contains(marker))
        || value == "detect"
        || value == "classify"
        || value == "record the phrase"
        || value.starts_with("detect ")
        || value.starts_with("classify ")
        || value.starts_with("record the phrase ")
}

pub(super) fn starts_closed_axis_imperative(value: &str) -> bool {
    let words = words(value);
    let words = strip_directive_prefixes(&words);
    matches!(
        words,
        [
            "answer"
                | "disable"
                | "enable"
                | "keep"
                | "leave"
                | "omit"
                | "remove"
                | "reply"
                | "respond"
                | "set"
                | "use"
                | "write",
            ..
        ]
    ) || [
        "기본 문구",
        "닫기 기능",
        "닫기 버튼",
        "로 해줘",
        "사용해",
        "설정해",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

pub(super) fn correction_directive(value: &str) -> bool {
    [
        "actually",
        "correction",
        "instead",
        "no",
        "rather",
        "아니",
        "대신",
        "실제로",
        "정정",
        "정정하면",
        "정정해서",
    ]
    .iter()
    .any(|prefix| {
        value.strip_prefix(prefix).is_some_and(|tail| {
            tail.chars().next().is_some_and(|character| {
                character.is_whitespace() || matches!(character, ',' | ':' | '–' | '—')
            })
        })
    })
}

pub(super) fn standalone_correction(value: &str) -> bool {
    matches!(
        value.trim_matches(|character: char| {
            matches!(character, ',' | ':') || character.is_whitespace()
        }),
        "actually"
            | "correction"
            | "instead"
            | "no"
            | "rather"
            | "아니"
            | "대신"
            | "실제로"
            | "정정"
            | "정정하면"
            | "정정해서"
    )
}

pub(super) fn strip_directive_prefixes<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    while words.first().is_some_and(|word| {
        matches!(
            *word,
            "actually"
                | "correction"
                | "else"
                | "instead"
                | "no"
                | "or"
                | "please"
                | "rather"
                | "아니"
                | "대신"
                | "실제로"
                | "정정"
                | "정정하면"
                | "정정해서"
        )
    }) {
        words = &words[1..];
    }
    words
}

pub(super) fn words(value: &str) -> Vec<&str> {
    let words = value
        .split(|character: char| {
            !character.is_alphanumeric() && !matches!(character, '-' | '\'' | '\u{2019}' | '_')
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    record_closed_axis_work(words.len());
    words
}

pub(super) fn has_any(words: &[&str], candidates: &[&str]) -> bool {
    record_closed_axis_work(words.len());
    words.iter().any(|word| candidates.contains(word))
}

pub(super) fn contains_sequence(words: &[&str], sequence: &[&str]) -> bool {
    record_closed_axis_work(words.len());
    !sequence.is_empty()
        && words
            .windows(sequence.len())
            .any(|window| window == sequence)
}

#[cfg(test)]
fn record_closed_axis_work(amount: usize) {
    CLOSED_AXIS_WORK.with(|work| work.set(work.get().saturating_add(amount)));
}

#[cfg(not(test))]
fn record_closed_axis_work(_amount: usize) {}
