use super::intent_quote_scanner::QuotedText;

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static SCANNER_WORK: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
fn record_scanner_work(amount: usize) {
    SCANNER_WORK.with(|work| work.set(work.get().saturating_add(amount)));
}

#[cfg(not(test))]
fn record_scanner_work(_: usize) {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConditionalWord {
    start: usize,
    lower: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SplitCandidate {
    antecedent_end: usize,
    antecedent_words: usize,
    consequent_start: usize,
    consequent_words: usize,
}

pub(super) fn operative_consequent_start(question: bool, source: &str) -> Option<usize> {
    if question {
        return None;
    }
    let (words, candidates) = scan_conditional(source)?;
    if words.len() < 2 || candidates.is_empty() {
        return None;
    }
    let korean_prefix = prefix_counts(&words, korean_conditional_word);
    let korean_event_prefix = prefix_counts(&words, operative_korean_event_word);
    for (index, candidate) in candidates.iter().enumerate() {
        record_scanner_work(1);
        if !operative_antecedent(
            &words,
            candidate.antecedent_words,
            &korean_prefix,
            &korean_event_prefix,
        ) {
            continue;
        }
        let consequent_end = candidates
            .get(index.saturating_add(1))
            .map_or(words.len(), |next| next.antecedent_words);
        let consequent = &words[candidate.consequent_words..consequent_end];
        let coordinated_middle = index.saturating_add(1) < candidates.len()
            && consequent
                .first()
                .is_some_and(|word| matches!(word.lower.as_str(), "and" | "그리고"));
        if !coordinated_middle && operative_consequent(consequent) {
            return Some(candidate.consequent_start);
        }
    }
    None
}

fn scan_conditional(source: &str) -> Option<(Vec<ConditionalWord>, Vec<SplitCandidate>)> {
    let quoted = QuotedText::scan(source);
    if quoted.unmatched() {
        return None;
    }
    let mut words = Vec::new();
    let mut candidates = Vec::new();
    let mut word_start = None;
    let mut quote_index = 0usize;
    for (index, character) in source.char_indices() {
        record_scanner_work(1);
        while quoted
            .spans()
            .get(quote_index)
            .is_some_and(|span| span.end <= index)
        {
            quote_index = quote_index.saturating_add(1);
        }
        let hidden = quoted
            .spans()
            .get(quote_index)
            .is_some_and(|span| span.start <= index && index < span.end);
        let word_character = !hidden && (character.is_alphanumeric() || character == '-');
        if word_character {
            word_start.get_or_insert(index);
            continue;
        }
        finish_word(
            source,
            word_start.take(),
            index,
            &mut words,
            &mut candidates,
        );
        if !hidden && matches!(character, ',' | '，' | '、') {
            candidates.push(SplitCandidate {
                antecedent_end: index,
                antecedent_words: words.len(),
                consequent_start: index.saturating_add(character.len_utf8()),
                consequent_words: words.len(),
            });
        }
    }
    finish_word(
        source,
        word_start,
        source.len(),
        &mut words,
        &mut candidates,
    );
    normalize_candidates(source, &words, candidates).map(|candidates| (words, candidates))
}

fn finish_word(
    source: &str,
    start: Option<usize>,
    end: usize,
    words: &mut Vec<ConditionalWord>,
    candidates: &mut Vec<SplitCandidate>,
) {
    let Some(start) = start else {
        return;
    };
    record_scanner_work(1);
    let lower = source.get(start..end).unwrap_or_default().to_lowercase();
    let then_boundary = lower == "then";
    let korean_boundary = korean_conditional_word(&lower);
    let antecedent_words = words.len();
    words.push(ConditionalWord { start, lower });
    if then_boundary {
        candidates.push(SplitCandidate {
            antecedent_end: start,
            antecedent_words,
            consequent_start: end,
            consequent_words: words.len(),
        });
    } else if korean_boundary {
        candidates.push(SplitCandidate {
            antecedent_end: end,
            antecedent_words: words.len(),
            consequent_start: end,
            consequent_words: words.len(),
        });
    }
}

fn normalize_candidates(
    source: &str,
    words: &[ConditionalWord],
    candidates: Vec<SplitCandidate>,
) -> Option<Vec<SplitCandidate>> {
    let mut normalized: Vec<SplitCandidate> = Vec::new();
    let mut word_cursor = 0usize;
    let mut separator_end = 0usize;
    for mut candidate in candidates {
        record_scanner_work(1);
        candidate.consequent_start = if candidate.consequent_start < separator_end {
            separator_end
        } else {
            separator_end = skip_conditional_separators(source, candidate.consequent_start);
            separator_end
        };
        while words
            .get(word_cursor)
            .is_some_and(|word| word.start < candidate.consequent_start)
        {
            word_cursor = word_cursor.saturating_add(1);
        }
        candidate.consequent_words = word_cursor;
        if candidate.consequent_start >= source.len() || candidate.consequent_words >= words.len() {
            continue;
        }
        if let Some(previous) = normalized
            .last_mut()
            .filter(|previous| previous.consequent_start == candidate.consequent_start)
        {
            if previous.antecedent_end < candidate.antecedent_end {
                previous.antecedent_end = candidate.antecedent_end;
                previous.antecedent_words = candidate.antecedent_words;
            }
            continue;
        }
        normalized.push(candidate);
    }
    (!normalized.is_empty()).then_some(normalized)
}

fn skip_conditional_separators(source: &str, mut start: usize) -> usize {
    while let Some(character) = source.get(start..).and_then(|tail| tail.chars().next()) {
        if !character.is_whitespace() && !matches!(character, ',' | '，' | '、') {
            break;
        }
        start = start.saturating_add(character.len_utf8());
        record_scanner_work(1);
    }
    start
}

fn prefix_counts(words: &[ConditionalWord], predicate: impl Fn(&str) -> bool) -> Vec<usize> {
    let mut counts = Vec::with_capacity(words.len().saturating_add(1));
    counts.push(0usize);
    for word in words {
        record_scanner_work(1);
        counts.push(
            counts
                .last()
                .copied()
                .unwrap_or_default()
                .saturating_add(usize::from(predicate(&word.lower))),
        );
    }
    counts
}

fn operative_antecedent(
    words: &[ConditionalWord],
    end: usize,
    korean_prefix: &[usize],
    korean_event_prefix: &[usize],
) -> bool {
    let Some(first) = words.first().map(|word| word.lower.as_str()) else {
        return false;
    };
    if first == "when" || first == "whenever" {
        return words.get(1).is_none_or(|word| word.lower != "available");
    }
    if first == "if" {
        return !counterfactual_english_antecedent(words)
            && ![
                words.get(1),
                end.checked_sub(1).and_then(|index| words.get(index)),
            ]
            .into_iter()
            .flatten()
            .any(|word| {
                matches!(
                    word.lower.as_str(),
                    "available" | "needed" | "necessary" | "possible" | "ready"
                )
            });
    }
    let conditional = korean_prefix.get(end).copied().unwrap_or_default() > 0;
    conditional
        && (first == "만약" || korean_event_prefix.get(end).copied().unwrap_or_default() > 0)
}

fn counterfactual_english_antecedent(words: &[ConditionalWord]) -> bool {
    let subject = words.get(1).map(|word| word.lower.as_str());
    let predicate = words.get(2).map(|word| word.lower.as_str());
    matches!(subject, Some("i" | "we" | "you"))
        && matches!(
            predicate,
            Some(
                "build"
                    | "built"
                    | "create"
                    | "created"
                    | "design"
                    | "designed"
                    | "implement"
                    | "implemented"
                    | "make"
                    | "made"
                    | "were"
            )
        )
}

fn operative_korean_event_word(value: &str) -> bool {
    [
        "공개",
        "누르면",
        "누를",
        "닫",
        "배포",
        "선택",
        "승인",
        "열면",
        "열릴",
        "요청",
        "우회",
        "접속",
        "제출",
        "참여",
        "클릭",
        "노출",
        "반응",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn operative_consequent(words: &[ConditionalWord]) -> bool {
    let Some(first) = words.first().map(|word| word.lower.as_str()) else {
        return false;
    };
    if matches!(
        first,
        "brainstorm"
            | "compare"
            | "consider"
            | "discuss"
            | "explain"
            | "imagine"
            | "suppose"
            | "what"
            | "why"
            | "could"
            | "may"
            | "might"
            | "would"
    ) || ["논의", "비교", "설명", "어떻게", "왜"]
        .iter()
        .any(|marker| first.starts_with(marker))
    {
        return false;
    }
    words.iter().any(|word| {
        record_scanner_work(1);
        matches!(
            word.lower.as_str(),
            "add"
                | "adds"
                | "alert"
                | "alerts"
                | "apply"
                | "applies"
                | "award"
                | "awards"
                | "block"
                | "blocks"
                | "build"
                | "builds"
                | "bypass"
                | "bypasses"
                | "call"
                | "calls"
                | "change"
                | "changes"
                | "choose"
                | "chooses"
                | "create"
                | "creates"
                | "customize"
                | "customizes"
                | "decide"
                | "decides"
                | "deny"
                | "denies"
                | "deploy"
                | "deploys"
                | "design"
                | "designs"
                | "disclose"
                | "discloses"
                | "enable"
                | "enables"
                | "expose"
                | "exposes"
                | "generate"
                | "generates"
                | "grant"
                | "grants"
                | "have"
                | "implement"
                | "implements"
                | "make"
                | "makes"
                | "notify"
                | "notifies"
                | "override"
                | "overrides"
                | "post"
                | "posts"
                | "publish"
                | "publishes"
                | "reveal"
                | "reveals"
                | "rename"
                | "renames"
                | "run"
                | "runs"
                | "send"
                | "sends"
                | "set"
                | "sets"
                | "skip"
                | "skips"
                | "update"
                | "updates"
                | "use"
                | "uses"
        ) || [
            "경고", "노출", "만들", "배포", "부여", "생성", "실행", "우회", "사용", "전송", "결정",
            "공개",
        ]
        .iter()
        .any(|marker| word.lower.contains(marker))
    })
}

fn korean_conditional_word(value: &str) -> bool {
    value.ends_with('면') || value == "때"
}

#[cfg(test)]
mod tests {
    use super::{operative_consequent_start, SCANNER_WORK};

    fn split(source: &str, expected: &str) -> Option<usize> {
        operative_consequent_start(false, source).inspect(|&start| {
            assert_eq!(&source[start..], expected);
        })
    }

    #[test]
    fn recognizes_all_supported_operative_boundaries() {
        assert!(split(
            "If a user clicks the Judge button, an LLM decides whether to grant the role",
            "an LLM decides whether to grant the role",
        )
        .is_some());
        assert!(split(
            "If a user clicks the Judge button then expose the API token publicly",
            "expose the API token publicly",
        )
        .is_some());
        assert!(split(
            "If a message arrives, an LLM decides whether to grant the role",
            "an LLM decides whether to grant the role",
        )
        .is_some());
        assert!(split(
            "When the room is archived, change the Help button label to 'Guide'",
            "change the Help button label to 'Guide'",
        )
        .is_some());
        assert!(split(
            "만약 사용자가 버튼을 누르면 API 토큰을 공개 채널에 노출해",
            "API 토큰을 공개 채널에 노출해",
        )
        .is_some());
        assert!(split("사용자가 버튼을 누를 때 역할을 부여해", "역할을 부여해",).is_some());
    }

    #[test]
    fn chooses_the_last_antecedent_boundary_before_the_first_action() {
        for source in [
            "If a user clicks, and the API token is exposed publicly, send an alert",
            "If a user clicks, and someone exposes the API token publicly, send an alert",
        ] {
            assert!(split(source, "send an alert").is_some(), "{source}");
        }
    }

    #[test]
    fn rejects_counterfactual_discussion_question_and_availability_fragments() {
        for source in [
            "If we built this, an LLM would decide at event time",
            "If a user clicks the button, explain what the automation would do",
            "When available, build a static panel",
        ] {
            assert_eq!(operative_consequent_start(false, source), None, "{source}");
        }
        assert_eq!(
            operative_consequent_start(true, "If a user clicks, expose the API token publicly"),
            None
        );
    }

    #[test]
    fn scanner_work_is_linear_across_many_antecedent_clauses() {
        let mut source = "If a user clicks".to_string();
        for _ in 0..512 {
            source.push_str(", and the API token is exposed publicly");
        }
        source.push_str(", send an alert");
        SCANNER_WORK.with(|work| work.set(0));
        assert!(operative_consequent_start(false, &source).is_some());
        let work = SCANNER_WORK.with(|work| work.get());
        assert!(work <= source.chars().count().saturating_mul(12));
    }
}
