use super::super::syntax::{
    ascii_case_insensitive_chars_equal, normalized_text, word_continuation, TextSpan,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in super::super) struct CanonicalWhitespaceMap {
    folded: String,
    characters: Vec<char>,
    canonical_to_original: Vec<TextSpan>,
    original_to_canonical: Vec<TextSpan>,
    byte_to_character: Vec<usize>,
    character_to_byte: Vec<usize>,
}

impl CanonicalWhitespaceMap {
    pub(in super::super) fn from_source(source: &str) -> Self {
        let original = source.chars().collect::<Vec<_>>();
        let mut characters = Vec::with_capacity(original.len());
        let mut canonical_to_original = Vec::with_capacity(original.len());
        let mut original_to_canonical = vec![TextSpan { start: 0, end: 0 }; original.len()];
        let mut index = 0usize;
        while index < original.len() {
            if original[index].is_whitespace() {
                let start = index;
                while index < original.len() && original[index].is_whitespace() {
                    index = index.saturating_add(1);
                }
                if !characters.is_empty() && index < original.len() {
                    let canonical = characters.len();
                    characters.push(' ');
                    canonical_to_original.push(TextSpan { start, end: index });
                    for span in &mut original_to_canonical[start..index] {
                        *span = TextSpan {
                            start: canonical,
                            end: canonical.saturating_add(1),
                        };
                    }
                } else {
                    let canonical = characters.len();
                    for span in &mut original_to_canonical[start..index] {
                        *span = TextSpan {
                            start: canonical,
                            end: canonical,
                        };
                    }
                }
                continue;
            }
            let canonical = characters.len();
            characters.push(original[index]);
            canonical_to_original.push(TextSpan {
                start: index,
                end: index.saturating_add(1),
            });
            original_to_canonical[index] = TextSpan {
                start: canonical,
                end: canonical.saturating_add(1),
            };
            index = index.saturating_add(1);
        }
        let text = characters.iter().collect::<String>();
        let folded = text.to_ascii_lowercase();
        let mut character_to_byte = text
            .char_indices()
            .map(|(byte, _)| byte)
            .collect::<Vec<_>>();
        character_to_byte.push(text.len());
        let mut byte_to_character = vec![usize::MAX; text.len().saturating_add(1)];
        for (character, byte) in character_to_byte.iter().copied().enumerate() {
            byte_to_character[byte] = character;
        }
        Self {
            folded,
            characters,
            canonical_to_original,
            original_to_canonical,
            byte_to_character,
            character_to_byte,
        }
    }

    fn canonical_span(&self, byte_start: usize, byte_end: usize) -> Option<TextSpan> {
        Some(TextSpan {
            start: *self.byte_to_character.get(byte_start)?,
            end: *self.byte_to_character.get(byte_end)?,
        })
        .filter(|span| span.start != usize::MAX && span.end != usize::MAX)
    }

    fn original_span(&self, canonical: TextSpan) -> Option<TextSpan> {
        if canonical.start >= canonical.end {
            return None;
        }
        let start = self.canonical_to_original.get(canonical.start)?.start;
        let end = self
            .canonical_to_original
            .get(canonical.end.saturating_sub(1))?
            .end;
        let original = TextSpan { start, end };
        let round_trip_start = self.original_to_canonical.get(original.start)?.start;
        let round_trip_end = self
            .original_to_canonical
            .get(original.end.saturating_sub(1))?
            .end;
        (round_trip_start == canonical.start && round_trip_end == canonical.end).then_some(original)
    }

    fn is_bounded(&self, candidate: &[char], span: TextSpan) -> bool {
        let left_valid = !candidate
            .first()
            .is_some_and(|value| word_continuation(*value))
            || !span
                .start
                .checked_sub(1)
                .and_then(|index| self.characters.get(index))
                .is_some_and(|value| word_continuation(*value));
        let right_valid = !candidate
            .last()
            .is_some_and(|value| word_continuation(*value))
            || !self
                .characters
                .get(span.end)
                .is_some_and(|value| word_continuation(*value))
            || self
                .character_to_byte
                .get(span.end)
                .is_some_and(|byte| known_korean_suffix_boundary(&self.folded[*byte..]));
        left_valid && right_valid
    }

    fn is_visible(&self, span: TextSpan, visible: &[char], candidate: &[char]) -> bool {
        self.canonical_to_original[span.start..span.end]
            .iter()
            .zip(candidate)
            .all(|(original, candidate)| {
                visible
                    .get(original.start..original.end)
                    .is_some_and(|value| {
                        if candidate.is_whitespace() {
                            value.iter().all(|character| character.is_whitespace())
                        } else {
                            ascii_case_insensitive_chars_equal(value, &[*candidate])
                        }
                    })
            })
    }
}

pub(in super::super) fn unique_visible_bounded_span(
    canonical_source: &CanonicalWhitespaceMap,
    visible: &[char],
    candidate: &str,
) -> Option<TextSpan> {
    let normalized_candidate = normalized_text(candidate);
    if normalized_candidate.is_empty() {
        return None;
    }
    let folded_candidate = normalized_candidate.to_ascii_lowercase();
    let candidate_characters = normalized_candidate.chars().collect::<Vec<_>>();
    let mut occurrence = None;
    for (byte_start, _) in canonical_source.folded.match_indices(&folded_candidate) {
        let byte_end = byte_start.saturating_add(folded_candidate.len());
        let Some(canonical_span) = canonical_source.canonical_span(byte_start, byte_end) else {
            continue;
        };
        if !canonical_source.is_bounded(&candidate_characters, canonical_span)
            || !canonical_source.is_visible(canonical_span, visible, &candidate_characters)
        {
            continue;
        }
        let Some(original_span) = canonical_source.original_span(canonical_span) else {
            continue;
        };
        if occurrence.is_some() {
            return None;
        }
        occurrence = Some(original_span);
    }
    occurrence
}

fn known_korean_suffix_boundary(value: &str) -> bool {
    ["해주세요", "해줘", "하도록", "하게", "하고", "하며"]
        .iter()
        .any(|suffix| value.starts_with(suffix))
}
