use std::num::NonZeroU64;

use automation_runtime_convergence::{
    FencingToken, PanelCertificateId, PanelReportDigestV1, RuntimeProcessIdentityV1,
};

use crate::{
    RuntimeBarrierIdV1, RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayReadyAttestationV2, RuntimeServingSlotV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeEvidenceErrorV2 {
    #[error("runtime evidence process identity does not match")]
    ProcessMismatch,
    #[error("runtime evidence connection epoch does not match")]
    ConnectionEpochMismatch,
    #[error("runtime evidence admission revision does not match")]
    AdmissionRevisionMismatch,
    #[error("runtime evidence pause does not follow the connected event")]
    PauseSequenceNotAfterConnected,
    #[error("runtime evidence pause does not precede resume")]
    PauseSequenceNotBeforeResume,
    #[error("runtime evidence does not prove explicit resume")]
    ReadyNotExplicitlyResumed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePanelEvidenceV2 {
    pub certificate_id: PanelCertificateId,
    pub report_digest: PanelReportDigestV1,
    pub process_identity: RuntimeProcessIdentityV1,
    pub controller_fencing_token: FencingToken,
}

impl RuntimePanelEvidenceV2 {
    pub fn matches_process_authority(
        &self,
        process_identity: &RuntimeProcessIdentityV1,
        controller_fencing_token: FencingToken,
    ) -> bool {
        self.process_identity == *process_identity
            && self.controller_fencing_token == controller_fencing_token
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeServingRouteAttestationV2 {
    pub identity: RuntimeProcessIdentityV1,
    pub controller_fencing_token: FencingToken,
    pub route_incarnation: NonZeroU64,
    pub activation_sequence: NonZeroU64,
}

impl RuntimeServingRouteAttestationV2 {
    pub fn slot(&self) -> RuntimeServingSlotV2 {
        RuntimeServingSlotV2::from_target(&self.identity.target)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeBarrierPauseWitnessV2 {
    pub coordinator_generation: NonZeroU64,
    pub connection_epoch: NonZeroU64,
    pub paused_admission_revision: NonZeroU64,
    pub pause_sequence: RuntimeGatewayAdmissionSequenceV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRouteAdmissionAttestationV2 {
    pub barrier_id: RuntimeBarrierIdV1,
    pub pause: RuntimeBarrierPauseWitnessV2,
    pub gateway: RuntimeGatewayReadyAttestationV2,
    pub gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub attested_owner_revision: NonZeroU64,
    pub route: RuntimeServingRouteAttestationV2,
}

impl RuntimeRouteAdmissionAttestationV2 {
    pub fn validate(&self) -> Result<(), RuntimeEvidenceErrorV2> {
        let process_instance_id = &self.route.identity.process_instance_id;
        if self.gateway.process_instance_id != *process_instance_id
            || self.gateway_owner_lease_id.process_instance_id != *process_instance_id
        {
            return Err(RuntimeEvidenceErrorV2::ProcessMismatch);
        }
        if self.pause.connection_epoch != self.gateway.connection_epoch {
            return Err(RuntimeEvidenceErrorV2::ConnectionEpochMismatch);
        }
        if self.pause.paused_admission_revision != self.gateway.admission_revision {
            return Err(RuntimeEvidenceErrorV2::AdmissionRevisionMismatch);
        }
        if !self.gateway.was_explicitly_resumed() {
            return Err(RuntimeEvidenceErrorV2::ReadyNotExplicitlyResumed);
        }
        if self.pause.pause_sequence.get() <= self.gateway.connected_event_sequence.get() {
            return Err(RuntimeEvidenceErrorV2::PauseSequenceNotAfterConnected);
        }
        if self.pause.pause_sequence.get() >= self.gateway.resume_sequence.get() {
            return Err(RuntimeEvidenceErrorV2::PauseSequenceNotBeforeResume);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        BindingRevision, FencingToken, PanelCertificateId, PanelReportDigestV1, ProcessInstanceId,
        RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1,
    };
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::{
        RuntimeBarrierPauseWitnessV2, RuntimeEvidenceErrorV2, RuntimePanelEvidenceV2,
        RuntimeRouteAdmissionAttestationV2, RuntimeServingRouteAttestationV2,
    };
    use crate::{
        GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBuildRevisionV1,
        RuntimeGatewayAdmissionSequenceV2, RuntimeGatewayOwnerLeaseIdV1,
        RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2, RuntimeServingSlotV2,
    };

    fn non_zero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).unwrap()
    }

    fn process(name: &str) -> RuntimeProcessIdentityV1 {
        RuntimeProcessIdentityV1 {
            target: RuntimeDeploymentTargetV1 {
                guild_id: GuildId(7),
                ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
                version: RuleSetVersionId::FIRST,
                content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
                binding_revision: BindingRevision::new(3).unwrap(),
                binding_fingerprint: ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap(),
            },
            runtime_generation: RuntimeGeneration::new(4).unwrap(),
            process_instance_id: ProcessInstanceId::parse(name).unwrap(),
        }
    }

    fn admission() -> RuntimeRouteAdmissionAttestationV2 {
        let process = process("process:1");
        RuntimeRouteAdmissionAttestationV2 {
            barrier_id: RuntimeBarrierIdV1::parse("00112233445566778899aabbccddeeff").unwrap(),
            pause: RuntimeBarrierPauseWitnessV2 {
                coordinator_generation: non_zero(8),
                connection_epoch: non_zero(9),
                paused_admission_revision: non_zero(10),
                pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(12)),
            },
            gateway: RuntimeGatewayReadyAttestationV2 {
                process_instance_id: process.process_instance_id.clone(),
                connection_epoch: non_zero(9),
                kind: RuntimeGatewayReadyKindV2::Resumed,
                admission_revision: non_zero(10),
                connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(11)),
                resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
            },
            gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
                gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
                process_instance_id: process.process_instance_id.clone(),
                lease_epoch: non_zero(5),
                expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            },
            attested_owner_revision: non_zero(6),
            route: RuntimeServingRouteAttestationV2 {
                identity: process,
                controller_fencing_token: FencingToken::new(14).unwrap(),
                route_incarnation: non_zero(15),
                activation_sequence: non_zero(16),
            },
        }
    }

    #[test]
    fn panel_evidence_matches_only_the_exact_process_and_fence() {
        let expected_process = process("process:1");
        let evidence = RuntimePanelEvidenceV2 {
            certificate_id: PanelCertificateId::parse("panel").unwrap(),
            report_digest: PanelReportDigestV1::parse("c".repeat(64)).unwrap(),
            process_identity: expected_process.clone(),
            controller_fencing_token: FencingToken::new(5).unwrap(),
        };

        assert!(
            evidence.matches_process_authority(&expected_process, FencingToken::new(5).unwrap())
        );
        assert!(!evidence
            .matches_process_authority(&process("process:2"), FencingToken::new(5).unwrap()));
        assert!(
            !evidence.matches_process_authority(&expected_process, FencingToken::new(6).unwrap())
        );
    }

    #[test]
    fn route_evidence_derives_the_exact_serving_slot() {
        let evidence = admission();

        assert_eq!(
            evidence.route.slot(),
            RuntimeServingSlotV2::new(GuildId(7), RuleSetKey::parse("studyroom").unwrap())
        );
    }

    #[test]
    fn route_admission_accepts_only_closed_causal_evidence() {
        assert_eq!(admission().validate(), Ok(()));

        let mut ready_after_explicit_resume = admission();
        ready_after_explicit_resume.gateway.kind = RuntimeGatewayReadyKindV2::Ready;
        assert_eq!(ready_after_explicit_resume.validate(), Ok(()));

        let mut gateway_process = admission();
        gateway_process.gateway.process_instance_id = ProcessInstanceId::parse("other").unwrap();
        assert_eq!(
            gateway_process.validate(),
            Err(RuntimeEvidenceErrorV2::ProcessMismatch)
        );

        let mut owner_process = admission();
        owner_process.gateway_owner_lease_id.process_instance_id =
            ProcessInstanceId::parse("other").unwrap();
        assert_eq!(
            owner_process.validate(),
            Err(RuntimeEvidenceErrorV2::ProcessMismatch)
        );

        let mut epoch = admission();
        epoch.gateway.connection_epoch = non_zero(17);
        assert_eq!(
            epoch.validate(),
            Err(RuntimeEvidenceErrorV2::ConnectionEpochMismatch)
        );

        let mut revision = admission();
        revision.gateway.admission_revision = non_zero(17);
        assert_eq!(
            revision.validate(),
            Err(RuntimeEvidenceErrorV2::AdmissionRevisionMismatch)
        );

        let mut pause_order = admission();
        pause_order.pause.pause_sequence = pause_order.gateway.resume_sequence;
        assert_eq!(
            pause_order.validate(),
            Err(RuntimeEvidenceErrorV2::PauseSequenceNotBeforeResume)
        );

        let mut connected_order = admission();
        connected_order.pause.pause_sequence = connected_order.gateway.connected_event_sequence;
        assert_eq!(
            connected_order.validate(),
            Err(RuntimeEvidenceErrorV2::PauseSequenceNotAfterConnected)
        );

        let mut reverse_connected_order = admission();
        reverse_connected_order.pause.pause_sequence =
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(10));
        assert_eq!(
            reverse_connected_order.validate(),
            Err(RuntimeEvidenceErrorV2::PauseSequenceNotAfterConnected)
        );

        let mut legacy = admission();
        legacy.gateway.connected_event_sequence = legacy.gateway.resume_sequence;
        assert_eq!(
            legacy.validate(),
            Err(RuntimeEvidenceErrorV2::ReadyNotExplicitlyResumed)
        );
    }
}
