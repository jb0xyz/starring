use super::intent_capability_grounding::{
    ground_unmapped_capability_evidence, CapabilityEvidenceGroundingError,
};

fn values(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn smoke_repairs_one_determiner_and_excludes_meta_instruction() {
    let human = "Build a stateful game where every message earns XP, levels unlock an economy, timers advance quests, and an LLM decides rewards at event time. Quest timers must be durable, and the economy ledger must be persistent. Preserve state across restarts and do not reduce the request to static responses.";
    let candidates = values(&[
        "LLM decides rewards at event time",
        "do not reduce the request to static responses",
        "every message earns XP",
        "levels unlock an economy",
        "timers advance quests",
    ]);
    assert_eq!(
        ground_unmapped_capability_evidence(human, candidates, 160).unwrap(),
        values(&[
            "an LLM decides rewards at event time",
            "every message earns XP",
            "levels unlock an economy",
            "timers advance quests",
        ])
    );
}

#[test]
fn ambiguous_repair_fails_closed() {
    let human =
        "an LLM decides rewards at event time, and LLM decides rewards at event time after review";
    assert_eq!(
        ground_unmapped_capability_evidence(
            human,
            values(&["LLM decides rewards at event time"]),
            160,
        )
        .unwrap_err(),
        CapabilityEvidenceGroundingError::Ambiguous
    );
}

#[test]
fn noun_phrase_remains_unchanged() {
    let human = "Require an external consensus lease";
    let candidate = "external consensus lease";
    assert_eq!(
        ground_unmapped_capability_evidence(human, values(&[candidate]), 160).unwrap(),
        values(&[candidate])
    );
}

#[test]
fn real_state_persistence_is_retained() {
    let human = "Preserve state across restarts";
    assert_eq!(
        ground_unmapped_capability_evidence(human, values(&[human]), 160).unwrap(),
        values(&[human])
    );
}

#[test]
fn korean_meta_instruction_is_excluded_without_losing_state_behavior() {
    let human = "요구사항을 누락하지 마. 재시작 후에도 상태를 보존해.";
    assert_eq!(
        ground_unmapped_capability_evidence(
            human,
            values(&["요구사항을 누락하지 마", "재시작 후에도 상태를 보존해"]),
            160,
        )
        .unwrap(),
        values(&["재시작 후에도 상태를 보존해"])
    );
}

#[test]
fn word_boundary_spoofs_are_ungrounded() {
    for human in [
        "an XLLM decides rewards at event time",
        "an X-LLM decides rewards at event time",
        "an X\u{0301}LLM decides rewards at event time",
        "an X\u{0483}LLM decides rewards at event time",
        "an X\u{05b0}LLM decides rewards at event time",
        "an X\u{0610}LLM decides rewards at event time",
        "an X\u{093c}LLM decides rewards at event time",
        "an X\u{200d}LLM decides rewards at event time",
        "an X\u{200e}LLM decides rewards at event time",
        "an X\u{2060}LLM decides rewards at event time",
        "an X\u{2066}LLM decides rewards at event time",
        "an X\u{02bc}LLM decides rewards at event time",
        "an X\u{055a}LLM decides rewards at event time",
        "an X\u{ff07}LLM decides rewards at event time",
        "an X\u{fe0f}LLM decides rewards at event time",
        "an X\u{e0100}LLM decides rewards at event time",
    ] {
        assert_eq!(
            ground_unmapped_capability_evidence(
                human,
                values(&["LLM decides rewards at event time"]),
                160,
            )
            .unwrap_err(),
            CapabilityEvidenceGroundingError::Ungrounded
        );
    }
}

#[test]
fn leading_articles_are_restored_for_closed_predicates() {
    let human =
        "a webhook signs receipts, an auditor approves payouts, and the scheduler advances jobs";
    assert_eq!(
        ground_unmapped_capability_evidence(
            human,
            values(&[
                "scheduler advances jobs",
                "webhook signs receipts",
                "auditor approves payouts",
            ]),
            160,
        )
        .unwrap(),
        values(&[
            "a webhook signs receipts",
            "an auditor approves payouts",
            "the scheduler advances jobs",
        ])
    );
}

#[test]
fn leading_quantifiers_are_restored_for_closed_predicates() {
    let human = "each worker awards points, and every message earns XP";
    assert_eq!(
        ground_unmapped_capability_evidence(
            human,
            values(&["message earns XP", "worker awards points"]),
            160,
        )
        .unwrap(),
        values(&["each worker awards points", "every message earns XP"])
    );
}

#[test]
fn grounding_is_idempotent_and_deduplicates_repaired_evidence() {
    let human = "an LLM decides rewards at event time. The bot archives support requests. do not summarize the request.";
    let first = ground_unmapped_capability_evidence(
        human,
        values(&[
            "LLM decides rewards at event time",
            "an LLM decides rewards at event time",
            "The bot archives support requests",
            "do not summarize the request",
        ]),
        160,
    )
    .unwrap();
    assert_eq!(
        first,
        values(&[
            "The bot archives support requests",
            "an LLM decides rewards at event time",
        ])
    );
    assert_eq!(
        ground_unmapped_capability_evidence(human, first.clone(), 160).unwrap(),
        first
    );
}

#[test]
fn concrete_request_and_requirement_behaviors_are_not_meta_filtered() {
    let mut candidates = values(&[
        "preserve request signatures",
        "do not weaken request authentication",
        "do not reduce the request rate",
        "preserve audit requirements",
        "the bot must not omit request signatures",
        "요청 로그를 누락하지 마",
        "요구사항 기록을 보존해",
    ]);
    let human = candidates.join(". ");
    candidates.sort();
    assert_eq!(
        ground_unmapped_capability_evidence(&human, candidates.clone(), 160).unwrap(),
        candidates
    );
}

#[test]
fn only_closed_meta_instruction_grammar_is_removed() {
    let human = "do not summarize the request. preserve all requirements. do not weaken these instructions. don't weaken the request. do not weaken requirements.";
    assert!(ground_unmapped_capability_evidence(
        human,
        values(&[
            "do not summarize the request",
            "preserve all requirements",
            "do not weaken these instructions",
            "don't weaken the request",
            "do not weaken requirements",
        ]),
        160,
    )
    .unwrap()
    .is_empty());
}

#[test]
fn mixed_behavior_candidate_is_never_deleted_as_meta() {
    let candidate = "do not weaken the request, and timers advance quests";
    assert_eq!(
        ground_unmapped_capability_evidence(candidate, values(&[candidate]), 160).unwrap(),
        values(&[candidate])
    );
}

#[test]
fn meta_prefix_of_a_behavior_clause_is_never_deleted() {
    let human = "do not reduce the request to static responses and timers advance quests";
    assert_eq!(
        ground_unmapped_capability_evidence(
            human,
            values(&["do not reduce the request to static responses"]),
            160,
        )
        .unwrap(),
        values(&["do not reduce the request to static responses"])
    );
}

#[test]
fn possessive_apostrophe_does_not_hide_a_later_meta_instruction() {
    let human = "Preserve users' settings across restarts. do not weaken the request.";
    assert!(ground_unmapped_capability_evidence(
        human,
        values(&["do not weaken the request"]),
        160,
    )
    .unwrap()
    .is_empty());
}

#[test]
fn quoted_and_literal_meta_text_is_retained() {
    let candidate = "do not reduce the request to static responses";
    for human in [
        "Post the message \"do not reduce the request to static responses\" when clicked",
        "Post the message 'do not reduce the request to static responses' when clicked",
        "Post the message `do not reduce the request to static responses` when clicked",
        "Content: do not reduce the request to static responses",
    ] {
        assert_eq!(
            ground_unmapped_capability_evidence(human, values(&[candidate]), 160).unwrap(),
            values(&[candidate])
        );
    }
}

#[test]
fn candidates_that_include_their_literal_delimiters_are_retained() {
    for candidate in [
        "\"do not reduce the request to static responses\"",
        "'do not reduce the request to static responses'",
        "`do not reduce the request to static responses`",
        "``do not reduce the request to static responses``",
    ] {
        let human = format!("{candidate}.");
        assert_eq!(
            ground_unmapped_capability_evidence(&human, values(&[candidate]), 160).unwrap(),
            values(&[candidate])
        );
    }
}

#[test]
fn repaired_value_over_utf16_limit_fails_closed() {
    let candidate = format!("{} signs receipts", "x".repeat(144));
    let human = format!("a {candidate}");
    assert_eq!(candidate.encode_utf16().count(), 159);
    assert_eq!(
        ground_unmapped_capability_evidence(&human, vec![candidate], 160).unwrap_err(),
        CapabilityEvidenceGroundingError::ExpandedTooLong
    );
}

#[test]
fn grounding_preserves_exact_source_case() {
    assert_eq!(
        ground_unmapped_capability_evidence(
            "An LLM decides rewards at event time",
            values(&["LLM decides rewards at event time"]),
            160,
        )
        .unwrap(),
        values(&["An LLM decides rewards at event time"])
    );
    assert_eq!(
        ground_unmapped_capability_evidence(
            "An LLM decides rewards at event time",
            values(&["llm decides rewards at event time"]),
            160,
        )
        .unwrap_err(),
        CapabilityEvidenceGroundingError::Ungrounded
    );
}
