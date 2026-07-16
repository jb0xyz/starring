use super::{
    closed_actor_safety_control_meaning, closed_korean_safety_control_clause,
    closed_subject_safety_control_meaning, KoreanSafetyControlClause, SafetyControlMeaning,
    MAX_KOREAN_CONTROL_CLAUSES,
};

#[test]
fn governed_actor_frames_preserve_polarity_symmetry() {
    for words in [
        &["block", "the", "bot", "from", "skipping", "approval"][..],
        &["prohibit", "the", "bot", "from", "bypassing", "validation"][..],
        &[
            "the", "bot", "is", "blocked", "from", "skipping", "approval",
        ][..],
        &["do", "not", "allow", "the", "bot", "to", "skip", "approval"][..],
        &[
            "never",
            "permit",
            "the",
            "bot",
            "to",
            "bypass",
            "validation",
        ][..],
    ] {
        assert_eq!(
            closed_actor_safety_control_meaning(words),
            Some(SafetyControlMeaning::PreservesControl),
            "preserving governance frame was not classified for {words:?}"
        );
    }
    for words in [
        &["allow", "the", "bot", "to", "skip", "approval"][..],
        &[
            "do", "not", "block", "the", "bot", "from", "skipping", "approval",
        ][..],
        &[
            "the", "bot", "is", "not", "blocked", "from", "skipping", "approval",
        ][..],
    ] {
        assert_eq!(
            closed_actor_safety_control_meaning(words),
            Some(SafetyControlMeaning::WeakensControl),
            "weakening governance frame was not classified for {words:?}"
        );
    }
}

#[test]
fn get_passives_are_bounded_and_polarity_symmetric() {
    for words in [
        &["approval", "gets", "bypassed"][..],
        &["validation", "gets", "skipped"][..],
        &["approval", "will", "get", "bypassed"][..],
    ] {
        assert_eq!(
            closed_subject_safety_control_meaning(words),
            Some(SafetyControlMeaning::WeakensControl),
            "weakening get-passive was not classified for {words:?}"
        );
    }
    for words in [
        &["approval", "must", "not", "get", "bypassed"][..],
        &["validation", "does", "not", "get", "skipped"][..],
        &["approval", "never", "gets", "bypassed"][..],
    ] {
        assert_eq!(
            closed_subject_safety_control_meaning(words),
            Some(SafetyControlMeaning::PreservesControl),
            "preserving get-passive was not classified for {words:?}"
        );
    }
    for words in [
        &["approval", "gets", "bypassed", "by", "the", "ledger"][..],
        &["approval", "budget", "gets", "bypassed"][..],
        &["approval", "gets", "reviewed"][..],
    ] {
        assert_eq!(
            closed_subject_safety_control_meaning(words),
            None,
            "unbounded or non-control get-passive was classified for {words:?}"
        );
    }
}

#[test]
fn korean_negative_coordination_is_iterative_and_budgeted() {
    let mut within_budget = Vec::new();
    for _ in 1..MAX_KOREAN_CONTROL_CLAUSES {
        within_budget.extend(["승인을", "건너뛰지", "말고"]);
    }
    within_budget.extend(["검증을", "유지해줘"]);
    assert_eq!(
        closed_korean_safety_control_clause(&within_budget),
        Some(KoreanSafetyControlClause::Control(
            SafetyControlMeaning::PreservesControl
        ))
    );

    let mut over_budget = Vec::new();
    for _ in 0..MAX_KOREAN_CONTROL_CLAUSES {
        over_budget.extend(["승인을", "건너뛰지", "말고"]);
    }
    over_budget.extend(["검증을", "유지해줘"]);
    assert_eq!(closed_korean_safety_control_clause(&over_budget), None);
}

#[test]
fn copular_state_meaning_is_symmetric_and_fully_consuming() {
    for words in [
        &["approval", "is", "no", "longer", "required"][..],
        &["validation", "is", "not", "enabled"][..],
        &["validation", "is", "not", "enforced"][..],
        &["validation", "is", "off"][..],
        &["safety", "gates", "aren't", "enforced"][..],
        &["safety", "gates", "aren", "t", "enforced"][..],
        &["safety", "gates", "are", "not", "active"][..],
    ] {
        assert_eq!(
            closed_subject_safety_control_meaning(words),
            Some(SafetyControlMeaning::WeakensControl),
            "weakening state was not classified for {words:?}"
        );
    }
    for words in [
        &["validation", "is", "enforced"][..],
        &["safety", "gates", "are", "active"][..],
        &["safety", "gates", "are", "intact"][..],
        &["approval", "isn't", "optional"][..],
        &["approval", "isn", "t", "optional"][..],
        &["safety", "gates", "aren't", "disabled"][..],
        &["safety", "gates", "aren", "t", "disabled"][..],
    ] {
        assert_eq!(
            closed_subject_safety_control_meaning(words),
            Some(SafetyControlMeaning::PreservesControl),
            "preserving state was not classified for {words:?}"
        );
    }
    for words in [
        &["validation", "is", "enforced", "for", "invoice", "routing"][..],
        &[
            "safety", "gates", "are", "active", "and", "publish", "a", "report",
        ][..],
        &[
            "approval", "is", "no", "longer", "required", "by", "the", "ledger",
        ][..],
    ] {
        assert_eq!(
            closed_subject_safety_control_meaning(words),
            None,
            "business tail was consumed for {words:?}"
        );
    }
}
