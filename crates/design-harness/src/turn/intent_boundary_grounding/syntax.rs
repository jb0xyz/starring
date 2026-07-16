use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

use super::super::intent_operative_conditionals::operative_consequent_start;
use super::super::intent_quote_scanner::QuotedText;

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
    " nor ",
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
    NegativeAlternative,
    ConditionalAlternative,
    Sequential,
    Scope,
    Barrier,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BoundaryUnit {
    pub(super) span: TextSpan,
    pub(super) link: UnitLink,
    pub(super) text: String,
    pub(super) hypothetical: bool,
    pub(super) non_authoritative_event: bool,
    pub(super) inherited_gate_action_negation: bool,
    pub(super) inherited_live_action_negation: bool,
    pub(super) inherited_secret_action_negation: bool,
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

pub(super) fn sentence_units(
    visible: &[char],
    sentence: TextSpan,
    question: bool,
) -> (Vec<BoundaryUnit>, Option<usize>) {
    let source = visible[sentence.start..sentence.end]
        .iter()
        .collect::<String>();
    let operative_start = operative_consequent_start(question, &source).map(|start| {
        sentence
            .start
            .saturating_add(source[..start].chars().count())
    });
    let mut units = Vec::new();
    let mut start = sentence.start;
    let mut link = UnitLink::Start;
    let mut index = sentence.start;
    let mut forced_split = operative_start;
    while index < sentence.end {
        if forced_split == Some(index) {
            push_boundary_unit(&mut units, visible, start, index, link);
            start = index;
            link = UnitLink::Additive;
            forced_split = None;
            continue;
        }
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
    (units, operative_start)
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
        non_authoritative_event: false,
        inherited_gate_action_negation: false,
        inherited_live_action_negation: false,
        inherited_secret_action_negation: false,
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
    } else if connector == " nor " {
        UnitLink::NegativeAlternative
    } else if connector == " unless " {
        UnitLink::ConditionalAlternative
    } else if matches!(connector, " after " | " before ") {
        UnitLink::Scope
    } else if matches!(
        connector,
        " and then " | " then " | " 다음 " | "한 다음 " | "한 뒤 "
    ) {
        UnitLink::Sequential
    } else if matches!(connector, " and " | " 그리고 " | "하고 " | "하며 ") {
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
    let quoted = QuotedText::scan(value);
    QuoteMask {
        visible: quoted.masked_characters(value),
        unmatched: quoted.unmatched(),
    }
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
