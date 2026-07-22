use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, ControllerId, DeploymentId, DeploymentRevision, FencingToken, InstallationId,
    PanelCertificateId, PanelReportDigestV1, ProcessInstanceId, RuntimeDeploymentTargetV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;

use super::{
    wire, RuntimeCanonicalCertificationIntentV2, RuntimeCertificationCanonicalErrorV2,
    RuntimeCertificationCanonicalFieldV2, RuntimeCertificationCanonicalRootV2,
    RuntimeCertificationIntentCorrelationV2, CERTIFICATION_INTENT_MAX_OCTETS,
};
use crate::{
    GatewayShardIdV1, RuntimeBindingPinV1, RuntimeBuildRevisionV1, RuntimeCanonicalValueErrorV2,
    RuntimeCertificationIntentFingerprintV2, RuntimeCertificationIntentV2,
    RuntimeCertificationOperationIdV2, RuntimeDeploymentScopeV1, RuntimeExecutionGuardV1,
    RuntimeGatewayOwnerLeaseIdV1, RuntimePanelEvidenceV2, RuntimeSessionActionIdV1,
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
