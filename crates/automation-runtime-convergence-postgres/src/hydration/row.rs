use std::num::{NonZeroU32, NonZeroU64};

use automation_ruleset::{
    content_hash, RuleSetContentHash, RuleSetKey, RuleSetSchemaVersion, RuleSetVersion,
    RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_runtime_controller::{
    runtime_desired_target_digest_v1, RuntimeDesiredTargetDigestV1,
};
use automation_runtime_convergence::{BindingRevision, DeploymentRevision};
use automation_state::InteractionRuleSet;
use chrono::{DateTime, Utc};
use discord_model::{GuildId, UserId};
use resource_resolution::{
    installation_authority_payload_digest_v1, resource_binding_fingerprint_v2,
    InstallationAuthorityPayloadDigestV1, InstallationAuthorityPayloadIdentityV1,
    InstallationAuthorityPolicyV1, InstallationAuthorityScopeV1, ResourceBindingFingerprint,
};
use serde_json::Value;
use sqlx::types::Json;

use crate::RuntimeConvergenceStoreError;

use super::bindings::decode_resource_bindings;
use super::{RuntimeExactTargetExecutionV1, RuntimeExactTargetV1};

const MAX_RUNTIME_EXACT_TARGET_RULESET_DEFINITION_BYTES: usize = 524_288;
const MAX_RUNTIME_EXACT_TARGET_RESOURCE_BINDINGS_BYTES: usize = 262_144;

#[derive(Clone, Debug, sqlx::FromRow)]
pub(super) struct RuntimeExactTargetRow {
    deployment_revision: i64,
    convergence_attempt_no: i64,
    desired_target_digest: String,
    installation_authority_revision: i64,
    installation_authority_payload_digest: String,
    installation_authority_policy_revision: i64,
    installation_authority_required_approvals: i32,
    installation_authority_activation_ttl_seconds: i64,
    current_authority_revision: i64,
    current_authority_payload_digest: String,
    current_authority_policy_revision: i64,
    current_authority_required_approvals: i32,
    current_authority_activation_ttl_seconds: i64,
    guild_id: String,
    ruleset_key: String,
    target_version: i64,
    schema_version: i64,
    definition: Option<Json<Value>>,
    content_hash: String,
    canonical_content_hash: Option<String>,
    created_by: String,
    binding_revision: i64,
    binding_fingerprint: String,
    resource_bindings: Option<Json<Value>>,
    database_observed_at: DateTime<Utc>,
}

impl RuntimeExactTargetRow {
    pub(super) fn decode(
        &self,
        execution: &RuntimeExactTargetExecutionV1<'_>,
    ) -> Result<RuntimeExactTargetV1, RuntimeConvergenceStoreError> {
        let invalid = || {
            RuntimeConvergenceStoreError::InvalidPersistedState("runtime exact target projection")
        };
        let snapshot = execution.snapshot;
        let target = &snapshot.target;
        let deployment_revision = u64::try_from(self.deployment_revision)
            .ok()
            .and_then(|value| DeploymentRevision::new(value).ok())
            .ok_or_else(invalid)?;
        let convergence_attempt = u32::try_from(self.convergence_attempt_no)
            .ok()
            .and_then(std::num::NonZeroU32::new)
            .ok_or_else(invalid)?;
        let guild_id = canonical_guild_id(&self.guild_id).ok_or_else(invalid)?;
        let ruleset_key = RuleSetKey::parse(&self.ruleset_key).map_err(|_| invalid())?;
        let version = u32::try_from(self.target_version)
            .ok()
            .and_then(|value| RuleSetVersionId::new(value).ok())
            .ok_or_else(invalid)?;
        let schema_version = u32::try_from(self.schema_version)
            .ok()
            .and_then(|value| RuleSetSchemaVersion::new(value).ok())
            .ok_or_else(invalid)?;
        let definition = self
            .definition
            .as_ref()
            .ok_or_else(invalid)
            .and_then(|value| {
                validate_json_size(&value.0, MAX_RUNTIME_EXACT_TARGET_RULESET_DEFINITION_BYTES)
                    .map_err(|_| invalid())?;
                serde_json::from_value::<InteractionRuleSet>(value.0.clone()).map_err(|_| invalid())
            })?;
        let persisted_hash =
            RuleSetContentHash::parse_hex(&self.content_hash).ok_or_else(invalid)?;
        let created_by = canonical_user_id(&self.created_by).ok_or_else(invalid)?;
        let binding_revision = u64::try_from(self.binding_revision)
            .ok()
            .and_then(|value| BindingRevision::new(value).ok())
            .ok_or_else(invalid)?;
        let persisted_fingerprint =
            ResourceBindingFingerprint::parse(&self.binding_fingerprint).map_err(|_| invalid())?;
        let bindings = self
            .resource_bindings
            .as_ref()
            .ok_or_else(invalid)
            .and_then(|value| {
                validate_json_size(&value.0, MAX_RUNTIME_EXACT_TARGET_RESOURCE_BINDINGS_BYTES)
                    .map_err(|_| invalid())?;
                decode_resource_bindings(value.0.clone()).map_err(|_| invalid())
            })?;
        let calculated_hash = content_hash(schema_version, &definition).map_err(|_| invalid())?;
        let calculated_fingerprint = resource_binding_fingerprint_v2(&bindings);
        let desired_target_digest =
            RuntimeDesiredTargetDigestV1::parse(self.desired_target_digest.clone())
                .map_err(|_| invalid())?;
        let calculated_desired_target_digest = runtime_desired_target_digest_v1(
            &snapshot.identity,
            target,
            snapshot.runtime_generation.get(),
            u64::try_from(self.installation_authority_revision).map_err(|_| invalid())?,
            snapshot.previous_runtime.as_ref(),
        );
        let (installation_authority_revision, installation_authority_payload_digest) =
            decode_authority_payload(
                snapshot.identity.tenant_id.as_str(),
                snapshot.identity.installation_id.as_str(),
                self.installation_authority_revision,
                &self.installation_authority_payload_digest,
                self.installation_authority_policy_revision,
                self.installation_authority_required_approvals,
                self.installation_authority_activation_ttl_seconds,
                binding_revision,
                &persisted_fingerprint,
            )?;
        let (current_authority_revision, current_authority_payload_digest) =
            decode_authority_payload(
                snapshot.identity.tenant_id.as_str(),
                snapshot.identity.installation_id.as_str(),
                self.current_authority_revision,
                &self.current_authority_payload_digest,
                self.current_authority_policy_revision,
                self.current_authority_required_approvals,
                self.current_authority_activation_ttl_seconds,
                binding_revision,
                &persisted_fingerprint,
            )?;
        let lease = snapshot.controller_lease.as_ref().ok_or_else(invalid)?;
        if deployment_revision != snapshot.revision
            || convergence_attempt != execution.convergence_attempt
            || lease.controller_id != *execution.controller_id
            || lease.fencing_token != execution.fencing_token
            || lease.acquired_at != execution.acquired_at
            || lease.expires_at != execution.expires_at
            || snapshot.last_fencing_token != Some(execution.fencing_token)
            || execution.acquired_at >= execution.expires_at
            || self.database_observed_at < execution.acquired_at
            || self.database_observed_at >= execution.expires_at
            || guild_id != target.guild_id
            || ruleset_key != target.ruleset_key
            || version != target.version
            || schema_version != CURRENT_RULESET_SCHEMA_VERSION
            || persisted_hash != target.content_hash
            || calculated_hash != target.content_hash
            || desired_target_digest != calculated_desired_target_digest
            || self.canonical_content_hash.as_deref() != Some(self.content_hash.as_str())
            || automation_core::validate_structural(&definition).is_err()
            || binding_revision != target.binding_revision
            || persisted_fingerprint != target.binding_fingerprint
            || calculated_fingerprint != target.binding_fingerprint
        {
            return Err(invalid());
        }
        Ok(RuntimeExactTargetV1 {
            snapshot: snapshot.clone(),
            desired_target_digest,
            installation_authority_revision,
            installation_authority_payload_digest,
            current_authority_revision,
            current_authority_payload_digest,
            artifact: RuleSetVersion {
                guild_id,
                ruleset_key,
                version,
                schema_version,
                definition,
                content_hash: persisted_hash,
                created_by,
            },
            bindings,
            database_observed_at: self.database_observed_at,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_authority_payload(
    tenant_id: &str,
    installation_id: &str,
    revision: i64,
    payload_digest: &str,
    policy_revision: i64,
    required_approvals: i32,
    activation_ttl_seconds: i64,
    binding_revision: BindingRevision,
    binding_fingerprint: &ResourceBindingFingerprint,
) -> Result<(u64, InstallationAuthorityPayloadDigestV1), RuntimeConvergenceStoreError> {
    let invalid =
        || RuntimeConvergenceStoreError::InvalidPersistedState("runtime exact target projection");
    let revision = positive_u64(revision).ok_or_else(invalid)?;
    let policy_revision = positive_u64(policy_revision).ok_or_else(invalid)?;
    let required_approvals = u32::try_from(required_approvals)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(invalid)?;
    let activation_ttl_seconds = positive_u64(activation_ttl_seconds).ok_or_else(invalid)?;
    let policy = InstallationAuthorityPolicyV1::new(
        policy_revision,
        required_approvals,
        activation_ttl_seconds,
    )
    .map_err(|_| invalid())?;
    let scope =
        InstallationAuthorityScopeV1::new(tenant_id, installation_id).map_err(|_| invalid())?;
    let identity = InstallationAuthorityPayloadIdentityV1::new(
        scope,
        revision,
        NonZeroU64::new(binding_revision.get()).ok_or_else(invalid)?,
        binding_fingerprint,
        policy,
    )
    .map_err(|_| invalid())?;
    let persisted =
        InstallationAuthorityPayloadDigestV1::parse(payload_digest).map_err(|_| invalid())?;
    if persisted != installation_authority_payload_digest_v1(&identity) {
        return Err(invalid());
    }
    Ok((revision.get(), persisted))
}

fn positive_u64(value: i64) -> Option<NonZeroU64> {
    u64::try_from(value).ok().and_then(NonZeroU64::new)
}

fn canonical_guild_id(value: &str) -> Option<GuildId> {
    canonical_snowflake(value).map(GuildId)
}

fn canonical_user_id(value: &str) -> Option<UserId> {
    canonical_snowflake(value).map(UserId)
}

fn canonical_snowflake(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    (parsed != 0 && parsed.to_string() == value).then_some(parsed)
}

fn validate_json_size(value: &Value, maximum: usize) -> Result<(), ()> {
    let encoded = serde_json::to_vec(value).map_err(|_| ())?;
    (encoded.len() <= maximum).then_some(()).ok_or(())
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU32, NonZeroU64};

    use resource_resolution::{
        installation_authority_payload_digest_v1, InstallationAuthorityPayloadIdentityV1,
        InstallationAuthorityPolicyV1, InstallationAuthorityScopeV1, ResourceBindingFingerprint,
    };
    use serde_json::json;

    use super::*;

    #[test]
    fn discord_identifiers_must_be_nonzero_canonical_snowflakes() {
        assert_eq!(canonical_guild_id("42"), Some(GuildId(42)));
        assert_eq!(canonical_user_id("99"), Some(UserId(99)));
        for value in ["", "0", "01", "+1", " 1", "18446744073709551616"] {
            assert_eq!(canonical_guild_id(value), None);
            assert_eq!(canonical_user_id(value), None);
        }
    }

    #[test]
    fn exact_target_json_payloads_are_bounded_by_encoded_bytes() {
        assert!(validate_json_size(&json!({"value": "small"}), 32).is_ok());
        assert!(validate_json_size(&json!({"value": "oversized"}), 8).is_err());
    }

    #[test]
    fn authority_payload_digest_is_recomputed_from_exact_persisted_evidence() {
        let fingerprint = ResourceBindingFingerprint::parse(&"a".repeat(64)).unwrap();
        let policy = InstallationAuthorityPolicyV1::new(
            NonZeroU64::new(3).unwrap(),
            NonZeroU32::new(2).unwrap(),
            NonZeroU64::new(7200).unwrap(),
        )
        .unwrap();
        let identity = InstallationAuthorityPayloadIdentityV1::new(
            InstallationAuthorityScopeV1::new("tenant", "installation").unwrap(),
            NonZeroU64::new(4).unwrap(),
            NonZeroU64::new(2).unwrap(),
            &fingerprint,
            policy,
        )
        .unwrap();
        let digest = installation_authority_payload_digest_v1(&identity);
        let decoded = decode_authority_payload(
            "tenant",
            "installation",
            4,
            digest.as_str(),
            3,
            2,
            7200,
            BindingRevision::new(2).unwrap(),
            &fingerprint,
        )
        .unwrap();
        assert_eq!(decoded, (4, digest.clone()));
        assert!(matches!(
            decode_authority_payload(
                "tenant",
                "installation",
                4,
                &"0".repeat(64),
                3,
                2,
                7200,
                BindingRevision::new(2).unwrap(),
                &fingerprint,
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime exact target projection"
            ))
        ));
        assert!(matches!(
            decode_authority_payload(
                "tenant",
                "installation",
                4,
                digest.as_str(),
                4,
                2,
                7200,
                BindingRevision::new(2).unwrap(),
                &fingerprint,
            ),
            Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "runtime exact target projection"
            ))
        ));
    }
}
