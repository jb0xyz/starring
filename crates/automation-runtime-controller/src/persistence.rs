use std::fmt::{Display, Formatter};

use automation_runtime_convergence::{
    DeploymentRevision, FencingToken, LiveAttestationV1, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentTargetV1, RuntimeProcessIdentityV1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    GatewayShardIdV1, PanelReportDigestV1, RuntimeAttestationIdV1, RuntimeBuildRevisionV1,
    RuntimeControllerDtoError,
};

const DESIRED_TARGET_DOMAIN_V1: &[u8] = b"starring.runtime.desired_target.v1\0";
const LIVE_ATTESTATION_DOMAIN_V1: &[u8] = b"starring.runtime.live_attestation.v1\0";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RuntimeDesiredTargetDigestV1(String);

impl RuntimeDesiredTargetDigestV1 {
    pub fn parse(value: impl Into<String>) -> Result<Self, RuntimeControllerDtoError> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(RuntimeControllerDtoError::InvalidDigest);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn from_sha256(bytes: [u8; 32]) -> Self {
        Self(lower_hex(bytes))
    }
}

impl Display for RuntimeDesiredTargetDigestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RuntimeDesiredTargetDigestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLiveAttestationRecordV1 {
    pub live: LiveAttestationV1,
    pub runtime_build_revision: RuntimeBuildRevisionV1,
    pub panel_report_digest: PanelReportDigestV1,
    pub gateway_shard_id: GatewayShardIdV1,
    pub controller_fencing_token: FencingToken,
    pub deployment_revision: DeploymentRevision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePersistenceContractErrorV1 {
    #[error("runtime persistence record encoding failed")]
    Encoding,
    #[error("runtime persistence record decoding failed")]
    Decoding,
}

pub fn runtime_desired_target_digest_v1(
    identity: &RuntimeDeploymentIdentityV1,
    target: &RuntimeDeploymentTargetV1,
    runtime_generation: u64,
    authority_revision: u64,
    previous_runtime: Option<&RuntimeProcessIdentityV1>,
) -> RuntimeDesiredTargetDigestV1 {
    let mut digest = LengthFramedDigestV1::new(DESIRED_TARGET_DOMAIN_V1);
    digest.update(identity.deployment_id.as_str().as_bytes());
    digest.update(identity.tenant_id.as_str().as_bytes());
    digest.update(identity.installation_id.as_str().as_bytes());
    digest.update(identity.promotion_id.as_str().as_bytes());
    digest.update(identity.activation_request_id.as_str().as_bytes());
    update_target(&mut digest, target);
    digest.update(&runtime_generation.to_be_bytes());
    digest.update(&authority_revision.to_be_bytes());
    match previous_runtime {
        Some(previous) => {
            digest.update(b"present");
            update_target(&mut digest, &previous.target);
            digest.update(&previous.runtime_generation.get().to_be_bytes());
            digest.update(previous.process_instance_id.as_str().as_bytes());
        }
        None => digest.update(b"absent"),
    }
    RuntimeDesiredTargetDigestV1::from_sha256(digest.finalize())
}

pub fn encode_runtime_live_attestation_record_v1(
    record: &RuntimeLiveAttestationRecordV1,
) -> Result<Vec<u8>, RuntimePersistenceContractErrorV1> {
    serde_json::to_vec(record).map_err(|_| RuntimePersistenceContractErrorV1::Encoding)
}

pub fn decode_runtime_live_attestation_record_v1(
    encoded: &[u8],
) -> Result<RuntimeLiveAttestationRecordV1, RuntimePersistenceContractErrorV1> {
    serde_json::from_slice(encoded).map_err(|_| RuntimePersistenceContractErrorV1::Decoding)
}

pub fn runtime_live_attestation_digest_v1(
    record: &RuntimeLiveAttestationRecordV1,
) -> Result<RuntimeAttestationIdV1, RuntimePersistenceContractErrorV1> {
    let encoded = encode_runtime_live_attestation_record_v1(record)?;
    let mut digest = LengthFramedDigestV1::new(LIVE_ATTESTATION_DOMAIN_V1);
    digest.update(&encoded);
    RuntimeAttestationIdV1::parse(lower_hex(digest.finalize()))
        .map_err(|_| RuntimePersistenceContractErrorV1::Encoding)
}

fn update_target(digest: &mut LengthFramedDigestV1, target: &RuntimeDeploymentTargetV1) {
    digest.update(target.guild_id.to_string().as_bytes());
    digest.update(target.ruleset_key.as_str().as_bytes());
    digest.update(&target.version.get().to_be_bytes());
    digest.update(target.content_hash.to_hex().as_bytes());
    digest.update(&target.binding_revision.get().to_be_bytes());
    digest.update(target.binding_fingerprint.as_str().as_bytes());
}

struct LengthFramedDigestV1 {
    hasher: Sha256,
}

impl LengthFramedDigestV1 {
    fn new(domain: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        update_length_framed(&mut hasher, domain);
        Self { hasher }
    }

    fn update(&mut self, value: &[u8]) {
        update_length_framed(&mut self.hasher, value);
    }

    fn finalize(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }
}

fn update_length_framed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn lower_hex(bytes: [u8; 32]) -> String {
    const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in bytes {
        value.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
        value.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
    }
    value
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
        DeploymentId, GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId,
        PanelCertificateId, PanelCertificateV1, ProcessInstanceId, PromotionId, RuntimeGeneration,
        TenantId,
    };
    use chrono::{DateTime, Utc};
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn target() -> RuntimeDeploymentTargetV1 {
        RuntimeDeploymentTargetV1 {
            guild_id: GuildId(1),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            content_hash: RuleSetContentHash::parse_hex(&"2".repeat(64)).unwrap(),
            binding_revision: BindingRevision::FIRST,
            binding_fingerprint: ResourceBindingFingerprint::parse(&"3".repeat(64)).unwrap(),
        }
    }

    fn identity() -> RuntimeDeploymentIdentityV1 {
        RuntimeDeploymentIdentityV1 {
            deployment_id: DeploymentId::parse("deployment").unwrap(),
            tenant_id: TenantId::parse("tenant").unwrap(),
            installation_id: InstallationId::parse("installation").unwrap(),
            promotion_id: PromotionId::parse("1".repeat(64)).unwrap(),
            activation_request_id: ActivationRequestId::parse("activation").unwrap(),
        }
    }

    fn record() -> RuntimeLiveAttestationRecordV1 {
        let target = target();
        let process_instance_id = ProcessInstanceId::parse("process").unwrap();
        let activation = ActivationAttestationV1 {
            activation_request_id: ActivationRequestId::parse("activation").unwrap(),
            target: target.clone(),
            runtime_generation: RuntimeGeneration::FIRST,
            kind: ActivationOutcomeKindV1::AlreadyActive,
            activated_at: at(13),
        };
        let panel_certificate = PanelCertificateV1 {
            certificate_id: PanelCertificateId::parse("panel").unwrap(),
            report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
            target: target.clone(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process_instance_id.clone(),
            declared_count: 0,
            installed_count: 0,
            unchanged_count: 0,
            skipped_transient_count: 0,
            skipped_unresolved_channel_count: 0,
            failed_count: 0,
            ambiguous_outcome_count: 0,
            stale_message_cleanup_pending_count: 0,
            orphan_message_cleanup_pending_count: 0,
            reposted_old_message_cleanup_pending_count: 0,
            reconciled_at: at(14),
        };
        let gateway_ready = GatewayReadyAttestationV1 {
            target: target.clone(),
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: process_instance_id.clone(),
            kind: GatewayReadyKindV1::DiscordReady,
            ready_at: at(15),
        };
        RuntimeLiveAttestationRecordV1 {
            live: LiveAttestationV1 {
                target,
                runtime_generation: RuntimeGeneration::FIRST,
                process_instance_id,
                activation,
                panel_certificate,
                gateway_ready,
                certified_at: at(16),
            },
            runtime_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
            panel_report_digest: PanelReportDigestV1::parse("4".repeat(64)).unwrap(),
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            controller_fencing_token: FencingToken::FIRST,
            deployment_revision: DeploymentRevision::new(9).unwrap(),
        }
    }

    #[test]
    fn desired_target_digest_has_a_golden_contract() {
        let digest = runtime_desired_target_digest_v1(
            &identity(),
            &target(),
            RuntimeGeneration::FIRST.get(),
            1,
            None,
        );
        assert_eq!(
            digest.as_str(),
            "b6fe385079d6012778ba079e57a5d0e44b2a2cc829f57f87a53fa81b03843d9c"
        );
    }

    #[test]
    fn live_attestation_bytes_and_digest_have_a_golden_contract() {
        let record = record();
        let encoded = encode_runtime_live_attestation_record_v1(&record).unwrap();
        let expected = concat!(
            "{\"live\":{\"target\":{\"guild_id\":\"1\",\"ruleset_key\":\"studyroom\",\"version\":1,",
            "\"content_hash\":\"2222222222222222222222222222222222222222222222222222222222222222\",",
            "\"binding_revision\":1,\"binding_fingerprint\":\"3333333333333333333333333333333333333333333333333333333333333333\"},",
            "\"runtime_generation\":1,\"process_instance_id\":\"process\",\"activation\":{",
            "\"activation_request_id\":\"activation\",\"target\":{\"guild_id\":\"1\",\"ruleset_key\":\"studyroom\",",
            "\"version\":1,\"content_hash\":\"2222222222222222222222222222222222222222222222222222222222222222\",",
            "\"binding_revision\":1,\"binding_fingerprint\":\"3333333333333333333333333333333333333333333333333333333333333333\"},",
            "\"runtime_generation\":1,\"kind\":\"already_active\",\"activated_at\":\"1970-01-01T00:00:13Z\"},",
            "\"panel_certificate\":{\"certificate_id\":\"panel\",",
            "\"report_digest\":\"4444444444444444444444444444444444444444444444444444444444444444\",",
            "\"target\":{\"guild_id\":\"1\",\"ruleset_key\":\"studyroom\",\"version\":1,",
            "\"content_hash\":\"2222222222222222222222222222222222222222222222222222222222222222\",",
            "\"binding_revision\":1,\"binding_fingerprint\":\"3333333333333333333333333333333333333333333333333333333333333333\"},",
            "\"runtime_generation\":1,\"process_instance_id\":\"process\",\"declared_count\":0,",
            "\"installed_count\":0,\"unchanged_count\":0,\"skipped_transient_count\":0,",
            "\"skipped_unresolved_channel_count\":0,\"failed_count\":0,\"ambiguous_outcome_count\":0,",
            "\"stale_message_cleanup_pending_count\":0,\"orphan_message_cleanup_pending_count\":0,",
            "\"reposted_old_message_cleanup_pending_count\":0,\"reconciled_at\":\"1970-01-01T00:00:14Z\"},",
            "\"gateway_ready\":{\"target\":{\"guild_id\":\"1\",\"ruleset_key\":\"studyroom\",\"version\":1,",
            "\"content_hash\":\"2222222222222222222222222222222222222222222222222222222222222222\",",
            "\"binding_revision\":1,\"binding_fingerprint\":\"3333333333333333333333333333333333333333333333333333333333333333\"},",
            "\"runtime_generation\":1,\"process_instance_id\":\"process\",\"kind\":\"discord_ready\",",
            "\"ready_at\":\"1970-01-01T00:00:15Z\"},\"certified_at\":\"1970-01-01T00:00:16Z\"},",
            "\"runtime_build_revision\":\"build:1\",",
            "\"panel_report_digest\":\"4444444444444444444444444444444444444444444444444444444444444444\",",
            "\"gateway_shard_id\":\"shard:0\",\"controller_fencing_token\":1,\"deployment_revision\":9}"
        );
        assert_eq!(String::from_utf8(encoded.clone()).unwrap(), expected);
        assert_eq!(
            runtime_live_attestation_digest_v1(&record)
                .unwrap()
                .as_str(),
            "6d748022390cd695047fea9af061a6f4a950679ea7bc8e478c8b147f420d288b"
        );
        assert_eq!(
            decode_runtime_live_attestation_record_v1(&encoded).unwrap(),
            record
        );
    }

    #[test]
    fn live_attestation_decoder_rejects_unknown_fields() {
        let mut value = serde_json::to_value(record()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_string(), serde_json::Value::Null);
        assert_eq!(
            decode_runtime_live_attestation_record_v1(&serde_json::to_vec(&value).unwrap()),
            Err(RuntimePersistenceContractErrorV1::Decoding)
        );
    }

    #[test]
    fn live_attestation_decoder_rejects_noncanonical_metadata() {
        for (field, invalid) in [
            ("runtime_build_revision", "".to_string()),
            ("runtime_build_revision", "a".repeat(129)),
            ("runtime_build_revision", "bad value".to_string()),
            ("gateway_shard_id", "".to_string()),
            ("gateway_shard_id", "a".repeat(129)),
            ("gateway_shard_id", "bad value".to_string()),
            ("panel_report_digest", "A".repeat(64)),
        ] {
            let mut value = serde_json::to_value(record()).unwrap();
            value
                .as_object_mut()
                .unwrap()
                .insert(field.to_string(), serde_json::Value::String(invalid));
            assert_eq!(
                decode_runtime_live_attestation_record_v1(&serde_json::to_vec(&value).unwrap()),
                Err(RuntimePersistenceContractErrorV1::Decoding)
            );
        }
    }

    #[test]
    fn desired_target_digest_decoder_is_strict() {
        for invalid in [
            "".to_string(),
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            format!("{}g", "a".repeat(63)),
        ] {
            assert!(serde_json::from_str::<RuntimeDesiredTargetDigestV1>(
                &serde_json::to_string(&invalid).unwrap()
            )
            .is_err());
        }
    }
}
