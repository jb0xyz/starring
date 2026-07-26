use std::num::{NonZeroU32, NonZeroU64};

use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, ControllerId, DeploymentId, DeploymentRevision, FencingToken, InstallationId,
    ProcessInstanceId, RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1,
    RuntimeFailureV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Utc};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use sha2::{Digest, Sha256};

use super::{
    validate_request, wire, RuntimeCanonicalSuspendAttemptV2,
    RuntimeSuspendAttemptCanonicalErrorV2, RuntimeSuspendAttemptCanonicalFieldV2,
    RuntimeSuspendAttemptCorrelationV2, SUSPEND_ATTEMPT_MAX_OCTETS,
};
use crate::v2_digest::suspend_attempt_digest_v2;
use crate::{
    GatewayShardIdV1, RuntimeAttemptDispositionV2, RuntimeBarrierIdV1,
    RuntimeBarrierPauseWitnessV2, RuntimeBuildRevisionV1, RuntimeCanonicalValueErrorV2,
    RuntimeClosedRecoveryRouteWitnessV2, RuntimeDeploymentScopeV1, RuntimeDrainObligationV2,
    RuntimeExactLocalRouteIdentityV2, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeLocalRouteEffectV2, RuntimePreviousServingLeaseIdentityV1,
    RuntimeRecoveryIdV2, RuntimeResumeCheckpointV2, RuntimeRouteMutationProvenanceV2,
    RuntimeServingSlotV2, RuntimeSessionActionIdV1, RuntimeShutdownRouteWitnessV2,
    RuntimeSuspendAttemptDigestV2, RuntimeSuspendAttemptRequestV2,
    RuntimeSuspendedRouteLifecycleV2, RuntimeSuspensionIdV2, RuntimeSuspensionSourcePhaseV2,
};

const SUSPENSION_ID: &str = "00112233445566778899aabbccddeeff";
const BARRIER_ID: &str = "ffeeddccbbaa99887766554433221100";
const RECOVERY_ID: &str = "11112222333344445555666677778888";
const GUILD_ID: u64 = 9_223_372_036_854_775_808;
const EXPECTED_SIMPLE_DIGEST: &str =
    "dfb0292d5c206775a9a9ce899a59ca848c9431fd350f3ffdd8eb1e1caa903a98";

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn at_microseconds(value: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(value).unwrap()
}

fn scope(deployment: &str) -> RuntimeDeploymentScopeV1 {
    RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse("tenant:1").unwrap(),
        installation_id: InstallationId::parse("installation:1").unwrap(),
        deployment_id: DeploymentId::parse(deployment).unwrap(),
    }
}

fn target(guild_id: u64, ruleset_key: &str) -> RuntimeDeploymentTargetV1 {
    RuntimeDeploymentTargetV1 {
        guild_id: GuildId(guild_id),
        ruleset_key: RuleSetKey::parse(ruleset_key).unwrap(),
        version: RuleSetVersionId::FIRST,
        content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
        binding_revision: BindingRevision::new(3).unwrap(),
        binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
    }
}

fn process(
    guild_id: u64,
    ruleset_key: &str,
    generation: u64,
    process_instance_id: &str,
) -> RuntimeProcessIdentityV1 {
    RuntimeProcessIdentityV1 {
        target: target(guild_id, ruleset_key),
        runtime_generation: RuntimeGeneration::new(generation).unwrap(),
        process_instance_id: ProcessInstanceId::parse(process_instance_id).unwrap(),
    }
}

fn guard() -> crate::RuntimeExecutionGuardV1 {
    crate::RuntimeExecutionGuardV1 {
        scope: scope("deployment:1"),
        expected_revision: DeploymentRevision::new(7).unwrap(),
        controller_id: ControllerId::parse("controller:1").unwrap(),
        fencing_token: FencingToken::new(8).unwrap(),
        runtime_generation: RuntimeGeneration::new(9).unwrap(),
        convergence_attempt: NonZeroU32::new(2).unwrap(),
    }
}

fn failure() -> RuntimeFailureV1 {
    RuntimeFailureV1 {
        failure_id: RuntimeFailureId::parse("failure:1").unwrap(),
        kind: RuntimeFailureKindV1::EnvironmentUnavailable,
        code: "dependency_unavailable".to_string(),
        message: "dependency unavailable".to_string(),
        recorded_at: at(20),
    }
}

fn local_route() -> RuntimeExactLocalRouteIdentityV2 {
    RuntimeExactLocalRouteIdentityV2 {
        identity: process(GUILD_ID, "studyroom", 9, "process:current"),
        controller_fencing_token: FencingToken::new(8).unwrap(),
        route_incarnation: non_zero(10),
    }
}

fn previous_route() -> RuntimePreviousServingLeaseIdentityV1 {
    RuntimePreviousServingLeaseIdentityV1 {
        scope: scope("deployment:previous"),
        attestation_id: crate::RuntimeAttestationIdV1::parse("d".repeat(64)).unwrap(),
        process: process(GUILD_ID, "studyroom", 7, "process:previous"),
        lease_epoch: non_zero(11),
        revision: non_zero(12),
    }
}

fn gateway_owner_lease_id(process_instance_id: &str) -> RuntimeGatewayOwnerLeaseIdV1 {
    RuntimeGatewayOwnerLeaseIdV1 {
        gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
        process_instance_id: ProcessInstanceId::parse(process_instance_id).unwrap(),
        lease_epoch: non_zero(31),
        expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
    }
}

fn ordinary_provenance() -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::Ordinary {
        barrier_id: RuntimeBarrierIdV1::parse(BARRIER_ID).unwrap(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(21),
            connection_epoch: non_zero(22),
            paused_admission_revision: non_zero(23),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(24)),
        },
    }
}

fn closed_recovery_provenance() -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::ClosedRecovery(RuntimeClosedRecoveryRouteWitnessV2 {
        recovery_id: RuntimeRecoveryIdV2::parse(RECOVERY_ID).unwrap(),
        originating_emergency_generation: non_zero(25),
        recovery_generation: non_zero(26),
        recovery_authority_revision: non_zero(27),
        gateway_owner_lease_id: gateway_owner_lease_id("process:gateway"),
        observed_owner_revision: non_zero(32),
        owner_expires_at: at(50),
        process_instance_id: ProcessInstanceId::parse("process:gateway").unwrap(),
        connection_epoch: non_zero(33),
        paused_admission_revision: non_zero(34),
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(35)),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(36)),
    })
}

fn shutdown_provenance() -> RuntimeRouteMutationProvenanceV2 {
    RuntimeRouteMutationProvenanceV2::Shutdown(RuntimeShutdownRouteWitnessV2 {
        shutdown_generation: non_zero(37),
        gateway_owner_lease_id: gateway_owner_lease_id("process:gateway"),
        observed_owner_revision: non_zero(38),
        owner_expires_at: at(51),
        process_instance_id: ProcessInstanceId::parse("process:gateway").unwrap(),
        connection_epoch: non_zero(39),
        paused_admission_revision: non_zero(40),
        connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(41)),
        pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(42)),
    })
}

fn request(
    local_effect: RuntimeLocalRouteEffectV2,
    drain_obligation: RuntimeDrainObligationV2,
) -> RuntimeSuspendAttemptRequestV2 {
    RuntimeSuspendAttemptRequestV2 {
        suspension_id: RuntimeSuspensionIdV2::parse(SUSPENSION_ID).unwrap(),
        action_id: RuntimeSessionActionIdV1::new(non_zero(10)),
        guard: guard(),
        source_phase: RuntimeSuspensionSourcePhaseV2::Requested,
        failure: failure(),
        disposition: RuntimeAttemptDispositionV2::Retryable {
            retry_not_before: at(40),
        },
        checkpoint: RuntimeResumeCheckpointV2::VerifyPreflight,
        local_effect,
        drain_obligation,
    }
}

fn simple_request() -> RuntimeSuspendAttemptRequestV2 {
    request(
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::None,
    )
}

fn exact_effect(lifecycle: RuntimeSuspendedRouteLifecycleV2) -> RuntimeLocalRouteEffectV2 {
    RuntimeLocalRouteEffectV2::ExactRoute {
        route: local_route(),
        lifecycle,
    }
}

fn absent_effect(
    expected_route: Option<RuntimeExactLocalRouteIdentityV2>,
    provenance: RuntimeRouteMutationProvenanceV2,
) -> RuntimeLocalRouteEffectV2 {
    RuntimeLocalRouteEffectV2::RouteAbsent {
        slot: RuntimeServingSlotV2::from_target(&local_route().identity.target),
        expected_route,
        provenance,
        observed_sequence: non_zero(43),
    }
}

fn expected_retryable_prefix() -> &'static str {
    concat!(
        "{\"format_version\":2,",
        "\"suspension_id\":\"00112233445566778899aabbccddeeff\",",
        "\"action_id\":10,",
        "\"guard\":{\"scope\":{\"tenant_id\":\"tenant:1\",",
        "\"installation_id\":\"installation:1\",\"deployment_id\":\"deployment:1\"},",
        "\"expected_revision\":7,\"controller_id\":\"controller:1\",",
        "\"fencing_token\":8,\"runtime_generation\":9,\"convergence_attempt\":2},",
        "\"source_phase\":\"requested\",",
        "\"failure\":{\"failure_id\":\"failure:1\",\"kind\":\"environment_unavailable\",",
        "\"code\":\"dependency_unavailable\",\"message\":\"dependency unavailable\",",
        "\"recorded_at_unix_microseconds\":20000000},",
        "\"disposition\":{\"kind\":\"retryable\",",
        "\"retry_not_before_unix_microseconds\":40000000},",
        "\"checkpoint\":\"verify_preflight\","
    )
}

fn expected_simple_json() -> String {
    [
        expected_retryable_prefix(),
        "\"local_effect\":{\"kind\":\"none\"},",
        "\"drain_obligation\":{\"kind\":\"none\"}}",
    ]
    .concat()
}

fn expected_local_route_json() -> &'static str {
    concat!(
        "{\"identity\":{\"target\":{\"guild_id\":\"9223372036854775808\",",
        "\"ruleset_key\":\"studyroom\",\"version\":1,",
        "\"content_hash\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\",",
        "\"binding_revision\":3,",
        "\"binding_fingerprint\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"},",
        "\"runtime_generation\":9,\"process_instance_id\":\"process:current\"},",
        "\"controller_fencing_token\":8,\"route_incarnation\":10}"
    )
}

fn expected_previous_json() -> &'static str {
    concat!(
        "{\"scope\":{\"tenant_id\":\"tenant:1\",",
        "\"installation_id\":\"installation:1\",",
        "\"deployment_id\":\"deployment:previous\"},",
        "\"attestation_id\":\"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd\",",
        "\"process\":{\"target\":{\"guild_id\":\"9223372036854775808\",",
        "\"ruleset_key\":\"studyroom\",\"version\":1,",
        "\"content_hash\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\",",
        "\"binding_revision\":3,",
        "\"binding_fingerprint\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"},",
        "\"runtime_generation\":7,\"process_instance_id\":\"process:previous\"},",
        "\"lease_epoch\":11,\"revision\":12}"
    )
}

fn expected_ordinary_provenance_json() -> &'static str {
    concat!(
        "{\"kind\":\"ordinary\",",
        "\"barrier_id\":\"ffeeddccbbaa99887766554433221100\",",
        "\"pause\":{\"coordinator_generation\":21,\"connection_epoch\":22,",
        "\"paused_admission_revision\":23,\"pause_sequence\":24}}"
    )
}

fn expected_closed_recovery_provenance_json() -> &'static str {
    concat!(
        "{\"kind\":\"closed_recovery\",\"witness\":{",
        "\"recovery_id\":\"11112222333344445555666677778888\",",
        "\"originating_emergency_generation\":25,\"recovery_generation\":26,",
        "\"recovery_authority_revision\":27,",
        "\"gateway_owner_lease_id\":{\"gateway_shard_id\":\"shard:0\",",
        "\"process_instance_id\":\"process:gateway\",\"lease_epoch\":31,",
        "\"expected_build_revision\":\"build:1\"},",
        "\"observed_owner_revision\":32,",
        "\"owner_expires_at_unix_microseconds\":50000000,",
        "\"process_instance_id\":\"process:gateway\",\"connection_epoch\":33,",
        "\"paused_admission_revision\":34,\"connected_event_sequence\":35,",
        "\"pause_sequence\":36}}"
    )
}

fn expected_shutdown_provenance_json() -> &'static str {
    concat!(
        "{\"kind\":\"shutdown\",\"witness\":{\"shutdown_generation\":37,",
        "\"gateway_owner_lease_id\":{\"gateway_shard_id\":\"shard:0\",",
        "\"process_instance_id\":\"process:gateway\",\"lease_epoch\":31,",
        "\"expected_build_revision\":\"build:1\"},",
        "\"observed_owner_revision\":38,",
        "\"owner_expires_at_unix_microseconds\":51000000,",
        "\"process_instance_id\":\"process:gateway\",\"connection_epoch\":39,",
        "\"paused_admission_revision\":40,\"connected_event_sequence\":41,",
        "\"pause_sequence\":42}}"
    )
}

fn assert_exact_retryable_payload(request: RuntimeSuspendAttemptRequestV2, payload: &str) {
    let expected = [expected_retryable_prefix(), payload].concat();
    let canonical = RuntimeCanonicalSuspendAttemptV2::new(request).unwrap();
    assert_eq!(
        canonical.suspend_attempt_request_bytes(),
        expected.as_bytes()
    );
    assert_eq!(
        canonical.suspend_attempt_digest().as_str(),
        independent_suspend_digest(expected.as_bytes())
    );
}

fn independent_suspend_digest(payload: &[u8]) -> String {
    let domain = b"starring.runtime.suspend_attempt.v2\0";
    let mut digest = Sha256::new();
    digest.update((domain.len() as u64).to_be_bytes());
    digest.update(domain);
    digest.update((payload.len() as u64).to_be_bytes());
    digest.update(payload);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn replace_once(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .replacen(from, to, 1)
        .into_bytes()
}

fn decode_error(bytes: &[u8], expected: RuntimeSuspendAttemptCanonicalErrorV2) {
    assert_eq!(
        wire::decode_suspend_attempt(bytes),
        Err(expected),
        "{}",
        String::from_utf8_lossy(bytes)
    );
}

fn correlation_error(
    request: &RuntimeSuspendAttemptRequestV2,
) -> RuntimeSuspendAttemptCanonicalErrorV2 {
    validate_request(request).unwrap_err()
}

#[test]
fn simple_root_matches_the_exact_byte_and_independent_digest_golden() {
    let canonical = RuntimeCanonicalSuspendAttemptV2::new(simple_request()).unwrap();
    let expected = expected_simple_json();

    assert_eq!(
        canonical.suspend_attempt_request_bytes(),
        expected.as_bytes()
    );
    assert_eq!(
        canonical.suspend_attempt_digest().as_str(),
        EXPECTED_SIMPLE_DIGEST
    );
    assert_eq!(
        independent_suspend_digest(expected.as_bytes()),
        EXPECTED_SIMPLE_DIGEST
    );
    assert_eq!(
        suspend_attempt_digest_v2(expected.as_bytes()).as_str(),
        EXPECTED_SIMPLE_DIGEST
    );
}

#[test]
fn string_escaping_has_exact_utf8_json_and_digest_goldens() {
    for (message, expected_fragment) in [
        ("quote \" value", "quote \\\" value"),
        ("path \\ value", "path \\\\ value"),
        ("line\n\t\u{0001}end", "line\\n\\t\\u0001end"),
        ("한글", "한글"),
        ("😀", "😀"),
    ] {
        let mut request = simple_request();
        request.failure.message = message.to_owned();
        let canonical = RuntimeCanonicalSuspendAttemptV2::new(request).unwrap();
        let expected =
            expected_simple_json().replacen("dependency unavailable", expected_fragment, 1);

        assert_eq!(
            canonical.suspend_attempt_request_bytes(),
            expected.as_bytes()
        );
        assert_eq!(
            canonical.suspend_attempt_digest().as_str(),
            independent_suspend_digest(expected.as_bytes())
        );
    }

    for (message, alternate) in [("한글", "\\ud55c\\uae00"), ("😀", "\\ud83d\\ude00")] {
        let mut request = simple_request();
        request.failure.message = message.to_owned();
        let canonical = RuntimeCanonicalSuspendAttemptV2::new(request).unwrap();
        let alternate = replace_once(
            canonical.suspend_attempt_request_bytes(),
            message,
            alternate,
        );
        decode_error(
            &alternate,
            RuntimeSuspendAttemptCanonicalErrorV2::NonCanonicalEncoding,
        );
    }
}

#[test]
fn persisted_root_requires_the_exact_typed_digest() {
    let canonical = RuntimeCanonicalSuspendAttemptV2::new(simple_request()).unwrap();
    let restored = RuntimeCanonicalSuspendAttemptV2::from_persisted(
        canonical.suspend_attempt_request_bytes(),
        canonical.suspend_attempt_digest(),
    )
    .unwrap();

    assert_eq!(restored, canonical);
    let wrong = RuntimeSuspendAttemptDigestV2::parse("0".repeat(64)).unwrap();
    assert_eq!(
        RuntimeCanonicalSuspendAttemptV2::from_persisted(
            canonical.suspend_attempt_request_bytes(),
            &wrong,
        ),
        Err(RuntimeSuspendAttemptCanonicalErrorV2::PersistedDigestMismatch)
    );
}

#[test]
fn every_source_phase_checkpoint_and_failure_kind_uses_a_fixed_tag() {
    for (phase, checkpoint, phase_tag, checkpoint_tag) in [
        (
            RuntimeSuspensionSourcePhaseV2::Requested,
            RuntimeResumeCheckpointV2::VerifyPreflight,
            "requested",
            "verify_preflight",
        ),
        (
            RuntimeSuspensionSourcePhaseV2::PreflightReady,
            RuntimeResumeCheckpointV2::RequestDrain,
            "preflight_ready",
            "request_drain",
        ),
        (
            RuntimeSuspensionSourcePhaseV2::DrainRequested,
            RuntimeResumeCheckpointV2::CompleteDrain,
            "drain_requested",
            "complete_drain",
        ),
        (
            RuntimeSuspensionSourcePhaseV2::Drained,
            RuntimeResumeCheckpointV2::BeginActivation,
            "drained",
            "begin_activation",
        ),
        (
            RuntimeSuspensionSourcePhaseV2::ActivationApplying,
            RuntimeResumeCheckpointV2::ObserveActivation,
            "activation_applying",
            "observe_activation",
        ),
        (
            RuntimeSuspensionSourcePhaseV2::RuntimePendingReady,
            RuntimeResumeCheckpointV2::BeginPanels,
            "runtime_pending_ready",
            "begin_panels",
        ),
        (
            RuntimeSuspensionSourcePhaseV2::ReconcilingPanels,
            RuntimeResumeCheckpointV2::ReconcilePanels,
            "reconciling_panels",
            "reconcile_panels",
        ),
    ] {
        let mut value = simple_request();
        value.source_phase = phase;
        value.checkpoint = checkpoint;
        let canonical = RuntimeCanonicalSuspendAttemptV2::new(value).unwrap();
        let expected = expected_simple_json()
            .replacen(
                "\"source_phase\":\"requested\"",
                &format!("\"source_phase\":\"{phase_tag}\""),
                1,
            )
            .replacen(
                "\"checkpoint\":\"verify_preflight\"",
                &format!("\"checkpoint\":\"{checkpoint_tag}\""),
                1,
            );

        assert_eq!(
            canonical.suspend_attempt_request_bytes(),
            expected.as_bytes()
        );
        assert_eq!(
            canonical.suspend_attempt_digest().as_str(),
            independent_suspend_digest(expected.as_bytes())
        );
    }

    for (kind, tag) in [
        (
            RuntimeFailureKindV1::EnvironmentUnavailable,
            "environment_unavailable",
        ),
        (
            RuntimeFailureKindV1::ActivationNotObservable,
            "activation_not_observable",
        ),
        (
            RuntimeFailureKindV1::PanelReconciliation,
            "panel_reconciliation",
        ),
        (RuntimeFailureKindV1::GatewayStart, "gateway_start"),
        (
            RuntimeFailureKindV1::GatewayReadyTimeout,
            "gateway_ready_timeout",
        ),
        (
            RuntimeFailureKindV1::InvariantViolation,
            "invariant_violation",
        ),
    ] {
        let mut value = simple_request();
        value.failure.kind = kind;
        let canonical = RuntimeCanonicalSuspendAttemptV2::new(value).unwrap();
        let expected = expected_simple_json().replacen(
            "\"kind\":\"environment_unavailable\"",
            &format!("\"kind\":\"{tag}\""),
            1,
        );

        assert_eq!(
            canonical.suspend_attempt_request_bytes(),
            expected.as_bytes()
        );
        assert_eq!(
            canonical.suspend_attempt_digest().as_str(),
            independent_suspend_digest(expected.as_bytes())
        );
    }
}

#[test]
fn payload_variants_use_explicit_kind_tags_and_named_fields() {
    let blocked = {
        let mut value = simple_request();
        value.disposition = RuntimeAttemptDispositionV2::Blocked;
        value
    };
    let blocked = RuntimeCanonicalSuspendAttemptV2::new(blocked).unwrap();
    let expected_blocked = expected_simple_json().replacen(
        "\"disposition\":{\"kind\":\"retryable\",\"retry_not_before_unix_microseconds\":40000000}",
        "\"disposition\":{\"kind\":\"blocked\"}",
        1,
    );
    assert_eq!(
        blocked.suspend_attempt_request_bytes(),
        expected_blocked.as_bytes()
    );
    assert_eq!(
        blocked.suspend_attempt_digest().as_str(),
        independent_suspend_digest(expected_blocked.as_bytes())
    );

    for (lifecycle, tag) in [
        (RuntimeSuspendedRouteLifecycleV2::Staged, "staged"),
        (RuntimeSuspendedRouteLifecycleV2::Draining, "draining"),
    ] {
        let canonical = RuntimeCanonicalSuspendAttemptV2::new(request(
            exact_effect(lifecycle),
            RuntimeDrainObligationV2::ExactLocalRoute(local_route()),
        ))
        .unwrap();
        let text = String::from_utf8(canonical.suspend_attempt_request_bytes().to_vec()).unwrap();
        assert!(text.contains("\"local_effect\":{\"kind\":\"exact_route\",\"route\":"));
        assert!(text.contains(&format!(",\"lifecycle\":\"{tag}\"}}")));
        assert!(text.contains("\"drain_obligation\":{\"kind\":\"exact_local_route\",\"route\":"));
    }

    let canonical = RuntimeCanonicalSuspendAttemptV2::new(request(
        exact_effect(RuntimeSuspendedRouteLifecycleV2::Staged),
        RuntimeDrainObligationV2::LocalAndPrevious {
            local: local_route(),
            previous: previous_route(),
        },
    ))
    .unwrap();
    let text = String::from_utf8(canonical.suspend_attempt_request_bytes().to_vec()).unwrap();
    assert!(text.contains("\"drain_obligation\":{\"kind\":\"local_and_previous\",\"local\":"));
    assert!(text.contains(",\"previous\":"));

    for (provenance, tag, named_field) in [
        (ordinary_provenance(), "ordinary", "\"barrier_id\":"),
        (
            closed_recovery_provenance(),
            "closed_recovery",
            "\"witness\":",
        ),
        (shutdown_provenance(), "shutdown", "\"witness\":"),
    ] {
        let canonical = RuntimeCanonicalSuspendAttemptV2::new(request(
            absent_effect(Some(local_route()), provenance),
            RuntimeDrainObligationV2::PreviousServing(previous_route()),
        ))
        .unwrap();
        let text = String::from_utf8(canonical.suspend_attempt_request_bytes().to_vec()).unwrap();
        assert!(text.contains("\"local_effect\":{\"kind\":\"route_absent\",\"slot\":"));
        assert!(text.contains("\"expected_route\":"));
        assert!(text.contains(&format!("\"provenance\":{{\"kind\":\"{tag}\",")));
        assert!(text.contains(named_field));
        assert!(text.contains("\"observed_sequence\":43"));
        assert!(text.contains("\"drain_obligation\":{\"kind\":\"previous_serving\",\"previous\":"));
    }
}

#[test]
fn nested_payload_variants_match_exact_byte_goldens() {
    let exact_local = [
        "\"local_effect\":{\"kind\":\"exact_route\",\"route\":",
        expected_local_route_json(),
        ",\"lifecycle\":\"staged\"},",
        "\"drain_obligation\":{\"kind\":\"exact_local_route\",\"route\":",
        expected_local_route_json(),
        "}}",
    ]
    .concat();
    assert_exact_retryable_payload(
        request(
            exact_effect(RuntimeSuspendedRouteLifecycleV2::Staged),
            RuntimeDrainObligationV2::ExactLocalRoute(local_route()),
        ),
        &exact_local,
    );

    let local_and_previous = [
        "\"local_effect\":{\"kind\":\"exact_route\",\"route\":",
        expected_local_route_json(),
        ",\"lifecycle\":\"draining\"},",
        "\"drain_obligation\":{\"kind\":\"local_and_previous\",\"local\":",
        expected_local_route_json(),
        ",\"previous\":",
        expected_previous_json(),
        "}}",
    ]
    .concat();
    assert_exact_retryable_payload(
        request(
            exact_effect(RuntimeSuspendedRouteLifecycleV2::Draining),
            RuntimeDrainObligationV2::LocalAndPrevious {
                local: local_route(),
                previous: previous_route(),
            },
        ),
        &local_and_previous,
    );

    let ordinary_absence = [
        "\"local_effect\":{\"kind\":\"route_absent\",",
        "\"slot\":{\"guild_id\":\"9223372036854775808\",",
        "\"ruleset_key\":\"studyroom\"},\"expected_route\":null,\"provenance\":",
        expected_ordinary_provenance_json(),
        ",\"observed_sequence\":43},",
        "\"drain_obligation\":{\"kind\":\"none\"}}",
    ]
    .concat();
    assert_exact_retryable_payload(
        request(
            absent_effect(None, ordinary_provenance()),
            RuntimeDrainObligationV2::None,
        ),
        &ordinary_absence,
    );

    let closed_recovery_absence = [
        "\"local_effect\":{\"kind\":\"route_absent\",",
        "\"slot\":{\"guild_id\":\"9223372036854775808\",",
        "\"ruleset_key\":\"studyroom\"},\"expected_route\":",
        expected_local_route_json(),
        ",\"provenance\":",
        expected_closed_recovery_provenance_json(),
        ",\"observed_sequence\":43},",
        "\"drain_obligation\":{\"kind\":\"previous_serving\",\"previous\":",
        expected_previous_json(),
        "}}",
    ]
    .concat();
    assert_exact_retryable_payload(
        request(
            absent_effect(Some(local_route()), closed_recovery_provenance()),
            RuntimeDrainObligationV2::PreviousServing(previous_route()),
        ),
        &closed_recovery_absence,
    );

    let shutdown_absence = [
        "\"local_effect\":{\"kind\":\"route_absent\",",
        "\"slot\":{\"guild_id\":\"9223372036854775808\",",
        "\"ruleset_key\":\"studyroom\"},\"expected_route\":null,\"provenance\":",
        expected_shutdown_provenance_json(),
        ",\"observed_sequence\":43},",
        "\"drain_obligation\":{\"kind\":\"none\"}}",
    ]
    .concat();
    assert_exact_retryable_payload(
        request(
            absent_effect(None, shutdown_provenance()),
            RuntimeDrainObligationV2::None,
        ),
        &shutdown_absence,
    );
}

#[test]
fn exactly_six_effect_obligation_combinations_are_accepted() {
    let accepted = [
        request(
            RuntimeLocalRouteEffectV2::None,
            RuntimeDrainObligationV2::None,
        ),
        request(
            RuntimeLocalRouteEffectV2::None,
            RuntimeDrainObligationV2::PreviousServing(previous_route()),
        ),
        request(
            exact_effect(RuntimeSuspendedRouteLifecycleV2::Staged),
            RuntimeDrainObligationV2::ExactLocalRoute(local_route()),
        ),
        request(
            exact_effect(RuntimeSuspendedRouteLifecycleV2::Draining),
            RuntimeDrainObligationV2::LocalAndPrevious {
                local: local_route(),
                previous: previous_route(),
            },
        ),
        request(
            absent_effect(None, ordinary_provenance()),
            RuntimeDrainObligationV2::None,
        ),
        request(
            absent_effect(Some(local_route()), shutdown_provenance()),
            RuntimeDrainObligationV2::PreviousServing(previous_route()),
        ),
    ];

    for value in accepted {
        assert!(RuntimeCanonicalSuspendAttemptV2::new(value).is_ok());
    }
}

#[test]
fn exactly_six_effect_obligation_combinations_are_rejected() {
    let rejected = [
        request(
            RuntimeLocalRouteEffectV2::None,
            RuntimeDrainObligationV2::ExactLocalRoute(local_route()),
        ),
        request(
            RuntimeLocalRouteEffectV2::None,
            RuntimeDrainObligationV2::LocalAndPrevious {
                local: local_route(),
                previous: previous_route(),
            },
        ),
        request(
            exact_effect(RuntimeSuspendedRouteLifecycleV2::Staged),
            RuntimeDrainObligationV2::None,
        ),
        request(
            exact_effect(RuntimeSuspendedRouteLifecycleV2::Staged),
            RuntimeDrainObligationV2::PreviousServing(previous_route()),
        ),
        request(
            absent_effect(None, ordinary_provenance()),
            RuntimeDrainObligationV2::ExactLocalRoute(local_route()),
        ),
        request(
            absent_effect(None, ordinary_provenance()),
            RuntimeDrainObligationV2::LocalAndPrevious {
                local: local_route(),
                previous: previous_route(),
            },
        ),
    ];

    for value in rejected {
        assert_eq!(
            RuntimeCanonicalSuspendAttemptV2::new(value),
            Err(RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
                field: RuntimeSuspendAttemptCorrelationV2::LocalEffectDrainObligation,
            })
        );
    }
}

#[test]
fn decoder_rejects_hostile_and_noncanonical_json() {
    let canonical = RuntimeCanonicalSuspendAttemptV2::new(simple_request()).unwrap();
    let bytes = canonical.suspend_attempt_request_bytes();

    for hostile in [
        replace_once(
            bytes,
            "\"format_version\":2,",
            "\"format_version\":2,\"format_version\":2,",
        ),
        replace_once(
            bytes,
            &format!("\"suspension_id\":\"{SUSPENSION_ID}\","),
            "",
        ),
        replace_once(
            bytes,
            "\"deployment_id\":\"deployment:1\"}",
            "\"deployment_id\":\"deployment:1\",\"unexpected\":true}",
        ),
        replace_once(
            bytes,
            "\"kind\":\"retryable\",",
            "\"kind\":\"retryable\",\"unexpected\":true,",
        ),
        replace_once(
            bytes,
            "\"local_effect\":{\"kind\":\"none\"}",
            "\"local_effect\":{\"kind\":\"none\",\"unexpected\":true}",
        ),
        replace_once(
            bytes,
            "\"drain_obligation\":{\"kind\":\"none\"}",
            "\"drain_obligation\":{\"kind\":\"none\",\"unexpected\":true}",
        ),
    ] {
        decode_error(&hostile, RuntimeSuspendAttemptCanonicalErrorV2::Decoding);
    }

    let mut unknown = String::from_utf8(bytes.to_vec()).unwrap();
    assert_eq!(unknown.pop(), Some('}'));
    unknown.push_str(",\"unexpected\":true}");
    decode_error(
        unknown.as_bytes(),
        RuntimeSuspendAttemptCanonicalErrorV2::Decoding,
    );

    let reordered = replace_once(
        bytes,
        &format!("{{\"format_version\":2,\"suspension_id\":\"{SUSPENSION_ID}\""),
        &format!("{{\"suspension_id\":\"{SUSPENSION_ID}\",\"format_version\":2"),
    );
    decode_error(
        &reordered,
        RuntimeSuspendAttemptCanonicalErrorV2::NonCanonicalEncoding,
    );

    let mut leading_whitespace = vec![b' '];
    leading_whitespace.extend_from_slice(bytes);
    decode_error(
        &leading_whitespace,
        RuntimeSuspendAttemptCanonicalErrorV2::NonCanonicalEncoding,
    );

    let mut trailing_whitespace = bytes.to_vec();
    trailing_whitespace.push(b'\n');
    decode_error(
        &trailing_whitespace,
        RuntimeSuspendAttemptCanonicalErrorV2::NonCanonicalEncoding,
    );

    let alternate_escape = replace_once(
        bytes,
        "dependency unavailable",
        "dependency\\u0020unavailable",
    );
    decode_error(
        &alternate_escape,
        RuntimeSuspendAttemptCanonicalErrorV2::NonCanonicalEncoding,
    );

    let wrong_version = replace_once(bytes, "\"format_version\":2", "\"format_version\":3");
    decode_error(
        &wrong_version,
        RuntimeSuspendAttemptCanonicalErrorV2::UnsupportedFormatVersion,
    );

    let mut trailing = bytes.to_vec();
    trailing.extend_from_slice(b"null");
    decode_error(&trailing, RuntimeSuspendAttemptCanonicalErrorV2::Decoding);

    let mut invalid_utf8 = bytes.to_vec();
    invalid_utf8[0] = 0xff;
    decode_error(
        &invalid_utf8,
        RuntimeSuspendAttemptCanonicalErrorV2::Decoding,
    );

    decode_error(
        &vec![b' '; SUSPEND_ATTEMPT_MAX_OCTETS + 1],
        RuntimeSuspendAttemptCanonicalErrorV2::PayloadTooLarge,
    );
}

#[test]
fn route_absence_requires_an_explicit_expected_route_field_even_when_null() {
    let canonical = RuntimeCanonicalSuspendAttemptV2::new(request(
        absent_effect(None, ordinary_provenance()),
        RuntimeDrainObligationV2::None,
    ))
    .unwrap();
    let bytes = canonical.suspend_attempt_request_bytes();

    assert!(String::from_utf8_lossy(bytes).contains("\"expected_route\":null"));
    let missing = replace_once(bytes, "\"expected_route\":null,", "");
    decode_error(&missing, RuntimeSuspendAttemptCanonicalErrorV2::Decoding);
}

#[test]
fn checkpoint_and_failure_rules_are_intrinsic_to_the_root() {
    let mut wrong_checkpoint = simple_request();
    wrong_checkpoint.checkpoint = RuntimeResumeCheckpointV2::RequestDrain;
    assert_eq!(
        correlation_error(&wrong_checkpoint),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::SourcePhaseCheckpoint,
        }
    );

    for invalid_code in [
        String::new(),
        "UPPERCASE".to_string(),
        "hyphen-code".to_string(),
        "a".repeat(65),
    ] {
        let mut value = simple_request();
        value.failure.code = invalid_code;
        assert_eq!(
            RuntimeCanonicalSuspendAttemptV2::new(value),
            Err(RuntimeSuspendAttemptCanonicalErrorV2::InvalidField {
                field: RuntimeSuspendAttemptCanonicalFieldV2::FailureCode,
            })
        );
    }

    for invalid_message in [String::new(), "   ".to_string(), "a".repeat(1025)] {
        let mut value = simple_request();
        value.failure.message = invalid_message;
        assert_eq!(
            RuntimeCanonicalSuspendAttemptV2::new(value),
            Err(RuntimeSuspendAttemptCanonicalErrorV2::InvalidField {
                field: RuntimeSuspendAttemptCanonicalFieldV2::FailureMessage,
            })
        );
    }

    let mut reverse_time = simple_request();
    reverse_time.disposition = RuntimeAttemptDispositionV2::Retryable {
        retry_not_before: at(19),
    };
    assert_eq!(
        correlation_error(&reverse_time),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::FailureDispositionTime,
        }
    );
}

#[test]
fn canonical_time_integer_and_snowflake_normalization_match_persistence() {
    for microseconds in [-62_135_596_800_000_000, -1, 0, 253_402_300_799_999_999] {
        let mut value = simple_request();
        value.failure.recorded_at = at_microseconds(microseconds);
        value.disposition = RuntimeAttemptDispositionV2::Blocked;
        let canonical = RuntimeCanonicalSuspendAttemptV2::new(value).unwrap();
        assert!(
            String::from_utf8_lossy(canonical.suspend_attempt_request_bytes())
                .contains(&format!("\"recorded_at_unix_microseconds\":{microseconds}"))
        );
    }

    let mut sub_microsecond = simple_request();
    sub_microsecond.failure.recorded_at = DateTime::<Utc>::from_timestamp(0, 1).unwrap();
    sub_microsecond.disposition = RuntimeAttemptDispositionV2::Blocked;
    assert_eq!(
        RuntimeCanonicalSuspendAttemptV2::new(sub_microsecond),
        Err(RuntimeSuspendAttemptCanonicalErrorV2::CanonicalValue {
            field: RuntimeSuspendAttemptCanonicalFieldV2::FailureRecordedAtUnixMicroseconds,
            reason: RuntimeCanonicalValueErrorV2::TimestampSubMicrosecond,
        })
    );

    let mut retry_sub_microsecond = simple_request();
    retry_sub_microsecond.disposition = RuntimeAttemptDispositionV2::Retryable {
        retry_not_before: DateTime::<Utc>::from_timestamp(40, 1).unwrap(),
    };
    assert_eq!(
        RuntimeCanonicalSuspendAttemptV2::new(retry_sub_microsecond),
        Err(RuntimeSuspendAttemptCanonicalErrorV2::CanonicalValue {
            field: RuntimeSuspendAttemptCanonicalFieldV2::RetryNotBeforeUnixMicroseconds,
            reason: RuntimeCanonicalValueErrorV2::TimestampSubMicrosecond,
        })
    );

    let mut max_integer = request(
        RuntimeLocalRouteEffectV2::ExactRoute {
            route: {
                let mut value = local_route();
                value.route_incarnation = non_zero(i64::MAX as u64);
                value
            },
            lifecycle: RuntimeSuspendedRouteLifecycleV2::Staged,
        },
        RuntimeDrainObligationV2::ExactLocalRoute({
            let mut value = local_route();
            value.route_incarnation = non_zero(i64::MAX as u64);
            value
        }),
    );
    assert!(RuntimeCanonicalSuspendAttemptV2::new(max_integer.clone()).is_ok());
    if let RuntimeLocalRouteEffectV2::ExactRoute { route, .. } = &mut max_integer.local_effect {
        route.route_incarnation = non_zero(i64::MAX as u64 + 1);
    }
    if let RuntimeDrainObligationV2::ExactLocalRoute(route) = &mut max_integer.drain_obligation {
        route.route_incarnation = non_zero(i64::MAX as u64 + 1);
    }
    assert_eq!(
        RuntimeCanonicalSuspendAttemptV2::new(max_integer),
        Err(RuntimeSuspendAttemptCanonicalErrorV2::CanonicalValue {
            field: RuntimeSuspendAttemptCanonicalFieldV2::LocalRouteIncarnation,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        })
    );

    let mut max_snowflake_route = local_route();
    max_snowflake_route.identity.target.guild_id = GuildId(u64::MAX);
    let max_snowflake = RuntimeCanonicalSuspendAttemptV2::new(request(
        RuntimeLocalRouteEffectV2::ExactRoute {
            route: max_snowflake_route.clone(),
            lifecycle: RuntimeSuspendedRouteLifecycleV2::Staged,
        },
        RuntimeDrainObligationV2::ExactLocalRoute(max_snowflake_route),
    ))
    .unwrap();
    assert!(
        String::from_utf8_lossy(max_snowflake.suspend_attempt_request_bytes())
            .contains("\"guild_id\":\"18446744073709551615\"")
    );

    let mut zero_snowflake_route = local_route();
    zero_snowflake_route.identity.target.guild_id = GuildId(0);
    assert_eq!(
        RuntimeCanonicalSuspendAttemptV2::new(request(
            RuntimeLocalRouteEffectV2::ExactRoute {
                route: zero_snowflake_route.clone(),
                lifecycle: RuntimeSuspendedRouteLifecycleV2::Staged,
            },
            RuntimeDrainObligationV2::ExactLocalRoute(zero_snowflake_route),
        )),
        Err(RuntimeSuspendAttemptCanonicalErrorV2::CanonicalValue {
            field: RuntimeSuspendAttemptCanonicalFieldV2::LocalTargetGuildId,
            reason: RuntimeCanonicalValueErrorV2::DiscordSnowflakeOutOfRange,
        })
    );
}

#[test]
fn decoder_rejects_noncanonical_time_integer_and_snowflake_forms() {
    let canonical = RuntimeCanonicalSuspendAttemptV2::new(simple_request()).unwrap();
    let bytes = canonical.suspend_attempt_request_bytes();
    let out_of_range_time = replace_once(
        bytes,
        "\"recorded_at_unix_microseconds\":20000000",
        "\"recorded_at_unix_microseconds\":253402300800000000",
    );
    decode_error(
        &out_of_range_time,
        RuntimeSuspendAttemptCanonicalErrorV2::CanonicalValue {
            field: RuntimeSuspendAttemptCanonicalFieldV2::FailureRecordedAtUnixMicroseconds,
            reason: RuntimeCanonicalValueErrorV2::TimestampOutOfRange,
        },
    );

    let local = RuntimeCanonicalSuspendAttemptV2::new(request(
        exact_effect(RuntimeSuspendedRouteLifecycleV2::Staged),
        RuntimeDrainObligationV2::ExactLocalRoute(local_route()),
    ))
    .unwrap();
    let local_bytes = local.suspend_attempt_request_bytes();
    let number_snowflake = replace_once(
        local_bytes,
        "\"guild_id\":\"9223372036854775808\"",
        "\"guild_id\":9223372036854775808",
    );
    decode_error(
        &number_snowflake,
        RuntimeSuspendAttemptCanonicalErrorV2::Decoding,
    );
    let leading_zero = replace_once(
        local_bytes,
        "\"guild_id\":\"9223372036854775808\"",
        "\"guild_id\":\"09223372036854775808\"",
    );
    decode_error(
        &leading_zero,
        RuntimeSuspendAttemptCanonicalErrorV2::CanonicalValue {
            field: RuntimeSuspendAttemptCanonicalFieldV2::LocalTargetGuildId,
            reason: RuntimeCanonicalValueErrorV2::DiscordSnowflakeNonCanonical,
        },
    );
    let persistence_overflow = replace_once(
        local_bytes,
        "\"route_incarnation\":10",
        "\"route_incarnation\":9223372036854775808",
    );
    decode_error(
        &persistence_overflow,
        RuntimeSuspendAttemptCanonicalErrorV2::CanonicalValue {
            field: RuntimeSuspendAttemptCanonicalFieldV2::LocalRouteIncarnation,
            reason: RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange,
        },
    );
}

#[test]
fn local_route_correlations_reject_identity_fence_generation_and_slot_drift() {
    let mut identity = local_route();
    identity.route_incarnation = non_zero(99);
    let mismatch = request(
        exact_effect(RuntimeSuspendedRouteLifecycleV2::Staged),
        RuntimeDrainObligationV2::ExactLocalRoute(identity),
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::LocalRouteIdentity,
        }
    );

    let mut generation = local_route();
    generation.identity.runtime_generation = RuntimeGeneration::new(10).unwrap();
    let mismatch = request(
        RuntimeLocalRouteEffectV2::ExactRoute {
            route: generation.clone(),
            lifecycle: RuntimeSuspendedRouteLifecycleV2::Staged,
        },
        RuntimeDrainObligationV2::ExactLocalRoute(generation),
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::LocalRouteRuntimeGeneration,
        }
    );

    let mut fence = local_route();
    fence.controller_fencing_token = FencingToken::new(9).unwrap();
    let mismatch = request(
        RuntimeLocalRouteEffectV2::ExactRoute {
            route: fence.clone(),
            lifecycle: RuntimeSuspendedRouteLifecycleV2::Staged,
        },
        RuntimeDrainObligationV2::ExactLocalRoute(fence),
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::LocalRouteControllerFencingToken,
        }
    );

    let mut expected_route = local_route();
    expected_route.identity.target.guild_id = GuildId(44);
    let mismatch = request(
        absent_effect(Some(expected_route), ordinary_provenance()),
        RuntimeDrainObligationV2::None,
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::ServingSlot,
        }
    );
}

#[test]
fn previous_serving_correlations_reject_product_generation_and_slot_drift() {
    let mut tenant = previous_route();
    tenant.scope.tenant_id = TenantId::parse("tenant:other").unwrap();
    let mismatch = request(
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::PreviousServing(tenant),
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::PreviousServingProductScope,
        }
    );

    let mut installation = previous_route();
    installation.scope.installation_id = InstallationId::parse("installation:other").unwrap();
    let mismatch = request(
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::PreviousServing(installation),
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::PreviousServingProductScope,
        }
    );

    let mut same_deployment = previous_route();
    same_deployment.scope.deployment_id = DeploymentId::parse("deployment:1").unwrap();
    let mismatch = request(
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::PreviousServing(same_deployment),
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::PreviousServingProductScope,
        }
    );

    let mut generation = previous_route();
    generation.process.runtime_generation = RuntimeGeneration::new(9).unwrap();
    let mismatch = request(
        RuntimeLocalRouteEffectV2::None,
        RuntimeDrainObligationV2::PreviousServing(generation),
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::PreviousServingRuntimeGeneration,
        }
    );

    let mut other_slot = previous_route();
    other_slot.process.target.ruleset_key = RuleSetKey::parse("other").unwrap();
    let mismatch = request(
        exact_effect(RuntimeSuspendedRouteLifecycleV2::Staged),
        RuntimeDrainObligationV2::LocalAndPrevious {
            local: local_route(),
            previous: other_slot,
        },
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::ServingSlot,
        }
    );
}

#[test]
fn recovery_and_shutdown_provenance_require_exact_generation_process_and_sequence() {
    let mut generation_mismatch = closed_recovery_provenance();
    if let RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) = &mut generation_mismatch {
        witness.recovery_generation = non_zero(27);
    }
    let mismatch = request(
        absent_effect(None, generation_mismatch),
        RuntimeDrainObligationV2::None,
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::RouteProvenanceGeneration,
        }
    );

    let mut process_mismatch = closed_recovery_provenance();
    if let RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) = &mut process_mismatch {
        witness.process_instance_id = ProcessInstanceId::parse("process:other").unwrap();
    }
    let mismatch = request(
        absent_effect(None, process_mismatch),
        RuntimeDrainObligationV2::None,
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::RouteProvenanceProcess,
        }
    );

    let mut sequence_mismatch = closed_recovery_provenance();
    if let RuntimeRouteMutationProvenanceV2::ClosedRecovery(witness) = &mut sequence_mismatch {
        witness.pause_sequence = witness.connected_event_sequence;
    }
    let mismatch = request(
        absent_effect(None, sequence_mismatch),
        RuntimeDrainObligationV2::None,
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::RouteProvenanceSequence,
        }
    );

    let mut process_mismatch = shutdown_provenance();
    if let RuntimeRouteMutationProvenanceV2::Shutdown(witness) = &mut process_mismatch {
        witness.gateway_owner_lease_id.process_instance_id =
            ProcessInstanceId::parse("process:other").unwrap();
    }
    let mismatch = request(
        absent_effect(None, process_mismatch),
        RuntimeDrainObligationV2::None,
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::RouteProvenanceProcess,
        }
    );

    let mut sequence_mismatch = shutdown_provenance();
    if let RuntimeRouteMutationProvenanceV2::Shutdown(witness) = &mut sequence_mismatch {
        witness.pause_sequence = RuntimeGatewayAdmissionSequenceV2::new(non_zero(
            witness.connected_event_sequence.get() - 1,
        ));
    }
    let mismatch = request(
        absent_effect(None, sequence_mismatch),
        RuntimeDrainObligationV2::None,
    );
    assert_eq!(
        correlation_error(&mismatch),
        RuntimeSuspendAttemptCanonicalErrorV2::CorrelationMismatch {
            field: RuntimeSuspendAttemptCorrelationV2::RouteProvenanceSequence,
        }
    );
}

#[test]
fn changing_identity_failure_or_effect_changes_the_typed_digest() {
    let canonical = RuntimeCanonicalSuspendAttemptV2::new(simple_request()).unwrap();
    let base = canonical.suspend_attempt_digest().clone();
    let mut variants = Vec::new();

    let mut value = simple_request();
    value.suspension_id = RuntimeSuspensionIdV2::parse("10112233445566778899aabbccddeeff").unwrap();
    variants.push(value);
    let mut value = simple_request();
    value.guard.expected_revision = DeploymentRevision::new(8).unwrap();
    variants.push(value);
    let mut value = simple_request();
    value.failure.failure_id = RuntimeFailureId::parse("failure:2").unwrap();
    variants.push(value);
    let mut value = simple_request();
    value.disposition = RuntimeAttemptDispositionV2::Blocked;
    variants.push(value);
    variants.push(request(
        exact_effect(RuntimeSuspendedRouteLifecycleV2::Staged),
        RuntimeDrainObligationV2::ExactLocalRoute(local_route()),
    ));
    variants.push(request(
        absent_effect(None, ordinary_provenance()),
        RuntimeDrainObligationV2::None,
    ));

    for value in variants {
        assert_ne!(
            RuntimeCanonicalSuspendAttemptV2::new(value)
                .unwrap()
                .suspend_attempt_digest(),
            &base
        );
    }
}
