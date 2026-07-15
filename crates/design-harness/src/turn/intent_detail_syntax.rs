use std::collections::BTreeSet;

use super::intent_core::IntentRecipeDetailFacetV3;
use super::intent_detail_grammar::{
    parse_detail_requirement_segment, strip_detail_command_prefix, DetailSlot, DetailValueShape,
};
use super::intent_detail_text::{closes_quote, normalized_whitespace, opening_quote};

pub(super) const LITERAL_SENTINEL: &str = "__literal__";

const DETAIL_REQUIREMENT_CONNECTORS: &[&str] = &[
    " as well as ",
    " along with ",
    " and ",
    " plus ",
    "하고 ",
    "하며 ",
    " 그리고 ",
    " 또한 ",
    " 및 ",
    " 또 ",
];

#[cfg(test)]
pub(super) fn supported_detail_facets(requirement: &str) -> Option<Vec<IntentRecipeDetailFacetV3>> {
    let syntax = supported_detail_syntax(requirement)?;
    syntax
        .all_slots_have_material_values()
        .then_some(syntax.facets)
}

pub(super) fn supported_detail_syntax(requirement: &str) -> Option<SupportedDetailSyntax> {
    if has_unsupported_structural_prefix(requirement) {
        return None;
    }
    let segments = detail_requirement_segments(requirement);
    if segments.is_empty() {
        return None;
    }
    let mut active_slot = None;
    let mut facets = BTreeSet::new();
    let mut assignments: Vec<DetailAssignmentClaim> = Vec::new();
    for segment in segments {
        let slot = parse_detail_requirement_segment(&segment, active_slot)?;
        let assignment = DetailAssignmentClaim {
            slot,
            part: detail_value_part(&segment, slot)?,
            value: detail_value_identity(&segment)?,
        };
        if let Some(existing) = assignments
            .iter()
            .find(|existing| existing.same_target(&assignment))
        {
            if !existing.same_value(&assignment) {
                return None;
            }
        } else {
            assignments.push(assignment);
        }
        facets.insert(slot.facet());
        active_slot = Some(slot);
    }
    Some(SupportedDetailSyntax {
        facets: facets.into_iter().collect(),
        assignments,
    })
}

pub(super) fn supported_detail_fragment(requirement: &str) -> bool {
    if has_unsupported_structural_prefix(requirement) {
        return false;
    }
    if supported_detail_syntax(requirement).is_some() {
        return true;
    }
    let segments = detail_requirement_segments(requirement);
    if segments.len() != 1 {
        return false;
    }
    let Some(tokens) = closed_detail_syntax_tokens(&segments[0]) else {
        return false;
    };
    let tokens = tokens.iter().map(String::as_str).collect::<Vec<_>>();
    let tokens = strip_detail_command_prefix(&tokens);
    let tokens = tokens.strip_prefix(&["its"]).unwrap_or(tokens);
    matches!(
        tokens,
        ["ephemeral", "response", LITERAL_SENTINEL]
            | ["ephemeral", "response", "is", LITERAL_SENTINEL]
            | ["ephemeral", "response", "is", "exactly", LITERAL_SENTINEL]
            | ["ephemeral", "response", "to", LITERAL_SENTINEL]
            | ["ephemeral", "message", LITERAL_SENTINEL]
            | ["ephemeral", "message", "is", LITERAL_SENTINEL]
            | ["일회성", "응답을", LITERAL_SENTINEL]
            | ["에페메랄", "응답을", LITERAL_SENTINEL]
    )
}

fn has_unsupported_structural_prefix(value: &str) -> bool {
    let value = value.trim_start();
    if value.starts_with('>')
        || value.starts_with('#')
        || value.starts_with("- ")
        || value.starts_with("* ")
        || value.starts_with("• ")
    {
        return true;
    }
    value.split_once(". ").is_some_and(|(prefix, _)| {
        !prefix.is_empty() && prefix.chars().all(|character| character.is_ascii_digit())
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetailValuePart {
    Direct,
    Prefix,
    Suffix,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DetailValueIdentity {
    Empty,
    Quoted(String),
    Unquoted(String),
}

pub(super) struct SupportedDetailSyntax {
    facets: Vec<IntentRecipeDetailFacetV3>,
    assignments: Vec<DetailAssignmentClaim>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DetailAssignmentClaim {
    slot: DetailSlot,
    part: DetailValuePart,
    value: DetailValueIdentity,
}

impl SupportedDetailSyntax {
    pub(super) fn facets(&self) -> &[IntentRecipeDetailFacetV3] {
        &self.facets
    }

    pub(super) fn assignments(&self) -> &[DetailAssignmentClaim] {
        &self.assignments
    }

    #[cfg(test)]
    fn all_slots_have_material_values(&self) -> bool {
        self.assignments.iter().all(|assignment| {
            self.assignments.iter().any(|candidate| {
                candidate.slot == assignment.slot && candidate.has_material_value()
            })
        })
    }
}

impl DetailAssignmentClaim {
    pub(super) fn same_target(&self, other: &Self) -> bool {
        self.slot == other.slot && self.part == other.part
    }

    pub(super) fn same_value(&self, other: &Self) -> bool {
        self.value == other.value
    }

    pub(super) fn same_slot(&self, other: &Self) -> bool {
        self.slot == other.slot
    }

    pub(super) fn has_material_value(&self) -> bool {
        self.value != DetailValueIdentity::Empty
    }
}

fn detail_value_part(segment: &str, slot: DetailSlot) -> Option<DetailValuePart> {
    if slot.value_shape() == DetailValueShape::Direct {
        return Some(DetailValuePart::Direct);
    }
    let tokens = closed_detail_syntax_tokens(segment)?;
    if tokens
        .iter()
        .any(|token| token == "prefix" || token.starts_with("접두"))
    {
        return Some(DetailValuePart::Prefix);
    }
    if tokens
        .iter()
        .any(|token| token == "suffix" || token.starts_with("접미"))
    {
        return Some(DetailValuePart::Suffix);
    }
    None
}

fn detail_value_identity(segment: &str) -> Option<DetailValueIdentity> {
    let tokens = closed_detail_syntax_tokens(segment)?;
    if tokens.iter().any(|token| token == LITERAL_SENTINEL) {
        return quoted_detail_literal(segment).map(DetailValueIdentity::Quoted);
    }
    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "empty" | "빈" | "비운" | "비어"))
    {
        return Some(DetailValueIdentity::Empty);
    }
    Some(DetailValueIdentity::Unquoted(
        normalized_whitespace(segment).to_lowercase(),
    ))
}

fn quoted_detail_literal(value: &str) -> Option<String> {
    let mut active_quote = None;
    let mut previous = None;
    let mut literal = String::new();
    let mut found = None;
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if let Some(expected_close) = active_quote {
            if closes_quote(
                character,
                expected_close,
                previous,
                characters.peek().copied(),
            ) {
                if literal.is_empty() || found.is_some() {
                    return None;
                }
                found = Some(std::mem::take(&mut literal));
                active_quote = None;
            } else {
                literal.push(character);
            }
            previous = Some(character);
            continue;
        }
        if let Some(expected_close) = opening_quote(character, previous, characters.peek().copied())
        {
            active_quote = Some(expected_close);
        }
        previous = Some(character);
    }
    active_quote.is_none().then_some(found).flatten()
}

pub(super) fn closed_detail_syntax_tokens(value: &str) -> Option<Vec<String>> {
    let characters = value.chars().collect::<Vec<_>>();
    let mut syntax = String::new();
    let mut active_quote = None;
    let mut quote_has_content = false;
    for (index, character) in characters.iter().copied().enumerate() {
        if let Some(expected_close) = active_quote {
            let previous = index.checked_sub(1).map(|previous| characters[previous]);
            let next = characters.get(index.saturating_add(1)).copied();
            if closes_quote(character, expected_close, previous, next) {
                active_quote = None;
                if quote_has_content {
                    syntax.push_str(LITERAL_SENTINEL);
                }
                quote_has_content = false;
                syntax.push(' ');
            } else if !character.is_whitespace() {
                quote_has_content = true;
            }
            continue;
        }
        let previous = index.checked_sub(1).map(|previous| characters[previous]);
        let next = characters.get(index.saturating_add(1)).copied();
        if let Some(expected_close) = opening_quote(character, previous, next) {
            active_quote = Some(expected_close);
            quote_has_content = false;
            syntax.push(' ');
        } else if character.is_alphanumeric() {
            syntax.extend(character.to_lowercase());
        } else {
            syntax.push(' ');
        }
    }
    if active_quote.is_some() {
        return None;
    }
    Some(syntax.split_whitespace().map(str::to_string).collect())
}

fn detail_requirement_segments(value: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut active_quote = None;
    let mut previous = None;
    let mut index = 0;

    while index < value.len() {
        let rest = &value[index..];
        if active_quote.is_none() {
            if let Some(connector_len) = detail_requirement_connector_len(rest) {
                push_detail_requirement_segment(&mut segments, &mut current);
                index += connector_len;
                previous = None;
                continue;
            }
        }
        let character = rest.chars().next().unwrap();
        let next = rest[character.len_utf8()..].chars().next();
        current.push(character);
        if let Some(expected_close) = active_quote {
            if closes_quote(character, expected_close, previous, next) {
                active_quote = None;
            }
        } else {
            active_quote = opening_quote(character, previous, next);
        }
        previous = Some(character);
        index += character.len_utf8();
    }
    push_detail_requirement_segment(&mut segments, &mut current);
    segments
}

pub(super) fn detail_requirement_connector_len(value: &str) -> Option<usize> {
    DETAIL_REQUIREMENT_CONNECTORS.iter().find_map(|connector| {
        value
            .get(..connector.len())
            .filter(|candidate| candidate.eq_ignore_ascii_case(connector))
            .map(str::len)
    })
}

fn push_detail_requirement_segment(segments: &mut Vec<String>, current: &mut String) {
    let segment = normalized_whitespace(current);
    current.clear();
    if !segment.is_empty() {
        segments.push(segment);
    }
}
