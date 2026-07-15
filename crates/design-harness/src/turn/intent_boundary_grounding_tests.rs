use super::intent_boundary_grounding::{
    analyze_safety_boundaries, ground_safety_boundary_requests,
    safety_boundary_owns_capability_evidence,
};
use super::intent_interpretation::IntentBoundaryRequestV2;

#[test]
fn grounds_gate_bypass_and_direct_live_mutation() {
    assert_eq!(
        ground_safety_boundary_requests(
            "Please bypass validation, preview, and approval, then connect to the live Discord server now and create the roles directly."
        ),
        vec![
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::BypassValidationPreviewApproval,
        ]
    );
}

#[test]
fn grounds_secret_disclosure_and_immediate_deployment() {
    assert_eq!(
        ground_safety_boundary_requests(
            "Immediately deploy to the production Discord server and publish the API token value in a public channel without masking it."
        ),
        vec![
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::SecretDisclosure,
        ]
    );
}

#[test]
fn distinguishes_one_gate_bypass_from_retained_gates() {
    assert_eq!(
        ground_safety_boundary_requests(
            "Keep validation and preview, skip only approval, and apply the changes to live Discord right away."
        ),
        vec![
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::BypassValidationPreviewApproval,
        ]
    );
}

#[test]
fn keeps_negated_live_mutation_out_of_an_all_gate_bypass() {
    assert_eq!(
        ground_safety_boundary_requests(
            "Prepare a static workflow, but omit every safety gate. Do not touch the live server or deploy anything."
        ),
        vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
    );
}

#[test]
fn accepts_safe_redaction_and_explicit_non_deployment() {
    assert!(ground_safety_boundary_requests(
        "Create a panel whose message says credentials are masked and substituted with [HIDDEN]. Keep every safety check and do not deploy it or reveal any real token."
    )
    .is_empty());
}

#[test]
fn ignores_quoted_and_code_formatted_interface_copy() {
    assert!(ground_safety_boundary_requests(
        "Create preview buttons labeled 'Skip approval', \"Reveal the API key\", and `Deploy to live Discord now`. Do not deploy the design."
    )
    .is_empty());
    assert!(ground_safety_boundary_requests(
        "Render ```bypass validation and expose the secret``` as documentation, without deploying it."
    )
    .is_empty());
}

#[test]
fn treats_hypotheticals_as_non_commands() {
    assert!(ground_safety_boundary_requests(
        "What happens if someone bypasses approval and deploys to a live server?"
    )
    .is_empty());
    assert!(
        ground_safety_boundary_requests("승인을 우회하고 실서버에 배포하면 어떻게 되나요?")
            .is_empty()
    );
}

#[test]
fn treats_polite_questions_as_requests() {
    assert_eq!(
        ground_safety_boundary_requests("Could you skip approval and deploy to live Discord now?"),
        vec![
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::BypassValidationPreviewApproval,
        ]
    );
}

#[test]
fn grounds_korean_gate_and_live_requests() {
    assert_eq!(
        ground_safety_boundary_requests(
            "검증과 미리보기는 유지하고 승인만 건너뛴 다음 지금 디스코드 실서버에 바로 적용해줘."
        ),
        vec![
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::BypassValidationPreviewApproval,
        ]
    );
}

#[test]
fn grounds_korean_secret_and_live_requests() {
    assert_eq!(
        ground_safety_boundary_requests(
            "환경 변수의 API 키 값을 가리지 말고 공개 채널에 게시하고 운영 서버에 즉시 배포해줘."
        ),
        vec![
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::SecretDisclosure,
        ]
    );
}

#[test]
fn keeps_contracted_negations_visible_and_safe() {
    assert!(ground_safety_boundary_requests(
        "Don't deploy or expose any secret, and don't skip approval."
    )
    .is_empty());
}

#[test]
fn returns_canonical_deduplicated_order() {
    assert_eq!(
        ground_safety_boundary_requests(
            "Bypass approval and validation, skip approval again, expose the API key in public, and deploy to the live server now."
        ),
        vec![
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            IntentBoundaryRequestV2::SecretDisclosure,
        ]
    );
}

#[test]
fn ordinary_discord_design_language_is_not_live_mutation() {
    assert!(ground_safety_boundary_requests(
        "Build a Discord automation design with validation, preview, approval, and redacted secret placeholders."
    )
    .is_empty());
}

#[test]
fn unmatched_quotes_do_not_hide_boundary_requests() {
    let human =
        "Use the label 'temporary and then bypass approval and deploy to the live server now";
    assert_eq!(
        ground_safety_boundary_requests(human),
        vec![
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::BypassValidationPreviewApproval,
        ]
    );
    assert!(!safety_boundary_owns_capability_evidence(
        human,
        "bypass approval"
    ));
}

#[test]
fn owns_exact_gate_and_live_boundary_fragments() {
    let human = "Skip only user approval, keep validation and preview, then connect to Discord now and deploy the live changes immediately.";
    for candidate in [
        "Skip only user approval",
        "connect to Discord",
        "deploy the live changes immediately",
    ] {
        assert!(safety_boundary_owns_capability_evidence(human, candidate));
    }
    assert!(!safety_boundary_owns_capability_evidence(
        human,
        "keep validation and preview"
    ));
}

#[test]
fn preserves_unrelated_requirement_in_the_same_clause() {
    let human = "Post an audit record and deploy the live changes immediately.";
    assert!(!safety_boundary_owns_capability_evidence(
        human,
        "Post an audit record"
    ));
    assert!(safety_boundary_owns_capability_evidence(
        human,
        "deploy the live changes immediately"
    ));
}

#[test]
fn owns_secret_disclosure_without_claiming_adjacent_behavior() {
    let human =
        "Post an audit record and expose the API token in a public channel without masking it.";
    assert!(!safety_boundary_owns_capability_evidence(
        human,
        "Post an audit record"
    ));
    assert!(safety_boundary_owns_capability_evidence(
        human,
        "expose the API token"
    ));
    assert!(!safety_boundary_owns_capability_evidence(
        human,
        "public channel"
    ));
}

#[test]
fn mixed_boundary_and_capability_evidence_fails_safe() {
    let mixed = "Post an immutable audit record and deploy the live changes immediately.";
    assert!(!safety_boundary_owns_capability_evidence(
        mixed,
        "Post an immutable audit record and deploy the live changes immediately"
    ));
    let subordinated = "Post a production readiness report before deploying to live Discord now.";
    assert!(!safety_boundary_owns_capability_evidence(
        subordinated,
        "Post a production readiness report"
    ));
}

#[test]
fn ambiguous_exact_candidate_fails_safe() {
    let human = "Deploy live Discord now. Deploy live Discord now.";
    assert!(!safety_boundary_owns_capability_evidence(
        human,
        "Deploy live Discord now"
    ));
}

#[test]
fn quoted_hypothetical_and_negated_candidates_are_not_owned() {
    let quoted =
        "Use the label 'Deploy live Discord now', then deploy the live changes immediately.";
    assert!(!safety_boundary_owns_capability_evidence(
        quoted,
        "Deploy live Discord now"
    ));
    assert!(safety_boundary_owns_capability_evidence(
        quoted,
        "deploy the live changes immediately"
    ));

    let hypothetical =
        "What happens if someone deploys to live Discord now? Deploy the live changes immediately.";
    assert!(!safety_boundary_owns_capability_evidence(
        hypothetical,
        "deploys to live Discord now"
    ));

    let negated = "Do not deploy live Discord now. Deploy the live changes immediately.";
    assert!(!safety_boundary_owns_capability_evidence(
        negated,
        "deploy live Discord now"
    ));
}

#[test]
fn owns_korean_boundary_fragments_without_claiming_adjacent_behavior() {
    let human = "감사 기록을 게시하고 승인만 건너뛴 다음 운영 서버에 즉시 배포해줘.";
    assert!(!safety_boundary_owns_capability_evidence(
        human,
        "감사 기록을 게시"
    ));
    assert!(safety_boundary_owns_capability_evidence(
        human,
        "승인만 건너뛴"
    ));
    assert!(safety_boundary_owns_capability_evidence(
        human,
        "운영 서버에 즉시 배포"
    ));
}

#[test]
fn relates_actions_to_their_own_targets() {
    let human = "Keep approval and skip the tutorial.";
    assert!(ground_safety_boundary_requests(human).is_empty());
    for candidate in ["approval", "skip the tutorial", human.trim_end_matches('.')] {
        assert!(!safety_boundary_owns_capability_evidence(human, candidate));
    }
}

#[test]
fn scopes_negation_to_the_local_predicate() {
    let human = "Skip approval and do not omit validation.";
    assert_eq!(
        ground_safety_boundary_requests(human),
        vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
    );
    for candidate in ["Skip approval", "approval"] {
        assert!(safety_boundary_owns_capability_evidence(human, candidate));
    }
    for candidate in ["do not omit validation", "omit validation", "validation"] {
        assert!(!safety_boundary_owns_capability_evidence(human, candidate));
    }

    let contrast = "Do not omit validation yet skip approval.";
    assert_eq!(
        ground_safety_boundary_requests(contrast),
        vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
    );
    assert!(safety_boundary_owns_capability_evidence(
        contrast,
        "skip approval"
    ));

    let korean = "승인을 건너뛰지 말고 검증을 유지해줘.";
    assert!(ground_safety_boundary_requests(korean).is_empty());
}

#[test]
fn excludes_negated_alternative_from_owned_gate_evidence() {
    let human = "Skip approval or do not omit validation.";
    assert_eq!(
        ground_safety_boundary_requests(human),
        vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
    );
    assert!(safety_boundary_owns_capability_evidence(
        human,
        "Skip approval"
    ));
    for candidate in ["omit validation", "validation"] {
        assert!(!safety_boundary_owns_capability_evidence(human, candidate));
    }
}

#[test]
fn owns_only_the_target_related_to_the_bypass_action() {
    let human = "Skip validation because approval remains mandatory.";
    assert_eq!(
        ground_safety_boundary_requests(human),
        vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
    );
    for candidate in ["Skip validation", "validation"] {
        assert!(safety_boundary_owns_capability_evidence(human, candidate));
    }
    for candidate in ["approval", "approval remains mandatory"] {
        assert!(!safety_boundary_owns_capability_evidence(human, candidate));
    }
}

#[test]
fn resumes_after_a_hypothetical_with_an_asserted_request() {
    let human = "What happens if someone skips approval, but please skip approval now.";
    assert_eq!(
        ground_safety_boundary_requests(human),
        vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
    );
    assert!(!safety_boundary_owns_capability_evidence(
        human,
        "skips approval"
    ));
    assert!(safety_boundary_owns_capability_evidence(
        human,
        "skip approval now"
    ));
}

#[test]
fn keeps_polite_hypothetical_explanations_non_executable() {
    let human = "Could you explain what happens if we skip approval and deploy live Discord now?";
    assert!(ground_safety_boundary_requests(human).is_empty());
    for candidate in ["skip approval", "deploy live Discord now"] {
        assert!(!safety_boundary_owns_capability_evidence(human, candidate));
    }
}

#[test]
fn propagates_asserted_and_hypothetical_question_scopes() {
    let asserted = "Could you skip approval and deploy live Discord now?";
    assert_eq!(
        ground_safety_boundary_requests(asserted),
        vec![
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::BypassValidationPreviewApproval,
        ]
    );
    for candidate in ["skip approval", "deploy live Discord now"] {
        assert!(safety_boundary_owns_capability_evidence(
            asserted, candidate
        ));
    }

    let targets = "Skip validation and approval.";
    assert_eq!(
        ground_safety_boundary_requests(targets),
        vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
    );
    for candidate in ["Skip validation", "validation", "approval"] {
        assert!(safety_boundary_owns_capability_evidence(targets, candidate));
    }
    assert!(safety_boundary_owns_capability_evidence(
        targets,
        "Skip validation and approval"
    ));
}

#[test]
fn separates_safe_redaction_from_a_later_disclosure() {
    let human = "Mask the API token in logs and expose the password in a public channel.";
    assert_eq!(
        ground_safety_boundary_requests(human),
        vec![IntentBoundaryRequestV2::SecretDisclosure]
    );
    assert!(!safety_boundary_owns_capability_evidence(
        human,
        "Mask the API token"
    ));
    assert!(safety_boundary_owns_capability_evidence(
        human,
        "expose the password"
    ));
    assert!(!safety_boundary_owns_capability_evidence(
        human,
        "public channel"
    ));
}

#[test]
fn masks_all_supported_unicode_quote_pairs() {
    for (open, close) in [
        ('«', '»'),
        ('‹', '›'),
        ('〈', '〉'),
        ('《', '》'),
        ('【', '】'),
    ] {
        let human =
            format!("Use the button label {open}Deploy live Discord now{close} in the preview.");
        assert!(ground_safety_boundary_requests(&human).is_empty());
        assert!(!safety_boundary_owns_capability_evidence(
            &human,
            "Deploy live Discord now"
        ));
    }
}

#[test]
fn combines_distributed_live_evidence_without_absorbing_lease_content() {
    for human in [
        "Connect to Discord and deploy immediately.",
        "Connect to Discord before deploying immediately.",
        "Connect to Discord in order to deploy immediately.",
    ] {
        assert_eq!(
            ground_safety_boundary_requests(human),
            vec![IntentBoundaryRequestV2::DirectLiveMutation]
        );
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "Connect to Discord"
        ));
    }
    let human = "Connect to Discord and deploy immediately.";
    assert!(safety_boundary_owns_capability_evidence(
        human,
        "deploy immediately"
    ));
    assert!(safety_boundary_owns_capability_evidence(
        human,
        "Connect to Discord and deploy immediately"
    ));
    let lease = "Acquire a production lease before deploying to live Discord now.";
    assert_eq!(
        ground_safety_boundary_requests(lease),
        vec![IntentBoundaryRequestV2::DirectLiveMutation]
    );
    assert!(!safety_boundary_owns_capability_evidence(
        lease,
        "Acquire a production lease"
    ));
    assert!(safety_boundary_owns_capability_evidence(
        lease,
        "deploying to live Discord now"
    ));
}

#[test]
fn counts_only_visible_bounded_candidate_occurrences() {
    let human = "Skip approval and store approval_code.";
    assert!(analyze_safety_boundaries(human).owns_capability_evidence("approval"));

    let duplicate = "Skip approval. Skip approval.";
    assert!(!analyze_safety_boundaries(duplicate).owns_capability_evidence("Skip approval"));

    let unicode = "🌟 한글 日本語 앞에서 Skip approval.";
    assert!(analyze_safety_boundaries(unicode).owns_capability_evidence("Skip approval"));
}

#[test]
fn reusable_analysis_is_the_single_source_for_requests_and_ownership() {
    let human = "Skip approval and connect to Discord before deploying immediately.";
    let analysis = analyze_safety_boundaries(human);
    assert_eq!(
        analysis.requests(),
        &[
            IntentBoundaryRequestV2::DirectLiveMutation,
            IntentBoundaryRequestV2::BypassValidationPreviewApproval,
        ]
    );
    assert!(analysis.owns_capability_evidence("Skip approval"));
    assert!(analysis.owns_capability_evidence("connect to Discord"));
}

#[test]
fn owns_plural_gate_language_without_absorbing_extra_content() {
    let human = "Bypass all design safety gates.";
    assert_eq!(
        ground_safety_boundary_requests(human),
        vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
    );
    assert!(safety_boundary_owns_capability_evidence(
        human,
        "Bypass all design safety gates"
    ));
}
