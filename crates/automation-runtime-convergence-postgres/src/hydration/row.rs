use automation_ruleset::{
    content_hash, RuleSetContentHash, RuleSetKey, RuleSetSchemaVersion, RuleSetVersion,
    RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_runtime_convergence::{BindingRevision, DeploymentRevision};
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingFingerprint};
use serde_json::Value;
use sqlx::types::Json;

use crate::RuntimeConvergenceStoreError;

use super::bindings::decode_resource_bindings;
use super::{RuntimeExactTargetExecutionV1, RuntimeExactTargetV1};

#[derive(Clone, Debug, sqlx::FromRow)]
pub(super) struct RuntimeExactTargetRow {
    deployment_revision: i64,
    convergence_attempt_no: i64,
    installation_authority_revision: i64,
    current_authority_revision: i64,
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
        let installation_authority_revision =
            u64::try_from(self.installation_authority_revision).map_err(|_| invalid())?;
        let current_authority_revision =
            u64::try_from(self.current_authority_revision).map_err(|_| invalid())?;
        let guild_id = self.guild_id.parse::<GuildId>().map_err(|_| invalid())?;
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
                serde_json::from_value::<InteractionRuleSet>(value.0.clone()).map_err(|_| invalid())
            })?;
        let persisted_hash =
            RuleSetContentHash::parse_hex(&self.content_hash).ok_or_else(invalid)?;
        let created_by = self.created_by.parse::<UserId>().map_err(|_| invalid())?;
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
            .and_then(|value| decode_resource_bindings(value.0.clone()).map_err(|_| invalid()))?;
        let calculated_hash = content_hash(schema_version, &definition).map_err(|_| invalid())?;
        let calculated_fingerprint = resource_binding_fingerprint_v2(&bindings);
        if deployment_revision != snapshot.revision
            || convergence_attempt != execution.convergence_attempt
            || installation_authority_revision == 0
            || current_authority_revision == 0
            || guild_id != target.guild_id
            || ruleset_key != target.ruleset_key
            || version != target.version
            || schema_version != CURRENT_RULESET_SCHEMA_VERSION
            || persisted_hash != target.content_hash
            || calculated_hash != target.content_hash
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
            installation_authority_revision,
            current_authority_revision,
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
        })
    }
}
