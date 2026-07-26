use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeAttestationIdV1, RuntimeObservePreviousServingV1, RuntimePreviousServingLeaseEvidenceV1,
    RuntimePreviousServingLeaseIdentityV1, RuntimePreviousServingObservationReceiptV1,
    RuntimePreviousServingStateV1,
};
use automation_runtime_convergence::{DeploymentId, InstallationId, TenantId};
use chrono::{DateTime, Utc};

use crate::RuntimeConvergenceStoreError;

#[derive(sqlx::FromRow)]
pub(super) struct PreviousServingObservationRow {
    state_name: String,
    observed_at: DateTime<Utc>,
    lease_tenant_id: Option<String>,
    lease_installation_id: Option<String>,
    lease_deployment_id: Option<String>,
    lease_attestation_id: Option<String>,
    lease_process_instance_id: Option<String>,
    lease_runtime_generation: Option<i64>,
    lease_guild_id: Option<String>,
    lease_ruleset_key: Option<String>,
    lease_target_version: Option<i64>,
    lease_target_content_hash: Option<String>,
    lease_binding_revision: Option<i64>,
    lease_binding_fingerprint: Option<String>,
    lease_epoch: Option<i64>,
    lease_revision: Option<i64>,
    lease_connected: Option<bool>,
    lease_serving: Option<bool>,
    lease_acquired_at: Option<DateTime<Utc>>,
    lease_last_heartbeat_at: Option<DateTime<Utc>>,
    lease_expires_at: Option<DateTime<Utc>>,
}

impl PreviousServingObservationRow {
    pub(super) fn decode(
        self,
        request: RuntimeObservePreviousServingV1,
    ) -> Result<RuntimePreviousServingObservationReceiptV1, RuntimeConvergenceStoreError> {
        let state = if self.state_name == "absent" {
            if request.expected_previous_runtime.is_some() || !self.lease_columns_are_empty() {
                return Err(invalid());
            }
            RuntimePreviousServingStateV1::Absent
        } else {
            self.decode_present_state(&request)?
        };
        Ok(RuntimePreviousServingObservationReceiptV1 {
            action_id: request.action_id,
            guard: request.guard,
            observed_at: self.observed_at,
            expected_target: request.expected_target,
            expected_previous_runtime: request.expected_previous_runtime,
            state,
        })
    }

    fn decode_present_state(
        &self,
        request: &RuntimeObservePreviousServingV1,
    ) -> Result<RuntimePreviousServingStateV1, RuntimeConvergenceStoreError> {
        let Some(expected_process) = request.expected_previous_runtime.clone() else {
            return Err(invalid());
        };
        let (
            Some(tenant_id),
            Some(installation_id),
            Some(deployment_id),
            Some(attestation_id),
            Some(process_instance_id),
            Some(runtime_generation),
            Some(guild_id),
            Some(ruleset_key),
            Some(target_version),
            Some(target_content_hash),
            Some(binding_revision),
            Some(binding_fingerprint),
            Some(lease_epoch),
            Some(lease_revision),
            Some(connected),
            Some(serving),
            Some(acquired_at),
            Some(last_heartbeat_at),
            Some(expires_at),
        ) = (
            self.lease_tenant_id.as_deref(),
            self.lease_installation_id.as_deref(),
            self.lease_deployment_id.as_deref(),
            self.lease_attestation_id.as_deref(),
            self.lease_process_instance_id.as_deref(),
            self.lease_runtime_generation,
            self.lease_guild_id.as_deref(),
            self.lease_ruleset_key.as_deref(),
            self.lease_target_version,
            self.lease_target_content_hash.as_deref(),
            self.lease_binding_revision,
            self.lease_binding_fingerprint.as_deref(),
            self.lease_epoch,
            self.lease_revision,
            self.lease_connected,
            self.lease_serving,
            self.lease_acquired_at,
            self.lease_last_heartbeat_at,
            self.lease_expires_at,
        )
        else {
            return Err(invalid());
        };
        let target = &expected_process.target;
        if tenant_id != request.guard.scope.tenant_id.as_str()
            || installation_id != request.guard.scope.installation_id.as_str()
            || deployment_id == request.guard.scope.deployment_id.as_str()
            || process_instance_id != expected_process.process_instance_id.as_str()
            || runtime_generation != runtime_i64(expected_process.runtime_generation.get())?
            || guild_id != target.guild_id.to_string()
            || ruleset_key != target.ruleset_key.as_str()
            || target_version != i64::from(target.version.get())
            || target_content_hash != target.content_hash.to_hex()
            || binding_revision != runtime_i64(target.binding_revision.get())?
            || binding_fingerprint != target.binding_fingerprint.as_str()
            || acquired_at > last_heartbeat_at
            || acquired_at > self.observed_at
        {
            return Err(invalid());
        }
        let lease = RuntimePreviousServingLeaseEvidenceV1 {
            identity: RuntimePreviousServingLeaseIdentityV1 {
                scope: automation_runtime_controller::RuntimeDeploymentScopeV1 {
                    tenant_id: TenantId::parse(tenant_id).map_err(|_| invalid())?,
                    installation_id: InstallationId::parse(installation_id)
                        .map_err(|_| invalid())?,
                    deployment_id: DeploymentId::parse(deployment_id).map_err(|_| invalid())?,
                },
                attestation_id: RuntimeAttestationIdV1::parse(attestation_id)
                    .map_err(|_| invalid())?,
                process: expected_process,
                lease_epoch: positive(lease_epoch)?,
                revision: positive(lease_revision)?,
            },
            acquired_at,
            last_heartbeat_at,
        };
        match self.state_name.as_str() {
            "disconnected"
                if !connected
                    && !serving
                    && last_heartbeat_at == expires_at
                    && expires_at <= self.observed_at =>
            {
                Ok(RuntimePreviousServingStateV1::Disconnected {
                    lease,
                    disconnected_at: last_heartbeat_at,
                })
            }
            "expired"
                if connected
                    && serving
                    && last_heartbeat_at < expires_at
                    && expires_at <= self.observed_at =>
            {
                Ok(RuntimePreviousServingStateV1::Expired { lease, expires_at })
            }
            "serving"
                if connected
                    && serving
                    && last_heartbeat_at <= self.observed_at
                    && self.observed_at < expires_at =>
            {
                Ok(RuntimePreviousServingStateV1::Serving { lease, expires_at })
            }
            _ => Err(invalid()),
        }
    }

    fn lease_columns_are_empty(&self) -> bool {
        self.lease_tenant_id.is_none()
            && self.lease_installation_id.is_none()
            && self.lease_deployment_id.is_none()
            && self.lease_attestation_id.is_none()
            && self.lease_process_instance_id.is_none()
            && self.lease_runtime_generation.is_none()
            && self.lease_guild_id.is_none()
            && self.lease_ruleset_key.is_none()
            && self.lease_target_version.is_none()
            && self.lease_target_content_hash.is_none()
            && self.lease_binding_revision.is_none()
            && self.lease_binding_fingerprint.is_none()
            && self.lease_epoch.is_none()
            && self.lease_revision.is_none()
            && self.lease_connected.is_none()
            && self.lease_serving.is_none()
            && self.lease_acquired_at.is_none()
            && self.lease_last_heartbeat_at.is_none()
            && self.lease_expires_at.is_none()
    }
}

fn runtime_i64(value: u64) -> Result<i64, RuntimeConvergenceStoreError> {
    i64::try_from(value).map_err(|_| invalid())
}

fn positive(value: i64) -> Result<NonZeroU64, RuntimeConvergenceStoreError> {
    u64::try_from(value)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or_else(invalid)
}

fn invalid() -> RuntimeConvergenceStoreError {
    RuntimeConvergenceStoreError::InvalidPersistedState(
        "runtime previous serving observation projection",
    )
}
