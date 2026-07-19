use automation_runtime_convergence::{RuntimeDeployment, RuntimeDeploymentSnapshotV1};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::digest::desired_target_digest;
use crate::model::{EnqueueDeploymentV1, RuntimeDigestV1};
use crate::RuntimeConvergenceStoreError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedRequestedDeploymentV1 {
    snapshot: RuntimeDeploymentSnapshotV1,
    desired_target_digest: RuntimeDigestV1,
    snapshot_json: Value,
    previous_runtime_json: Option<Value>,
}

impl PreparedRequestedDeploymentV1 {
    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        &self.snapshot
    }

    pub fn desired_target_digest(&self) -> &str {
        self.desired_target_digest.as_str()
    }

    pub fn snapshot_json(&self) -> &Value {
        &self.snapshot_json
    }

    pub fn previous_runtime_json(&self) -> Option<&Value> {
        self.previous_runtime_json.as_ref()
    }
}

pub fn prepare_requested_deployment_v1(
    request: EnqueueDeploymentV1,
    requested_at: DateTime<Utc>,
) -> Result<PreparedRequestedDeploymentV1, RuntimeConvergenceStoreError> {
    if request.installation_authority_revision == 0 {
        return Err(RuntimeConvergenceStoreError::InvalidInput(
            "installation authority revision",
        ));
    }
    let desired_target_digest = desired_target_digest(
        &request.identity,
        &request.target,
        request.runtime_generation.get(),
        request.installation_authority_revision,
        request.previous_runtime.as_ref(),
    );
    let deployment = RuntimeDeployment::request(
        request.identity,
        request.target,
        request.runtime_generation,
        request.previous_runtime,
        requested_at,
    )?;
    let snapshot = deployment.snapshot();
    let snapshot_json = serde_json::to_value(&snapshot)
        .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("runtime deployment snapshot"))?;
    let snapshot_size = serde_json::to_vec(&snapshot_json)
        .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("runtime deployment snapshot"))?
        .len();
    if !(32..=262_144).contains(&snapshot_size) {
        return Err(RuntimeConvergenceStoreError::InvalidInput(
            "runtime deployment snapshot",
        ));
    }
    let previous_runtime_json = snapshot
        .previous_runtime
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("previous runtime"))?;
    Ok(PreparedRequestedDeploymentV1 {
        snapshot,
        desired_target_digest,
        snapshot_json,
        previous_runtime_json,
    })
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{RuleSetContentHash, RuleSetVersionId};
    use automation_runtime_convergence::{
        ActivationRequestId, BindingRevision, DeploymentId, InstallationId, PromotionId,
        RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1, RuntimeGeneration, TenantId,
    };
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::*;

    fn request() -> EnqueueDeploymentV1 {
        EnqueueDeploymentV1 {
            identity: RuntimeDeploymentIdentityV1 {
                deployment_id: DeploymentId::parse("runtime-pg-deployment").unwrap(),
                tenant_id: TenantId::parse("runtime-pg-tenant").unwrap(),
                installation_id: InstallationId::parse("runtime-pg-installation").unwrap(),
                promotion_id: PromotionId::parse("a".repeat(64)).unwrap(),
                activation_request_id: ActivationRequestId::parse("runtime_pg_activation").unwrap(),
            },
            target: RuntimeDeploymentTargetV1 {
                guild_id: GuildId(9_200_101),
                ruleset_key: "runtime_pg_ruleset".parse().unwrap(),
                version: RuleSetVersionId::FIRST,
                content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
                binding_revision: BindingRevision::FIRST,
                binding_fingerprint: ResourceBindingFingerprint::parse(&"c".repeat(64)).unwrap(),
            },
            runtime_generation: RuntimeGeneration::FIRST,
            previous_runtime: None,
            installation_authority_revision: 1,
        }
    }

    #[test]
    fn requested_builder_has_a_stable_v1_digest_and_exact_snapshot() {
        let requested_at = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let prepared = prepare_requested_deployment_v1(request(), requested_at).unwrap();
        assert_eq!(
            prepared.desired_target_digest(),
            "653a374215a7860b866639ab9d600bb6e11ea4cd865da82c009e0b1e0be70f4a"
        );
        assert_eq!(prepared.snapshot().requested_at, requested_at);
        assert_eq!(
            prepared.snapshot_json()["identity"]["deployment_id"],
            "runtime-pg-deployment"
        );
        assert_eq!(prepared.snapshot_json()["phase"]["phase"], "requested");
        assert!(prepared.previous_runtime_json().is_none());
    }

    #[test]
    fn requested_builder_rejects_an_unversioned_authority() {
        let mut invalid = request();
        invalid.installation_authority_revision = 0;
        assert!(matches!(
            prepare_requested_deployment_v1(invalid, Utc::now()),
            Err(RuntimeConvergenceStoreError::InvalidInput(
                "installation authority revision"
            ))
        ));
    }
}
