use super::intent_detail_requirement::{contains_quote_delimiter, split_first_unquoted_colon};
use super::intent_detail_text::{closes_quote, normalized_whitespace, opening_quote};

const DYNAMIC_SCOPE_START_WORDS: &[&str] = &[
    "after",
    "before",
    "depending",
    "during",
    "each",
    "every",
    "if",
    "on",
    "per",
    "unless",
    "upon",
    "when",
    "whenever",
    "while",
];

const NEGATIVE_SCOPE_START_WORDS: &[&str] = &[
    "avoid", "disable", "disabled", "except", "exclude", "ignore", "never", "not", "omit", "omits",
    "without",
];

const HYPOTHETICAL_SCOPE_START_WORDS: &[&str] = &["hypothetically", "imagine", "suppose"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DefaultDetailHeaderPolicy {
    Facets {
        copy: bool,
        naming: bool,
        controls: bool,
    },
    ExactlyOneCopy,
}

pub(super) fn is_unsafe_scope(value: &str) -> bool {
    let recognized_header = split_first_unquoted_colon(value).is_some_and(|(header, _)| {
        declares_exact_override_list(header) || declares_default_detail_header(header)
    });
    if recognized_header {
        return false;
    }
    let masked = masked_lowercase(value);
    let scope = strip_scope_modifiers(&masked);
    if is_non_detail_interaction_directive(scope) {
        return false;
    }
    let words = detail_words(scope);
    let dynamic = has_dynamic_detail_context(scope);
    let negative = has_negative_detail_context(scope);
    let first_word = words.first().copied();
    let dynamic_scope = first_word.is_some_and(|word| DYNAMIC_SCOPE_START_WORDS.contains(&word))
        || scope.starts_with("as soon as ");
    let negative_scope = first_word.is_some_and(|word| NEGATIVE_SCOPE_START_WORDS.contains(&word))
        || ["do not ", "don't ", "don’t ", "dont ", "leave out "]
            .iter()
            .any(|prefix| scope.starts_with(prefix));
    let hypothetical_scope = first_word
        .is_some_and(|word| HYPOTHETICAL_SCOPE_START_WORDS.contains(&word))
        || scope.starts_with("what if ");
    let korean_scope = words.len() <= 8
        && [
            "클릭",
            "누르면",
            "경우",
            "때",
            "하지 마",
            "하지마",
            "않",
            "없이",
            "제외",
            "무시",
        ]
        .iter()
        .any(|marker| masked.contains(marker));
    ((value.trim_end().ends_with(':') || value.trim_end().ends_with('：')) && (dynamic || negative))
        || dynamic_scope
        || negative_scope
        || hypothetical_scope
        || korean_scope
}

fn is_non_detail_interaction_directive(value: &str) -> bool {
    matches!(
        value,
        "do not ask a follow-up question"
            | "do not ask follow-up questions"
            | "don't ask a follow-up question"
            | "don't ask follow-up questions"
            | "don’t ask a follow-up question"
            | "don’t ask follow-up questions"
            | "dont ask a follow-up question"
            | "dont ask follow-up questions"
    )
}

fn strip_scope_modifiers(mut value: &str) -> &str {
    loop {
        let stripped = ["please", "only"]
            .iter()
            .find_map(|prefix| strip_scope_modifier(value, prefix));
        let Some(stripped) = stripped else {
            return value;
        };
        value = stripped;
    }
}

fn strip_scope_modifier<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let value = value.strip_prefix(prefix)?;
    if value.chars().next().is_some_and(char::is_alphanumeric) {
        return None;
    }
    Some(value.trim_start_matches(|character: char| character == ',' || character.is_whitespace()))
}

pub(super) fn declares_exact_override_list(value: &str) -> bool {
    if contains_quote_delimiter(value) {
        return false;
    }
    let value = masked_lowercase(value);
    if has_forbidden_detail_context(&value) {
        return false;
    }
    let words = detail_words(&value);
    if exact_override_marker(&words) {
        return true;
    }
    if let Some(words) = words
        .strip_prefix(&["use"])
        .or_else(|| words.strip_prefix(&["apply"]))
    {
        if exact_override_marker(words) {
            return true;
        }
        let words = words
            .strip_prefix(&["english"])
            .or_else(|| words.strip_prefix(&["korean"]))
            .unwrap_or(words);
        if let Some(words) = words.strip_prefix(&["defaults", "except", "for"]) {
            if exact_override_marker(words) {
                return true;
            }
        }
    }
    matches!(
        value.as_str(),
        "정확한 재정의" | "다음 정확한 재정의" | "아래 정확한 재정의"
    )
}

fn exact_override_marker(words: &[&str]) -> bool {
    matches!(
        words,
        ["exact", "override"]
            | ["exact", "overrides"]
            | ["these", "exact", "overrides"]
            | ["following", "exact", "overrides"]
            | ["the", "following", "exact", "overrides"]
    )
}

pub(super) fn declares_default_detail_header(value: &str) -> bool {
    default_detail_header_policy(value).is_some()
}

pub(super) fn default_detail_header_policy(value: &str) -> Option<DefaultDetailHeaderPolicy> {
    if contains_quote_delimiter(value) {
        return None;
    }
    let value = masked_lowercase(value);
    let words = detail_words(&value);
    if let Some(policy) = closed_default_detail_header(&words) {
        return Some(policy);
    }
    if has_forbidden_detail_context(&value) {
        return None;
    }
    const ALLOWED: &[&str] = &[
        "and", "controls", "copy", "default", "defaults", "except", "for", "naming", "the", "use",
        "with",
    ];
    if !words.iter().all(|word| ALLOWED.contains(word))
        || !words
            .iter()
            .any(|word| matches!(*word, "default" | "defaults"))
        || words.iter().filter(|word| **word == "except").count() != 1
    {
        return None;
    }
    let exception = words.iter().position(|word| *word == "except")?;
    let exception_words = &words[exception.saturating_add(1)..];
    if !exception_words.iter().all(|word| {
        matches!(
            *word,
            "and" | "controls" | "copy" | "for" | "naming" | "the" | "with"
        )
    }) {
        return None;
    }
    let copy = exception_words.contains(&"copy");
    let naming = exception_words.contains(&"naming");
    let controls = exception_words.contains(&"controls");
    (copy || naming || controls).then_some(DefaultDetailHeaderPolicy::Facets {
        copy,
        naming,
        controls,
    })
}

fn closed_default_detail_header(words: &[&str]) -> Option<DefaultDetailHeaderPolicy> {
    let words = words.strip_prefix(&["use"])?;
    let words = words
        .strip_prefix(&["english"])
        .or_else(|| words.strip_prefix(&["korean"]))
        .unwrap_or(words);
    if matches!(words, ["defaults", "except", "for", "generated", "names"]) {
        return Some(DefaultDetailHeaderPolicy::Facets {
            copy: false,
            naming: true,
            controls: false,
        });
    }
    matches!(
        words,
        [
            "defaults", "for", "every", "name", "and", "room", "control", "with", "exactly", "one",
            "copy", "override"
        ]
    )
    .then_some(DefaultDetailHeaderPolicy::ExactlyOneCopy)
}

fn has_forbidden_detail_context(value: &str) -> bool {
    let value = masked_lowercase(value);
    has_dynamic_detail_context(&value) || has_negative_detail_context(&value)
}

fn detail_words(value: &str) -> Vec<&str> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect()
}

fn has_dynamic_detail_context(value: &str) -> bool {
    let words = detail_words(value);
    let forbidden_words = [
        "activation",
        "add",
        "adds",
        "always",
        "another",
        "before",
        "click",
        "clicked",
        "clicking",
        "clicks",
        "conditional",
        "depending",
        "determine",
        "determines",
        "during",
        "each",
        "emit",
        "emits",
        "event",
        "every",
        "forward",
        "forwards",
        "when",
        "whenever",
        "if",
        "unless",
        "after",
        "invoke",
        "invokes",
        "panel",
        "per",
        "post",
        "posts",
        "press",
        "pressed",
        "pressing",
        "prohibited",
        "publish",
        "publishes",
        "reply",
        "replies",
        "respond",
        "responds",
        "send",
        "sends",
        "sent",
        "submit",
        "submitted",
        "submits",
        "then",
        "trigger",
        "triggered",
        "triggers",
        "while",
        "delete",
        "deletes",
        "remove",
        "removes",
        "grant",
        "grants",
        "revoke",
        "revokes",
        "acquire",
        "acquires",
        "external",
        "lease",
        "webhook",
    ];
    if words.iter().any(|word| forbidden_words.contains(word)) {
        return true;
    }
    let forbidden_phrases = [
        "as soon as",
        "create another",
        "create a role",
        "create role",
        "create a channel",
        "create channel",
        "create a button",
        "create a panel",
        "create panel",
        "for every",
        "make a role",
        "make a channel",
        "on click",
        "each time",
        "upon a",
        "upon the",
    ];
    if forbidden_phrases
        .iter()
        .any(|phrase| value.contains(phrase))
    {
        return true;
    }
    let forbidden_korean = [
        "하지 마",
        "하지마",
        "않",
        "없이",
        "누르면",
        "클릭",
        "마다",
        "때",
        "경우",
        "동안",
        "전에",
        "후에",
        "그다음",
        "그 다음",
        "전송",
        "보내",
        "게시",
        "역할 생성",
        "채널 생성",
        "부여",
        "삭제",
        "제거",
        "호출",
    ];
    forbidden_korean.iter().any(|phrase| value.contains(phrase))
}

fn has_negative_detail_context(value: &str) -> bool {
    let words = detail_words(value);
    words.iter().any(|word| {
        matches!(
            *word,
            "avoid"
                | "disable"
                | "disabled"
                | "exclude"
                | "excludes"
                | "ignore"
                | "ignores"
                | "never"
                | "not"
                | "omit"
                | "omits"
                | "omitted"
                | "without"
        )
    }) || [
        "do not",
        "don't",
        "don’t",
        "dont",
        "leave out",
        "하지 마",
        "하지마",
        "않",
        "없이",
        "제외",
        "무시",
    ]
    .iter()
    .any(|phrase| value.contains(phrase))
}

fn masked_lowercase(value: &str) -> String {
    let mut masked = String::new();
    let mut active_quote = None;
    let mut previous = None;
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if let Some(expected_close) = active_quote {
            masked.push(' ');
            if closes_quote(
                character,
                expected_close,
                previous,
                characters.peek().copied(),
            ) {
                active_quote = None;
            }
            previous = Some(character);
            continue;
        }
        if let Some(expected_close) = opening_quote(character, previous, characters.peek().copied())
        {
            active_quote = Some(expected_close);
            masked.push(' ');
        } else {
            masked.extend(character.to_lowercase());
        }
        previous = Some(character);
    }
    normalized_whitespace(&masked)
}
