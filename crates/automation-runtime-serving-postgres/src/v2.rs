use std::num::NonZeroU64;
use std::time::Duration;

use automation_runtime_controller::{RuntimeServingIdentityV2, RuntimeServingReceiptV2};
use chrono::{DateTime, Utc};
use sqlx::PgConnection;

use crate::connection::ServingConnectionGuardV1;
use crate::contract::{DISCONNECT_V2_QUERY, HEARTBEAT_V2_QUERY, OBSERVE_V2_QUERY};
use crate::database::{begin_serving_mutation_transaction, verify_runtime_serving_binding_v1};
use crate::error::{
    map_mutation_commit_error, map_mutation_error, map_query_error, validate_millisecond_duration,
};
use crate::{
    PostgresRuntimeServingLeaseV1, RuntimeServingPersistenceErrorV1,
    MAX_RUNTIME_SERVING_LEASE_DURATION, MIN_RUNTIME_SERVING_LEASE_DURATION,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeServingObservationV2 {
    Absent {
        observed_at: DateTime<Utc>,
    },
    Current {
        serving: Box<RuntimeServingReceiptV2>,
        observed_at: DateTime<Utc>,
    },
    Diverged {
        observed_at: DateTime<Utc>,
    },
}

enum RuntimeServingOperationV2<'a> {
    Observe(&'a RuntimeServingIdentityV2),
    Heartbeat {
        identity: &'a RuntimeServingIdentityV2,
        lease_for: Duration,
    },
    Disconnect(&'a RuntimeServingIdentityV2),
}

enum RuntimeServingOperationResultV2 {
    Observation(RuntimeServingObservationV2),
    Mutation(Box<RuntimeServingReceiptV2>),
}

impl PostgresRuntimeServingLeaseV1 {
    pub async fn observe_serving_v2(
        &self,
        identity: &RuntimeServingIdentityV2,
    ) -> Result<RuntimeServingObservationV2, RuntimeServingPersistenceErrorV1> {
        match self
            .execute_v2(RuntimeServingOperationV2::Observe(identity))
            .await?
        {
            RuntimeServingOperationResultV2::Observation(observation) => Ok(observation),
            RuntimeServingOperationResultV2::Mutation(_) => {
                Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    pub async fn heartbeat_serving_v2(
        &self,
        identity: &RuntimeServingIdentityV2,
        lease_for: Duration,
    ) -> Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1> {
        match self
            .execute_v2(RuntimeServingOperationV2::Heartbeat {
                identity,
                lease_for,
            })
            .await?
        {
            RuntimeServingOperationResultV2::Mutation(receipt) => Ok(*receipt),
            RuntimeServingOperationResultV2::Observation(_) => {
                Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    pub async fn disconnect_serving_if_current_v2(
        &self,
        identity: &RuntimeServingIdentityV2,
    ) -> Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1> {
        match self
            .execute_v2(RuntimeServingOperationV2::Disconnect(identity))
            .await?
        {
            RuntimeServingOperationResultV2::Mutation(receipt) => Ok(*receipt),
            RuntimeServingOperationResultV2::Observation(_) => {
                Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
            }
        }
    }

    async fn execute_v2(
        &self,
        operation: RuntimeServingOperationV2<'_>,
    ) -> Result<RuntimeServingOperationResultV2, RuntimeServingPersistenceErrorV1> {
        let mutation = !matches!(operation, RuntimeServingOperationV2::Observe(_));
        let deadline = tokio::time::Instant::now() + self.timeouts.statement_timeout();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
            .await
            .map_err(|_| RuntimeServingPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        let mut connection = ServingConnectionGuardV1::new(connection);
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)?;
        let result = tokio::time::timeout_at(
            deadline,
            self.execute_v2_on_connection(database_connection, operation),
        )
        .await;
        match result {
            Ok(result) => {
                connection.release_to_pool();
                result
            }
            Err(_) if mutation => Err(RuntimeServingPersistenceErrorV1::Indeterminate),
            Err(_) => Err(RuntimeServingPersistenceErrorV1::Timeout),
        }
    }

    async fn execute_v2_on_connection(
        &self,
        connection: &mut PgConnection,
        operation: RuntimeServingOperationV2<'_>,
    ) -> Result<RuntimeServingOperationResultV2, RuntimeServingPersistenceErrorV1> {
        let mutation = !matches!(&operation, RuntimeServingOperationV2::Observe(_));
        let mut transaction = begin_serving_mutation_transaction(connection, self.timeouts).await?;
        verify_runtime_serving_binding_v1(&mut transaction, &self.expectation).await?;
        let result = match operation {
            RuntimeServingOperationV2::Observe(identity) => {
                let rows = sqlx::query_as::<_, RuntimeServingObserveRowV2>(OBSERVE_V2_QUERY)
                    .bind(identity.operation_id.as_str())
                    .bind(identity.scope.tenant_id.as_str())
                    .bind(identity.scope.installation_id.as_str())
                    .bind(identity.scope.deployment_id.as_str())
                    .bind(identity.attestation_digest.as_str())
                    .bind(identity.process_identity.process_instance_id.as_str())
                    .bind(runtime_i64(
                        identity.process_identity.runtime_generation.get(),
                    )?)
                    .bind(runtime_i64(identity.lease_epoch.get())?)
                    .fetch_all(&mut *transaction)
                    .await
                    .map_err(map_query_error)?;
                let [row] = rows.as_slice() else {
                    return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
                };
                RuntimeServingOperationResultV2::Observation(row.decode(identity)?)
            }
            RuntimeServingOperationV2::Heartbeat {
                identity,
                lease_for,
            } => {
                let lease_milliseconds =
                    validate_millisecond_duration(lease_for, MAX_RUNTIME_SERVING_LEASE_DURATION)?;
                if lease_for < MIN_RUNTIME_SERVING_LEASE_DURATION {
                    return Err(RuntimeServingPersistenceErrorV1::InvalidInput);
                }
                let rows = sqlx::query_as::<_, RuntimeServingMutationRowV2>(HEARTBEAT_V2_QUERY)
                    .bind(identity.operation_id.as_str())
                    .bind(identity.scope.tenant_id.as_str())
                    .bind(identity.scope.installation_id.as_str())
                    .bind(identity.scope.deployment_id.as_str())
                    .bind(identity.attestation_digest.as_str())
                    .bind(identity.process_identity.process_instance_id.as_str())
                    .bind(runtime_i64(
                        identity.process_identity.runtime_generation.get(),
                    )?)
                    .bind(runtime_i64(identity.lease_epoch.get())?)
                    .bind(runtime_i64(identity.revision.get())?)
                    .bind(lease_milliseconds)
                    .fetch_all(&mut *transaction)
                    .await
                    .map_err(map_mutation_error)?;
                let [row] = rows.as_slice() else {
                    return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
                };
                RuntimeServingOperationResultV2::Mutation(Box::new(
                    row.decode_heartbeat(identity, lease_for)?,
                ))
            }
            RuntimeServingOperationV2::Disconnect(identity) => {
                let rows = sqlx::query_as::<_, RuntimeServingMutationRowV2>(DISCONNECT_V2_QUERY)
                    .bind(identity.operation_id.as_str())
                    .bind(identity.scope.tenant_id.as_str())
                    .bind(identity.scope.installation_id.as_str())
                    .bind(identity.scope.deployment_id.as_str())
                    .bind(identity.attestation_digest.as_str())
                    .bind(identity.process_identity.process_instance_id.as_str())
                    .bind(runtime_i64(
                        identity.process_identity.runtime_generation.get(),
                    )?)
                    .bind(runtime_i64(identity.lease_epoch.get())?)
                    .bind(runtime_i64(identity.revision.get())?)
                    .fetch_all(&mut *transaction)
                    .await
                    .map_err(map_mutation_error)?;
                let [row] = rows.as_slice() else {
                    return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
                };
                RuntimeServingOperationResultV2::Mutation(Box::new(
                    row.decode_disconnect(identity)?,
                ))
            }
        };
        let commit = transaction.commit().await;
        match commit {
            Ok(()) => Ok(result),
            Err(error) if mutation => Err(map_mutation_commit_error(error)),
            Err(error) => Err(map_query_error(error)),
        }
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct RuntimeServingObserveRowV2 {
    outcome_name: String,
    operation_id: Option<String>,
    tenant_id: Option<String>,
    installation_id: Option<String>,
    deployment_id: Option<String>,
    guild_id: Option<String>,
    ruleset_key: Option<String>,
    target_version: Option<i64>,
    target_content_hash: Option<String>,
    binding_revision: Option<i64>,
    binding_fingerprint: Option<String>,
    attestation_digest: Option<String>,
    process_instance_id: Option<String>,
    runtime_generation: Option<i64>,
    lease_epoch: Option<i64>,
    serving_revision: Option<i64>,
    acquired_at: Option<DateTime<Utc>>,
    last_heartbeat_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    connected: Option<bool>,
    serving: Option<bool>,
    observed_at: DateTime<Utc>,
}

impl RuntimeServingObserveRowV2 {
    fn decode(
        &self,
        expected: &RuntimeServingIdentityV2,
    ) -> Result<RuntimeServingObservationV2, RuntimeServingPersistenceErrorV1> {
        match self.outcome_name.as_str() {
            "absent" => {
                self.require_empty()?;
                Ok(RuntimeServingObservationV2::Absent {
                    observed_at: self.observed_at,
                })
            }
            "diverged" => {
                self.require_empty()?;
                Ok(RuntimeServingObservationV2::Diverged {
                    observed_at: self.observed_at,
                })
            }
            "current" => {
                let row = RuntimeServingMutationRowV2 {
                    operation_id: required(self.operation_id.clone())?,
                    tenant_id: required(self.tenant_id.clone())?,
                    installation_id: required(self.installation_id.clone())?,
                    deployment_id: required(self.deployment_id.clone())?,
                    guild_id: required(self.guild_id.clone())?,
                    ruleset_key: required(self.ruleset_key.clone())?,
                    target_version: required(self.target_version)?,
                    target_content_hash: required(self.target_content_hash.clone())?,
                    binding_revision: required(self.binding_revision)?,
                    binding_fingerprint: required(self.binding_fingerprint.clone())?,
                    attestation_digest: required(self.attestation_digest.clone())?,
                    process_instance_id: required(self.process_instance_id.clone())?,
                    runtime_generation: required(self.runtime_generation)?,
                    lease_epoch: required(self.lease_epoch)?,
                    serving_revision: required(self.serving_revision)?,
                    acquired_at: required(self.acquired_at)?,
                    last_heartbeat_at: required(self.last_heartbeat_at)?,
                    expires_at: required(self.expires_at)?,
                    connected: required(self.connected)?,
                    serving: required(self.serving)?,
                };
                if row.serving_revision != runtime_i64(expected.revision.get())? {
                    return Ok(RuntimeServingObservationV2::Diverged {
                        observed_at: self.observed_at,
                    });
                }
                Ok(RuntimeServingObservationV2::Current {
                    serving: Box::new(row.decode_current(expected)?),
                    observed_at: self.observed_at,
                })
            }
            _ => Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt),
        }
    }

    fn require_empty(&self) -> Result<(), RuntimeServingPersistenceErrorV1> {
        let empty = self.operation_id.is_none()
            && self.tenant_id.is_none()
            && self.installation_id.is_none()
            && self.deployment_id.is_none()
            && self.guild_id.is_none()
            && self.ruleset_key.is_none()
            && self.target_version.is_none()
            && self.target_content_hash.is_none()
            && self.binding_revision.is_none()
            && self.binding_fingerprint.is_none()
            && self.attestation_digest.is_none()
            && self.process_instance_id.is_none()
            && self.runtime_generation.is_none()
            && self.lease_epoch.is_none()
            && self.serving_revision.is_none()
            && self.acquired_at.is_none()
            && self.last_heartbeat_at.is_none()
            && self.expires_at.is_none()
            && self.connected.is_none()
            && self.serving.is_none();
        if empty {
            Ok(())
        } else {
            Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
        }
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct RuntimeServingMutationRowV2 {
    operation_id: String,
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
    guild_id: String,
    ruleset_key: String,
    target_version: i64,
    target_content_hash: String,
    binding_revision: i64,
    binding_fingerprint: String,
    attestation_digest: String,
    process_instance_id: String,
    runtime_generation: i64,
    lease_epoch: i64,
    serving_revision: i64,
    acquired_at: DateTime<Utc>,
    last_heartbeat_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    connected: bool,
    serving: bool,
}

impl RuntimeServingMutationRowV2 {
    fn decode_current(
        &self,
        expected: &RuntimeServingIdentityV2,
    ) -> Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1> {
        self.decode(
            expected,
            self.serving_revision,
            self.connected,
            self.serving,
        )
    }

    fn decode_heartbeat(
        &self,
        expected: &RuntimeServingIdentityV2,
        lease_for: Duration,
    ) -> Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1> {
        let expected_expiry = chrono::Duration::from_std(lease_for)
            .map_err(|_| RuntimeServingPersistenceErrorV1::PersistenceCorrupt)?;
        let next_revision = next_revision(expected.revision)?;
        if self.serving_revision != runtime_i64(next_revision.get())?
            || self
                .expires_at
                .signed_duration_since(self.last_heartbeat_at)
                != expected_expiry
        {
            return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
        }
        self.decode(expected, self.serving_revision, true, true)
    }

    fn decode_disconnect(
        &self,
        expected: &RuntimeServingIdentityV2,
    ) -> Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1> {
        let next_revision = next_revision(expected.revision)?;
        if self.serving_revision != runtime_i64(next_revision.get())?
            || self.last_heartbeat_at != self.expires_at
        {
            return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
        }
        self.decode(expected, self.serving_revision, false, false)
    }

    fn decode(
        &self,
        expected: &RuntimeServingIdentityV2,
        serving_revision: i64,
        connected: bool,
        serving: bool,
    ) -> Result<RuntimeServingReceiptV2, RuntimeServingPersistenceErrorV1> {
        let target = &expected.process_identity.target;
        if self.operation_id != expected.operation_id.as_str()
            || self.tenant_id != expected.scope.tenant_id.as_str()
            || self.installation_id != expected.scope.installation_id.as_str()
            || self.deployment_id != expected.scope.deployment_id.as_str()
            || self.guild_id != target.guild_id.to_string()
            || self.ruleset_key != target.ruleset_key.as_str()
            || self.target_version != i64::from(target.version.get())
            || self.target_content_hash != target.content_hash.to_hex()
            || self.binding_revision != runtime_i64(target.binding_revision.get())?
            || self.binding_fingerprint != target.binding_fingerprint.as_str()
            || self.attestation_digest != expected.attestation_digest.as_str()
            || self.process_instance_id != expected.process_identity.process_instance_id.as_str()
            || self.runtime_generation
                != runtime_i64(expected.process_identity.runtime_generation.get())?
            || self.lease_epoch != runtime_i64(expected.lease_epoch.get())?
            || self.acquired_at > self.last_heartbeat_at
            || self.last_heartbeat_at > self.expires_at
            || self.connected != connected
            || self.serving != serving
        {
            return Err(RuntimeServingPersistenceErrorV1::PersistenceCorrupt);
        }
        let revision = positive_nonzero(serving_revision)?;
        Ok(RuntimeServingReceiptV2 {
            identity: RuntimeServingIdentityV2 {
                scope: expected.scope.clone(),
                operation_id: expected.operation_id.clone(),
                attestation_digest: expected.attestation_digest.clone(),
                process_identity: expected.process_identity.clone(),
                lease_epoch: expected.lease_epoch,
                revision,
            },
            acquired_at: self.acquired_at,
            last_heartbeat_at: self.last_heartbeat_at,
            expires_at: self.expires_at,
            connected: self.connected,
            serving: self.serving,
        })
    }
}

fn required<T>(value: Option<T>) -> Result<T, RuntimeServingPersistenceErrorV1> {
    value.ok_or(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
}

fn runtime_i64(value: u64) -> Result<i64, RuntimeServingPersistenceErrorV1> {
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(RuntimeServingPersistenceErrorV1::InvalidInput)
}

fn positive_nonzero(value: i64) -> Result<NonZeroU64, RuntimeServingPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeServingPersistenceErrorV1::PersistenceCorrupt)
}

fn next_revision(revision: NonZeroU64) -> Result<NonZeroU64, RuntimeServingPersistenceErrorV1> {
    revision
        .get()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .ok_or(RuntimeServingPersistenceErrorV1::InvalidInput)
}
