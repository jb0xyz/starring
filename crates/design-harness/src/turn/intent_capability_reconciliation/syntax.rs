use super::super::intent_metalinguistic_scope::{
    semantic_unit_delimiter, trim_terminal_semantic_delimiters,
};
use super::super::intent_operative_conditionals::operative_consequent_start;
use super::super::intent_quote_scanner::{QuotedSpan, QuotedText};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static QUOTE_CURSOR_WORK: Cell<usize> = const { Cell::new(0) };
    static OCCURRENCE_AUTHORITY_CURSOR_WORK: Cell<usize> = const { Cell::new(0) };
    static LEADING_CONNECTOR_WORK: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Span {
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Token {
    pub(super) span: Span,
    pub(super) lower: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Clause {
    pub(super) span: Span,
    pub(super) tokens: Vec<Token>,
    pub(super) hypothetical: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Sentence {
    span: Span,
    tokens: Vec<Token>,
    hypothetical: bool,
    operative_consequent_start: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SourceSyntaxError {
    UnbalancedQuote,
}

pub(super) struct SourceText<'a> {
    source: &'a str,
    quoted: QuotedText,
    sentences: Vec<Sentence>,
    clauses: Vec<Clause>,
}

struct OccurrenceAuthorityCursor<'a> {
    quoted: &'a [QuotedSpan],
    sentences: &'a [Sentence],
    quote_index: usize,
    sentence_index: usize,
}

impl<'a> SourceText<'a> {
    pub(super) fn analyze(source: &'a str) -> Result<Self, SourceSyntaxError> {
        let quoted = QuotedText::scan(source);
        if quoted.unmatched() {
            return Err(SourceSyntaxError::UnbalancedQuote);
        }
        let mut sentence_spans = Vec::new();
        let mut sentence_start = 0usize;
        let mut quote_index = 0usize;
        for (index, character) in source.char_indices() {
            let end = index.saturating_add(character.len_utf8());
            let hidden = overlaps_next_quote(quoted.spans(), &mut quote_index, index, end);
            if !hidden && semantic_unit_delimiter(character) {
                push_trimmed_sentence_span(
                    source,
                    sentence_start,
                    index,
                    matches!(character, '?' | '？'),
                    &mut sentence_spans,
                );
                sentence_start = end;
            }
        }
        push_trimmed_sentence_span(
            source,
            sentence_start,
            source.len(),
            false,
            &mut sentence_spans,
        );
        let sentences = sentence_spans
            .into_iter()
            .filter_map(|(span, question)| {
                let tokens = tokenize(source, span, &quoted);
                if tokens.is_empty() {
                    return None;
                }
                let operative_consequent_start =
                    operative_sentence_consequent_start(source, span, question);
                Some(Sentence {
                    span,
                    hypothetical: hypothetical_tokens(&tokens),
                    tokens,
                    operative_consequent_start,
                })
            })
            .collect();
        let mut analyzed = Self {
            source,
            quoted,
            sentences,
            clauses: Vec::new(),
        };
        analyzed.clauses = analyzed.build_clauses();
        Ok(analyzed)
    }

    pub(super) fn value(&self, span: Span) -> Option<&'a str> {
        self.source.get(span.start..span.end)
    }

    pub(super) fn clauses(&self) -> &[Clause] {
        &self.clauses
    }

    fn build_clauses(&self) -> Vec<Clause> {
        let mut clauses = Vec::new();
        for sentence in &self.sentences {
            let mut start = 0usize;
            for index in 1..sentence.tokens.len() {
                let previous = &sentence.tokens[index - 1];
                let next = &sentence.tokens[index];
                let gap = self
                    .source
                    .get(previous.span.end..next.span.start)
                    .unwrap_or_default();
                let comma = gap.chars().any(|character| character == ',');
                let hard_delimiter = gap.chars().any(|character| matches!(character, ';' | '；'));
                if hard_delimiter
                    || (comma
                        && !continues_shared_negative_alternative(&sentence.tokens[start..=index]))
                {
                    self.push_clause(sentence, start, index, &mut clauses);
                    start = index;
                }
            }
            self.push_clause(sentence, start, sentence.tokens.len(), &mut clauses);
        }
        clauses
    }

    pub(super) fn has_asserted_occurrence(&self, value: &str) -> bool {
        if value.is_empty() {
            return false;
        }
        let mut cursor = OccurrenceAuthorityCursor::new(&self.quoted, &self.sentences);
        self.source.match_indices(value).any(|(start, _)| {
            let span = self.authority_span(Span {
                start,
                end: start.saturating_add(value.len()),
            });
            cursor.authority(span).0
        })
    }

    pub(super) fn has_only_proven_irrelevant_occurrences(&self, value: &str) -> bool {
        if value.is_empty() {
            return false;
        }
        let mut cursor = OccurrenceAuthorityCursor::new(&self.quoted, &self.sentences);
        let mut found = false;
        for (start, _) in self.source.match_indices(value) {
            found = true;
            let span = self.authority_span(Span {
                start,
                end: start.saturating_add(value.len()),
            });
            if !cursor.authority(span).1 {
                return false;
            }
        }
        found
    }

    pub(super) fn contains_asserted_token(&self, value: &str) -> bool {
        self.sentences.iter().any(|sentence| {
            sentence
                .tokens
                .iter()
                .any(|token| token.lower == value && sentence.authoritative_span(token.span))
        })
    }

    pub(super) fn unique_complete_asserted_clause_tokens(
        &self,
        value: &str,
    ) -> Option<Vec<String>> {
        if value.is_empty() {
            return None;
        }
        let value = trim_terminal_semantic_delimiters(value);
        let mut matching = self.clauses.iter().filter(|clause| {
            !clause.hypothetical
                && !self.overlaps_quote(clause.span)
                && self
                    .value(clause.span)
                    .is_some_and(|clause| clause.eq_ignore_ascii_case(value))
        });
        let clause = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        Some(
            clause
                .tokens
                .iter()
                .map(|token| token.lower.clone())
                .collect(),
        )
    }

    pub(super) fn unique_complete_asserted_sentence_tokens(
        &self,
        value: &str,
    ) -> Option<Vec<String>> {
        if value.is_empty() {
            return None;
        }
        let value = trim_terminal_semantic_delimiters(value);
        let mut matching = self.sentences.iter().filter(|sentence| {
            sentence.authoritative_span(sentence.span)
                && !self.overlaps_quote(sentence.span)
                && self
                    .value(sentence.span)
                    .is_some_and(|sentence| sentence.eq_ignore_ascii_case(value))
        });
        let sentence = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        Some(
            sentence
                .tokens
                .iter()
                .map(|token| token.lower.clone())
                .collect(),
        )
    }

    fn authority_span(&self, mut span: Span) -> Span {
        while let Some((relative, character)) = self
            .source
            .get(span.start..span.end)
            .and_then(|value| value.char_indices().next_back())
        {
            let start = span.start.saturating_add(relative);
            if !semantic_unit_delimiter(character) || self.quoted.overlaps(start, span.end) {
                break;
            }
            span.end = start;
        }
        span
    }

    pub(super) fn overlaps_quote(&self, span: Span) -> bool {
        self.quoted.overlaps(span.start, span.end)
    }

    fn push_clause(
        &self,
        sentence: &Sentence,
        start: usize,
        end: usize,
        clauses: &mut Vec<Clause>,
    ) {
        let leading_connectors = sentence.tokens[start..end]
            .iter()
            .take_while(|token| matches!(token.lower.as_str(), "and" | "but" | "then"))
            .count();
        #[cfg(test)]
        LEADING_CONNECTOR_WORK.with(|work| {
            work.set(
                work.get()
                    .saturating_add(leading_connectors.saturating_add(1)),
            )
        });
        let tokens = sentence.tokens[start.saturating_add(leading_connectors)..end].to_vec();
        if tokens.is_empty() {
            return;
        }
        let span = Span {
            start: tokens[0].span.start,
            end: tokens[tokens.len() - 1].span.end,
        };
        clauses.push(Clause {
            span,
            tokens,
            hypothetical: !sentence.authoritative_span(span),
        });
    }
}

impl<'a> OccurrenceAuthorityCursor<'a> {
    fn new(quoted: &'a QuotedText, sentences: &'a [Sentence]) -> Self {
        Self {
            quoted: quoted.spans(),
            sentences,
            quote_index: 0,
            sentence_index: 0,
        }
    }

    fn authority(&mut self, span: Span) -> (bool, bool) {
        #[cfg(test)]
        OCCURRENCE_AUTHORITY_CURSOR_WORK.with(|work| work.set(work.get().saturating_add(1)));
        while self
            .quoted
            .get(self.quote_index)
            .is_some_and(|quote| quote.end <= span.start)
        {
            #[cfg(test)]
            OCCURRENCE_AUTHORITY_CURSOR_WORK.with(|work| work.set(work.get().saturating_add(1)));
            self.quote_index = self.quote_index.saturating_add(1);
        }
        if self
            .quoted
            .get(self.quote_index)
            .is_some_and(|quote| quote.start <= span.start && span.end <= quote.end)
        {
            return (false, true);
        }
        while self
            .sentences
            .get(self.sentence_index)
            .is_some_and(|sentence| sentence.span.end <= span.start)
        {
            #[cfg(test)]
            OCCURRENCE_AUTHORITY_CURSOR_WORK.with(|work| work.set(work.get().saturating_add(1)));
            self.sentence_index = self.sentence_index.saturating_add(1);
        }
        let Some(sentence) = self
            .sentences
            .get(self.sentence_index)
            .filter(|sentence| sentence.span.start <= span.start && span.end <= sentence.span.end)
        else {
            return (false, false);
        };
        let asserted = sentence.authoritative_span(span);
        (asserted, !asserted)
    }
}

impl Sentence {
    fn authoritative_span(&self, span: Span) -> bool {
        self.operative_consequent_start
            .map_or(!self.hypothetical, |start| start <= span.start)
    }
}

impl Clause {
    pub(super) fn suffix_after(&self, word: &str) -> Self {
        let Some(index) = self.tokens.iter().rposition(|token| token.lower == word) else {
            return self.clone();
        };
        let tokens = self.tokens[index.saturating_add(1)..].to_vec();
        if tokens.is_empty() {
            return self.clone();
        }
        Self {
            span: Span {
                start: tokens[0].span.start,
                end: tokens[tokens.len() - 1].span.end,
            },
            tokens,
            hypothetical: self.hypothetical,
        }
    }

    pub(super) fn without_request_prefix(&self) -> Self {
        let words = self
            .tokens
            .iter()
            .map(|token| token.lower.as_str())
            .collect::<Vec<_>>();
        let mut start: usize = if words.first() == Some(&"please") {
            1
        } else {
            0
        };
        if words
            .get(start..start.saturating_add(3))
            .is_some_and(|prefix| matches!(prefix, ["can" | "could" | "will" | "would", "you", _]))
        {
            start = start.saturating_add(2);
        }
        if words.get(start).is_some_and(|word| {
            matches!(
                *word,
                "add" | "build" | "configure" | "create" | "design" | "implement" | "make"
            )
        }) {
            start = start.saturating_add(1);
            if words.get(start) == Some(&"me") {
                start = start.saturating_add(1);
            }
        } else if words.get(start..start.saturating_add(2)) == Some(&["set", "up"]) {
            start = start.saturating_add(2);
        }
        let tokens = self.tokens.get(start..).unwrap_or_default().to_vec();
        if tokens.is_empty() {
            return self.clone();
        }
        Self {
            span: Span {
                start: tokens[0].span.start,
                end: tokens[tokens.len() - 1].span.end,
            },
            tokens,
            hypothetical: self.hypothetical,
        }
    }
}

fn tokenize(source: &str, span: Span, quoted: &QuotedText) -> Vec<Token> {
    let Some(value) = source.get(span.start..span.end) else {
        return Vec::new();
    };
    let mut tokens = Vec::new();
    let mut token_start = None;
    let mut quote_index = quoted
        .spans()
        .partition_point(|quote| quote.end <= span.start);
    for (relative, character) in value.char_indices() {
        let start = span.start.saturating_add(relative);
        let end = start.saturating_add(character.len_utf8());
        let hidden = overlaps_next_quote(quoted.spans(), &mut quote_index, start, end);
        if !hidden && token_character(character) {
            token_start.get_or_insert(start);
        } else if let Some(token_start) = token_start.take() {
            push_token(source, token_start, start, &mut tokens);
        }
    }
    if let Some(token_start) = token_start {
        push_token(source, token_start, span.end, &mut tokens);
    }
    tokens
}

fn overlaps_next_quote(
    quoted: &[QuotedSpan],
    quote_index: &mut usize,
    start: usize,
    end: usize,
) -> bool {
    record_quote_cursor_work(1);
    while quoted
        .get(*quote_index)
        .is_some_and(|quote| quote.end <= start)
    {
        record_quote_cursor_work(1);
        *quote_index = quote_index.saturating_add(1);
    }
    quoted
        .get(*quote_index)
        .is_some_and(|quote| quote.start < end)
}

#[cfg(test)]
fn record_quote_cursor_work(count: usize) {
    QUOTE_CURSOR_WORK.with(|work| work.set(work.get().saturating_add(count)));
}

#[cfg(not(test))]
fn record_quote_cursor_work(_count: usize) {}

fn push_token(source: &str, start: usize, end: usize, tokens: &mut Vec<Token>) {
    let Some(value) = source.get(start..end) else {
        return;
    };
    tokens.push(Token {
        span: Span { start, end },
        lower: value.to_lowercase(),
    });
}

fn token_character(character: char) -> bool {
    character.is_alphanumeric()
        || matches!(
            character,
            '_' | '-' | '\'' | '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2019}'
        )
}

fn continues_shared_negative_alternative(tokens: &[Token]) -> bool {
    let word = |index: usize| tokens.get(index).map(|token| token.lower.as_str());
    match tokens.last().map(|token| token.lower.as_str()) {
        Some("nor") => word(0) == Some("neither"),
        Some("or") => {
            (word(0) == Some("do") && word(1) == Some("not") && word(2) == Some("either"))
                || (word(0) == Some("don") && word(1) == Some("t") && word(2) == Some("either"))
                || (word(0)
                    .is_some_and(|word| matches!(word, "don't" | "dont" | "don’t" | "never"))
                    && word(1) == Some("either"))
        }
        _ => false,
    }
}

fn push_trimmed_sentence_span(
    source: &str,
    start: usize,
    end: usize,
    question: bool,
    spans: &mut Vec<(Span, bool)>,
) {
    let Some(value) = source.get(start..end) else {
        return;
    };
    let leading = value.len().saturating_sub(value.trim_start().len());
    let trailing = value.len().saturating_sub(value.trim_end().len());
    let start = start.saturating_add(leading);
    let end = end.saturating_sub(trailing);
    if start < end {
        spans.push((Span { start, end }, question));
    }
}

fn operative_sentence_consequent_start(source: &str, span: Span, question: bool) -> Option<usize> {
    let sentence = source.get(span.start..span.end)?;
    operative_consequent_start(question, sentence).map(|start| span.start.saturating_add(start))
}

fn hypothetical_tokens(tokens: &[Token]) -> bool {
    let words = tokens
        .iter()
        .take(4)
        .map(|token| token.lower.as_str())
        .collect::<Vec<_>>();
    matches!(
        words.as_slice(),
        ["if", ..]
            | ["imagine", ..]
            | ["suppose", ..]
            | ["hypothetically", ..]
            | ["example", ..]
            | ["examples", ..]
            | ["explain", "whether", ..]
            | ["please", "explain", "whether", ..]
            | [
                "can" | "could" | "will" | "would",
                "you",
                "explain",
                "whether",
                ..
            ]
            | ["what", "if", ..]
            | ["for", "example", ..]
    )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{
        SourceText, LEADING_CONNECTOR_WORK, OCCURRENCE_AUTHORITY_CURSOR_WORK, QUOTE_CURSOR_WORK,
    };

    fn quote_cursor_work(repetitions: usize) -> usize {
        let source = format!("{}visible", "visible 'hidden' ".repeat(repetitions));
        QUOTE_CURSOR_WORK.with(|work| work.set(0));
        SourceText::analyze(&source).unwrap();
        QUOTE_CURSOR_WORK.with(Cell::get)
    }

    fn occurrence_authority_cursor_work(repetitions: usize, asserted_query: bool) -> usize {
        let source = format!(
            "{}Use hidden.",
            "If hidden. Label 'hidden'. ".repeat(repetitions)
        );
        let source = SourceText::analyze(&source).unwrap();
        OCCURRENCE_AUTHORITY_CURSOR_WORK.with(|work| work.set(0));
        if asserted_query {
            assert!(source.has_asserted_occurrence("hidden"));
        } else {
            assert!(!source.has_only_proven_irrelevant_occurrences("hidden"));
        }
        OCCURRENCE_AUTHORITY_CURSOR_WORK.with(Cell::get)
    }

    fn leading_connector_work(repetitions: usize) -> usize {
        let source = format!("{}keep validation", "and ".repeat(repetitions));
        LEADING_CONNECTOR_WORK.with(|work| work.set(0));
        let source = SourceText::analyze(&source).unwrap();
        assert_eq!(source.clauses().len(), 1);
        LEADING_CONNECTOR_WORK.with(Cell::get)
    }

    #[test]
    fn complete_clause_identity_is_ascii_case_insensitive() {
        let source = SourceText::analyze("Keep Validation and Preview").unwrap();

        assert_eq!(
            source.unique_complete_asserted_clause_tokens("keep validation and preview"),
            Some(vec![
                "keep".to_string(),
                "validation".to_string(),
                "and".to_string(),
                "preview".to_string(),
            ])
        );
    }

    #[test]
    fn case_insensitive_clause_identity_preserves_exact_unique_semantics() {
        let duplicate =
            SourceText::analyze("Keep validation and preview. KEEP VALIDATION AND PREVIEW")
                .unwrap();
        assert!(duplicate
            .unique_complete_asserted_clause_tokens("keep validation and preview")
            .is_none());

        let longer = SourceText::analyze("Keep Validation and Preview logs").unwrap();
        assert!(longer
            .unique_complete_asserted_clause_tokens("keep validation and preview")
            .is_none());

        let embedded = SourceText::analyze("Bookkeep validation and preview").unwrap();
        assert!(embedded
            .unique_complete_asserted_clause_tokens("keep validation and preview")
            .is_none());
    }

    #[test]
    fn quoted_and_hypothetical_duplicates_do_not_poison_visible_uniqueness() {
        for (open, close) in [
            ('"', '"'),
            ('\'', '\''),
            ('“', '”'),
            ('‘', '’'),
            ('«', '»'),
            ('‹', '›'),
            ('〈', '〉'),
            ('《', '》'),
            ('「', '」'),
            ('『', '』'),
            ('【', '】'),
        ] {
            let human = format!(
                "Keep validation and preview. Label it {open}keep validation and preview{close}."
            );
            let source = SourceText::analyze(&human).unwrap();
            assert!(source
                .unique_complete_asserted_clause_tokens("keep validation and preview")
                .is_some());
        }

        for fence_len in 1..=8 {
            let fence = "`".repeat(fence_len);
            let human = format!(
                "Keep validation and preview. Label it {fence}keep validation and preview{fence}."
            );
            let source = SourceText::analyze(&human).unwrap();
            assert!(source
                .unique_complete_asserted_clause_tokens("keep validation and preview")
                .is_some());
        }
    }

    #[test]
    fn asserted_occurrences_may_contain_but_not_live_inside_quoted_literals() {
        let source = SourceText::analyze(
            "When clicked, change the channel name to 'closed'. Label it 'closed'.",
        )
        .unwrap();
        assert!(source.has_asserted_occurrence("channel name to 'closed'"));
        assert!(!source.has_asserted_occurrence("closed"));
        assert!(!source.has_asserted_occurrence("'closed'"));
    }

    #[test]
    fn quote_cursor_work_scales_linearly() {
        let small = quote_cursor_work(128);
        let large = quote_cursor_work(256);
        assert!(small > 0);
        assert!(large <= small.saturating_mul(2).saturating_add(8));
    }

    #[test]
    fn occurrence_authority_queries_share_monotonic_cursors() {
        for asserted_query in [false, true] {
            let small = occurrence_authority_cursor_work(1_024, asserted_query);
            let large = occurrence_authority_cursor_work(2_048, asserted_query);
            assert!(small > 0);
            assert!(large <= small.saturating_mul(2).saturating_add(8));
            assert!(large <= 2_048usize.saturating_mul(8).saturating_add(8));
        }
    }

    #[test]
    fn leading_connector_trimming_scales_linearly() {
        let small = leading_connector_work(1_024);
        let large = leading_connector_work(2_048);
        assert_eq!(small, 1_025);
        assert_eq!(large, 2_049);
    }
}
