#[derive(Default)]
pub(super) struct QuotedLiteralScope {
    active: bool,
}

impl QuotedLiteralScope {
    pub(super) fn classify(&mut self, between: &str, suffix: &str) -> bool {
        let (local, sentence_reset) = quote_local_segment(between);
        if sentence_reset {
            self.active = false;
        }
        let local = local.trim().to_lowercase();
        let literal_here = quote_prefix_is_literal(&local);
        if let Some(reset) = last_authoritative_quote_reset_index(&local) {
            self.active = quote_prefix_is_literal(&local[reset..]);
        } else if literal_here {
            self.active = true;
        }
        self.active || quote_suffix_is_literal(suffix)
    }
}

pub(super) fn analyzes_metalinguistic_copy(unit: &str) -> bool {
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

pub(super) fn ends_metalinguistic_copy(unit: &str) -> bool {
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

pub(super) fn first_ascii_word_index(value: &str, expected: &str) -> Option<usize> {
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

pub(super) fn first_copy_carrier_index(unit: &str) -> Option<usize> {
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

pub(super) fn english_ui_owner_before(unit: &str, boundary: usize) -> bool {
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

pub(super) fn korean_ui_content_start(unit: &str) -> Option<usize> {
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

pub(super) fn metalinguistic_carrier(unit: &str) -> bool {
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

fn quote_local_segment(value: &str) -> (&str, bool) {
    let boundary = value.char_indices().rev().find_map(|(index, character)| {
        matches!(character, '.' | '!' | '?' | ';' | '\n' | '\r')
            .then_some(index.saturating_add(character.len_utf8()))
    });
    boundary.map_or((value, false), |start| (&value[start..], true))
}

fn quote_prefix_is_literal(prefix: &str) -> bool {
    let explicit_ui_copy = [
        "button labeled",
        "buttons labeled",
        "label it",
        "panel titled",
        "panels titled",
        "use the button label",
        "use the label",
        "use the panel title",
    ]
    .iter()
    .any(|carrier| prefix.contains(carrier));
    let ui_copy = english_ui_owner_before(prefix, prefix.len())
        && [
            " label",
            " label it",
            " labeled",
            " labelled",
            " text",
            " title",
            " with copy",
        ]
        .iter()
        .any(|carrier| prefix.ends_with(carrier) || prefix.contains(carrier));
    first_copy_carrier_index(prefix).is_some()
        || explicit_ui_copy
        || ui_copy
        || metalinguistic_carrier(prefix)
        || [
            "example",
            "example prompt",
            "literal",
            "sample",
            "sample prompt",
            "the user said",
            "user said",
            "예시",
            "예시 프롬프트",
            "사용자가 말함",
        ]
        .iter()
        .any(|carrier| prefix.ends_with(carrier))
}

fn quote_suffix_is_literal(suffix: &str) -> bool {
    let suffix = suffix
        .trim_start()
        .chars()
        .take(64)
        .collect::<String>()
        .to_lowercase();
    ["as documentation", "as literal text", "as text"]
        .iter()
        .any(|carrier| suffix.starts_with(carrier))
}

fn last_authoritative_quote_reset_index(value: &str) -> Option<usize> {
    [
        "apply",
        "carry out",
        "do this",
        "execute",
        "follow",
        "implement",
        "obey",
        "perform",
        "run",
        "수행",
        "실행",
        "적용",
    ]
    .iter()
    .filter_map(|marker| last_ascii_word_index_before(value, marker, value.len()))
    .filter(|start| direct_quote_reset_boundary(value, *start))
    .max()
}

fn direct_quote_reset_boundary(value: &str, start: usize) -> bool {
    let prefix = value[..start].trim_end();
    if prefix.is_empty()
        || prefix
            .chars()
            .next_back()
            .is_some_and(|character| matches!(character, ',' | ':' | '，' | '、'))
    {
        return true;
    }
    prefix.split_whitespace().next_back().is_some_and(|word| {
        matches!(
            word,
            "actually"
                | "and"
                | "but"
                | "instead"
                | "next"
                | "now"
                | "please"
                | "then"
                | "그리고"
                | "대신"
                | "이제"
        )
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
