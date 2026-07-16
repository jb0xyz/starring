use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::intent_core::IntentRecipeDetailFacetV3;
use super::intent_detail_grammar::{
    grounded_detail_assignment_with_slot, match_detail_slot, parse_detail_requirement_segment,
    strip_detail_command_prefix, unquoted_detail_literal_shape, DetailSlot, DetailValueShape,
    GroundedDetailAssignment, UnquotedDetailLiteralShape,
};
use super::intent_detail_text::{closes_quote, opening_quote};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IntentRecipeDetailFieldV4 {
    LauncherContent,
    CreateButtonLabel,
    ModalTitle,
    RoomNameLabel,
    WelcomeContentPrefix,
    WelcomeContentSuffix,
    HubAnnouncementPrefix,
    HubAnnouncementSuffix,
    CompletedResponsePrefix,
    CompletedResponseSuffix,
    ChannelNamePrefix,
    ChannelNameSuffix,
    MemberRoleNamePrefix,
    MemberRoleNameSuffix,
    HelpLabel,
    HelpResponse,
    JoinLabel,
    JoinedResponse,
    CloseLabel,
    ClosedResponse,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IntentRecipeDetailExpectationV4 {
    field: IntentRecipeDetailFieldV4,
    literal: String,
}

impl IntentRecipeDetailExpectationV4 {
    pub(crate) fn field(&self) -> IntentRecipeDetailFieldV4 {
        self.field
    }

    pub(crate) fn literal(&self) -> &str {
        &self.literal
    }
}

impl IntentRecipeDetailFieldV4 {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::LauncherContent => "launcher_content",
            Self::CreateButtonLabel => "create_button_label",
            Self::ModalTitle => "modal_title",
            Self::RoomNameLabel => "room_name_label",
            Self::WelcomeContentPrefix => "welcome_content_prefix",
            Self::WelcomeContentSuffix => "welcome_content_suffix",
            Self::HubAnnouncementPrefix => "hub_announcement_prefix",
            Self::HubAnnouncementSuffix => "hub_announcement_suffix",
            Self::CompletedResponsePrefix => "completed_response_prefix",
            Self::CompletedResponseSuffix => "completed_response_suffix",
            Self::ChannelNamePrefix => "channel_name_prefix",
            Self::ChannelNameSuffix => "channel_name_suffix",
            Self::MemberRoleNamePrefix => "member_role_name_prefix",
            Self::MemberRoleNameSuffix => "member_role_name_suffix",
            Self::HelpLabel => "help_label",
            Self::HelpResponse => "help_response",
            Self::JoinLabel => "join_label",
            Self::JoinedResponse => "joined_response",
            Self::CloseLabel => "close_label",
            Self::ClosedResponse => "closed_response",
        }
    }

    pub(crate) fn facet(self) -> IntentRecipeDetailFacetV3 {
        match self {
            Self::LauncherContent
            | Self::CreateButtonLabel
            | Self::ModalTitle
            | Self::RoomNameLabel
            | Self::WelcomeContentPrefix
            | Self::WelcomeContentSuffix
            | Self::HubAnnouncementPrefix
            | Self::HubAnnouncementSuffix
            | Self::CompletedResponsePrefix
            | Self::CompletedResponseSuffix => IntentRecipeDetailFacetV3::Copy,
            Self::ChannelNamePrefix
            | Self::ChannelNameSuffix
            | Self::MemberRoleNamePrefix
            | Self::MemberRoleNameSuffix => IntentRecipeDetailFacetV3::Naming,
            Self::HelpLabel
            | Self::HelpResponse
            | Self::JoinLabel
            | Self::JoinedResponse
            | Self::CloseLabel
            | Self::ClosedResponse => IntentRecipeDetailFacetV3::Controls,
        }
    }
}

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
            value: detail_value_identity(&segment, slot)?,
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

pub(super) fn grounded_detail_assignment_scope(
    requirement: &str,
) -> Option<GroundedDetailAssignment> {
    grounded_detail_assignment_scope_with_slot(requirement).map(|(assignment, _)| assignment)
}

pub(super) fn grounded_detail_assignment_scope_with_slot(
    requirement: &str,
) -> Option<(GroundedDetailAssignment, DetailSlot)> {
    let mut observed = None;
    for segment in detail_requirement_segments(requirement) {
        let tokens = closed_detail_syntax_tokens(&segment)?;
        let tokens = tokens.iter().map(String::as_str).collect::<Vec<_>>();
        match grounded_detail_assignment_with_slot(&tokens) {
            Some((GroundedDetailAssignment::Unsupported, slot)) => {
                return Some((GroundedDetailAssignment::Unsupported, slot));
            }
            Some((GroundedDetailAssignment::Static, slot)) => {
                observed = Some((GroundedDetailAssignment::Static, slot));
            }
            None => {}
        }
    }
    observed
}

pub(super) fn grounded_static_detail_continuation(
    requirement: &str,
    active_slot: DetailSlot,
) -> bool {
    parse_detail_requirement_segment(requirement, Some(active_slot)).is_some()
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
    Material(String),
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

    pub(super) fn material_expectation(&self) -> Option<IntentRecipeDetailExpectationV4> {
        let DetailValueIdentity::Material(literal) = &self.value else {
            return None;
        };
        Some(IntentRecipeDetailExpectationV4 {
            field: detail_assignment_field(self.slot, self.part),
            literal: literal.clone(),
        })
    }
}

pub(super) fn canonical_material_detail_expectations(
    assignments: &[DetailAssignmentClaim],
) -> Vec<IntentRecipeDetailExpectationV4> {
    assignments
        .iter()
        .filter_map(DetailAssignmentClaim::material_expectation)
        .map(|expectation| (expectation.field, expectation))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect()
}

#[cfg(test)]
pub(super) fn canonical_material_detail_fields(
    assignments: &[DetailAssignmentClaim],
) -> Vec<IntentRecipeDetailFieldV4> {
    canonical_material_detail_expectations(assignments)
        .iter()
        .map(IntentRecipeDetailExpectationV4::field)
        .collect()
}

fn detail_assignment_field(slot: DetailSlot, part: DetailValuePart) -> IntentRecipeDetailFieldV4 {
    match (slot, part) {
        (DetailSlot::LauncherContent, DetailValuePart::Direct) => {
            IntentRecipeDetailFieldV4::LauncherContent
        }
        (DetailSlot::CreateButtonLabel, DetailValuePart::Direct) => {
            IntentRecipeDetailFieldV4::CreateButtonLabel
        }
        (DetailSlot::ModalTitle, DetailValuePart::Direct) => IntentRecipeDetailFieldV4::ModalTitle,
        (DetailSlot::RoomNameLabel, DetailValuePart::Direct) => {
            IntentRecipeDetailFieldV4::RoomNameLabel
        }
        (DetailSlot::WelcomeContent, DetailValuePart::Prefix) => {
            IntentRecipeDetailFieldV4::WelcomeContentPrefix
        }
        (DetailSlot::WelcomeContent, DetailValuePart::Suffix) => {
            IntentRecipeDetailFieldV4::WelcomeContentSuffix
        }
        (DetailSlot::HubAnnouncement, DetailValuePart::Prefix) => {
            IntentRecipeDetailFieldV4::HubAnnouncementPrefix
        }
        (DetailSlot::HubAnnouncement, DetailValuePart::Suffix) => {
            IntentRecipeDetailFieldV4::HubAnnouncementSuffix
        }
        (DetailSlot::CompletedResponse, DetailValuePart::Prefix) => {
            IntentRecipeDetailFieldV4::CompletedResponsePrefix
        }
        (DetailSlot::CompletedResponse, DetailValuePart::Suffix) => {
            IntentRecipeDetailFieldV4::CompletedResponseSuffix
        }
        (DetailSlot::ChannelName, DetailValuePart::Prefix) => {
            IntentRecipeDetailFieldV4::ChannelNamePrefix
        }
        (DetailSlot::ChannelName, DetailValuePart::Suffix) => {
            IntentRecipeDetailFieldV4::ChannelNameSuffix
        }
        (DetailSlot::MemberRoleName, DetailValuePart::Prefix) => {
            IntentRecipeDetailFieldV4::MemberRoleNamePrefix
        }
        (DetailSlot::MemberRoleName, DetailValuePart::Suffix) => {
            IntentRecipeDetailFieldV4::MemberRoleNameSuffix
        }
        (DetailSlot::HelpLabel, DetailValuePart::Direct) => IntentRecipeDetailFieldV4::HelpLabel,
        (DetailSlot::HelpResponse, DetailValuePart::Direct) => {
            IntentRecipeDetailFieldV4::HelpResponse
        }
        (DetailSlot::JoinLabel, DetailValuePart::Direct) => IntentRecipeDetailFieldV4::JoinLabel,
        (DetailSlot::JoinResponse, DetailValuePart::Direct) => {
            IntentRecipeDetailFieldV4::JoinedResponse
        }
        (DetailSlot::CloseLabel, DetailValuePart::Direct) => IntentRecipeDetailFieldV4::CloseLabel,
        (DetailSlot::CloseResponse, DetailValuePart::Direct) => {
            IntentRecipeDetailFieldV4::ClosedResponse
        }
        (
            DetailSlot::LauncherContent
            | DetailSlot::CreateButtonLabel
            | DetailSlot::ModalTitle
            | DetailSlot::RoomNameLabel
            | DetailSlot::HelpLabel
            | DetailSlot::HelpResponse
            | DetailSlot::JoinLabel
            | DetailSlot::JoinResponse
            | DetailSlot::CloseLabel
            | DetailSlot::CloseResponse,
            DetailValuePart::Prefix | DetailValuePart::Suffix,
        )
        | (
            DetailSlot::WelcomeContent
            | DetailSlot::HubAnnouncement
            | DetailSlot::CompletedResponse
            | DetailSlot::ChannelName
            | DetailSlot::MemberRoleName,
            DetailValuePart::Direct,
        ) => unreachable!(),
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

fn detail_value_identity(segment: &str, slot: DetailSlot) -> Option<DetailValueIdentity> {
    let tokens = closed_detail_syntax_tokens(segment)?;
    if tokens.iter().any(|token| token == LITERAL_SENTINEL) {
        return quoted_detail_literal(segment)
            .map(|literal| literal.replace("\r\n", "\n"))
            .filter(|literal| !literal.trim().is_empty())
            .filter(|literal| {
                slot.value_shape() == DetailValueShape::Affix || literal.trim() == literal
            })
            .map(DetailValueIdentity::Material);
    }
    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "empty" | "빈" | "비운" | "비어"))
    {
        return Some(DetailValueIdentity::Empty);
    }
    unquoted_detail_literal(segment, slot).map(DetailValueIdentity::Material)
}

fn unquoted_detail_literal(segment: &str, slot: DetailSlot) -> Option<String> {
    let shape = unquoted_detail_literal_shape(segment, slot)?;
    let segment = segment
        .trim()
        .trim_end_matches(['.', '!', '?', '。', '！', '？'])
        .trim_end();
    let literal = match shape {
        UnquotedDetailLiteralShape::FinalToken => {
            segment.split_whitespace().next_back()?.to_owned()
        }
        UnquotedDetailLiteralShape::KoreanDirect => korean_unquoted_direct_literal(segment, slot)?,
        UnquotedDetailLiteralShape::KoreanAffix => korean_unquoted_affix_literal(segment, slot)?,
    };
    (!literal.is_empty()).then_some(literal)
}

fn korean_unquoted_affix_literal(segment: &str, expected_slot: DetailSlot) -> Option<String> {
    let tokens = closed_detail_syntax_tokens(segment)?;
    let token_values = tokens.iter().map(String::as_str).collect::<Vec<_>>();
    let stripped = strip_detail_command_prefix(&token_values);
    let (slot, tail) = match_detail_slot(stripped)?;
    if slot != expected_slot {
        return None;
    }
    let tail_start = token_values.len().checked_sub(tail.len())?;
    let spans = unquoted_alphanumeric_spans(segment);
    if spans.len() != token_values.len() {
        return None;
    }
    let value_start = spans.get(tail_start)?.1;
    let terminal_start = spans.last()?.0;
    let with_particle = segment.get(value_start..terminal_start)?.trim();
    let literal = strip_korean_assignment_particle(with_particle)?.trim();
    (!literal.is_empty() && literal.split_whitespace().count() <= 1).then(|| literal.to_owned())
}

fn korean_unquoted_direct_literal(segment: &str, expected_slot: DetailSlot) -> Option<String> {
    let tokens = closed_detail_syntax_tokens(segment)?;
    let token_values = tokens.iter().map(String::as_str).collect::<Vec<_>>();
    let stripped = strip_detail_command_prefix(&token_values);
    let (slot, tail) = match_detail_slot(stripped)?;
    if slot != expected_slot {
        return None;
    }
    let tail_start = token_values.len().checked_sub(tail.len())?;
    let spans = unquoted_alphanumeric_spans(segment);
    if spans.len() != token_values.len() || tail_start == 0 {
        return None;
    }
    let value_start = spans.get(tail_start.checked_sub(1)?)?.1;
    let terminal_start = spans.last()?.0;
    let with_particle = segment.get(value_start..terminal_start)?.trim();
    let literal = strip_korean_assignment_particle(with_particle)?.trim();
    let whitespace_tokens = literal.split_whitespace().count();
    (!literal.is_empty() && whitespace_tokens <= 2).then(|| literal.to_owned())
}

fn unquoted_alphanumeric_spans(value: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (index, character) in value.char_indices() {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            spans.push((start, index));
        }
    }
    if let Some(start) = start {
        spans.push((start, value.len()));
    }
    spans
}

fn strip_korean_assignment_particle(value: &str) -> Option<&str> {
    value
        .strip_suffix(" 로")
        .or_else(|| value.strip_suffix("으로"))
        .or_else(|| value.strip_suffix("로"))
        .map(str::trim_end)
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
    let segment = current.trim().to_owned();
    current.clear();
    if !segment.is_empty() {
        segments.push(segment);
    }
}
