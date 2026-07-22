use std::num::NonZeroU64;
use std::time::Duration;

use automation_runtime_convergence::{RuntimeDeploymentTargetV1, RuntimeProcessIdentityV1};
use chrono::{DateTime, Utc};

use crate::{
    RuntimeBindingPinV1, RuntimeBuildRevisionV1, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationOperationIdV2, RuntimeExecutionGuardV1, RuntimeGatewayOwnerLeaseIdV1,
    RuntimePanelEvidenceV2, RuntimeRouteAdmissionAttestationV2, RuntimeSessionActionIdV1,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCertificationIntentV2 {
    pub action_id: RuntimeSessionActionIdV1,
    pub operation_id: RuntimeCertificationOperationIdV2,
    pub guard: RuntimeExecutionGuardV1,
    pub target: RuntimeDeploymentTargetV1,
    pub binding_pin: RuntimeBindingPinV1,
    pub process_identity: RuntimeProcessIdentityV1,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub observed_owner_revision: NonZeroU64,
    pub runtime_build_revision: RuntimeBuildRevisionV1,
    pub panel: RuntimePanelEvidenceV2,
    pub serving_lease_for: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCertificationRequestV2 {
    pub intent: RuntimeCertificationIntentV2,
    pub intent_fingerprint: RuntimeCertificationIntentFingerprintV2,
    pub must_commit_before: DateTime<Utc>,
    pub route_admission: RuntimeRouteAdmissionAttestationV2,
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};
    use std::time::Duration;

    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        BindingRevision, ControllerId, DeploymentId, DeploymentRevision, FencingToken,
        InstallationId, PanelCertificateId, PanelReportDigestV1, ProcessInstanceId,
        RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
    };
    use chrono::{DateTime, Utc};
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::{RuntimeCertificationIntentV2, RuntimeCertificationRequestV2};
    use crate::{
        GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBindingPinV1,
        RuntimeBuildRevisionV1, RuntimeCertificationIntentFingerprintV2,
        RuntimeCertificationOperationIdV2, RuntimeDeploymentScopeV1, RuntimeExecutionGuardV1,
        RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
        RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2, RuntimePanelEvidenceV2,
        RuntimeRouteAdmissionAttestationV2, RuntimeServingRouteAttestationV2,
        RuntimeSessionActionIdV1,
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

    fn owner_lease_id() -> RuntimeGatewayOwnerLeaseIdV1 {
        RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            lease_epoch: non_zero(5),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        }
    }

    fn intent() -> RuntimeCertificationIntentV2 {
        let process_identity = process_identity();
        RuntimeCertificationIntentV2 {
            action_id: RuntimeSessionActionIdV1::new(non_zero(1)),
            operation_id: RuntimeCertificationOperationIdV2::parse(
                "00112233445566778899aabbccddeeff",
            )
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
            gateway_owner_lease_id: owner_lease_id(),
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

    fn route_admission() -> RuntimeRouteAdmissionAttestationV2 {
        let process_identity = process_identity();
        RuntimeRouteAdmissionAttestationV2 {
            barrier_id: RuntimeBarrierIdV1::parse("ffeeddccbbaa99887766554433221100").unwrap(),
            pause: RuntimeBarrierPauseWitnessV2 {
                coordinator_generation: non_zero(8),
                connection_epoch: non_zero(9),
                paused_admission_revision: non_zero(10),
                pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(12)),
            },
            gateway: RuntimeGatewayReadyAttestationV2 {
                process_instance_id: process_identity.process_instance_id.clone(),
                connection_epoch: non_zero(9),
                kind: RuntimeGatewayReadyKindV2::Resumed,
                admission_revision: non_zero(10),
                connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(11)),
                resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
            },
            gateway_owner_lease_id: owner_lease_id(),
            attested_owner_revision: non_zero(7),
            route: RuntimeServingRouteAttestationV2 {
                identity: process_identity,
                controller_fencing_token: FencingToken::new(3).unwrap(),
                route_incarnation: non_zero(14),
                activation_sequence: non_zero(15),
            },
        }
    }

    #[test]
    fn certification_intent_carries_the_exact_prepared_inputs() {
        let intent = intent();

        assert_eq!(intent.action_id.get(), 1);
        assert_eq!(
            intent.operation_id.as_str(),
            "00112233445566778899aabbccddeeff"
        );
        assert_eq!(intent.guard.expected_revision.get(), 2);
        assert_eq!(intent.target, target());
        assert!(intent
            .binding_pin
            .matches(&intent.guard.scope, &intent.target));
        assert_eq!(intent.process_identity, process_identity());
        assert_eq!(intent.gateway_owner_lease_id, owner_lease_id());
        assert_eq!(intent.observed_owner_revision.get(), 7);
        assert_eq!(intent.runtime_build_revision.as_str(), "build:1");
        assert_eq!(intent.panel.process_identity, process_identity());
        assert_eq!(intent.serving_lease_for, Duration::from_secs(30));
    }

    #[test]
    fn certification_request_carries_only_the_intent_commit_bound_and_route_evidence() {
        let must_commit_before =
            DateTime::<Utc>::from_timestamp(1_700_000_000, 123_000_000).unwrap();
        let request = RuntimeCertificationRequestV2 {
            intent: intent(),
            intent_fingerprint: RuntimeCertificationIntentFingerprintV2::parse("d".repeat(64))
                .unwrap(),
            must_commit_before,
            route_admission: route_admission(),
        };

        assert_eq!(request.intent, intent());
        assert_eq!(request.intent_fingerprint.as_str(), "d".repeat(64));
        assert_eq!(request.must_commit_before, must_commit_before);
        assert_eq!(request.route_admission, route_admission());
    }
}
