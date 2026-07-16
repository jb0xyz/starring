#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static SCAN_CHARACTER_VISITS: Cell<usize> = const { Cell::new(0) };
    static OVERLAP_LOOKUP_PROBES: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct QuotedSpan {
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct QuotedText {
    spans: Vec<QuotedSpan>,
    unmatched_start: Option<usize>,
}

#[derive(Clone, Copy)]
struct ActiveQuote {
    close: char,
    fence_len: usize,
    start: usize,
}

impl QuotedText {
    pub(super) fn scan(value: &str) -> Self {
        let characters = value.char_indices().collect::<Vec<_>>();
        let escaped = escape_parity(&characters);
        let mut spans = Vec::new();
        let mut active: Option<ActiveQuote> = None;
        let mut index = 0usize;
        while index < characters.len() {
            record_scan_visits(1);
            let (byte_start, character) = characters[index];
            if let Some(quote) = active {
                if quote.close == '`' && character == '`' && !escaped[index] {
                    let run = repeated_character_count(&characters, index, '`');
                    if run >= quote.fence_len {
                        spans.push(QuotedSpan {
                            start: quote.start,
                            end: character_end(value, &characters, index.saturating_add(run) - 1),
                        });
                        index = index.saturating_add(run);
                        active = None;
                        continue;
                    }
                    index = index.saturating_add(run);
                    continue;
                }
                if character == quote.close
                    && !escaped[index]
                    && !is_inner_apostrophe(&characters, index)
                {
                    spans.push(QuotedSpan {
                        start: quote.start,
                        end: character_end(value, &characters, index),
                    });
                    index = index.saturating_add(1);
                    active = None;
                    continue;
                }
                index = index.saturating_add(1);
                continue;
            }

            let Some((close, fence_len)) = opening_quote(&characters, index) else {
                index = index.saturating_add(1);
                continue;
            };
            if escaped[index] || is_inner_apostrophe(&characters, index) {
                index = index.saturating_add(1);
                continue;
            }
            active = Some(ActiveQuote {
                close,
                fence_len,
                start: byte_start,
            });
            index = index.saturating_add(fence_len);
        }
        Self {
            spans,
            unmatched_start: active.map(|quote| quote.start),
        }
    }

    pub(super) fn spans(&self) -> &[QuotedSpan] {
        &self.spans
    }

    pub(super) fn unmatched(&self) -> bool {
        self.unmatched_start.is_some()
    }

    pub(super) fn overlaps(&self, start: usize, end: usize) -> bool {
        let index = first_span_ending_after(&self.spans, start);
        self.spans.get(index).is_some_and(|span| span.start < end)
    }

    pub(super) fn masked_characters(&self, value: &str) -> Vec<char> {
        let mut span_index = 0usize;
        value
            .char_indices()
            .map(|(start, character)| {
                while self
                    .spans
                    .get(span_index)
                    .is_some_and(|span| span.end <= start)
                {
                    span_index = span_index.saturating_add(1);
                }
                if self
                    .spans
                    .get(span_index)
                    .is_some_and(|span| span.start <= start && start < span.end)
                {
                    ' '
                } else {
                    character
                }
            })
            .collect()
    }
}

fn first_span_ending_after(spans: &[QuotedSpan], start: usize) -> usize {
    let mut left = 0usize;
    let mut right = spans.len();
    while left < right {
        record_overlap_probe();
        let middle = left.saturating_add(right.saturating_sub(left) / 2);
        if spans[middle].end <= start {
            left = middle.saturating_add(1);
        } else {
            right = middle;
        }
    }
    left
}

fn escape_parity(characters: &[(usize, char)]) -> Vec<bool> {
    let mut preceding_slashes = 0usize;
    characters
        .iter()
        .map(|(_, character)| {
            let escaped = preceding_slashes % 2 == 1;
            if *character == '\\' {
                preceding_slashes = preceding_slashes.saturating_add(1);
            } else {
                preceding_slashes = 0;
            }
            escaped
        })
        .collect()
}

fn opening_quote(characters: &[(usize, char)], index: usize) -> Option<(char, usize)> {
    match characters[index].1 {
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

fn repeated_character_count(characters: &[(usize, char)], start: usize, expected: char) -> usize {
    let count = characters[start..]
        .iter()
        .take_while(|(_, character)| *character == expected)
        .count();
    record_scan_visits(count.saturating_sub(1));
    count
}

fn character_end(value: &str, characters: &[(usize, char)], index: usize) -> usize {
    characters[index]
        .0
        .saturating_add(characters[index].1.len_utf8())
        .min(value.len())
}

fn is_inner_apostrophe(characters: &[(usize, char)], index: usize) -> bool {
    matches!(characters[index].1, '\'' | '’')
        && index > 0
        && index + 1 < characters.len()
        && characters[index - 1].1.is_alphanumeric()
        && characters[index + 1].1.is_alphanumeric()
}

#[cfg(test)]
fn record_scan_visits(count: usize) {
    SCAN_CHARACTER_VISITS.with(|visits| visits.set(visits.get().saturating_add(count)));
}

#[cfg(not(test))]
fn record_scan_visits(_count: usize) {}

#[cfg(test)]
fn record_overlap_probe() {
    OVERLAP_LOOKUP_PROBES.with(|probes| probes.set(probes.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_overlap_probe() {}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{QuotedText, OVERLAP_LOOKUP_PROBES, SCAN_CHARACTER_VISITS};

    fn scan_work(repetitions: usize) -> usize {
        let value = format!(
            "{}{}",
            "visible 【hidden】 ``fenced`` ".repeat(repetitions),
            "visible"
        );
        SCAN_CHARACTER_VISITS.with(|visits| visits.set(0));
        let scan = QuotedText::scan(&value);
        assert!(!scan.unmatched());
        SCAN_CHARACTER_VISITS.with(Cell::get)
    }

    fn overlap_work(repetitions: usize) -> usize {
        let value = format!("{}visible", "'hidden' ".repeat(repetitions));
        let scan = QuotedText::scan(&value);
        OVERLAP_LOOKUP_PROBES.with(|probes| probes.set(0));
        assert!(!scan.overlaps(value.len().saturating_sub(7), value.len()));
        OVERLAP_LOOKUP_PROBES.with(Cell::get)
    }

    #[test]
    fn scans_every_supported_pair_and_fence_length() {
        let pairs = [
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
        ];
        for (open, close) in pairs {
            let value = format!("before {open}hidden{close} after");
            let scan = QuotedText::scan(&value);
            assert_eq!(
                scan.masked_characters(&value).iter().collect::<String>(),
                "before          after"
            );
            assert!(!scan.unmatched());
        }
        for length in 1..=8 {
            let fence = "`".repeat(length);
            let value = format!("before {fence}hidden{fence} after");
            let scan = QuotedText::scan(&value);
            assert!(!scan.unmatched());
            assert!(!scan
                .masked_characters(&value)
                .iter()
                .collect::<String>()
                .contains("hidden"));
        }
    }

    #[test]
    fn escape_parity_applies_to_openers_and_closers() {
        let odd_opener = r#"before \"visible after"#;
        let odd_scan = QuotedText::scan(odd_opener);
        assert!(!odd_scan.unmatched());
        assert_eq!(
            odd_scan
                .masked_characters(odd_opener)
                .iter()
                .collect::<String>(),
            odd_opener
        );

        let even_opener = r#"before \\"hidden" after"#;
        let even_scan = QuotedText::scan(even_opener);
        assert!(!even_scan.unmatched());
        assert!(!even_scan
            .masked_characters(even_opener)
            .iter()
            .collect::<String>()
            .contains("hidden"));

        let escaped_close = r#"before "hidden \" still hidden" after"#;
        let close_scan = QuotedText::scan(escaped_close);
        assert!(!close_scan.unmatched());
        assert!(!close_scan
            .masked_characters(escaped_close)
            .iter()
            .collect::<String>()
            .contains("still hidden"));
    }

    #[test]
    fn apostrophes_inside_words_do_not_change_quote_state() {
        let plain = "don't deploy";
        let plain_scan = QuotedText::scan(plain);
        assert!(!plain_scan.unmatched());
        assert_eq!(
            plain_scan
                .masked_characters(plain)
                .iter()
                .collect::<String>(),
            plain
        );

        let curly = "‘don’t deploy’";
        let curly_scan = QuotedText::scan(curly);
        assert!(!curly_scan.unmatched());
        assert!(curly_scan
            .masked_characters(curly)
            .iter()
            .all(|character| *character == ' '));
    }

    #[test]
    fn unmatched_openers_remain_visible_and_fail_closed() {
        let value = "before 'visible after";
        let scan = QuotedText::scan(value);
        assert!(scan.unmatched());
        assert_eq!(
            scan.masked_characters(value).iter().collect::<String>(),
            value
        );
    }

    #[test]
    fn scanner_character_work_scales_linearly() {
        let small = scan_work(128);
        let large = scan_work(256);
        assert_eq!(large, small.saturating_mul(2).saturating_sub(7));
    }

    #[test]
    fn overlap_lookup_work_scales_logarithmically() {
        let small = overlap_work(128);
        let large = overlap_work(256);
        assert!(small > 0);
        assert!(large <= small.saturating_add(2));
    }
}
