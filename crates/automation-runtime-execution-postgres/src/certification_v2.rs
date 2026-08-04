use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

use automation_runtime_controller::{
    RuntimeCanonicalCertificationIntentV2, RuntimeCanonicalLiveAttestationV2,
    RuntimeCertificationDivergenceV2, RuntimeCertificationIntentFingerprintV2,
    RuntimeCertificationLookupV2, RuntimeCertificationObservationV2, RuntimeCertificationReceiptV2,
    RuntimeCertificationRequestDigestV2, RuntimeLiveAttestationDigestV2,
    RuntimeReservedCertificationIntentV2, RuntimeServingIdentityV2, RuntimeServingReceiptV2,
};
use automation_runtime_convergence::{
    DeploymentRevision, RuntimeDeployment, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
    TransitionOutcomeV1,
};
use automation_runtime_worker::{
    RuntimeAbortErrorV2, RuntimeAbortRecoveryPortV2, RuntimeAuthorizedCertificationRequestV2,
    RuntimeCertificationRecoveryOutcomeV2, RuntimeCommitCompletionErrorV2,
    RuntimeCommitRecoveryPortV2, RuntimeLiveCertificationPortV2,
    RuntimePreparedLiveCertificationPortV2, RuntimeRecoveryPendingV2,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};

use crate::database::{
    begin_owned_execution_locked_observation_transaction,
    begin_owned_execution_mutation_transaction, verify_runtime_execution_binding_v1,
};
use crate::error::{map_mutation_commit_error, map_query_error};
use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

const PREPARE_QUERY: &str = "SELECT * FROM public.starring_runtime_certification_prepare_v2(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)";
const COMMIT_QUERY: &str = "SELECT * FROM public.starring_runtime_certification_commit_v2(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)";
const OBSERVE_QUERY: &str = "SELECT * FROM public.starring_runtime_certification_observe_v2(\
        $1, $2, $3, $4, $5, $6, $7)";
const PREPARE_DEADLINE_QUERY: &str = "SELECT pg_catalog.clock_timestamp() \
        + $1::BIGINT * INTERVAL '1 millisecond' AS must_commit_before";

pub struct PostgresPreparedRuntimeCertificationV2 {
    transaction: Transaction<'static, Postgres>,
    store: PostgresRuntimeExecutionV1,
    reservation: RuntimeReservedCertificationIntentV2,
    must_commit_before: DateTime<Utc>,
}

pub struct PostgresRuntimeCertificationCommitRecoveryV2 {
    store: PostgresRuntimeExecutionV1,
    lookup: RuntimeCertificationLookupV2,
}

pub struct PostgresRuntimeCertificationAbortRecoveryV2;

impl PostgresRuntimeExecutionV1 {
    async fn prepare_certification_v2(
        &self,
        reservation: RuntimeReservedCertificationIntentV2,
    ) -> Result<PostgresPreparedRuntimeCertificationV2, RuntimeExecutionPersistenceErrorV1> {
        let operation_scope = reservation.operation_scope();
        let intent = reservation.canonical_intent().intent();
        let guard = &intent.guard;
        let mut transaction =
            begin_owned_execution_mutation_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;

        let prepare_window_milliseconds =
            i64::try_from(self.timeouts.statement_timeout().as_millis())
                .map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
        let mut deadline_rows =
            sqlx::query_as::<_, RuntimeCertificationDeadlineRowV2>(PREPARE_DEADLINE_QUERY)
                .bind(prepare_window_milliseconds)
                .fetch_all(&mut *transaction)
                .await
                .map_err(map_query_error)?;
        if deadline_rows.len() != 1 {
            let _ = transaction.rollback().await;
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        let must_commit_before = deadline_rows
            .pop()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
            .must_commit_before;

        let query_result = sqlx::query_as::<_, RuntimeCertificationPrepareRowV2>(PREPARE_QUERY)
            .bind(reservation.operation_id().as_str())
            .bind(reservation.intent_fingerprint().as_str())
            .bind(operation_scope.scope().tenant_id.as_str())
            .bind(operation_scope.scope().installation_id.as_str())
            .bind(operation_scope.scope().deployment_id.as_str())
            .bind(positive_i64(operation_scope.deployment_revision().get())?)
            .bind(guard.controller_id.as_str())
            .bind(positive_i64(guard.fencing_token.get())?)
            .bind(positive_i64(guard.runtime_generation.get())?)
            .bind(i64::from(operation_scope.convergence_attempt().get()))
            .bind(must_commit_before)
            .fetch_all(&mut *transaction)
            .await;
        let mut rows = match query_result {
            Ok(rows) => rows,
            Err(error) => {
                let mapped = map_query_error(error);
                let _ = transaction.rollback().await;
                return Err(mapped);
            }
        };
        if rows.len() != 1 {
            let _ = transaction.rollback().await;
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        let prepared = rows
            .pop()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        if prepared.outcome_name != "prepared"
            || prepared.operation_id != reservation.operation_id().as_str()
            || prepared.intent_fingerprint != reservation.intent_fingerprint().as_str()
            || prepared.certification_intent_bytes != reservation.certification_intent_bytes()
            || prepared.locked_convergence_attempt_no
                != i64::from(operation_scope.convergence_attempt().get())
            || prepared.must_commit_before != must_commit_before
            || prepared.observed_at >= must_commit_before
        {
            let _ = transaction.rollback().await;
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        let snapshot = decode_snapshot(prepared.locked_snapshot)?;
        if snapshot.revision != operation_scope.deployment_revision()
            || !operation_scope.scope().matches(&snapshot.identity)
            || snapshot.runtime_generation != guard.runtime_generation
            || !matches!(
                snapshot.phase,
                RuntimeDeploymentPhaseV1::AwaitingGatewayReady
            )
        {
            let _ = transaction.rollback().await;
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        Ok(PostgresPreparedRuntimeCertificationV2 {
            transaction,
            store: self.clone(),
            reservation,
            must_commit_before,
        })
    }

    pub async fn observe_certification_v2(
        &self,
        lookup: RuntimeCertificationLookupV2,
    ) -> Result<RuntimeCertificationObservationV2, RuntimeExecutionPersistenceErrorV1> {
        let mut transaction =
            begin_owned_execution_locked_observation_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        let mut rows = sqlx::query_as::<_, RuntimeCertificationObserveRowV2>(OBSERVE_QUERY)
            .bind(lookup.operation_id.as_str())
            .bind(lookup.scope.tenant_id.as_str())
            .bind(lookup.scope.installation_id.as_str())
            .bind(lookup.scope.deployment_id.as_str())
            .bind(positive_i64(lookup.deployment_revision.get())?)
            .bind(i64::from(lookup.convergence_attempt.get()))
            .bind(lookup.request_digest.as_str())
            .fetch_all(&mut *transaction)
            .await
            .map_err(map_query_error)?;
        if rows.len() != 1 {
            let _ = transaction.rollback().await;
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        let row = rows
            .pop()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let observation = row.decode(&lookup)?;
        transaction.commit().await.map_err(map_query_error)?;
        Ok(observation)
    }
}

impl RuntimeLiveCertificationPortV2 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;
    type Prepared = PostgresPreparedRuntimeCertificationV2;

    async fn prepare_live_v2(
        &self,
        reservation: RuntimeReservedCertificationIntentV2,
    ) -> Result<Self::Prepared, Self::Error> {
        self.prepare_certification_v2(reservation).await
    }

    async fn observe_live_v2(
        &self,
        lookup: RuntimeCertificationLookupV2,
    ) -> Result<RuntimeCertificationObservationV2, Self::Error> {
        self.observe_certification_v2(lookup).await
    }
}

impl RuntimePreparedLiveCertificationPortV2 for PostgresPreparedRuntimeCertificationV2 {
    type Error = RuntimeExecutionPersistenceErrorV1;
    type TransactionEnded = ();
    type AbortRecovery = PostgresRuntimeCertificationAbortRecoveryV2;
    type CommitRecovery = PostgresRuntimeCertificationCommitRecoveryV2;

    fn must_commit_before(&self) -> DateTime<Utc> {
        self.must_commit_before
    }

    async fn commit_live_v2(
        mut self,
        authorized: RuntimeAuthorizedCertificationRequestV2,
    ) -> Result<
        RuntimeCertificationReceiptV2,
        RuntimeCommitCompletionErrorV2<Self::Error, Self::CommitRecovery, Self::TransactionEnded>,
    > {
        let canonical = authorized.canonical();
        let request = canonical.request();
        let guard = &request.intent.guard;
        let lookup = lookup_for(canonical);
        let recovery = PostgresRuntimeCertificationCommitRecoveryV2 {
            store: self.store.clone(),
            lookup,
        };
        if canonical.certification_intent_bytes() != self.reservation.certification_intent_bytes()
            || canonical.intent_fingerprint() != self.reservation.intent_fingerprint()
            || request.must_commit_before != self.must_commit_before
        {
            let source = RuntimeExecutionPersistenceErrorV1::InvalidInput;
            return rollback_before_commit(self.transaction, source, recovery).await;
        }
        let expected_revision = match positive_i64(guard.expected_revision.get()) {
            Ok(value) => value,
            Err(source) => {
                return rollback_before_commit(self.transaction, source, recovery).await;
            }
        };
        let fencing_token = match positive_i64(guard.fencing_token.get()) {
            Ok(value) => value,
            Err(source) => {
                return rollback_before_commit(self.transaction, source, recovery).await;
            }
        };
        let runtime_generation = match positive_i64(guard.runtime_generation.get()) {
            Ok(value) => value,
            Err(source) => {
                return rollback_before_commit(self.transaction, source, recovery).await;
            }
        };

        let query_result = sqlx::query_as::<_, RuntimeCertificationCommitRowV2>(COMMIT_QUERY)
            .bind(request.intent.operation_id.as_str())
            .bind(request.intent_fingerprint.as_str())
            .bind(guard.scope.tenant_id.as_str())
            .bind(guard.scope.installation_id.as_str())
            .bind(guard.scope.deployment_id.as_str())
            .bind(expected_revision)
            .bind(guard.controller_id.as_str())
            .bind(fencing_token)
            .bind(runtime_generation)
            .bind(i64::from(guard.convergence_attempt.get()))
            .bind(canonical.certification_request_bytes())
            .bind(canonical.request_digest().as_str())
            .bind(canonical.live_attestation_record_bytes())
            .bind(canonical.live_attestation_digest().as_str())
            .fetch_all(&mut *self.transaction)
            .await;
        let mut rows = match query_result {
            Ok(rows) => rows,
            Err(error) => {
                return rollback_before_commit(self.transaction, map_query_error(error), recovery)
                    .await;
            }
        };
        if rows.len() != 1 {
            return rollback_before_commit(
                self.transaction,
                RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
                recovery,
            )
            .await;
        }
        let row = match rows.pop() {
            Some(row) => row,
            None => {
                return rollback_before_commit(
                    self.transaction,
                    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt,
                    recovery,
                )
                .await;
            }
        };
        let receipt = match row.decode(canonical) {
            Ok(receipt) => receipt,
            Err(source) => {
                return rollback_before_commit(self.transaction, source, recovery).await;
            }
        };
        match self.transaction.commit().await {
            Ok(()) => Ok(receipt),
            Err(error) => Err(RuntimeCommitCompletionErrorV2::CommitUnknown {
                source: map_mutation_commit_error(error),
                recovery,
            }),
        }
    }

    async fn abort(
        self,
    ) -> Result<Self::TransactionEnded, RuntimeAbortErrorV2<Self::Error, Self::AbortRecovery>> {
        match self.transaction.rollback().await {
            Ok(()) => Ok(()),
            Err(error) => Err(RuntimeAbortErrorV2 {
                source: map_query_error(error),
                recovery: PostgresRuntimeCertificationAbortRecoveryV2,
            }),
        }
    }
}

impl RuntimeAbortRecoveryPortV2 for PostgresRuntimeCertificationAbortRecoveryV2 {
    type Error = RuntimeExecutionPersistenceErrorV1;
    type TransactionEnded = ();

    async fn quiesce(
        self,
        _timeout: Duration,
    ) -> Result<Self::TransactionEnded, RuntimeRecoveryPendingV2<Self::Error, Self>> {
        Err(RuntimeRecoveryPendingV2 {
            source: RuntimeExecutionPersistenceErrorV1::Indeterminate,
            recovery: self,
        })
    }
}

impl RuntimeCommitRecoveryPortV2 for PostgresRuntimeCertificationCommitRecoveryV2 {
    type Error = RuntimeExecutionPersistenceErrorV1;
    type TransactionEnded = ();

    fn lookup(&self) -> &RuntimeCertificationLookupV2 {
        &self.lookup
    }

    async fn quiesce_and_observe(
        self,
        timeout: Duration,
    ) -> Result<
        RuntimeCertificationRecoveryOutcomeV2<Self::TransactionEnded>,
        RuntimeRecoveryPendingV2<Self::Error, Self>,
    > {
        let result = tokio::time::timeout(
            timeout,
            self.store.observe_certification_v2(self.lookup.clone()),
        )
        .await;
        match result {
            Ok(Ok(observation)) => Ok(RuntimeCertificationRecoveryOutcomeV2 {
                transaction_ended: (),
                observation,
            }),
            Ok(Err(source)) => Err(RuntimeRecoveryPendingV2 {
                source,
                recovery: self,
            }),
            Err(_) => Err(RuntimeRecoveryPendingV2 {
                source: RuntimeExecutionPersistenceErrorV1::Timeout,
                recovery: self,
            }),
        }
    }
}

async fn rollback_before_commit(
    transaction: Transaction<'static, Postgres>,
    source: RuntimeExecutionPersistenceErrorV1,
    recovery: PostgresRuntimeCertificationCommitRecoveryV2,
) -> Result<
    RuntimeCertificationReceiptV2,
    RuntimeCommitCompletionErrorV2<
        RuntimeExecutionPersistenceErrorV1,
        PostgresRuntimeCertificationCommitRecoveryV2,
        (),
    >,
> {
    match transaction.rollback().await {
        Ok(()) => Err(RuntimeCommitCompletionErrorV2::DefinitelyRolledBack {
            source,
            transaction_ended: (),
        }),
        Err(_) => Err(RuntimeCommitCompletionErrorV2::CommitUnknown { source, recovery }),
    }
}

fn lookup_for(canonical: &RuntimeCanonicalLiveAttestationV2) -> RuntimeCertificationLookupV2 {
    let request = canonical.request();
    RuntimeCertificationLookupV2 {
        scope: request.intent.guard.scope.clone(),
        deployment_revision: request.intent.guard.expected_revision,
        convergence_attempt: request.intent.guard.convergence_attempt,
        operation_id: request.intent.operation_id.clone(),
        request_digest: canonical.request_digest().clone(),
    }
}

#[derive(sqlx::FromRow)]
struct RuntimeCertificationDeadlineRowV2 {
    must_commit_before: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct RuntimeCertificationPrepareRowV2 {
    outcome_name: String,
    locked_snapshot: Json<Value>,
    locked_convergence_attempt_no: i64,
    observed_at: DateTime<Utc>,
    operation_id: String,
    certification_intent_bytes: Vec<u8>,
    intent_fingerprint: String,
    must_commit_before: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct RuntimeCertificationCommitRowV2 {
    outcome_name: String,
    previous_snapshot: Json<Value>,
    snapshot: Json<Value>,
    convergence_attempt_no: i64,
    operation_id: String,
    intent_fingerprint: String,
    certification_request_bytes: Vec<u8>,
    request_digest: String,
    live_attestation_record_bytes: Vec<u8>,
    attestation_digest: String,
    route_admission: Json<Value>,
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
    guild_id: String,
    ruleset_key: String,
    process_instance_id: String,
    runtime_generation: i64,
    lease_epoch: i64,
    serving_revision: i64,
    acquired_at: DateTime<Utc>,
    last_heartbeat_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    connected: bool,
    serving: bool,
    certified_at: DateTime<Utc>,
}

impl RuntimeCertificationCommitRowV2 {
    fn decode(
        self,
        canonical: &RuntimeCanonicalLiveAttestationV2,
    ) -> Result<RuntimeCertificationReceiptV2, RuntimeExecutionPersistenceErrorV1> {
        let expected = canonical.request();
        let intent = &expected.intent;
        let previous = decode_snapshot(self.previous_snapshot)?;
        let snapshot = decode_snapshot(self.snapshot)?;
        let convergence_attempt = positive_u32(self.convergence_attempt_no)?;
        let lease_epoch = positive_nonzero_u64(self.lease_epoch)?;
        let serving_revision = positive_nonzero_u64(self.serving_revision)?;
        let next_revision = intent
            .guard
            .expected_revision
            .next()
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let outcome = match self.outcome_name.as_str() {
            "applied" => TransitionOutcomeV1::Applied {
                revision: next_revision,
            },
            "replayed" => TransitionOutcomeV1::Replayed {
                revision: next_revision,
            },
            _ => return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt),
        };
        let expected_route = canonical_route_admission(canonical.certification_request_bytes())?;
        if previous.revision != intent.guard.expected_revision
            || snapshot.revision != next_revision
            || !matches!(snapshot.phase, RuntimeDeploymentPhaseV1::Live)
            || !intent.guard.scope.matches(&snapshot.identity)
            || snapshot.target != intent.target
            || snapshot.runtime_generation != intent.guard.runtime_generation
            || self.convergence_attempt_no != i64::from(intent.guard.convergence_attempt.get())
            || self.operation_id != intent.operation_id.as_str()
            || self.intent_fingerprint != expected.intent_fingerprint.as_str()
            || self.certification_request_bytes != canonical.certification_request_bytes()
            || self.request_digest != canonical.request_digest().as_str()
            || self.live_attestation_record_bytes != canonical.live_attestation_record_bytes()
            || self.attestation_digest != canonical.live_attestation_digest().as_str()
            || self.route_admission.0 != expected_route
            || self.tenant_id != intent.guard.scope.tenant_id.as_str()
            || self.installation_id != intent.guard.scope.installation_id.as_str()
            || self.deployment_id != intent.guard.scope.deployment_id.as_str()
            || self.guild_id != intent.target.guild_id.to_string()
            || self.ruleset_key != intent.target.ruleset_key.as_str()
            || self.process_instance_id != intent.process_identity.process_instance_id.as_str()
            || positive_u64(self.runtime_generation)? != intent.guard.runtime_generation.get()
            || self.acquired_at != self.certified_at
            || self.last_heartbeat_at < self.acquired_at
            || self.last_heartbeat_at >= self.expires_at
            || self.certified_at > expected.must_commit_before
            || !self.connected
            || !self.serving
        {
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        Ok(RuntimeCertificationReceiptV2 {
            action_id: intent.action_id,
            outcome,
            snapshot,
            convergence_attempt,
            operation_id: intent.operation_id.clone(),
            intent_fingerprint: expected.intent_fingerprint.clone(),
            request_digest: canonical.request_digest().clone(),
            attestation_digest: canonical.live_attestation_digest().clone(),
            route_admission: expected.route_admission.clone(),
            serving: RuntimeServingReceiptV2 {
                identity: RuntimeServingIdentityV2 {
                    scope: intent.guard.scope.clone(),
                    operation_id: intent.operation_id.clone(),
                    attestation_digest: canonical.live_attestation_digest().clone(),
                    process_identity: intent.process_identity.clone(),
                    lease_epoch,
                    revision: serving_revision,
                },
                acquired_at: self.acquired_at,
                last_heartbeat_at: self.last_heartbeat_at,
                expires_at: self.expires_at,
                connected: self.connected,
                serving: self.serving,
            },
            certified_at: self.certified_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct RuntimeCertificationObserveRowV2 {
    outcome_name: String,
    snapshot: Option<Json<Value>>,
    convergence_attempt_no: Option<i64>,
    observed_deployment_revision: Option<i64>,
    observed_at: DateTime<Utc>,
    operation_id: Option<String>,
    intent_fingerprint: Option<String>,
    certification_intent_bytes: Option<Vec<u8>>,
    certification_request_bytes: Option<Vec<u8>>,
    request_digest: Option<String>,
    live_attestation_record_bytes: Option<Vec<u8>>,
    attestation_digest: Option<String>,
    route_admission: Option<Json<Value>>,
    tenant_id: Option<String>,
    installation_id: Option<String>,
    deployment_id: Option<String>,
    guild_id: Option<String>,
    ruleset_key: Option<String>,
    process_instance_id: Option<String>,
    runtime_generation: Option<i64>,
    lease_epoch: Option<i64>,
    serving_revision: Option<i64>,
    acquired_at: Option<DateTime<Utc>>,
    last_heartbeat_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    connected: Option<bool>,
    serving: Option<bool>,
    certified_at: Option<DateTime<Utc>>,
}

impl RuntimeCertificationObserveRowV2 {
    fn decode(
        self,
        lookup: &RuntimeCertificationLookupV2,
    ) -> Result<RuntimeCertificationObservationV2, RuntimeExecutionPersistenceErrorV1> {
        match self.outcome_name.as_str() {
            "not_committed" => {
                let snapshot = decode_snapshot(required(self.snapshot)?)?;
                let convergence_attempt = positive_u32(required(self.convergence_attempt_no)?)?;
                let observed_deployment_revision = DeploymentRevision::new(positive_u64(
                    required(self.observed_deployment_revision)?,
                )?)
                .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
                if convergence_attempt != lookup.convergence_attempt
                    || observed_deployment_revision != lookup.deployment_revision
                    || required(self.operation_id)?.as_str() != lookup.operation_id.as_str()
                    || required(self.request_digest)?.as_str() != lookup.request_digest.as_str()
                    || !lookup.scope.matches(&snapshot.identity)
                {
                    return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
                }
                Ok(RuntimeCertificationObservationV2::NotCommitted {
                    snapshot,
                    convergence_attempt,
                    operation_id: lookup.operation_id.clone(),
                    request_digest: lookup.request_digest.clone(),
                    observed_deployment_revision,
                    observed_at: self.observed_at,
                })
            }
            "committed" => self.decode_committed(lookup),
            "diverged" => {
                let divergence = match self.snapshot {
                    Some(snapshot) => {
                        let snapshot = decode_snapshot(snapshot)?;
                        if snapshot.revision > lookup.deployment_revision {
                            RuntimeCertificationDivergenceV2::DeploymentAdvanced { snapshot }
                        } else {
                            RuntimeCertificationDivergenceV2::PersistenceCorrupt
                        }
                    }
                    None => RuntimeCertificationDivergenceV2::PersistenceCorrupt,
                };
                Ok(RuntimeCertificationObservationV2::Diverged(divergence))
            }
            _ => Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt),
        }
    }

    fn decode_committed(
        self,
        lookup: &RuntimeCertificationLookupV2,
    ) -> Result<RuntimeCertificationObservationV2, RuntimeExecutionPersistenceErrorV1> {
        let snapshot = decode_snapshot(required(self.snapshot)?)?;
        let convergence_attempt = positive_u32(required(self.convergence_attempt_no)?)?;
        let persisted_operation_id =
            automation_runtime_controller::RuntimeCertificationOperationIdV2::parse(required(
                self.operation_id,
            )?)
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let fingerprint =
            RuntimeCertificationIntentFingerprintV2::parse(required(self.intent_fingerprint)?)
                .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let intent_bytes = required(self.certification_intent_bytes)?;
        let canonical_intent =
            RuntimeCanonicalCertificationIntentV2::from_persisted(&intent_bytes, &fingerprint)
                .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let request_digest =
            RuntimeCertificationRequestDigestV2::parse(required(self.request_digest)?)
                .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let attestation_digest =
            RuntimeLiveAttestationDigestV2::parse(required(self.attestation_digest)?)
                .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let request_bytes = required(self.certification_request_bytes)?;
        let live_bytes = required(self.live_attestation_record_bytes)?;
        let canonical = RuntimeCanonicalLiveAttestationV2::from_persisted(
            &canonical_intent,
            &request_bytes,
            &request_digest,
            &live_bytes,
            &attestation_digest,
        )
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let request = canonical.request();
        let intent = &request.intent;
        let route = required(self.route_admission)?;
        let expected_route = canonical_route_admission(&request_bytes)?;
        let lease_epoch = positive_nonzero_u64(required(self.lease_epoch)?)?;
        let serving_revision = positive_nonzero_u64(required(self.serving_revision)?)?;
        let acquired_at = required(self.acquired_at)?;
        let last_heartbeat_at = required(self.last_heartbeat_at)?;
        let expires_at = required(self.expires_at)?;
        let certified_at = required(self.certified_at)?;
        if persisted_operation_id != lookup.operation_id
            || request_digest != lookup.request_digest
            || convergence_attempt != lookup.convergence_attempt
            || intent.guard.expected_revision != lookup.deployment_revision
            || intent.guard.scope != lookup.scope
            || route.0 != expected_route
            || required(self.tenant_id)?.as_str() != lookup.scope.tenant_id.as_str()
            || required(self.installation_id)?.as_str() != lookup.scope.installation_id.as_str()
            || required(self.deployment_id)?.as_str() != lookup.scope.deployment_id.as_str()
            || required(self.guild_id)?.as_str() != intent.target.guild_id.to_string()
            || required(self.ruleset_key)?.as_str() != intent.target.ruleset_key.as_str()
            || required(self.process_instance_id)?.as_str()
                != intent.process_identity.process_instance_id.as_str()
            || positive_u64(required(self.runtime_generation)?)?
                != intent.guard.runtime_generation.get()
            || !lookup.scope.matches(&snapshot.identity)
            || !matches!(snapshot.phase, RuntimeDeploymentPhaseV1::Live)
            || snapshot.revision
                != lookup
                    .deployment_revision
                    .next()
                    .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
            || acquired_at != certified_at
            || last_heartbeat_at < acquired_at
            || last_heartbeat_at >= expires_at
            || !required(self.connected)?
            || !required(self.serving)?
        {
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        let revision = snapshot.revision;
        Ok(RuntimeCertificationObservationV2::Committed(
            RuntimeCertificationReceiptV2 {
                action_id: intent.action_id,
                outcome: TransitionOutcomeV1::Replayed { revision },
                snapshot,
                convergence_attempt,
                operation_id: persisted_operation_id,
                intent_fingerprint: fingerprint,
                request_digest,
                attestation_digest: attestation_digest.clone(),
                route_admission: request.route_admission.clone(),
                serving: RuntimeServingReceiptV2 {
                    identity: RuntimeServingIdentityV2 {
                        scope: intent.guard.scope.clone(),
                        operation_id: intent.operation_id.clone(),
                        attestation_digest,
                        process_identity: intent.process_identity.clone(),
                        lease_epoch,
                        revision: serving_revision,
                    },
                    acquired_at,
                    last_heartbeat_at,
                    expires_at,
                    connected: true,
                    serving: true,
                },
                certified_at,
            },
        ))
    }
}

fn decode_snapshot(
    value: Json<Value>,
) -> Result<RuntimeDeploymentSnapshotV1, RuntimeExecutionPersistenceErrorV1> {
    let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(value.0)
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
    RuntimeDeployment::restore(snapshot.clone())
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
    Ok(snapshot)
}

fn canonical_route_admission(
    certification_request_bytes: &[u8],
) -> Result<Value, RuntimeExecutionPersistenceErrorV1> {
    let request = serde_json::from_slice::<Value>(certification_request_bytes)
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
    request
        .as_object()
        .and_then(|request| request.get("route_admission"))
        .cloned()
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

fn required<T>(value: Option<T>) -> Result<T, RuntimeExecutionPersistenceErrorV1> {
    value.ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

fn positive_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(RuntimeExecutionPersistenceErrorV1::InvalidInput)
}

fn positive_u64(value: i64) -> Result<u64, RuntimeExecutionPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

fn positive_nonzero_u64(value: i64) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    NonZeroU64::new(positive_u64(value)?)
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}

fn positive_u32(value: i64) -> Result<NonZeroU32, RuntimeExecutionPersistenceErrorV1> {
    let value =
        u32::try_from(value).map_err(|_| RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
    NonZeroU32::new(value).ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
}
