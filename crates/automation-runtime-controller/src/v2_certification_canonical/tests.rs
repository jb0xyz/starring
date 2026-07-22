use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, ControllerId, DeploymentId, DeploymentRevision, FencingToken, InstallationId,
    PanelCertificateId, PanelReportDigestV1, ProcessInstanceId, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    wire, RuntimeCanonicalCertificationIntentV2, RuntimeCanonicalLiveAttestationV2,
    RuntimeCertificationCanonicalErrorV2, RuntimeCertificationCanonicalFieldV2,
    RuntimeCertificationCanonicalRootV2, RuntimeCertificationIntentCorrelationV2,
    RuntimeCertificationRequestCorrelationV2, RuntimeLiveAttestationRecordV2,
    CERTIFICATION_INTENT_MAX_OCTETS, CERTIFICATION_REQUEST_MAX_OCTETS, LIVE_ATTESTATION_MAX_OCTETS,
};
use crate::{
    GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBindingPinV1,
    RuntimeBuildRevisionV1, RuntimeCanonicalValueErrorV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationIntentV2, RuntimeCertificationOperationIdV2,
    RuntimeCertificationRequestDigestV2, RuntimeCertificationRequestV2, RuntimeDeploymentScopeV1,
    RuntimeEvidenceErrorV2, RuntimeExecutionGuardV1, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2,
    RuntimeLiveAttestationDigestV2, RuntimePanelEvidenceV2, RuntimeRouteAdmissionAttestationV2,
    RuntimeServingRouteAttestationV2, RuntimeSessionActionIdV1,
};

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn target() -> RuntimeDeploymentTargetV1 {
    RuntimeDeploymentTargetV1 {
        guild_id: GuildId(7),
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        version: RuleSetVersionId::FIRST,
        content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
        binding_revision: BindingRevision::new(3).unwrap(),
        binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
    }
}

fn process_identity() -> RuntimeProcessIdentityV1 {
    RuntimeProcessIdentityV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::new(4).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
    }
}

fn intent() -> RuntimeCertificationIntentV2 {
    let process_identity = process_identity();
    RuntimeCertificationIntentV2 {
        action_id: RuntimeSessionActionIdV1::new(non_zero(1)),
        operation_id: RuntimeCertificationOperationIdV2::parse("00112233445566778899aabbccddeeff")
            .unwrap(),
        guard: RuntimeExecutionGuardV1 {
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: TenantId::parse("tenant:1").unwrap(),
                installation_id: InstallationId::parse("installation:1").unwrap(),
                deployment_id: DeploymentId::parse("deployment:1").unwrap(),
            },
            expected_revision: DeploymentRevision::new(2).unwrap(),
            controller_id: ControllerId::parse("controller:1").unwrap(),
            fencing_token: FencingToken::new(3).unwrap(),
            runtime_generation: RuntimeGeneration::new(4).unwrap(),
            convergence_attempt: NonZeroU32::new(5).unwrap(),
        },
        target: target(),
        binding_pin: RuntimeBindingPinV1 {
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            installation_authority_revision: non_zero(6),
            binding_revision: BindingRevision::new(3).unwrap(),
            binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
        },
        process_identity: process_identity.clone(),
        gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            lease_epoch: non_zero(5),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        },
        observed_owner_revision: non_zero(7),
        runtime_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        panel: RuntimePanelEvidenceV2 {
            certificate_id: PanelCertificateId::parse("panel:1").unwrap(),
            report_digest: PanelReportDigestV1::parse("c".repeat(64)).unwrap(),
            process_identity,
            controller_fencing_token: FencingToken::new(3).unwrap(),
        },
        serving_lease_for: Duration::from_secs(30),
    }
}

fn expected_bytes() -> Vec<u8> {
    let target = format!(
        concat!(
            "{{\"guild_id\":\"7\",\"ruleset_key\":\"studyroom\",\"version\":1,",
            "\"content_hash\":\"{}\",\"binding_revision\":3,",
            "\"binding_fingerprint\":\"{}\"}}"
        ),
        "b".repeat(64),
        "a".repeat(64),
    );
    let process = format!(
        "{{\"target\":{target},\"runtime_generation\":4,\"process_instance_id\":\"process:1\"}}"
    );
    format!(
        concat!(
            "{{\"format_version\":2,\"action_id\":1,",
            "\"operation_id\":\"00112233445566778899aabbccddeeff\",",
            "\"guard\":{{\"scope\":{{\"tenant_id\":\"tenant:1\",",
            "\"installation_id\":\"installation:1\",",
            "\"deployment_id\":\"deployment:1\"}},\"expected_revision\":2,",
            "\"controller_id\":\"controller:1\",\"fencing_token\":3,",
            "\"runtime_generation\":4,\"convergence_attempt\":5}},",
            "\"target\":{target},",
            "\"binding_pin\":{{\"tenant_id\":\"tenant:1\",",
            "\"installation_id\":\"installation:1\",",
            "\"installation_authority_revision\":6,\"binding_revision\":3,",
            "\"binding_fingerprint\":\"{}\"}},",
            "\"process_identity\":{process},",
            "\"gateway_owner_lease_id\":{{\"gateway_shard_id\":\"shard:0\",",
            "\"process_instance_id\":\"process:1\",\"lease_epoch\":5,",
            "\"expected_build_revision\":\"build:1\"}},",
            "\"observed_owner_revision\":7,\"runtime_build_revision\":\"build:1\",",
            "\"panel\":{{\"certificate_id\":\"panel:1\",",
            "\"report_digest\":\"{}\",\"process_identity\":{process},",
            "\"controller_fencing_token\":3}},\"serving_lease_milliseconds\":30000}}"
        ),
        "a".repeat(64),
        "c".repeat(64),
        target = target,
        process = process,
    )
    .into_bytes()
}

fn route_admission(intent: &RuntimeCertificationIntentV2) -> RuntimeRouteAdmissionAttestationV2 {
    RuntimeRouteAdmissionAttestationV2 {
        barrier_id: RuntimeBarrierIdV1::parse("ffeeddccbbaa99887766554433221100").unwrap(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(8),
            connection_epoch: non_zero(9),
            paused_admission_revision: non_zero(10),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(12)),
        },
        gateway: RuntimeGatewayReadyAttestationV2 {
            process_instance_id: intent.process_identity.process_instance_id.clone(),
            connection_epoch: non_zero(9),
            kind: RuntimeGatewayReadyKindV2::Resumed,
            admission_revision: non_zero(10),
            connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(11)),
            resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
        },
        gateway_owner_lease_id: intent.gateway_owner_lease_id.clone(),
        attested_owner_revision: intent.observed_owner_revision,
        route: RuntimeServingRouteAttestationV2 {
            identity: intent.process_identity.clone(),
            controller_fencing_token: intent.guard.fencing_token,
            route_incarnation: non_zero(14),
            activation_sequence: non_zero(15),
        },
    }
}

fn reserved_and_request() -> (
    RuntimeCanonicalCertificationIntentV2,
    RuntimeCertificationRequestV2,
) {
    let reserved = RuntimeCanonicalCertificationIntentV2::new(intent()).unwrap();
    let request = RuntimeCertificationRequestV2 {
        intent: reserved.intent().clone(),
        intent_fingerprint: reserved.intent_fingerprint().clone(),
        must_commit_before: DateTime::<Utc>::from_timestamp(1_700_000_000, 123_000_000).unwrap(),
        route_admission: route_admission(reserved.intent()),
    };
    (reserved, request)
}

fn canonical_live() -> RuntimeCanonicalLiveAttestationV2 {
    let (reserved, request) = reserved_and_request();
    let record = RuntimeLiveAttestationRecordV2::from_request(request).unwrap();
    reserved.bind_live_record(record).unwrap()
}

fn canonical_error(
    field: RuntimeCertificationCanonicalFieldV2,
    reason: RuntimeCanonicalValueErrorV2,
) -> RuntimeCertificationCanonicalErrorV2 {
    RuntimeCertificationCanonicalErrorV2::CanonicalValue {
        root: RuntimeCertificationCanonicalRootV2::Intent,
        field,
        reason,
    }
}

fn correlation_error(
    field: RuntimeCertificationIntentCorrelationV2,
) -> RuntimeCertificationCanonicalErrorV2 {
    RuntimeCertificationCanonicalErrorV2::CorrelationMismatch { field }
}

fn request_correlation_error(
    field: RuntimeCertificationRequestCorrelationV2,
) -> RuntimeCertificationCanonicalErrorV2 {
    RuntimeCertificationCanonicalErrorV2::RequestCorrelationMismatch { field }
}

fn replace_last(source: &str, needle: &str, replacement: &str) -> String {
    let position = source.rfind(needle).unwrap();
    let mut value = String::with_capacity(source.len() - needle.len() + replacement.len());
    value.push_str(&source[..position]);
    value.push_str(replacement);
    value.push_str(&source[position + needle.len()..]);
    value
}

#[test]
fn intent_root_matches_the_exact_byte_and_fingerprint_golden() {
    let canonical = RuntimeCanonicalCertificationIntentV2::new(intent()).unwrap();
    let expected = expected_bytes();

    assert_eq!(expected.len(), 1_844);
    assert_eq!(canonical.certification_intent_bytes(), expected);
    assert_eq!(
        canonical.intent_fingerprint().as_str(),
        "686ccbc5e00269f5b373bd5eec398e3b845e17d938cce2b4ae3e1ef19923b99d"
    );
    assert_eq!(canonical.intent(), &intent());
}

#[test]
fn persisted_intent_requires_exact_canonical_bytes_and_fingerprint() {
    let canonical = RuntimeCanonicalCertificationIntentV2::new(intent()).unwrap();
    let restored = RuntimeCanonicalCertificationIntentV2::from_persisted(
        canonical.certification_intent_bytes(),
        canonical.intent_fingerprint(),
    )
    .unwrap();

    assert_eq!(restored, canonical);
    let wrong = RuntimeCertificationIntentFingerprintV2::parse("d".repeat(64)).unwrap();
    assert_eq!(
        RuntimeCanonicalCertificationIntentV2::from_persisted(
            canonical.certification_intent_bytes(),
            &wrong,
        ),
        Err(
            RuntimeCertificationCanonicalErrorV2::PersistedFingerprintMismatch {
                root: RuntimeCertificationCanonicalRootV2::Intent,
            }
        )
    );
}

#[test]
fn intent_decoder_rejects_noncanonical_and_structurally_invalid_json() {
    let canonical = String::from_utf8(expected_bytes()).unwrap();
    let cases = [
        format!(" {canonical}"),
        canonical.replacen("{\"format_version\":2", "{\"format_version\":3", 1),
        canonical.replacen(
            "{\"format_version\":2,\"action_id\":1",
            "{\"action_id\":1,\"format_version\":2",
            1,
        ),
        canonical.replacen(
            "\"tenant_id\":\"tenant:1\"",
            "\"tenant_id\":\"tenant\\u003a1\"",
            1,
        ),
        canonical.replacen(
            "\"deployment_id\":\"deployment:1\"}",
            "\"deployment_id\":\"deployment:1\",\"unexpected\":true}",
            1,
        ),
        canonical.replacen(
            "\"expected_revision\":2",
            "\"expected_revision\":2,\"expected_revision\":2",
            1,
        ),
        canonical.replacen(
            "\"operation_id\":\"00112233445566778899aabbccddeeff\",",
            "",
            1,
        ),
    ];

    for encoded in cases {
        assert!(wire::decode_certification_intent(encoded.as_bytes()).is_err());
    }

    assert_eq!(
        wire::decode_certification_intent(&vec![b' '; CERTIFICATION_INTENT_MAX_OCTETS + 1]),
        Err(RuntimeCertificationCanonicalErrorV2::PayloadTooLarge {
            root: RuntimeCertificationCanonicalRootV2::Intent,
        })
    );
}

#[test]
fn intent_correlations_are_closed_before_fingerprinting() {
    let mut cases = Vec::new();

    let mut value = intent();
    value.binding_pin.tenant_id = TenantId::parse("tenant:2").unwrap();
    cases.push((
        value,
        RuntimeCertificationIntentCorrelationV2::BindingPinScope,
    ));

    let mut value = intent();
    value.binding_pin.binding_revision = BindingRevision::new(4).unwrap();
    cases.push((
        value,
        RuntimeCertificationIntentCorrelationV2::BindingPinTarget,
    ));

    let mut value = intent();
    value.guard.runtime_generation = RuntimeGeneration::new(5).unwrap();
    cases.push((
        value,
        RuntimeCertificationIntentCorrelationV2::GuardRuntimeGeneration,
    ));

    let mut value = intent();
    value.process_identity.target.version = RuleSetVersionId::new(2).unwrap();
    cases.push((
        value,
        RuntimeCertificationIntentCorrelationV2::ProcessTarget,
    ));

    let mut value = intent();
    value.gateway_owner_lease_id.process_instance_id =
        ProcessInstanceId::parse("process:2").unwrap();
    cases.push((
        value,
        RuntimeCertificationIntentCorrelationV2::GatewayOwnerProcessInstance,
    ));

    let mut value = intent();
    value.gateway_owner_lease_id.expected_build_revision =
        RuntimeBuildRevisionV1::parse("build:2").unwrap();
    cases.push((
        value,
        RuntimeCertificationIntentCorrelationV2::GatewayOwnerBuildRevision,
    ));

    let mut value = intent();
    value.panel.process_identity.process_instance_id =
        ProcessInstanceId::parse("process:2").unwrap();
    cases.push((
        value,
        RuntimeCertificationIntentCorrelationV2::PanelProcessIdentity,
    ));

    let mut value = intent();
    value.panel.controller_fencing_token = FencingToken::new(4).unwrap();
    cases.push((
        value,
        RuntimeCertificationIntentCorrelationV2::PanelControllerFencingToken,
    ));

    for (value, field) in cases {
        assert_eq!(
            RuntimeCanonicalCertificationIntentV2::new(value),
            Err(correlation_error(field))
        );
    }
}

#[test]
fn intent_snowflakes_use_canonical_full_u64_decimal_strings() {
    let mut value = intent();
    value.target.guild_id = GuildId(u64::MAX);
    value.process_identity.target.guild_id = GuildId(u64::MAX);
    value.panel.process_identity.target.guild_id = GuildId(u64::MAX);
    let canonical = RuntimeCanonicalCertificationIntentV2::new(value).unwrap();
    let text = String::from_utf8(canonical.certification_intent_bytes().to_vec()).unwrap();
    assert_eq!(
        text.matches("\"guild_id\":\"18446744073709551615\"")
            .count(),
        3
    );

    let mut value = intent();
    value.target.guild_id = GuildId(0);
    value.process_identity.target.guild_id = GuildId(0);
    value.panel.process_identity.target.guild_id = GuildId(0);
    assert_eq!(
        RuntimeCanonicalCertificationIntentV2::new(value),
        Err(canonical_error(
            RuntimeCertificationCanonicalFieldV2::TargetGuildId,
            RuntimeCanonicalValueErrorV2::DiscordSnowflakeOutOfRange,
        ))
    );

    for replacement in ["\"01\"", "0", "\"18446744073709551616\""] {
        let encoded = String::from_utf8(expected_bytes())
            .unwrap()
            .replace("\"guild_id\":\"7\"", &format!("\"guild_id\":{replacement}"));
        assert!(wire::decode_certification_intent(encoded.as_bytes()).is_err());
    }
}

#[test]
fn intent_persistence_integers_stop_at_postgresql_bigint_max() {
    let mut value = intent();
    value.action_id = RuntimeSessionActionIdV1::new(non_zero(i64::MAX as u64));
    assert!(RuntimeCanonicalCertificationIntentV2::new(value).is_ok());

    let mut value = intent();
    value.action_id = RuntimeSessionActionIdV1::new(non_zero(i64::MAX as u64 + 1));
    assert_eq!(
        RuntimeCanonicalCertificationIntentV2::new(value),
        Err(canonical_error(
            RuntimeCertificationCanonicalFieldV2::ActionId,
            RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        ))
    );

    let mut value = intent();
    value.observed_owner_revision = non_zero(i64::MAX as u64 + 1);
    assert_eq!(
        RuntimeCanonicalCertificationIntentV2::new(value),
        Err(canonical_error(
            RuntimeCertificationCanonicalFieldV2::ObservedOwnerRevision,
            RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        ))
    );
}

#[test]
fn intent_serving_lease_accepts_only_exact_bounded_milliseconds() {
    for milliseconds in [1_000, 300_000] {
        let mut value = intent();
        value.serving_lease_for = Duration::from_millis(milliseconds);
        assert!(RuntimeCanonicalCertificationIntentV2::new(value).is_ok());
    }

    for milliseconds in [999, 300_001] {
        let mut value = intent();
        value.serving_lease_for = Duration::from_millis(milliseconds);
        assert_eq!(
            RuntimeCanonicalCertificationIntentV2::new(value),
            Err(canonical_error(
                RuntimeCertificationCanonicalFieldV2::ServingLeaseMilliseconds,
                RuntimeCanonicalValueErrorV2::ServingLeaseOutOfRange,
            ))
        );
    }

    let mut value = intent();
    value.serving_lease_for = Duration::new(1, 1);
    assert_eq!(
        RuntimeCanonicalCertificationIntentV2::new(value),
        Err(canonical_error(
            RuntimeCertificationCanonicalFieldV2::ServingLeaseMilliseconds,
            RuntimeCanonicalValueErrorV2::ServingLeaseSubMillisecond,
        ))
    );
}

#[test]
fn intent_decoder_reports_invalid_fixed_fields_without_fallbacks() {
    let fingerprint = RuntimeCertificationIntentFingerprintV2::parse("d".repeat(64)).unwrap();
    let operation = String::from_utf8(expected_bytes()).unwrap().replacen(
        "00112233445566778899aabbccddeeff",
        "not-an-operation",
        1,
    );
    assert_eq!(
        RuntimeCanonicalCertificationIntentV2::from_persisted(operation.as_bytes(), &fingerprint),
        Err(RuntimeCertificationCanonicalErrorV2::InvalidField {
            root: RuntimeCertificationCanonicalRootV2::Intent,
            field: RuntimeCertificationCanonicalFieldV2::OperationId,
        })
    );

    let report =
        String::from_utf8(expected_bytes())
            .unwrap()
            .replacen(&"c".repeat(64), &"C".repeat(64), 1);
    assert_eq!(
        RuntimeCanonicalCertificationIntentV2::from_persisted(report.as_bytes(), &fingerprint),
        Err(RuntimeCertificationCanonicalErrorV2::InvalidField {
            root: RuntimeCertificationCanonicalRootV2::Intent,
            field: RuntimeCertificationCanonicalFieldV2::PanelReportDigest,
        })
    );
}

#[test]
fn request_and_live_roots_match_the_independent_goldens() {
    let canonical = canonical_live();
    let request_bytes = canonical.certification_request_bytes();
    let live_bytes = canonical.live_attestation_record_bytes();

    assert_eq!(request_bytes.len(), 2_937);
    assert_eq!(
        canonical.request_digest().as_str(),
        "94458ec4d7149cb5ebc0a5de501d7c36b5f7c8c54cd60bbf7cc1dbc40fd50749"
    );
    assert_eq!(live_bytes.len(), 3_052);
    assert_eq!(
        canonical.live_attestation_digest().as_str(),
        "0d32c22194c7ad09c12ff445263cce7095c9b88d109da1c64b305d729ff05873"
    );
    let request_text = String::from_utf8(request_bytes.to_vec()).unwrap();
    let live_text = String::from_utf8(live_bytes.to_vec()).unwrap();
    assert!(request_text.starts_with("{\"format_version\":2,\"intent\":{\"format_version\":2,"));
    assert_eq!(request_text.matches("\"format_version\":2").count(), 2);
    assert_eq!(live_text.matches("\"format_version\":2").count(), 3);

    let expected_live = [
        b"{\"format_version\":2,\"request_digest\":\"".as_slice(),
        canonical.request_digest().as_str().as_bytes(),
        b"\",\"request\":".as_slice(),
        request_bytes,
        b"}".as_slice(),
    ]
    .concat();
    assert_eq!(live_bytes, expected_live);
    assert_eq!(
        canonical.certification_intent_bytes(),
        canonical.reserved_intent().certification_intent_bytes()
    );
    assert_eq!(canonical.request(), canonical.record().request());
}

#[test]
fn persisted_live_requires_all_three_exact_roots_and_typed_digests() {
    let canonical = canonical_live();
    let restored = RuntimeCanonicalLiveAttestationV2::from_persisted(
        canonical.reserved_intent(),
        canonical.certification_request_bytes(),
        canonical.request_digest(),
        canonical.live_attestation_record_bytes(),
        canonical.live_attestation_digest(),
    )
    .unwrap();
    assert_eq!(restored, canonical);

    let wrong_request = RuntimeCertificationRequestDigestV2::parse("d".repeat(64)).unwrap();
    assert_eq!(
        RuntimeCanonicalLiveAttestationV2::from_persisted(
            canonical.reserved_intent(),
            canonical.certification_request_bytes(),
            &wrong_request,
            canonical.live_attestation_record_bytes(),
            canonical.live_attestation_digest(),
        ),
        Err(
            RuntimeCertificationCanonicalErrorV2::PersistedDigestMismatch {
                root: RuntimeCertificationCanonicalRootV2::Request,
            }
        )
    );

    let wrong_live = RuntimeLiveAttestationDigestV2::parse("e".repeat(64)).unwrap();
    assert_eq!(
        RuntimeCanonicalLiveAttestationV2::from_persisted(
            canonical.reserved_intent(),
            canonical.certification_request_bytes(),
            canonical.request_digest(),
            canonical.live_attestation_record_bytes(),
            &wrong_live,
        ),
        Err(
            RuntimeCertificationCanonicalErrorV2::PersistedDigestMismatch {
                root: RuntimeCertificationCanonicalRootV2::LiveAttestation,
            }
        )
    );
}

#[test]
fn request_correlations_are_closed_before_digesting() {
    let (_, base) = reserved_and_request();
    let mut cases = Vec::new();

    let mut request = base.clone();
    request.intent_fingerprint =
        RuntimeCertificationIntentFingerprintV2::parse("d".repeat(64)).unwrap();
    cases.push((
        request,
        RuntimeCertificationRequestCorrelationV2::IntentFingerprint,
    ));

    let mut request = base.clone();
    request.route_admission.route.identity.target.guild_id = GuildId(8);
    cases.push((
        request,
        RuntimeCertificationRequestCorrelationV2::RouteServingSlot,
    ));

    let mut request = base.clone();
    request.route_admission.route.identity.target.version = RuleSetVersionId::new(2).unwrap();
    cases.push((
        request,
        RuntimeCertificationRequestCorrelationV2::RouteProcessIdentity,
    ));

    let mut request = base.clone();
    request.route_admission.route.controller_fencing_token = FencingToken::new(4).unwrap();
    cases.push((
        request,
        RuntimeCertificationRequestCorrelationV2::RouteControllerFencingToken,
    ));

    let mut request = base.clone();
    request
        .route_admission
        .gateway_owner_lease_id
        .gateway_shard_id = GatewayShardIdV1::parse("shard:1").unwrap();
    cases.push((
        request,
        RuntimeCertificationRequestCorrelationV2::GatewayOwnerLease,
    ));

    let mut request = base;
    request.route_admission.attested_owner_revision = non_zero(8);
    cases.push((
        request,
        RuntimeCertificationRequestCorrelationV2::GatewayOwnerRevision,
    ));

    for (request, field) in cases {
        assert_eq!(
            RuntimeLiveAttestationRecordV2::from_request(request),
            Err(request_correlation_error(field))
        );
    }
}

#[test]
fn route_admission_requires_explicit_causal_resume_evidence() {
    let (_, mut request) = reserved_and_request();
    request.route_admission.gateway.kind = RuntimeGatewayReadyKindV2::Ready;
    assert_eq!(
        RuntimeLiveAttestationRecordV2::from_request(request),
        Err(RuntimeCertificationCanonicalErrorV2::RouteAdmission {
            reason: RuntimeEvidenceErrorV2::ReadyNotExplicitlyResumed,
        })
    );

    let (_, mut request) = reserved_and_request();
    request.route_admission.gateway.connection_epoch = non_zero(10);
    assert_eq!(
        RuntimeLiveAttestationRecordV2::from_request(request),
        Err(RuntimeCertificationCanonicalErrorV2::RouteAdmission {
            reason: RuntimeEvidenceErrorV2::ConnectionEpochMismatch,
        })
    );

    let canonical = canonical_live();
    let invalid_tag = String::from_utf8(canonical.certification_request_bytes().to_vec())
        .unwrap()
        .replacen("\"kind\":\"resumed\"", "\"kind\":\"unknown\"", 1);
    assert_eq!(
        wire::decode_certification_request(invalid_tag.as_bytes()),
        Err(RuntimeCertificationCanonicalErrorV2::InvalidField {
            root: RuntimeCertificationCanonicalRootV2::Request,
            field: RuntimeCertificationCanonicalFieldV2::GatewayReadyKind,
        })
    );
}

#[test]
fn reserved_intent_binding_rejects_a_self_consistent_substitution() {
    let (reserved, _) = reserved_and_request();
    let mut other_intent = intent();
    other_intent.action_id = RuntimeSessionActionIdV1::new(non_zero(2));
    let other_reserved = RuntimeCanonicalCertificationIntentV2::new(other_intent).unwrap();
    let other_request = RuntimeCertificationRequestV2 {
        intent: other_reserved.intent().clone(),
        intent_fingerprint: other_reserved.intent_fingerprint().clone(),
        must_commit_before: DateTime::<Utc>::from_timestamp(1_700_000_000, 123_000_000).unwrap(),
        route_admission: route_admission(other_reserved.intent()),
    };
    let record = RuntimeLiveAttestationRecordV2::from_request(other_request).unwrap();

    assert_eq!(
        reserved.bind_live_record(record),
        Err(request_correlation_error(
            RuntimeCertificationRequestCorrelationV2::ReservedIntentRoot,
        ))
    );
}

#[test]
fn persisted_live_rejects_a_different_self_consistent_request_record() {
    let original = canonical_live();
    let (reserved, mut request) = reserved_and_request();
    request.must_commit_before =
        DateTime::<Utc>::from_timestamp(1_700_000_000, 124_000_000).unwrap();
    let changed = reserved
        .bind_live_record(RuntimeLiveAttestationRecordV2::from_request(request).unwrap())
        .unwrap();

    assert_eq!(
        RuntimeCanonicalLiveAttestationV2::from_persisted(
            original.reserved_intent(),
            original.certification_request_bytes(),
            original.request_digest(),
            changed.live_attestation_record_bytes(),
            changed.live_attestation_digest(),
        ),
        Err(request_correlation_error(
            RuntimeCertificationRequestCorrelationV2::LiveRequestDigest,
        ))
    );
}

#[test]
fn request_and_live_decoders_reject_noncanonical_or_oversize_payloads() {
    let canonical = canonical_live();
    let request = String::from_utf8(canonical.certification_request_bytes().to_vec()).unwrap();
    let request_cases = [
        format!(" {request}"),
        request.replacen("{\"format_version\":2,\"intent\":", "{\"intent\":", 1),
        request.replacen(
            "{\"format_version\":2,\"intent\":",
            "{\"intent\":null,\"format_version\":2,\"intent\":",
            1,
        ),
        request.replacen("\"intent\":{\"format_version\":2,", "\"intent\":{", 1),
        request.replacen(
            "\"route_admission\":{",
            "\"route_admission\":{\"unexpected\":true,",
            1,
        ),
        request.replacen("\"intent\":{", "\"intent\": {", 1),
    ];
    for encoded in request_cases {
        assert!(wire::decode_certification_request(encoded.as_bytes()).is_err());
    }
    assert_eq!(
        wire::decode_certification_request(&vec![b' '; CERTIFICATION_REQUEST_MAX_OCTETS + 1]),
        Err(RuntimeCertificationCanonicalErrorV2::PayloadTooLarge {
            root: RuntimeCertificationCanonicalRootV2::Request,
        })
    );

    let live = String::from_utf8(canonical.live_attestation_record_bytes().to_vec()).unwrap();
    let live_cases = [
        format!(" {live}"),
        live.replacen("{\"format_version\":2", "{\"format_version\":3", 1),
        live.replacen("\"request\":{", "\"request\": {", 1),
        live.replacen(
            "\"request_digest\":",
            "\"unexpected\":true,\"request_digest\":",
            1,
        ),
    ];
    for encoded in live_cases {
        assert!(wire::decode_live_attestation_record(encoded.as_bytes()).is_err());
    }
    assert_eq!(
        wire::decode_live_attestation_record(&vec![b' '; LIVE_ATTESTATION_MAX_OCTETS + 1]),
        Err(RuntimeCertificationCanonicalErrorV2::PayloadTooLarge {
            root: RuntimeCertificationCanonicalRootV2::LiveAttestation,
        })
    );
    assert!(wire::decode_certification_request(&[0xff]).is_err());
    assert!(wire::decode_live_attestation_record(&[0xff]).is_err());

    let invalid_digest = live.replacen(canonical.request_digest().as_str(), &"D".repeat(64), 1);
    assert_eq!(
        wire::decode_live_attestation_record(invalid_digest.as_bytes()),
        Err(RuntimeCertificationCanonicalErrorV2::InvalidField {
            root: RuntimeCertificationCanonicalRootV2::LiveAttestation,
            field: RuntimeCertificationCanonicalFieldV2::RequestDigest,
        })
    );

    let wrong_digest = live.replacen(canonical.request_digest().as_str(), &"d".repeat(64), 1);
    assert_eq!(
        wire::decode_live_attestation_record(wrong_digest.as_bytes()),
        Err(request_correlation_error(
            RuntimeCertificationRequestCorrelationV2::LiveRequestDigest,
        ))
    );
}

#[test]
fn every_new_request_projection_rejects_hostile_nested_shapes() {
    let canonical = canonical_live();
    let request = String::from_utf8(canonical.certification_request_bytes().to_vec()).unwrap();
    let decoding = RuntimeCertificationCanonicalErrorV2::Decoding {
        root: RuntimeCertificationCanonicalRootV2::Request,
    };
    let hostile = [
        request.replacen(
            "\"pause\":{\"coordinator_generation\":8",
            "\"pause\":{\"unexpected\":true,\"coordinator_generation\":8",
            1,
        ),
        request.replacen(
            "\"gateway\":{\"process_instance_id\":\"process:1\"",
            "\"gateway\":{\"unexpected\":true,\"process_instance_id\":\"process:1\"",
            1,
        ),
        replace_last(
            &request,
            "\"gateway_owner_lease_id\":{",
            "\"gateway_owner_lease_id\":{\"unexpected\":true,",
        ),
        request.replacen(
            "\"route\":{\"identity\":",
            "\"route\":{\"unexpected\":true,\"identity\":",
            1,
        ),
        replace_last(
            &request,
            "\"identity\":{\"target\":",
            "\"identity\":{\"unexpected\":true,\"target\":",
        ),
        replace_last(
            &request,
            "\"target\":{\"guild_id\":",
            "\"target\":{\"unexpected\":true,\"guild_id\":",
        ),
        request.replacen(
            "\"pause_sequence\":12",
            "\"pause_sequence\":12,\"pause_sequence\":12",
            1,
        ),
        request.replacen("\"kind\":\"resumed\",", "", 1),
        request.replacen(",\"activation_sequence\":15", "", 1),
    ];
    for encoded in hostile {
        assert_eq!(
            wire::decode_certification_request(encoded.as_bytes()),
            Err(decoding)
        );
    }

    let route_snowflake = replace_last(&request, "\"guild_id\":\"7\"", "\"guild_id\":\"01\"");
    assert_eq!(
        wire::decode_certification_request(route_snowflake.as_bytes()),
        Err(RuntimeCertificationCanonicalErrorV2::CanonicalValue {
            root: RuntimeCertificationCanonicalRootV2::Request,
            field: RuntimeCertificationCanonicalFieldV2::RouteTargetGuildId,
            reason: RuntimeCanonicalValueErrorV2::DiscordSnowflakeNonCanonical,
        })
    );

    let request_version = request.replacen(
        "{\"format_version\":2,\"intent\":",
        "{\"format_version\":3,\"intent\":",
        1,
    );
    assert_eq!(
        wire::decode_certification_request(request_version.as_bytes()),
        Err(
            RuntimeCertificationCanonicalErrorV2::UnsupportedFormatVersion {
                root: RuntimeCertificationCanonicalRootV2::Request,
            }
        )
    );

    let live = String::from_utf8(canonical.live_attestation_record_bytes().to_vec()).unwrap();
    let live_version = live.replacen(
        "{\"format_version\":2,\"request_digest\":",
        "{\"format_version\":3,\"request_digest\":",
        1,
    );
    assert_eq!(
        wire::decode_live_attestation_record(live_version.as_bytes()),
        Err(
            RuntimeCertificationCanonicalErrorV2::UnsupportedFormatVersion {
                root: RuntimeCertificationCanonicalRootV2::LiveAttestation,
            }
        )
    );
}

#[test]
fn request_time_and_route_integer_normalization_match_persistence() {
    for microseconds in [-62_135_596_800_000_000, -1, 0, 253_402_300_799_999_999] {
        let (_, mut request) = reserved_and_request();
        request.must_commit_before = DateTime::<Utc>::from_timestamp_micros(microseconds).unwrap();
        assert!(RuntimeLiveAttestationRecordV2::from_request(request).is_ok());
    }

    let (_, mut request) = reserved_and_request();
    request.must_commit_before = DateTime::<Utc>::from_timestamp(0, 1).unwrap();
    assert_eq!(
        RuntimeLiveAttestationRecordV2::from_request(request),
        Err(RuntimeCertificationCanonicalErrorV2::CanonicalValue {
            root: RuntimeCertificationCanonicalRootV2::Request,
            field: RuntimeCertificationCanonicalFieldV2::MustCommitBeforeUnixMicroseconds,
            reason: RuntimeCanonicalValueErrorV2::TimestampSubMicrosecond,
        })
    );

    let (_, mut request) = reserved_and_request();
    request.must_commit_before = DateTime::<Utc>::from_timestamp(59, 1_000_000_000).unwrap();
    assert_eq!(
        RuntimeLiveAttestationRecordV2::from_request(request),
        Err(RuntimeCertificationCanonicalErrorV2::CanonicalValue {
            root: RuntimeCertificationCanonicalRootV2::Request,
            field: RuntimeCertificationCanonicalFieldV2::MustCommitBeforeUnixMicroseconds,
            reason: RuntimeCanonicalValueErrorV2::TimestampLeapSecond,
        })
    );

    let (_, mut request) = reserved_and_request();
    request.route_admission.route.route_incarnation = non_zero(i64::MAX as u64);
    assert!(RuntimeLiveAttestationRecordV2::from_request(request).is_ok());

    let (_, mut request) = reserved_and_request();
    request.route_admission.route.route_incarnation = non_zero(i64::MAX as u64 + 1);
    assert_eq!(
        RuntimeLiveAttestationRecordV2::from_request(request),
        Err(RuntimeCertificationCanonicalErrorV2::CanonicalValue {
            root: RuntimeCertificationCanonicalRootV2::Request,
            field: RuntimeCertificationCanonicalFieldV2::RouteIncarnation,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })
    );

    let canonical = canonical_live();
    let timestamp_out_of_range =
        String::from_utf8(canonical.certification_request_bytes().to_vec())
            .unwrap()
            .replacen("1700000000123000", "253402300800000000", 1);
    assert_eq!(
        wire::decode_certification_request(timestamp_out_of_range.as_bytes()),
        Err(RuntimeCertificationCanonicalErrorV2::CanonicalValue {
            root: RuntimeCertificationCanonicalRootV2::Request,
            field: RuntimeCertificationCanonicalFieldV2::MustCommitBeforeUnixMicroseconds,
            reason: RuntimeCanonicalValueErrorV2::TimestampOutOfRange,
        })
    );
}

#[test]
fn route_snowflakes_preserve_the_full_unsigned_range_as_text() {
    let mut value = intent();
    value.target.guild_id = GuildId(u64::MAX);
    value.process_identity.target.guild_id = GuildId(u64::MAX);
    value.panel.process_identity.target.guild_id = GuildId(u64::MAX);
    let reserved = RuntimeCanonicalCertificationIntentV2::new(value).unwrap();
    let request = RuntimeCertificationRequestV2 {
        intent: reserved.intent().clone(),
        intent_fingerprint: reserved.intent_fingerprint().clone(),
        must_commit_before: DateTime::<Utc>::from_timestamp(1_700_000_000, 123_000_000).unwrap(),
        route_admission: route_admission(reserved.intent()),
    };
    let record = RuntimeLiveAttestationRecordV2::from_request(request).unwrap();
    let canonical = reserved.bind_live_record(record).unwrap();
    let text = String::from_utf8(canonical.certification_request_bytes().to_vec()).unwrap();
    assert_eq!(
        text.matches("\"guild_id\":\"18446744073709551615\"")
            .count(),
        4
    );

    let invalid = text.replacen(
        "\"guild_id\":\"18446744073709551615\"",
        "\"guild_id\":18446744073709551615",
        1,
    );
    assert!(wire::decode_certification_request(invalid.as_bytes()).is_err());
}

#[test]
fn request_and_live_digests_change_with_commit_or_route_evidence() {
    let original = canonical_live();
    let (reserved, mut request) = reserved_and_request();
    request.must_commit_before =
        DateTime::<Utc>::from_timestamp(1_700_000_000, 124_000_000).unwrap();
    let deadline = reserved
        .bind_live_record(RuntimeLiveAttestationRecordV2::from_request(request).unwrap())
        .unwrap();
    assert_ne!(original.request_digest(), deadline.request_digest());
    assert_ne!(
        original.live_attestation_digest(),
        deadline.live_attestation_digest()
    );

    let (reserved, mut request) = reserved_and_request();
    request.route_admission.route.activation_sequence = non_zero(16);
    let route = reserved
        .bind_live_record(RuntimeLiveAttestationRecordV2::from_request(request).unwrap())
        .unwrap();
    assert_ne!(original.request_digest(), route.request_digest());
    assert_ne!(
        original.live_attestation_digest(),
        route.live_attestation_digest()
    );
}
