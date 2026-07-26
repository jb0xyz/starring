use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use automation_runtime_controller::{
    encode_runtime_live_attestation_record_v1, runtime_live_attestation_digest_v1,
    RuntimeAttestationIdV1, RuntimeCertificationReceiptV1, RuntimeCertificationRequestV1,
    RuntimeDeploymentScopeV1, RuntimeLiveAttestationRecordV1, RuntimeServingIdentityV1,
    RuntimeServingReceiptV1,
};
use automation_runtime_convergence::{
    CommandGuardV1, RuntimeDeployment, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
    TransitionOutcomeV1,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};

use crate::error::{map_query_error, validate_millisecond_duration};
use crate::RuntimeExecutionPersistenceErrorV1;

pub const MIN_RUNTIME_CERTIFICATION_SERVING_LEASE_DURATION: Duration = Duration::from_secs(1);
pub const MAX_RUNTIME_CERTIFICATION_SERVING_LEASE_DURATION: Duration = Duration::from_secs(300);

const CERTIFY_PREPARE_QUERY: &str =
    "SELECT * FROM public.starring_runtime_execution_certify_prepare_v1(\
     $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)";

const CERTIFY_COMMIT_QUERY: &str =
    "SELECT * FROM public.starring_runtime_execution_certify_commit_v1(\
     $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)";

pub(crate) struct RuntimeCertificationBindingsV1 {
    expected_revision: i64,
    fencing_token: i64,
    convergence_attempt: i64,
    runtime_generation: i64,
    gateway_ready: Value,
    serving_lease_milliseconds: i64,
}

impl RuntimeCertificationBindingsV1 {
    pub(crate) fn from_request(
        request: &RuntimeCertificationRequestV1,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        let gateway_ready = serde_json::to_value(&request.gateway_ready)
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
        Ok(Self {
            expected_revision: incrementable_i64(request.guard.expected_revision.get())?,
            fencing_token: positive_i64(request.guard.fencing_token.get())?,
            convergence_attempt: i64::from(request.guard.convergence_attempt.get()),
            runtime_generation: positive_i64(request.guard.runtime_generation.get())?,
            gateway_ready,
            serving_lease_milliseconds: validate_serving_lease_duration(request.serving_lease_for)?,
        })
    }
}

pub(crate) async fn execute_certification_v1(
    transaction: &mut Transaction<'_, Postgres>,
    request: &RuntimeCertificationRequestV1,
    bindings: &RuntimeCertificationBindingsV1,
) -> Result<RuntimeCertificationReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    let guard = &request.guard;
    let mut prepared_rows =
        sqlx::query_as::<_, RuntimeCertificationPrepareRowV1>(CERTIFY_PREPARE_QUERY)
            .bind(guard.scope.tenant_id.as_str())
            .bind(guard.scope.installation_id.as_str())
            .bind(guard.scope.deployment_id.as_str())
            .bind(bindings.expected_revision)
            .bind(guard.controller_id.as_str())
            .bind(bindings.fencing_token)
            .bind(bindings.convergence_attempt)
            .bind(bindings.runtime_generation)
            .bind(Json(bindings.gateway_ready.clone()))
            .bind(request.metadata.runtime_build_revision.as_str())
            .bind(request.metadata.panel_report_digest.as_str())
            .bind(request.metadata.gateway_shard_id.as_str())
            .bind(bindings.serving_lease_milliseconds)
            .fetch_all(&mut **transaction)
            .await
            .map_err(map_query_error)?;
    if prepared_rows.len() != 1 {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    }
    let prepared = prepared_rows
        .pop()
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
        .decode()?;
    let plan = RuntimeCertificationPlanV1::prove(request, prepared)?;
    let mut committed_rows =
        sqlx::query_as::<_, RuntimeCertificationOperationRowV1>(CERTIFY_COMMIT_QUERY)
            .bind(guard.scope.tenant_id.as_str())
            .bind(guard.scope.installation_id.as_str())
            .bind(guard.scope.deployment_id.as_str())
            .bind(bindings.expected_revision)
            .bind(guard.controller_id.as_str())
            .bind(bindings.fencing_token)
            .bind(bindings.convergence_attempt)
            .bind(bindings.runtime_generation)
            .bind(Json(bindings.gateway_ready.clone()))
            .bind(request.metadata.runtime_build_revision.as_str())
            .bind(request.metadata.panel_report_digest.as_str())
            .bind(request.metadata.gateway_shard_id.as_str())
            .bind(bindings.serving_lease_milliseconds)
            .bind(plan.mutation_clock)
            .bind(Json(plan.observed_snapshot_value.clone()))
            .bind(plan.attestation_id.as_str())
            .bind(Json(plan.record_value.clone()))
            .bind(plan.record_bytes.as_str())
            .fetch_all(&mut **transaction)
            .await
            .map_err(map_query_error)?;
    if committed_rows.len() != 1 {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    }
    let committed = committed_rows
        .pop()
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
        .decode()?;
    plan.prove_commit(request, committed)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeCertificationPreparationV1 {
    Apply,
    Replayed,
}

#[derive(sqlx::FromRow)]
struct RuntimeCertificationPrepareRowV1 {
    preparation_name: Option<String>,
    observed_snapshot: Option<Json<Value>>,
    convergence_attempt_no: Option<i64>,
    mutation_clock: Option<DateTime<Utc>>,
    certified_at: Option<DateTime<Utc>>,
}

struct DecodedRuntimeCertificationPrepareV1 {
    preparation: RuntimeCertificationPreparationV1,
    observed_snapshot_value: Value,
    observed: RuntimeDeployment,
    convergence_attempt: NonZeroU32,
    mutation_clock: DateTime<Utc>,
    certified_at: DateTime<Utc>,
}

impl RuntimeCertificationPrepareRowV1 {
    fn decode(
        self,
    ) -> Result<DecodedRuntimeCertificationPrepareV1, RuntimeExecutionPersistenceErrorV1> {
        let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
        let preparation = match self.preparation_name.as_deref() {
            Some("apply") => RuntimeCertificationPreparationV1::Apply,
            Some("replayed") => RuntimeCertificationPreparationV1::Replayed,
            _ => return Err(invalid()),
        };
        let observed_snapshot_value = self.observed_snapshot.ok_or_else(invalid)?.0;
        let observed = decode_deployment(observed_snapshot_value.clone())?;
        let convergence_attempt =
            positive_u32(self.convergence_attempt_no.ok_or_else(invalid)?).ok_or_else(invalid)?;
        let mutation_clock = self.mutation_clock.ok_or_else(invalid)?;
        let certified_at = self.certified_at.ok_or_else(invalid)?;
        if certified_at > mutation_clock {
            return Err(invalid());
        }
        Ok(DecodedRuntimeCertificationPrepareV1 {
            preparation,
            observed_snapshot_value,
            observed,
            convergence_attempt,
            mutation_clock,
            certified_at,
        })
    }
}

#[derive(Clone, sqlx::FromRow)]
struct RuntimeCertificationOperationRowV1 {
    outcome_name: Option<String>,
    previous_snapshot: Option<Json<Value>>,
    snapshot: Option<Json<Value>>,
    convergence_attempt_no: Option<i64>,
    tenant_id: Option<String>,
    installation_id: Option<String>,
    deployment_id: Option<String>,
    guild_id: Option<String>,
    ruleset_key: Option<String>,
    attestation_id: Option<String>,
    process_instance_id: Option<String>,
    runtime_generation: Option<i64>,
    lease_epoch: Option<i64>,
    serving_revision: Option<i64>,
    acquired_at: Option<DateTime<Utc>>,
    last_heartbeat_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    connected: Option<bool>,
    serving: Option<bool>,
}

struct DecodedRuntimeCertificationOperationV1 {
    preparation: RuntimeCertificationPreparationV1,
    previous: RuntimeDeployment,
    current: RuntimeDeployment,
    convergence_attempt: NonZeroU32,
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
    guild_id: String,
    ruleset_key: String,
    attestation_id: RuntimeAttestationIdV1,
    process_instance_id: String,
    runtime_generation: NonZeroU64,
    lease_epoch: NonZeroU64,
    serving_revision: NonZeroU64,
    acquired_at: DateTime<Utc>,
    last_heartbeat_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    connected: bool,
    serving: bool,
}

impl RuntimeCertificationOperationRowV1 {
    fn decode(
        self,
    ) -> Result<DecodedRuntimeCertificationOperationV1, RuntimeExecutionPersistenceErrorV1> {
        let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
        let preparation = match self.outcome_name.as_deref() {
            Some("applied") => RuntimeCertificationPreparationV1::Apply,
            Some("replayed") => RuntimeCertificationPreparationV1::Replayed,
            _ => return Err(invalid()),
        };
        let acquired_at = self.acquired_at.ok_or_else(invalid)?;
        let last_heartbeat_at = self.last_heartbeat_at.ok_or_else(invalid)?;
        let expires_at = self.expires_at.ok_or_else(invalid)?;
        if acquired_at > last_heartbeat_at || last_heartbeat_at >= expires_at {
            return Err(invalid());
        }
        Ok(DecodedRuntimeCertificationOperationV1 {
            preparation,
            previous: decode_deployment(self.previous_snapshot.ok_or_else(invalid)?.0)?,
            current: decode_deployment(self.snapshot.ok_or_else(invalid)?.0)?,
            convergence_attempt: positive_u32(self.convergence_attempt_no.ok_or_else(invalid)?)
                .ok_or_else(invalid)?,
            tenant_id: self.tenant_id.ok_or_else(invalid)?,
            installation_id: self.installation_id.ok_or_else(invalid)?,
            deployment_id: self.deployment_id.ok_or_else(invalid)?,
            guild_id: self.guild_id.ok_or_else(invalid)?,
            ruleset_key: self.ruleset_key.ok_or_else(invalid)?,
            attestation_id: RuntimeAttestationIdV1::parse(self.attestation_id.ok_or_else(invalid)?)
                .map_err(|_| invalid())?,
            process_instance_id: self.process_instance_id.ok_or_else(invalid)?,
            runtime_generation: positive_u64(self.runtime_generation.ok_or_else(invalid)?)
                .ok_or_else(invalid)?,
            lease_epoch: positive_u64(self.lease_epoch.ok_or_else(invalid)?).ok_or_else(invalid)?,
            serving_revision: positive_u64(self.serving_revision.ok_or_else(invalid)?)
                .ok_or_else(invalid)?,
            acquired_at,
            last_heartbeat_at,
            expires_at,
            connected: self.connected.ok_or_else(invalid)?,
            serving: self.serving.ok_or_else(invalid)?,
        })
    }
}

struct RuntimeCertificationPlanV1 {
    preparation: RuntimeCertificationPreparationV1,
    observed_snapshot_value: Value,
    observed_snapshot: RuntimeDeploymentSnapshotV1,
    expected_snapshot: RuntimeDeploymentSnapshotV1,
    expected_outcome: TransitionOutcomeV1,
    convergence_attempt: NonZeroU32,
    mutation_clock: DateTime<Utc>,
    certified_at: DateTime<Utc>,
    attestation_id: RuntimeAttestationIdV1,
    record_value: Value,
    record_bytes: String,
}

impl RuntimeCertificationPlanV1 {
    fn prove(
        request: &RuntimeCertificationRequestV1,
        prepared: DecodedRuntimeCertificationPrepareV1,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
        let observed_snapshot = prepared.observed.snapshot();
        let expected_revision = request
            .guard
            .expected_revision
            .next()
            .map_err(|_| invalid())?;
        if prepared.convergence_attempt != request.guard.convergence_attempt
            || !request.guard.scope.matches(&observed_snapshot.identity)
            || observed_snapshot.runtime_generation != request.guard.runtime_generation
            || observed_snapshot.target != request.gateway_ready.target
            || request.gateway_ready.runtime_generation != request.guard.runtime_generation
            || observed_snapshot.last_fencing_token != Some(request.guard.fencing_token)
        {
            return Err(invalid());
        }
        match prepared.preparation {
            RuntimeCertificationPreparationV1::Apply => {
                let lease = observed_snapshot
                    .controller_lease
                    .as_ref()
                    .ok_or_else(invalid)?;
                if observed_snapshot.revision != request.guard.expected_revision
                    || !matches!(
                        observed_snapshot.phase,
                        RuntimeDeploymentPhaseV1::AwaitingGatewayReady
                    )
                    || lease.controller_id != request.guard.controller_id
                    || lease.fencing_token != request.guard.fencing_token
                    || lease.acquired_at > prepared.mutation_clock
                    || lease.expires_at <= prepared.mutation_clock
                    || prepared.certified_at != prepared.mutation_clock
                {
                    return Err(invalid());
                }
            }
            RuntimeCertificationPreparationV1::Replayed => {
                let live = observed_snapshot.live.as_ref().ok_or_else(invalid)?;
                if observed_snapshot.revision != expected_revision
                    || !matches!(observed_snapshot.phase, RuntimeDeploymentPhaseV1::Live)
                    || observed_snapshot.controller_lease.is_some()
                    || live.gateway_ready != request.gateway_ready
                    || live.certified_at != prepared.certified_at
                {
                    return Err(invalid());
                }
            }
        }
        let command_guard = CommandGuardV1 {
            expected_revision: request.guard.expected_revision,
            controller_id: request.guard.controller_id.clone(),
            fencing_token: request.guard.fencing_token,
            runtime_generation: request.guard.runtime_generation,
            now: prepared.mutation_clock,
        };
        let mut reconstructed = prepared.observed.clone();
        let expected_outcome = reconstructed
            .certify_live(
                &command_guard,
                request.gateway_ready.clone(),
                prepared.certified_at,
            )
            .map_err(|_| invalid())?;
        let expected_snapshot = reconstructed.snapshot();
        let expected_preparation = match expected_outcome {
            TransitionOutcomeV1::Applied { revision } if revision == expected_revision => {
                RuntimeCertificationPreparationV1::Apply
            }
            TransitionOutcomeV1::Replayed { revision } if revision == expected_revision => {
                RuntimeCertificationPreparationV1::Replayed
            }
            _ => return Err(invalid()),
        };
        if expected_preparation != prepared.preparation {
            return Err(invalid());
        }
        let live = expected_snapshot.live.clone().ok_or_else(invalid)?;
        if live.gateway_ready != request.gateway_ready
            || live.certified_at != prepared.certified_at
            || expected_snapshot
                .panel_certificate
                .as_ref()
                .map(|panel| &panel.report_digest)
                != Some(&request.metadata.panel_report_digest)
        {
            return Err(invalid());
        }
        let record = RuntimeLiveAttestationRecordV1 {
            live,
            runtime_build_revision: request.metadata.runtime_build_revision.clone(),
            panel_report_digest: request.metadata.panel_report_digest.clone(),
            gateway_shard_id: request.metadata.gateway_shard_id.clone(),
            controller_fencing_token: request.guard.fencing_token,
            deployment_revision: expected_revision,
        };
        let attestation_id = runtime_live_attestation_digest_v1(&record).map_err(|_| invalid())?;
        let encoded = encode_runtime_live_attestation_record_v1(&record).map_err(|_| invalid())?;
        let record_bytes = String::from_utf8(encoded).map_err(|_| invalid())?;
        let record_value = serde_json::to_value(record).map_err(|_| invalid())?;
        Ok(Self {
            preparation: prepared.preparation,
            observed_snapshot_value: prepared.observed_snapshot_value,
            observed_snapshot,
            expected_snapshot,
            expected_outcome,
            convergence_attempt: prepared.convergence_attempt,
            mutation_clock: prepared.mutation_clock,
            certified_at: prepared.certified_at,
            attestation_id,
            record_value,
            record_bytes,
        })
    }

    fn prove_commit(
        self,
        request: &RuntimeCertificationRequestV1,
        committed: DecodedRuntimeCertificationOperationV1,
    ) -> Result<RuntimeCertificationReceiptV1, RuntimeExecutionPersistenceErrorV1> {
        let invalid = || RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt;
        let previous = committed.previous.snapshot();
        let current = committed.current.snapshot();
        if committed.preparation != self.preparation
            || committed.convergence_attempt != self.convergence_attempt
            || self.convergence_attempt != request.guard.convergence_attempt
            || current != self.expected_snapshot
            || committed.tenant_id != request.guard.scope.tenant_id.as_str()
            || committed.installation_id != request.guard.scope.installation_id.as_str()
            || committed.deployment_id != request.guard.scope.deployment_id.as_str()
            || committed.guild_id != request.gateway_ready.target.guild_id.to_string()
            || committed.ruleset_key != request.gateway_ready.target.ruleset_key.as_str()
            || committed.attestation_id != self.attestation_id
            || committed.process_instance_id != request.gateway_ready.process_instance_id.as_str()
            || committed.runtime_generation.get() != request.guard.runtime_generation.get()
            || !committed.connected
            || !committed.serving
            || committed.acquired_at != self.certified_at
            || committed.last_heartbeat_at != committed.acquired_at
            || committed.last_heartbeat_at > self.mutation_clock
            || committed.expires_at <= self.mutation_clock
        {
            return Err(invalid());
        }
        let observed_relation_is_exact = match self.preparation {
            RuntimeCertificationPreparationV1::Apply => previous == self.observed_snapshot,
            RuntimeCertificationPreparationV1::Replayed => {
                previous == self.expected_snapshot && previous == current
            }
        };
        if !observed_relation_is_exact {
            return Err(invalid());
        }
        let lease_duration = committed
            .expires_at
            .signed_duration_since(committed.last_heartbeat_at)
            .to_std()
            .map_err(|_| invalid())?;
        if lease_duration != request.serving_lease_for {
            return Err(invalid());
        }
        Ok(RuntimeCertificationReceiptV1 {
            action_id: request.action_id,
            outcome: self.expected_outcome,
            snapshot: current,
            convergence_attempt: committed.convergence_attempt,
            metadata: request.metadata.clone(),
            serving: RuntimeServingReceiptV1 {
                identity: RuntimeServingIdentityV1 {
                    scope: RuntimeDeploymentScopeV1 {
                        tenant_id: request.guard.scope.tenant_id.clone(),
                        installation_id: request.guard.scope.installation_id.clone(),
                        deployment_id: request.guard.scope.deployment_id.clone(),
                    },
                    attestation_id: committed.attestation_id,
                    process_instance_id: request.gateway_ready.process_instance_id.clone(),
                    runtime_generation: request.guard.runtime_generation,
                    lease_epoch: committed.lease_epoch,
                    expected_revision: committed.serving_revision,
                },
                runtime_generation: request.guard.runtime_generation,
                acquired_at: committed.acquired_at,
                last_heartbeat_at: committed.last_heartbeat_at,
                expires_at: committed.expires_at,
                connected: committed.connected,
                serving: committed.serving,
            },
        })
    }
}

fn validate_serving_lease_duration(
    duration: Duration,
) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    if duration < MIN_RUNTIME_CERTIFICATION_SERVING_LEASE_DURATION {
        return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
    }
    validate_millisecond_duration(duration, MAX_RUNTIME_CERTIFICATION_SERVING_LEASE_DURATION)
}

fn positive_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    let value =
        i64::try_from(value).map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
    if value == 0 {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    } else {
        Ok(value)
    }
}

fn incrementable_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    let value = positive_i64(value)?;
    if value == i64::MAX {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    } else {
        Ok(value)
    }
}

fn decode_deployment(
    value: Value,
) -> Result<RuntimeDeployment, RuntimeExecutionPersistenceErrorV1> {
    let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(value)
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
    RuntimeDeployment::restore(snapshot)
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

fn positive_u64(value: i64) -> Option<NonZeroU64> {
    u64::try_from(value).ok().and_then(NonZeroU64::new)
}

fn positive_u32(value: i64) -> Option<NonZeroU32> {
    u32::try_from(value).ok().and_then(NonZeroU32::new)
}

#[cfg(test)]
mod tests;
