use std::fmt::{Debug, Formatter};

use automation_runtime_convergence::{RuntimeDeployment, RuntimeDeploymentSnapshotV1};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::model::{EnqueueDeploymentV1, RuntimeDigestV1};
use crate::persistence::desired_target_digest_v1;
use crate::RuntimeConvergenceStoreError;

const MIN_RUNTIME_DEPLOYMENT_SNAPSHOT_BYTES: usize = 32;
const MAX_RUNTIME_DEPLOYMENT_SNAPSHOT_BYTES: usize = 262_144;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PreparedRuntimeDeploymentSnapshotV1 {
    snapshot: RuntimeDeploymentSnapshotV1,
    snapshot_json: Value,
    snapshot_bytes: Box<[u8]>,
    snapshot_digest: RuntimeDigestV1,
}

impl PreparedRuntimeDeploymentSnapshotV1 {
    pub(crate) fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        &self.snapshot
    }

    pub(crate) fn snapshot_json(&self) -> &Value {
        &self.snapshot_json
    }

    pub(crate) fn snapshot_bytes(&self) -> &[u8] {
        &self.snapshot_bytes
    }

    pub(crate) fn snapshot_digest(&self) -> &str {
        self.snapshot_digest.as_str()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PreparedRequestedDeploymentV1 {
    prepared_snapshot: PreparedRuntimeDeploymentSnapshotV1,
    desired_target_digest: RuntimeDigestV1,
    previous_runtime_json: Option<Value>,
}

impl PreparedRequestedDeploymentV1 {
    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        self.prepared_snapshot.snapshot()
    }

    pub fn desired_target_digest(&self) -> &str {
        self.desired_target_digest.as_str()
    }

    pub fn snapshot_json(&self) -> &Value {
        self.prepared_snapshot.snapshot_json()
    }

    pub fn snapshot_bytes(&self) -> &[u8] {
        self.prepared_snapshot.snapshot_bytes()
    }

    pub fn snapshot_digest(&self) -> &str {
        self.prepared_snapshot.snapshot_digest()
    }

    pub fn previous_runtime_json(&self) -> Option<&Value> {
        self.previous_runtime_json.as_ref()
    }
}

impl Debug for PreparedRequestedDeploymentV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PreparedRequestedDeploymentV1(<opaque>)")
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
    let desired_target_digest = desired_target_digest_v1(
        &request.identity,
        &request.target,
        request.runtime_generation.get(),
        request.installation_authority_revision,
        request.previous_runtime.as_ref(),
    )?;
    let deployment = RuntimeDeployment::request(
        request.identity,
        request.target,
        request.runtime_generation,
        request.previous_runtime,
        requested_at,
    )?;
    let snapshot = deployment.snapshot();
    let prepared_snapshot = prepare_runtime_deployment_snapshot_v1(snapshot)?;
    let previous_runtime_json = prepared_snapshot
        .snapshot()
        .previous_runtime
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("previous runtime"))?;
    Ok(PreparedRequestedDeploymentV1 {
        prepared_snapshot,
        desired_target_digest,
        previous_runtime_json,
    })
}

pub(crate) fn prepare_runtime_deployment_snapshot_v1(
    snapshot: RuntimeDeploymentSnapshotV1,
) -> Result<PreparedRuntimeDeploymentSnapshotV1, RuntimeConvergenceStoreError> {
    RuntimeDeployment::restore(snapshot.clone())?;
    let snapshot_bytes = serde_json::to_vec(&snapshot)
        .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("runtime deployment snapshot"))?;
    if !(MIN_RUNTIME_DEPLOYMENT_SNAPSHOT_BYTES..=MAX_RUNTIME_DEPLOYMENT_SNAPSHOT_BYTES)
        .contains(&snapshot_bytes.len())
    {
        return Err(RuntimeConvergenceStoreError::InvalidInput(
            "runtime deployment snapshot",
        ));
    }
    let snapshot_json = serde_json::from_slice::<Value>(&snapshot_bytes)
        .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("runtime deployment snapshot"))?;
    let decoded = serde_json::from_slice::<RuntimeDeploymentSnapshotV1>(&snapshot_bytes)
        .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("runtime deployment snapshot"))?;
    if decoded != snapshot || RuntimeDeployment::restore(decoded).is_err() {
        return Err(RuntimeConvergenceStoreError::InvalidInput(
            "runtime deployment snapshot",
        ));
    }
    let snapshot_digest = RuntimeDigestV1::parse(lower_hex(Sha256::digest(&snapshot_bytes)))?;
    Ok(PreparedRuntimeDeploymentSnapshotV1 {
        snapshot,
        snapshot_json,
        snapshot_bytes: snapshot_bytes.into_boxed_slice(),
        snapshot_digest,
    })
}

fn lower_hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
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
        assert_eq!(
            prepared.snapshot_bytes(),
            serde_json::to_vec(prepared.snapshot()).unwrap()
        );
        assert_eq!(
            prepared.snapshot_json(),
            &serde_json::from_slice::<Value>(prepared.snapshot_bytes()).unwrap()
        );
        assert_eq!(
            prepared.snapshot_digest(),
            "19368b840a9ac56edb06c1f43d058cc68b025f571965f5a1d47e491bfd7cd860"
        );
        assert_eq!(
            format!("{prepared:?}"),
            "PreparedRequestedDeploymentV1(<opaque>)"
        );
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
