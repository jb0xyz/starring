use super::capability::{
    assess_intent_capabilities_v2, intent_capability_manifest_digest_v2,
    intent_capability_manifest_v2, CapabilityPolicyIdV2, CapabilityStatusV2, IntentCapabilityIdV2,
    IntentCapabilityRequirementV2, IntentRequirementEvidenceV2, IntentRouteEffectV2,
    IntentSafetyBoundaryIdV2, IntentSafetyBoundaryRequestV2, INTENT_CAPABILITY_MANIFEST_VERSION_V2,
};

fn evidence(path: &str, description: &str) -> IntentRequirementEvidenceV2 {
    IntentRequirementEvidenceV2 {
        semantic_path: path.to_string(),
        description: description.to_string(),
    }
}

fn requirement(
    id: IntentCapabilityIdV2,
    path: &str,
    description: &str,
) -> IntentCapabilityRequirementV2 {
    IntentCapabilityRequirementV2 {
        id,
        evidence: evidence(path, description),
    }
}

fn boundary(
    id: IntentSafetyBoundaryIdV2,
    path: &str,
    description: &str,
) -> IntentSafetyBoundaryRequestV2 {
    IntentSafetyBoundaryRequestV2 {
        id,
        evidence: evidence(path, description),
    }
}

#[test]
fn manifest_has_exact_sorted_capabilities_and_safety_boundaries() {
    let manifest = intent_capability_manifest_v2();
    assert_eq!(manifest.version, INTENT_CAPABILITY_MANIFEST_VERSION_V2);
    assert_eq!(
        manifest
            .capabilities
            .iter()
            .map(|descriptor| descriptor.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "durable_timer",
            "event_time_llm_decision",
            "instance_creator_teardown_authorization",
            "persistent_economy_ledger",
            "restart_persistent_state",
            "unclassified_intent_requirement",
        ]
    );
    assert_eq!(
        manifest
            .capabilities
            .iter()
            .map(|descriptor| descriptor.status)
            .collect::<Vec<_>>(),
        vec![
            CapabilityStatusV2::Unavailable,
            CapabilityStatusV2::ForbiddenPolicy,
            CapabilityStatusV2::Unavailable,
            CapabilityStatusV2::Unavailable,
            CapabilityStatusV2::Unavailable,
            CapabilityStatusV2::Unclassified,
        ]
    );
    assert_eq!(
        manifest.capabilities[1].policy_id,
        Some(CapabilityPolicyIdV2::EventTimeLlmExecutionForbiddenV1)
    );
    assert!(manifest
        .capabilities
        .iter()
        .enumerate()
        .all(|(index, descriptor)| index == 1 || descriptor.policy_id.is_none()));
    assert!(manifest.capabilities.iter().all(|descriptor| {
        descriptor.route_effect == IntentRouteEffectV2::CapabilityGap
            && !descriptor.label.en.is_empty()
            && !descriptor.label.ko.is_empty()
    }));
    assert_eq!(
        manifest
            .safety_boundaries
            .iter()
            .map(|descriptor| descriptor.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "bypass_validation_preview_approval",
            "direct_live_mutation",
            "secret_disclosure",
        ]
    );
    assert!(manifest.safety_boundaries.iter().all(|descriptor| {
        descriptor.route_effect == IntentRouteEffectV2::Reject
            && !descriptor.label.en.is_empty()
            && !descriptor.label.ko.is_empty()
    }));
}

#[test]
fn manifest_digest_is_canonical_domain_separated_and_golden() {
    let manifest = intent_capability_manifest_v2();
    let digest = intent_capability_manifest_digest_v2(&manifest).unwrap();
    assert_eq!(digest.len(), 64);
    assert!(digest
        .chars()
        .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()));
    assert_eq!(
        digest,
        "68de3f4d9355c99b213ba7546f41a772cd21e59ac4f750cc5ff33d99a0cc5d53"
    );
}

#[test]
fn manifest_digest_ignores_descriptor_input_order() {
    let manifest = intent_capability_manifest_v2();
    let expected = intent_capability_manifest_digest_v2(&manifest).unwrap();
    let mut reversed = manifest;
    reversed.capabilities.reverse();
    reversed.safety_boundaries.reverse();
    assert_eq!(
        intent_capability_manifest_digest_v2(&reversed).unwrap(),
        expected
    );
}

#[test]
fn manifest_rejects_duplicate_and_drifted_descriptors() {
    let mut duplicate = intent_capability_manifest_v2();
    duplicate.capabilities[1] = duplicate.capabilities[0].clone();
    assert_eq!(
        intent_capability_manifest_digest_v2(&duplicate)
            .unwrap_err()
            .code,
        "DUPLICATE_INTENT_CAPABILITY_ID"
    );

    let mut drifted = intent_capability_manifest_v2();
    drifted.capabilities[0].status = CapabilityStatusV2::Available;
    assert_eq!(
        intent_capability_manifest_digest_v2(&drifted)
            .unwrap_err()
            .code,
        "INVALID_INTENT_CAPABILITY_DESCRIPTOR"
    );

    let mut incomplete = intent_capability_manifest_v2();
    incomplete.safety_boundaries.pop();
    assert_eq!(
        intent_capability_manifest_digest_v2(&incomplete)
            .unwrap_err()
            .code,
        "INCOMPLETE_INTENT_SAFETY_BOUNDARY_MANIFEST"
    );
}

#[test]
fn closed_identifiers_reject_unknown_wire_values() {
    assert!(serde_json::from_str::<IntentCapabilityIdV2>(r#""future_capability""#).is_err());
    assert!(serde_json::from_str::<IntentSafetyBoundaryIdV2>(r#""future_boundary""#).is_err());
}

#[test]
fn assessment_returns_exact_sorted_stateful_blockers() {
    let requirements = vec![
        requirement(
            IntentCapabilityIdV2::RestartPersistentState,
            "runtime.persistence",
            "restart persistent",
        ),
        requirement(
            IntentCapabilityIdV2::PersistentEconomyLedger,
            "runtime.economy",
            "persistent ledger",
        ),
        requirement(
            IntentCapabilityIdV2::EventTimeLlmDecision,
            "runtime.event_time_llm",
            "event-time LLM",
        ),
        requirement(
            IntentCapabilityIdV2::DurableTimer,
            "runtime.timers",
            "durable timer",
        ),
    ];
    let assessment = assess_intent_capabilities_v2(&requirements, &[]).unwrap();
    assert_eq!(
        assessment
            .blockers
            .iter()
            .map(|blocker| (blocker.id.as_str(), blocker.status))
            .collect::<Vec<_>>(),
        vec![
            ("durable_timer", CapabilityStatusV2::Unavailable),
            (
                "event_time_llm_decision",
                CapabilityStatusV2::ForbiddenPolicy
            ),
            ("persistent_economy_ledger", CapabilityStatusV2::Unavailable),
            ("restart_persistent_state", CapabilityStatusV2::Unavailable),
        ]
    );
    assert_eq!(
        assessment.blockers[1].policy_id,
        Some(CapabilityPolicyIdV2::EventTimeLlmExecutionForbiddenV1)
    );
    assert_eq!(
        assessment.route_effect(),
        Some(IntentRouteEffectV2::CapabilityGap)
    );
    assert!(assessment.boundary_violations.is_empty());
}

#[test]
fn assessment_deduplicates_and_preserves_unclassified_evidence() {
    let first = requirement(
        IntentCapabilityIdV2::UnclassifiedIntentRequirement,
        "unclassified_requirements[1]",
        "cross-service quorum",
    );
    let second = requirement(
        IntentCapabilityIdV2::UnclassifiedIntentRequirement,
        "unclassified_requirements[0]",
        "external scheduler lease",
    );
    let assessment =
        assess_intent_capabilities_v2(&[first.clone(), second.clone(), first.clone()], &[])
            .unwrap();
    assert_eq!(assessment.blockers.len(), 1);
    assert_eq!(
        assessment.blockers[0].id,
        IntentCapabilityIdV2::UnclassifiedIntentRequirement
    );
    assert_eq!(
        assessment.blockers[0].status,
        CapabilityStatusV2::Unclassified
    );
    assert_eq!(
        assessment.blockers[0].evidence,
        vec![second.evidence, first.evidence]
    );
}

#[test]
fn safety_boundaries_take_blocking_effect_precedence_without_losing_findings() {
    let requirements = [requirement(
        IntentCapabilityIdV2::DurableTimer,
        "runtime.timers",
        "durable timer",
    )];
    let boundaries = [
        boundary(
            IntentSafetyBoundaryIdV2::SecretDisclosure,
            "boundary_requests.secret_disclosure",
            "publish a secret",
        ),
        boundary(
            IntentSafetyBoundaryIdV2::DirectLiveMutation,
            "boundary_requests.direct_live_mutation",
            "mutate Discord now",
        ),
    ];
    let assessment = assess_intent_capabilities_v2(&requirements, &boundaries).unwrap();
    assert_eq!(assessment.blockers.len(), 1);
    assert_eq!(
        assessment
            .boundary_violations
            .iter()
            .map(|violation| violation.id.as_str())
            .collect::<Vec<_>>(),
        vec!["direct_live_mutation", "secret_disclosure"]
    );
    assert_eq!(assessment.route_effect(), Some(IntentRouteEffectV2::Reject));
}

#[test]
fn empty_assessment_has_no_blocking_effect() {
    let assessment = assess_intent_capabilities_v2(&[], &[]).unwrap();
    assert!(assessment.blockers.is_empty());
    assert!(assessment.boundary_violations.is_empty());
    assert_eq!(assessment.route_effect(), None);
    assert_eq!(
        assessment.manifest_digest,
        intent_capability_manifest_digest_v2(&intent_capability_manifest_v2()).unwrap()
    );
}
