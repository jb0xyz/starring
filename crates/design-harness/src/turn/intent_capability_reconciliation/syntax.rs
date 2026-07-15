use super::super::intent_detail_text::{closes_quote, opening_quote};

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SourceSyntaxError {
    UnbalancedQuote,
}

pub(super) struct SourceText<'a> {
    source: &'a str,
    quoted: Vec<Span>,
    sentences: Vec<Sentence>,
}

impl<'a> SourceText<'a> {
    pub(super) fn analyze(source: &'a str) -> Result<Self, SourceSyntaxError> {
        let mut quoted = Vec::new();
        let mut sentence_spans = Vec::new();
        let mut closing_quote = None;
        let mut quote_start = None;
        let mut sentence_start = 0usize;
        let mut previous = None;
        for (index, character) in source.char_indices() {
            let next = source
                .get(index.saturating_add(character.len_utf8())..)
                .and_then(|suffix| suffix.chars().next());
            if let Some(expected) = closing_quote {
                if closes_quote(character, expected, previous, next) {
                    quoted.push(Span {
                        start: quote_start.unwrap_or(index),
                        end: index.saturating_add(character.len_utf8()),
                    });
                    closing_quote = None;
                    quote_start = None;
                }
            } else if let Some(expected) = opening_quote(character, previous, next) {
                closing_quote = Some(expected);
                quote_start = Some(index);
            } else if sentence_delimiter(character) {
                push_trimmed_span(source, sentence_start, index, &mut sentence_spans);
                sentence_start = index.saturating_add(character.len_utf8());
            }
            previous = Some(character);
        }
        if closing_quote.is_some() {
            return Err(SourceSyntaxError::UnbalancedQuote);
        }
        push_trimmed_span(source, sentence_start, source.len(), &mut sentence_spans);
        let sentences = sentence_spans
            .into_iter()
            .filter_map(|span| {
                let tokens = tokenize(source, span, &quoted);
                if tokens.is_empty() {
                    return None;
                }
                Some(Sentence {
                    span,
                    hypothetical: hypothetical_tokens(&tokens),
                    tokens,
                })
            })
            .collect();
        Ok(Self {
            source,
            quoted,
            sentences,
        })
    }

    pub(super) fn value(&self, span: Span) -> Option<&'a str> {
        self.source.get(span.start..span.end)
    }

    pub(super) fn clauses(&self) -> Vec<Clause> {
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
                if gap
                    .chars()
                    .any(|character| matches!(character, ',' | ';' | '；'))
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
        self.source.match_indices(value).any(|(start, _)| {
            let span = Span {
                start,
                end: start.saturating_add(value.len()),
            };
            !self.inside_quote(span)
                && self.sentences.iter().any(|sentence| {
                    !sentence.hypothetical
                        && sentence.span.start <= span.start
                        && sentence.span.end >= span.end
                })
        })
    }

    pub(super) fn contains_asserted_token(&self, value: &str) -> bool {
        self.sentences.iter().any(|sentence| {
            !sentence.hypothetical && sentence.tokens.iter().any(|token| token.lower == value)
        })
    }

    pub(super) fn overlaps_quote(&self, span: Span) -> bool {
        self.quoted
            .iter()
            .any(|quoted| quoted.start < span.end && span.start < quoted.end)
    }

    fn inside_quote(&self, span: Span) -> bool {
        self.quoted
            .iter()
            .any(|quoted| quoted.start <= span.start && span.end <= quoted.end)
    }

    fn push_clause(
        &self,
        sentence: &Sentence,
        start: usize,
        end: usize,
        clauses: &mut Vec<Clause>,
    ) {
        let mut tokens = sentence.tokens[start..end].to_vec();
        while tokens
            .first()
            .is_some_and(|token| matches!(token.lower.as_str(), "and" | "but" | "then"))
        {
            tokens.remove(0);
        }
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
            hypothetical: sentence.hypothetical,
        });
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

fn tokenize(source: &str, span: Span, quoted: &[Span]) -> Vec<Token> {
    let Some(value) = source.get(span.start..span.end) else {
        return Vec::new();
    };
    let mut tokens = Vec::new();
    let mut token_start = None;
    for (relative, character) in value.char_indices() {
        let start = span.start.saturating_add(relative);
        let end = start.saturating_add(character.len_utf8());
        let hidden = quoted
            .iter()
            .any(|quote| quote.start < end && start < quote.end);
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
            '_' | '-' | '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}'
        )
}

fn sentence_delimiter(character: char) -> bool {
    matches!(
        character,
        '.' | '!' | '?' | ';' | '\n' | '。' | '！' | '？' | '；'
    )
}

fn push_trimmed_span(source: &str, start: usize, end: usize, spans: &mut Vec<Span>) {
    let Some(value) = source.get(start..end) else {
        return;
    };
    let leading = value.len().saturating_sub(value.trim_start().len());
    let trailing = value.len().saturating_sub(value.trim_end().len());
    let start = start.saturating_add(leading);
    let end = end.saturating_sub(trailing);
    if start < end {
        spans.push(Span { start, end });
    }
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
            | ["what", "if", ..]
            | ["for", "example", ..]
    )
}
