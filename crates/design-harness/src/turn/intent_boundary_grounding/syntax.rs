use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

const BOUNDARY_UNIT_CONNECTORS: &[&str] = &[
    " in order to ",
    " because ",
    " and then ",
    " but then ",
    " so that ",
    " however ",
    " instead ",
    " before ",
    " after ",
    " while ",
    " when ",
    " until ",
    " unless ",
    " yet ",
    " then ",
    " and ",
    " but ",
    " or ",
    "하기 전에 ",
    "한 후에 ",
    "하는 동안 ",
    "할 때 ",
    "하도록 ",
    " 그리고 ",
    " 하지만 ",
    " 그러나 ",
    " 대신 ",
    " 다음 ",
    "한 다음 ",
    "한 뒤 ",
    "하면서 ",
    "하고 ",
    "하며 ",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TextSpan {
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UnitLink {
    Start,
    Additive,
    Alternative,
    Barrier,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BoundaryUnit {
    pub(super) span: TextSpan,
    pub(super) link: UnitLink,
    pub(super) text: String,
    pub(super) hypothetical: bool,
    pub(super) inherited_action_negation: bool,
}

#[derive(Clone, Copy)]
struct QuoteState {
    end: char,
    fence_len: usize,
    start: usize,
}

pub(super) struct QuoteMask {
    pub(super) visible: Vec<char>,
    pub(super) unmatched: bool,
}

pub(super) fn sentence_spans(visible: &[char]) -> Vec<(TextSpan, bool)> {
    let mut spans = Vec::new();
    let mut start = 0usize;
    for (index, character) in visible.iter().enumerate() {
        if is_sentence_boundary(*character) {
            if let Some(span) = trimmed_span(visible, start, index) {
                spans.push((span, is_question_mark(*character)));
            }
            start = index.saturating_add(1);
        }
    }
    if let Some(span) = trimmed_span(visible, start, visible.len()) {
        spans.push((span, false));
    }
    spans
}

pub(super) fn sentence_units(visible: &[char], sentence: TextSpan) -> Vec<BoundaryUnit> {
    let mut units = Vec::new();
    let mut start = sentence.start;
    let mut link = UnitLink::Start;
    let mut index = sentence.start;
    while index < sentence.end {
        if matches!(visible[index], ',' | '，' | '、') {
            push_boundary_unit(&mut units, visible, start, index, link);
            start = index.saturating_add(1);
            link = UnitLink::Additive;
            index = start;
            continue;
        }
        if let Some((length, next_link)) = connector_at(visible, index, sentence.end) {
            push_boundary_unit(&mut units, visible, start, index, link);
            index = index.saturating_add(length);
            start = index;
            link = next_link;
            continue;
        }
        index = index.saturating_add(1);
    }
    push_boundary_unit(&mut units, visible, start, sentence.end, link);
    units
}

fn push_boundary_unit(
    units: &mut Vec<BoundaryUnit>,
    visible: &[char],
    start: usize,
    end: usize,
    link: UnitLink,
) {
    let Some(span) = trimmed_span(visible, start, end) else {
        return;
    };
    let text = normalized_text(
        &visible[span.start..span.end]
            .iter()
            .collect::<String>()
            .to_lowercase(),
    );
    units.push(BoundaryUnit {
        span,
        link,
        text,
        hypothetical: false,
        inherited_action_negation: false,
    });
}

fn trimmed_span(value: &[char], start: usize, end: usize) -> Option<TextSpan> {
    let start = (start..end).find(|index| !value[*index].is_whitespace())?;
    let end = (start..end)
        .rfind(|index| !value[*index].is_whitespace())?
        .saturating_add(1);
    Some(TextSpan { start, end })
}

fn connector_at(visible: &[char], start: usize, end: usize) -> Option<(usize, UnitLink)> {
    BOUNDARY_UNIT_CONNECTORS
        .iter()
        .filter_map(|connector| {
            let connector_length = connector.chars().count();
            let connector_end = start.saturating_add(connector_length);
            (connector_end <= end
                && ascii_case_insensitive_str_equal(&visible[start..connector_end], connector))
            .then_some((connector_length, connector_link(connector)))
        })
        .max_by_key(|(length, _)| *length)
}

fn connector_link(connector: &str) -> UnitLink {
    if connector == " or " {
        UnitLink::Alternative
    } else if matches!(
        connector,
        " and then "
            | " then "
            | " and "
            | " 그리고 "
            | " 다음 "
            | "한 다음 "
            | "한 뒤 "
            | "하고 "
            | "하며 "
    ) {
        UnitLink::Additive
    } else {
        UnitLink::Barrier
    }
}

fn ascii_case_insensitive_str_equal(left: &[char], right: &str) -> bool {
    left.iter().zip(right.chars()).all(|(left, right)| {
        *left == right || (left.is_ascii() && right.is_ascii() && left.eq_ignore_ascii_case(&right))
    })
}

pub(super) fn ascii_case_insensitive_chars_equal(left: &[char], right: &[char]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left == right
                || (left.is_ascii() && right.is_ascii() && left.eq_ignore_ascii_case(right))
        })
}

pub(super) fn word_continuation(character: char) -> bool {
    if character.is_whitespace() {
        return false;
    }
    matches!(
        character.general_category_group(),
        GeneralCategoryGroup::Letter
            | GeneralCategoryGroup::Mark
            | GeneralCategoryGroup::Number
            | GeneralCategoryGroup::Other
    ) || matches!(
        character.general_category(),
        GeneralCategory::ConnectorPunctuation | GeneralCategory::DashPunctuation
    )
}

pub(super) fn mask_quoted_text(value: &str) -> QuoteMask {
    let characters = value.chars().collect::<Vec<_>>();
    let mut masked = characters.clone();
    let mut quote: Option<QuoteState> = None;
    let mut index = 0usize;
    while index < characters.len() {
        if let Some(active) = quote {
            if active.end == '`' && characters[index] == '`' {
                let run = repeated_character_count(&characters, index, '`');
                if run >= active.fence_len {
                    for value in masked.iter_mut().skip(index).take(active.fence_len) {
                        *value = ' ';
                    }
                    index = index.saturating_add(active.fence_len);
                    quote = None;
                    continue;
                }
            } else if characters[index] == active.end
                && !is_escaped(&characters, index)
                && !is_inner_apostrophe(&characters, index)
            {
                masked[index] = ' ';
                index = index.saturating_add(1);
                quote = None;
                continue;
            }
            masked[index] = ' ';
            index = index.saturating_add(1);
            continue;
        }

        let Some((end, fence_len)) = opening_quote(&characters, index) else {
            index = index.saturating_add(1);
            continue;
        };
        if is_escaped(&characters, index) || is_inner_apostrophe(&characters, index) {
            index = index.saturating_add(1);
            continue;
        }
        let start = index;
        for value in masked.iter_mut().skip(index).take(fence_len) {
            *value = ' ';
        }
        index = index.saturating_add(fence_len);
        quote = Some(QuoteState {
            end,
            fence_len,
            start,
        });
    }
    let unmatched = quote.is_some();
    if let Some(active) = quote {
        masked[active.start..].copy_from_slice(&characters[active.start..]);
    }
    QuoteMask {
        visible: masked,
        unmatched,
    }
}

fn opening_quote(characters: &[char], index: usize) -> Option<(char, usize)> {
    match characters[index] {
        '"' => Some(('"', 1)),
        '\'' => Some(('\'', 1)),
        '`' => Some(('`', repeated_character_count(characters, index, '`'))),
        '“' => Some(('”', 1)),
        '‘' => Some(('’', 1)),
        '«' => Some(('»', 1)),
        '‹' => Some(('›', 1)),
        '〈' => Some(('〉', 1)),
        '《' => Some(('》', 1)),
        '「' => Some(('」', 1)),
        '『' => Some(('』', 1)),
        '【' => Some(('】', 1)),
        _ => None,
    }
}

fn repeated_character_count(characters: &[char], start: usize, expected: char) -> usize {
    characters[start..]
        .iter()
        .take_while(|character| **character == expected)
        .count()
}

fn is_escaped(characters: &[char], index: usize) -> bool {
    let preceding_slashes = characters[..index]
        .iter()
        .rev()
        .take_while(|character| **character == '\\')
        .count();
    preceding_slashes % 2 == 1
}

fn is_inner_apostrophe(characters: &[char], index: usize) -> bool {
    characters[index] == '\''
        && index > 0
        && index + 1 < characters.len()
        && characters[index - 1].is_alphanumeric()
        && characters[index + 1].is_alphanumeric()
}

fn is_sentence_boundary(character: char) -> bool {
    matches!(
        character,
        '.' | '!' | '?' | ';' | '\n' | '\r' | '。' | '！' | '？'
    )
}

fn is_question_mark(character: char) -> bool {
    matches!(character, '?' | '？')
}

pub(super) fn normalized_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
