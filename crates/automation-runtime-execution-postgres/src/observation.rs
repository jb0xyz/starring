use std::num::NonZeroU64;

use automation_runtime_controller::{
    RuntimeAttestationIdV1, RuntimeObservePreviousServingV1, RuntimePreviousServingLeaseEvidenceV1,
    RuntimePreviousServingLeaseIdentityV1, RuntimePreviousServingObservationReceiptV1,
    RuntimePreviousServingStateV1,
};
use automation_runtime_convergence::{DeploymentId, InstallationId, TenantId};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};

use crate::error::map_query_error;
use crate::RuntimeExecutionPersistenceErrorV1;

const OBSERVE_PREVIOUS_SERVING_QUERY: &str =
    "SELECT * FROM public.starring_runtime_observe_previous_serving_v1(\
     $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)";
const MAX_PREVIOUS_RUNTIME_BYTES: usize = 16_384;

pub(crate) struct RuntimeObservePreviousServingBindingsV1 {
    expected_revision: i64,
    fencing_token: i64,
    convergence_attempt: i64,
    runtime_generation: i64,
    guild_id: String,
    target_version: i64,
    binding_revision: i64,
    previous_runtime: Option<Json<Value>>,
}

impl RuntimeObservePreviousServingBindingsV1 {
    pub(crate) fn from_request(
        request: &RuntimeObservePreviousServingV1,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        let expected_revision = runtime_i64(request.guard.expected_revision.get())?;
        let fencing_token = runtime_i64(request.guard.fencing_token.get())?;
        let runtime_generation = runtime_i64(request.guard.runtime_generation.get())?;
        let binding_revision = runtime_i64(request.expected_target.binding_revision.get())?;
        let previous_runtime = request
            .expected_previous_runtime
            .as_ref()
            .map(|previous| {
                if !request.expected_target.same_slot(&previous.target)
                    || previous.runtime_generation >= request.guard.runtime_generation
                {
                    return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
                }
                runtime_i64(previous.runtime_generation.get())?;
                runtime_i64(previous.target.binding_revision.get())?;
                let value = serde_json::to_value(previous)
                    .map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
                let size = serde_json::to_vec(&value)
                    .map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?
                    .len();
                if size > MAX_PREVIOUS_RUNTIME_BYTES {
                    return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
                }
                Ok(Json(value))
            })
            .transpose()?;
        Ok(Self {
            expected_revision,
            fencing_token,
            convergence_attempt: i64::from(request.guard.convergence_attempt.get()),
            runtime_generation,
            guild_id: request.expected_target.guild_id.to_string(),
            target_version: i64::from(request.expected_target.version.get()),
            binding_revision,
            previous_runtime,
        })
    }
}

pub(crate) async fn execute_observe_previous_serving_v1(
    transaction: &mut Transaction<'_, Postgres>,
    request: RuntimeObservePreviousServingV1,
    bindings: RuntimeObservePreviousServingBindingsV1,
) -> Result<RuntimePreviousServingObservationReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    let guard = &request.guard;
    let target = &request.expected_target;
    let rows =
        sqlx::query_as::<_, RuntimePreviousServingObservationRowV1>(OBSERVE_PREVIOUS_SERVING_QUERY)
            .bind(guard.scope.tenant_id.as_str())
            .bind(guard.scope.installation_id.as_str())
            .bind(guard.scope.deployment_id.as_str())
            .bind(bindings.expected_revision)
            .bind(guard.controller_id.as_str())
            .bind(bindings.fencing_token)
            .bind(bindings.convergence_attempt)
            .bind(bindings.runtime_generation)
            .bind(bindings.guild_id)
            .bind(target.ruleset_key.as_str())
            .bind(bindings.target_version)
            .bind(target.content_hash.to_hex())
            .bind(bindings.binding_revision)
            .bind(target.binding_fingerprint.as_str())
            .bind(bindings.previous_runtime)
            .fetch_all(&mut **transaction)
            .await
            .map_err(map_query_error)?;
    match rows.len() {
        0 => Err(RuntimeExecutionPersistenceErrorV1::OwnershipLost),
        1 => rows
            .into_iter()
            .next()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
            .decode(request),
        _ => Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt),
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct RuntimePreviousServingObservationRowV1 {
    state_name: Option<String>,
    observed_at: Option<DateTime<Utc>>,
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

impl RuntimePreviousServingObservationRowV1 {
    fn decode(
        self,
        request: RuntimeObservePreviousServingV1,
    ) -> Result<RuntimePreviousServingObservationReceiptV1, RuntimeExecutionPersistenceErrorV1>
    {
        let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
        let observed_at = self.observed_at.ok_or_else(invalid)?;
        let state = match self.state_name.as_deref() {
            Some("absent")
                if request.expected_previous_runtime.is_none()
                    && self.lease_columns_are_empty() =>
            {
                RuntimePreviousServingStateV1::Absent
            }
            Some("disconnected" | "expired" | "serving") => {
                self.decode_present_state(&request, observed_at)?
            }
            _ => return Err(invalid()),
        };
        Ok(RuntimePreviousServingObservationReceiptV1 {
            action_id: request.action_id,
            guard: request.guard,
            observed_at,
            expected_target: request.expected_target,
            expected_previous_runtime: request.expected_previous_runtime,
            state,
        })
    }

    fn decode_present_state(
        &self,
        request: &RuntimeObservePreviousServingV1,
        observed_at: DateTime<Utc>,
    ) -> Result<RuntimePreviousServingStateV1, RuntimeExecutionPersistenceErrorV1> {
        let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
        let expected_process = request
            .expected_previous_runtime
            .as_ref()
            .ok_or_else(invalid)?;
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
        if !request.expected_target.same_slot(target)
            || expected_process.runtime_generation >= request.guard.runtime_generation
            || tenant_id != request.guard.scope.tenant_id.as_str()
            || installation_id != request.guard.scope.installation_id.as_str()
            || deployment_id == request.guard.scope.deployment_id.as_str()
            || process_instance_id != expected_process.process_instance_id.as_str()
            || runtime_generation
                != runtime_i64_persisted(expected_process.runtime_generation.get())?
            || guild_id != target.guild_id.to_string()
            || ruleset_key != target.ruleset_key.as_str()
            || target_version != i64::from(target.version.get())
            || target_content_hash != target.content_hash.to_hex()
            || binding_revision != runtime_i64_persisted(target.binding_revision.get())?
            || binding_fingerprint != target.binding_fingerprint.as_str()
            || acquired_at > last_heartbeat_at
            || acquired_at > observed_at
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
                process: expected_process.clone(),
                lease_epoch: positive_u64(lease_epoch)?,
                revision: positive_u64(lease_revision)?,
            },
            acquired_at,
            last_heartbeat_at,
        };
        match self.state_name.as_deref() {
            Some("disconnected")
                if !connected
                    && !serving
                    && last_heartbeat_at == expires_at
                    && expires_at <= observed_at =>
            {
                Ok(RuntimePreviousServingStateV1::Disconnected {
                    lease,
                    disconnected_at: last_heartbeat_at,
                })
            }
            Some("expired")
                if connected
                    && serving
                    && last_heartbeat_at < expires_at
                    && expires_at <= observed_at =>
            {
                Ok(RuntimePreviousServingStateV1::Expired { lease, expires_at })
            }
            Some("serving")
                if connected
                    && serving
                    && last_heartbeat_at <= observed_at
                    && observed_at < expires_at =>
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

fn runtime_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    i64::try_from(value).map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)
}

fn runtime_i64_persisted(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    i64::try_from(value).map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

fn positive_u64(value: i64) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use automation_runtime_controller::{RuntimeConvergenceSessionV1, RuntimeExecutionReceiptV1};
    use automation_runtime_convergence::{
        CommandGuardV1, ControllerId, FencingToken, LeaseRequestV1, PreflightAttestationV1,
        RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
        RuntimeGeneration, RuntimeProcessIdentityV1,
    };
    use serde_json::json;

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + second, 0).unwrap()
    }

    fn target(version: u32, generation: u64) -> (RuntimeDeploymentTargetV1, RuntimeGeneration) {
        (
            serde_json::from_value(json!({
                "guild_id": "42",
                "ruleset_key": "studyroom",
                "version": version,
                "content_hash": "2".repeat(64),
                "binding_revision": version,
                "binding_fingerprint": "3".repeat(64)
            }))
            .unwrap(),
            RuntimeGeneration::new(generation).unwrap(),
        )
    }

    fn observation_request() -> RuntimeObservePreviousServingV1 {
        let (previous_target, previous_generation) = target(1, 1);
        let previous = RuntimeProcessIdentityV1 {
            target: previous_target,
            runtime_generation: previous_generation,
            process_instance_id: automation_runtime_convergence::ProcessInstanceId::parse(
                "previous-process",
            )
            .unwrap(),
        };
        let (current_target, current_generation) = target(2, 2);
        let identity: RuntimeDeploymentIdentityV1 = serde_json::from_value(json!({
            "deployment_id": "deployment",
            "tenant_id": "tenant",
            "installation_id": "installation",
            "promotion_id": "1".repeat(64),
            "activation_request_id": "activation"
        }))
        .unwrap();
        let mut deployment = RuntimeDeployment::request(
            identity,
            current_target.clone(),
            current_generation,
            Some(previous.clone()),
            at(0),
        )
        .unwrap();
        let controller = ControllerId::parse("controller").unwrap();
        deployment
            .acquire_lease(LeaseRequestV1 {
                expected_revision: deployment.revision(),
                controller_id: controller.clone(),
                fencing_token: FencingToken::FIRST,
                now: at(1),
                expires_at: at(100),
            })
            .unwrap();
        let guard = |deployment: &RuntimeDeployment, now| CommandGuardV1 {
            expected_revision: deployment.revision(),
            controller_id: controller.clone(),
            fencing_token: FencingToken::FIRST,
            runtime_generation: current_generation,
            now,
        };
        deployment
            .accept_preflight(
                &guard(&deployment, at(2)),
                PreflightAttestationV1 {
                    target: current_target,
                    runtime_generation: current_generation,
                    observed_runtime: Some(previous),
                    checked_at: at(2),
                },
            )
            .unwrap();
        deployment
            .request_drain(&guard(&deployment, at(3)))
            .unwrap();
        let snapshot = deployment.snapshot();
        let lease = snapshot.controller_lease.as_ref().unwrap();
        let mut session = RuntimeConvergenceSessionV1::from_claim(RuntimeExecutionReceiptV1 {
            snapshot: snapshot.clone(),
            controller_id: controller,
            fencing_token: FencingToken::FIRST,
            convergence_attempt: NonZeroU32::MIN,
            acquired_at: lease.acquired_at,
            expires_at: lease.expires_at,
        })
        .unwrap();
        session.begin_previous_serving_observation().unwrap()
    }

    fn absent_row() -> RuntimePreviousServingObservationRowV1 {
        RuntimePreviousServingObservationRowV1 {
            state_name: Some("absent".to_string()),
            observed_at: Some(at(4)),
            lease_tenant_id: None,
            lease_installation_id: None,
            lease_deployment_id: None,
            lease_attestation_id: None,
            lease_process_instance_id: None,
            lease_runtime_generation: None,
            lease_guild_id: None,
            lease_ruleset_key: None,
            lease_target_version: None,
            lease_target_content_hash: None,
            lease_binding_revision: None,
            lease_binding_fingerprint: None,
            lease_epoch: None,
            lease_revision: None,
            lease_connected: None,
            lease_serving: None,
            lease_acquired_at: None,
            lease_last_heartbeat_at: None,
            lease_expires_at: None,
        }
    }

    fn serving_row(
        request: &RuntimeObservePreviousServingV1,
    ) -> RuntimePreviousServingObservationRowV1 {
        let previous = request.expected_previous_runtime.as_ref().unwrap();
        RuntimePreviousServingObservationRowV1 {
            state_name: Some("serving".to_string()),
            observed_at: Some(at(4)),
            lease_tenant_id: Some(request.guard.scope.tenant_id.as_str().to_string()),
            lease_installation_id: Some(request.guard.scope.installation_id.as_str().to_string()),
            lease_deployment_id: Some("previous-deployment".to_string()),
            lease_attestation_id: Some("4".repeat(64)),
            lease_process_instance_id: Some(previous.process_instance_id.as_str().to_string()),
            lease_runtime_generation: Some(previous.runtime_generation.get() as i64),
            lease_guild_id: Some(previous.target.guild_id.to_string()),
            lease_ruleset_key: Some(previous.target.ruleset_key.as_str().to_string()),
            lease_target_version: Some(i64::from(previous.target.version.get())),
            lease_target_content_hash: Some(previous.target.content_hash.to_hex()),
            lease_binding_revision: Some(previous.target.binding_revision.get() as i64),
            lease_binding_fingerprint: Some(
                previous.target.binding_fingerprint.as_str().to_string(),
            ),
            lease_epoch: Some(1),
            lease_revision: Some(1),
            lease_connected: Some(true),
            lease_serving: Some(true),
            lease_acquired_at: Some(at(0)),
            lease_last_heartbeat_at: Some(at(3)),
            lease_expires_at: Some(at(10)),
        }
    }

    #[test]
    fn bindings_validate_previous_runtime_relationships_before_database_access() {
        let request = observation_request();
        assert!(RuntimeObservePreviousServingBindingsV1::from_request(&request).is_ok());
        let mut forged = request;
        forged
            .expected_previous_runtime
            .as_mut()
            .unwrap()
            .runtime_generation = forged.guard.runtime_generation;
        assert!(matches!(
            RuntimeObservePreviousServingBindingsV1::from_request(&forged),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        ));
    }

    #[test]
    fn absent_projection_rejects_expected_or_projected_previous_runtime() {
        let request = observation_request();
        assert_eq!(
            absent_row().decode(request.clone()).unwrap_err(),
            RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
        );
        let mut forged = absent_row();
        forged.lease_epoch = Some(1);
        let mut without_previous = request;
        without_previous.expected_previous_runtime = None;
        assert_eq!(
            forged.decode(without_previous).unwrap_err(),
            RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
        );
    }

    #[test]
    fn present_projection_is_exact_and_rejects_state_or_identity_forgery() {
        let request = observation_request();
        let receipt = serving_row(&request).decode(request.clone()).unwrap();
        assert!(matches!(
            receipt.state,
            RuntimePreviousServingStateV1::Serving { .. }
        ));
        let mut wrong_state = serving_row(&request);
        wrong_state.lease_serving = Some(false);
        assert_eq!(
            wrong_state.decode(request.clone()).unwrap_err(),
            RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
        );
        let mut wrong_process = serving_row(&request);
        wrong_process.lease_process_instance_id = Some("forged-process".to_string());
        assert_eq!(
            wrong_process.decode(request).unwrap_err(),
            RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
        );
    }

    #[test]
    fn observation_query_is_function_only_and_positionally_exact() {
        assert_eq!(OBSERVE_PREVIOUS_SERVING_QUERY.matches('$').count(), 15);
        for forbidden in [
            "runtime_deployments",
            "runtime_serving_leases",
            "INSERT ",
            "UPDATE ",
            "DELETE ",
        ] {
            assert!(!OBSERVE_PREVIOUS_SERVING_QUERY.contains(forbidden));
        }
    }
}
