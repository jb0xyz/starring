use std::collections::BTreeSet;

use super::intent_core::IntentRecipeDetailFacetV3;
use super::intent_detail_grammar::punctuation_continues_korean_assignment;
use super::intent_detail_policy::{
    declares_exact_override_list, default_detail_header_policy, is_unsafe_scope,
    DefaultDetailHeaderPolicy,
};
use super::intent_detail_syntax::{
    canonical_material_detail_expectations, detail_requirement_connector_len,
    supported_detail_fragment, supported_detail_syntax, DetailAssignmentClaim,
    IntentRecipeDetailExpectationV4, IntentRecipeDetailFieldV4,
};
use super::intent_detail_text::{closes_quote, normalized_whitespace, opening_quote};

pub(crate) struct PrivateStudyRoomDetailAnalysis {
    facets: Vec<IntentRecipeDetailFacetV3>,
    expectations: Vec<IntentRecipeDetailExpectationV4>,
    fields: Vec<IntentRecipeDetailFieldV4>,
    normalized_human: String,
    evidence_entries: Vec<IndexedDetailEvidence>,
}

struct DetailEvidenceEntry {
    text: String,
    facets: Vec<IntentRecipeDetailFacetV3>,
    assignments: Vec<DetailAssignmentClaim>,
}

struct HumanDetailSentence {
    text: String,
    terminator: Option<char>,
}

struct IndexedDetailEvidence {
    text: String,
    outside_quote: Vec<bool>,
    quote_delimiter_prefix: Vec<usize>,
    previous_boundary_end: Vec<usize>,
    next_boundary_start: Vec<usize>,
}

enum MultilineDetailEvidence {
    Absent,
    Invalid,
    Valid(Vec<DetailEvidenceEntry>),
}

impl IndexedDetailEvidence {
    fn new(text: String) -> Self {
        let outside_quote = quote_boundary_index(&text);
        let quote_delimiter_prefix = quote_delimiter_prefix_index(&text);
        let boundaries = coordination_boundaries(&text);
        let (previous_boundary_end, next_boundary_start) =
            coordination_boundary_index(text.len(), &boundaries);
        Self {
            text,
            outside_quote,
            quote_delimiter_prefix,
            previous_boundary_end,
            next_boundary_start,
        }
    }

    fn supported_occurrences(&self, requirement: &str) -> usize {
        let mut supported = 0;
        for (start, _) in self.text.match_indices(requirement) {
            let end = start + requirement.len();
            if !self.outside_quote[start]
                || !self.outside_quote[end]
                || !self.has_closed_literal_context(start, end)
            {
                return 0;
            }
            supported += 1;
        }
        supported
    }

    fn has_closed_literal_context(&self, start: usize, end: usize) -> bool {
        let prefix_start = self.previous_boundary_end[start];
        let suffix_end = self.next_boundary_start[end];
        !self.has_quote_delimiter(prefix_start, start) && !self.has_quote_delimiter(end, suffix_end)
    }

    fn has_quote_delimiter(&self, start: usize, end: usize) -> bool {
        self.quote_delimiter_prefix[end] > self.quote_delimiter_prefix[start]
    }
}

impl PrivateStudyRoomDetailAnalysis {
    pub(super) fn facets(&self) -> &[IntentRecipeDetailFacetV3] {
        &self.facets
    }

    pub(crate) fn fields(&self) -> &[IntentRecipeDetailFieldV4] {
        &self.fields
    }

    pub(crate) fn expectations(&self) -> &[IntentRecipeDetailExpectationV4] {
        &self.expectations
    }

    pub(super) fn explains_requirement(&self, requirement: &str) -> bool {
        let requirement = normalized_whitespace(requirement);
        if requirement.is_empty() || !supported_detail_fragment(&requirement) {
            return false;
        }
        let total_occurrences = self
            .normalized_human
            .match_indices(requirement.as_str())
            .count();
        if total_occurrences == 0 {
            return false;
        }
        let supported_occurrences = self
            .evidence_entries
            .iter()
            .map(|entry| entry.supported_occurrences(&requirement))
            .sum::<usize>();
        supported_occurrences == total_occurrences
    }
}

pub(crate) fn analyze_private_study_room_details(
    human_message: &str,
) -> PrivateStudyRoomDetailAnalysis {
    let mut facets = BTreeSet::new();
    let mut evidence_entries = Vec::new();
    let mut assignments = Vec::new();
    let sentences = human_detail_sentences(human_message);
    if sentences
        .iter()
        .any(|sentence| is_unsafe_scope(&sentence.text))
    {
        return empty_detail_analysis(human_message);
    }
    match multiline_detail_evidence(human_message) {
        MultilineDetailEvidence::Invalid => return empty_detail_analysis(human_message),
        MultilineDetailEvidence::Valid(entries) => {
            for entry in entries {
                if !merge_assignments(&mut assignments, &entry.assignments) {
                    return empty_detail_analysis(human_message);
                }
                facets.extend(entry.facets);
                evidence_entries.push(IndexedDetailEvidence::new(entry.text));
            }
        }
        MultilineDetailEvidence::Absent => {}
    }
    for sentence in sentences {
        if matches!(sentence.terminator, Some('?' | '？')) {
            continue;
        }
        let Some(entries) = detail_evidence_entries(&sentence.text) else {
            continue;
        };
        for entry in entries {
            if !merge_assignments(&mut assignments, &entry.assignments) {
                return empty_detail_analysis(human_message);
            }
            facets.extend(entry.facets);
            evidence_entries.push(IndexedDetailEvidence::new(entry.text));
        }
    }
    if !all_slots_have_material_values(&assignments) {
        return empty_detail_analysis(human_message);
    }
    let expectations = canonical_material_detail_expectations(&assignments);
    let fields = expectations
        .iter()
        .map(IntentRecipeDetailExpectationV4::field)
        .collect();
    debug_assert!(expectations
        .iter()
        .all(|expectation| !expectation.literal().is_empty()));
    PrivateStudyRoomDetailAnalysis {
        facets: facets.into_iter().collect(),
        expectations,
        fields,
        normalized_human: normalized_whitespace(human_message),
        evidence_entries,
    }
}

fn merge_assignments(
    assignments: &mut Vec<DetailAssignmentClaim>,
    incoming: &[DetailAssignmentClaim],
) -> bool {
    for claim in incoming {
        if let Some(existing) = assignments
            .iter()
            .find(|existing| existing.same_target(claim))
        {
            if !existing.same_value(claim) {
                return false;
            }
        } else {
            assignments.push(claim.clone());
        }
    }
    true
}

fn all_slots_have_material_values(assignments: &[DetailAssignmentClaim]) -> bool {
    assignments.iter().all(|assignment| {
        assignments
            .iter()
            .any(|candidate| candidate.same_slot(assignment) && candidate.has_material_value())
    })
}

fn empty_detail_analysis(human_message: &str) -> PrivateStudyRoomDetailAnalysis {
    PrivateStudyRoomDetailAnalysis {
        facets: Vec::new(),
        expectations: Vec::new(),
        fields: Vec::new(),
        normalized_human: normalized_whitespace(human_message),
        evidence_entries: Vec::new(),
    }
}

fn multiline_detail_evidence(value: &str) -> MultilineDetailEvidence {
    let lines = value.lines().collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut found = false;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        let Some((header, body)) = split_first_unquoted_colon(line) else {
            index += 1;
            continue;
        };
        if !normalized_whitespace(body).is_empty() {
            index += 1;
            continue;
        }
        let default_policy = default_detail_header_policy(header);
        let recognized = declares_exact_override_list(header) || default_policy.is_some();
        if !recognized {
            let next_is_list = next_nonempty_line(&lines, index.saturating_add(1))
                .and_then(|index| lines.get(index))
                .is_some_and(|line| list_entry_text(line.trim()).is_some());
            if next_is_list {
                return MultilineDetailEvidence::Invalid;
            }
            index += 1;
            continue;
        }
        found = true;
        let Some(next_index) = next_nonempty_line(&lines, index.saturating_add(1)) else {
            return MultilineDetailEvidence::Invalid;
        };
        index = next_index;
        let group_start = entries.len();
        while index < lines.len() {
            let line = lines[index].trim();
            if line.is_empty() {
                break;
            }
            let Some(text) = list_entry_text(line) else {
                return MultilineDetailEvidence::Invalid;
            };
            if text.trim_end().ends_with('?') || text.trim_end().ends_with('？') {
                return MultilineDetailEvidence::Invalid;
            }
            let Some(syntax) = supported_detail_syntax(text) else {
                return MultilineDetailEvidence::Invalid;
            };
            entries.push(DetailEvidenceEntry {
                text: normalized_whitespace(text),
                facets: syntax.facets().to_vec(),
                assignments: syntax.assignments().to_vec(),
            });
            index += 1;
        }
        if entries.len() == group_start {
            return MultilineDetailEvidence::Invalid;
        }
        if default_policy
            .is_some_and(|policy| !default_header_accepts_entries(policy, &entries[group_start..]))
        {
            return MultilineDetailEvidence::Invalid;
        }
    }
    if found {
        MultilineDetailEvidence::Valid(entries)
    } else {
        MultilineDetailEvidence::Absent
    }
}

fn next_nonempty_line(lines: &[&str], mut index: usize) -> Option<usize> {
    while index < lines.len() {
        if !lines[index].trim().is_empty() {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn list_entry_text(value: &str) -> Option<&str> {
    if let Some(value) = value
        .strip_prefix("- ")
        .or_else(|| value.strip_prefix("* "))
        .or_else(|| value.strip_prefix("• "))
    {
        return Some(value.trim());
    }
    let (number, value) = value.split_once(". ")?;
    (!number.is_empty() && number.chars().all(|character| character.is_ascii_digit()))
        .then_some(value.trim())
}

fn human_detail_sentences(value: &str) -> Vec<HumanDetailSentence> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut active_quote = None;
    let mut previous = None;
    let mut characters = value.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        let next = characters.peek().map(|(_, character)| *character);
        if let Some(expected_close) = active_quote {
            current.push(character);
            if closes_quote(character, expected_close, previous, next) {
                active_quote = None;
            }
            previous = Some(character);
            continue;
        }
        if let Some(expected_close) = opening_quote(character, previous, next) {
            active_quote = Some(expected_close);
            current.push(character);
            previous = Some(character);
            continue;
        }
        let numbered_list_marker = character == '.'
            && !current.trim().is_empty()
            && current
                .trim()
                .chars()
                .all(|character| character.is_ascii_digit())
            && characters
                .peek()
                .is_some_and(|(_, character)| character.is_whitespace());
        let particle_bound_punctuation = matches!(character, '.' | '!' | '?' | '。' | '！' | '？')
            && punctuation_continues_korean_assignment(
                &value[index.saturating_add(character.len_utf8())..],
            );
        if numbered_list_marker || particle_bound_punctuation {
            current.push(character);
            previous = Some(character);
        } else if matches!(
            character,
            '.' | '!' | '?' | '\n' | '\r' | '。' | '！' | '？'
        ) {
            push_human_sentence(&mut sentences, &mut current, Some(character));
            previous = None;
        } else {
            current.push(character);
            previous = Some(character);
        }
    }
    push_human_sentence(&mut sentences, &mut current, None);
    sentences
}

fn push_human_sentence(
    sentences: &mut Vec<HumanDetailSentence>,
    current: &mut String,
    terminator: Option<char>,
) {
    let text = current.trim().to_owned();
    current.clear();
    if !text.is_empty() {
        sentences.push(HumanDetailSentence { text, terminator });
    }
}

fn detail_evidence_entries(sentence: &str) -> Option<Vec<DetailEvidenceEntry>> {
    if let Some((header, body)) = split_first_unquoted_colon(sentence) {
        if declares_exact_override_list(header) {
            let entries = split_unquoted_semicolons(body);
            if entries.is_empty() {
                return None;
            }
            return entries
                .into_iter()
                .map(|text| {
                    let syntax = supported_detail_syntax(&text)?;
                    Some(DetailEvidenceEntry {
                        facets: syntax.facets().to_vec(),
                        assignments: syntax.assignments().to_vec(),
                        text: normalized_whitespace(&text),
                    })
                })
                .collect();
        }
        if let Some(policy) = default_detail_header_policy(header) {
            let entries = general_detail_entries(body)?;
            return default_header_accepts_entries(policy, &entries).then_some(entries);
        }
    }
    general_detail_entries(sentence)
}

fn default_header_accepts_entries(
    policy: DefaultDetailHeaderPolicy,
    entries: &[DetailEvidenceEntry],
) -> bool {
    match policy {
        DefaultDetailHeaderPolicy::Facets {
            copy,
            naming,
            controls,
        } => entries.iter().all(|entry| {
            !entry.facets.is_empty()
                && entry.facets.iter().all(|facet| match facet {
                    IntentRecipeDetailFacetV3::Copy => copy,
                    IntentRecipeDetailFacetV3::Naming => naming,
                    IntentRecipeDetailFacetV3::Controls => controls,
                })
        }),
        DefaultDetailHeaderPolicy::ExactlyOneCopy => {
            entries.iter().all(|entry| {
                !entry.facets.is_empty()
                    && entry
                        .facets
                        .iter()
                        .all(|facet| *facet == IntentRecipeDetailFacetV3::Copy)
            }) && entries
                .iter()
                .map(|entry| entry.assignments.len())
                .sum::<usize>()
                == 1
        }
    }
}

fn general_detail_entries(value: &str) -> Option<Vec<DetailEvidenceEntry>> {
    let value = static_override_tail(value).unwrap_or(value);
    let entries = split_unquoted_semicolons(value);
    if entries.is_empty() {
        return None;
    }
    entries
        .into_iter()
        .map(|text| {
            let syntax = supported_detail_syntax(&text)?;
            Some(DetailEvidenceEntry {
                facets: syntax.facets().to_vec(),
                assignments: syntax.assignments().to_vec(),
                text: normalized_whitespace(&text),
            })
        })
        .collect()
}

fn static_override_tail(value: &str) -> Option<&str> {
    let lowercase = value.to_ascii_lowercase();
    for prefix in [
        "use defaults except that ",
        "use english defaults except that ",
        "use korean defaults except that ",
    ] {
        if lowercase.starts_with(prefix) {
            return value
                .get(prefix.len()..)
                .filter(|tail| !tail.trim().is_empty());
        }
    }
    let split = lowercase.find(" but ")?;
    let prefix = normalized_whitespace(&value[..split]).to_ascii_lowercase();
    let suffix = &value[split + " but ".len()..];
    matches!(
        prefix.as_str(),
        "use default copy" | "use default naming" | "use default controls"
    )
    .then_some(suffix)
}

pub(super) fn split_first_unquoted_colon(value: &str) -> Option<(&str, &str)> {
    let mut active_quote = None;
    let mut previous = None;
    let mut characters = value.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if let Some(expected_close) = active_quote {
            let next = characters.peek().map(|(_, character)| *character);
            if closes_quote(character, expected_close, previous, next) {
                active_quote = None;
            }
            previous = Some(character);
            continue;
        }
        let next = characters.peek().map(|(_, character)| *character);
        if let Some(expected_close) = opening_quote(character, previous, next) {
            active_quote = Some(expected_close);
        } else if matches!(character, ':' | '：') {
            let body_start = index + character.len_utf8();
            return Some((&value[..index], &value[body_start..]));
        }
        previous = Some(character);
    }
    None
}

fn split_unquoted_semicolons(value: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut current = String::new();
    let mut active_quote = None;
    let mut previous = None;
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if let Some(expected_close) = active_quote {
            current.push(character);
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
            current.push(character);
        } else if matches!(character, ';' | '；') {
            push_trimmed_text(&mut entries, &mut current);
        } else {
            current.push(character);
        }
        previous = Some(character);
    }
    push_trimmed_text(&mut entries, &mut current);
    entries
}

fn push_trimmed_text(values: &mut Vec<String>, current: &mut String) {
    let value = current.trim().to_owned();
    current.clear();
    if !value.is_empty() {
        values.push(value);
    }
}

fn quote_boundary_index(value: &str) -> Vec<bool> {
    let mut outside = vec![false; value.len().saturating_add(1)];
    let mut active_quote = None;
    let mut previous = None;
    let mut characters = value.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        outside[index] = active_quote.is_none();
        if let Some(expected_close) = active_quote {
            let next = characters.peek().map(|(_, character)| *character);
            if closes_quote(character, expected_close, previous, next) {
                active_quote = None;
            }
        } else {
            let next = characters.peek().map(|(_, character)| *character);
            active_quote = opening_quote(character, previous, next);
        }
        previous = Some(character);
        outside[index + character.len_utf8()] = active_quote.is_none();
    }
    outside
}

fn quote_delimiter_prefix_index(value: &str) -> Vec<usize> {
    let mut prefix = vec![0; value.len().saturating_add(1)];
    for (index, character) in value.char_indices() {
        if is_quote_delimiter(character) {
            prefix[index.saturating_add(1)] = 1;
        }
    }
    for index in 1..prefix.len() {
        prefix[index] += prefix[index.saturating_sub(1)];
    }
    prefix
}

fn coordination_boundary_index(
    length: usize,
    boundaries: &[(usize, usize)],
) -> (Vec<usize>, Vec<usize>) {
    let mut previous = vec![0; length.saturating_add(1)];
    let mut boundary_index = 0;
    let mut current_end = 0;
    for (position, value) in previous.iter_mut().enumerate() {
        while boundaries
            .get(boundary_index)
            .is_some_and(|(_, end)| *end <= position)
        {
            current_end = boundaries[boundary_index].1;
            boundary_index += 1;
        }
        *value = current_end;
    }

    let mut next = vec![length; length.saturating_add(1)];
    boundary_index = 0;
    for (position, value) in next.iter_mut().enumerate() {
        while boundaries
            .get(boundary_index)
            .is_some_and(|(start, _)| *start < position)
        {
            boundary_index += 1;
        }
        if let Some((start, _)) = boundaries.get(boundary_index) {
            *value = *start;
        }
    }
    (previous, next)
}

fn coordination_boundaries(value: &str) -> Vec<(usize, usize)> {
    let mut boundaries = Vec::new();
    let mut active_quote = None;
    let mut previous = None;
    let mut index = 0;
    while index < value.len() {
        let rest = &value[index..];
        let character = rest.chars().next().unwrap();
        let next = rest[character.len_utf8()..].chars().next();
        if let Some(expected_close) = active_quote {
            if closes_quote(character, expected_close, previous, next) {
                active_quote = None;
            }
            previous = Some(character);
            index += character.len_utf8();
            continue;
        }
        if let Some(expected_close) = opening_quote(character, previous, next) {
            active_quote = Some(expected_close);
            previous = Some(character);
            index += character.len_utf8();
            continue;
        }
        if matches!(character, ',' | ';' | '，' | '；') {
            boundaries.push((index, index + character.len_utf8()));
            previous = Some(character);
            index += character.len_utf8();
            continue;
        }
        if let Some(length) = detail_requirement_connector_len(rest) {
            boundaries.push((index, index + length));
            index += length;
            previous = None;
            continue;
        }
        previous = Some(character);
        index += character.len_utf8();
    }
    boundaries
}

pub(super) fn contains_quote_delimiter(value: &str) -> bool {
    value.chars().any(is_quote_delimiter)
}

fn is_quote_delimiter(character: char) -> bool {
    matches!(
        character,
        '\'' | '"'
            | '`'
            | '‘'
            | '’'
            | '“'
            | '”'
            | '「'
            | '」'
            | '『'
            | '』'
            | '«'
            | '»'
            | '‹'
            | '›'
            | '《'
            | '》'
            | '〈'
            | '〉'
    )
}
